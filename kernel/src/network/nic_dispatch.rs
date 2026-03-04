//! NIC Dispatch Layer — routes packets between loopback and virtio-net.
//!
//! Phase 12: Connects the TCP/UDP socket stack to the real NIC.
//! transmit_ip() routes by destination: 127.x → LOOPBACK, else → Ethernet → NIC.
//! nic_receive_one() polls NIC RX, parses Ethernet, dispatches to ARP/TCP/UDP.

#![allow(dead_code)]

use crate::serial_println;
use super::ip::Ipv4Header;
use super::ethernet::{EthernetFrame, ETHERTYPE_IPV4, ETHERTYPE_ARP};
use super::arp;
use super::tcp;
use super::udp;
use super::SOCKETS;
use super::LOOPBACK;

/// Our IP address on the virtio-net interface (QEMU user-mode default).
pub const GUEST_IP: [u8; 4] = [10, 0, 2, 15];
/// Gateway IP (QEMU user-mode default).
pub const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
/// Subnet mask.
pub const SUBNET_MASK: [u8; 4] = [255, 255, 255, 0];

/// DNS response capture buffer (filled by nic_receive_one when it sees UDP for our DNS port).
/// Used by dns::dns_resolve() to capture DNS responses without going through the socket layer.
pub static DNS_RESPONSE: spin::Mutex<DnsResponseBuf> = spin::Mutex::new(DnsResponseBuf::new());

/// Buffer for capturing a single DNS response.
pub struct DnsResponseBuf {
    pub data: [u8; 512],
    pub len: usize,
    pub ready: bool,
}

impl DnsResponseBuf {
    pub const fn new() -> Self {
        Self {
            data: [0u8; 512],
            len: 0,
            ready: false,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.ready = false;
    }
}

/// Check if a destination IP is loopback (127.x.x.x).
pub fn is_loopback(dst_ip: &[u8; 4]) -> bool {
    dst_ip[0] == 127
}

/// Get the NIC's MAC address (if NIC is initialized).
pub fn get_nic_mac() -> Option<[u8; 6]> {
    let nic = crate::virtio::net::NIC.lock();
    nic.as_ref().map(|n| n.mac)
}

/// Transmit an IP packet via the appropriate interface.
///
/// If destination is 127.x.x.x → enqueue to LOOPBACK.
/// Otherwise → wrap in Ethernet frame, resolve gateway MAC via ARP, send via NIC.
///
/// This function may be called while SOCKETS is held (from tcp.rs state machine).
/// Lock ordering: SOCKETS → NIC is safe (NIC is never held before SOCKETS).
pub fn transmit_ip(packet: &[u8]) {
    let ip_hdr = match Ipv4Header::from_bytes(packet) {
        Some(h) => h,
        None => return,
    };

    if is_loopback(&ip_hdr.dst_addr) {
        // Loopback path — same as Phase 9
        let mut lo = LOOPBACK.lock();
        lo.enqueue(packet);
    } else {
        // NIC path — wrap in Ethernet and send
        let src_mac = match get_nic_mac() {
            Some(m) => m,
            None => return, // No NIC available
        };

        // All non-loopback traffic goes through the gateway
        // Look up gateway MAC from ARP table
        let dst_mac = match arp::arp_lookup(GATEWAY_IP) {
            Some(m) => m,
            None => {
                // No ARP entry for gateway — use broadcast as fallback
                // (ARP resolve should have been called during init)
                [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]
            }
        };

        let frame = EthernetFrame::build(dst_mac, src_mac, ETHERTYPE_IPV4, packet);

        let mut nic = crate::virtio::net::NIC.lock();
        if let Some(ref mut nic) = *nic {
            nic.send_packet(&frame);
        }
    }
}

/// Static counter for tracking TX packets sent via NIC (for OCRB testing).
static NIC_TX_COUNT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Get number of packets transmitted via NIC.
pub fn nic_tx_count() -> u32 {
    NIC_TX_COUNT.load(core::sync::atomic::Ordering::Relaxed)
}

/// Poll the NIC RX queue for one received packet and process it.
///
/// Returns true if a packet was received and processed.
///
/// Lock ordering: NIC (poll+copy, drop) → SOCKETS (deliver) → NIC or LOOPBACK (response).
/// NIC is always dropped before acquiring SOCKETS — no circular dependency.
pub fn nic_receive_one() -> bool {
    // Phase 1: Poll NIC, copy data to stack, recycle descriptor
    let mut frame_buf = [0u8; 1600]; // MTU + headers
    let frame_len;
    let desc_idx;

    {
        let mut nic = crate::virtio::net::NIC.lock();
        let nic = match nic.as_mut() {
            Some(n) => n,
            None => return false,
        };

        match nic.poll_rx() {
            Some((ptr, len)) => {
                let copy_len = len.min(frame_buf.len());
                unsafe {
                    core::ptr::copy_nonoverlapping(ptr, frame_buf.as_mut_ptr(), copy_len);
                }
                frame_len = copy_len;
                // Get the descriptor index from the used ring (it was the last polled)
                desc_idx = nic.rx_queue.last_used_idx.wrapping_sub(1);
                nic.recycle_rx(desc_idx);
            }
            None => return false,
        }
    }
    // NIC lock dropped here

    // Phase 2: Parse Ethernet frame
    let eth = match EthernetFrame::parse(&frame_buf[..frame_len]) {
        Some(e) => e,
        None => return false,
    };

    // Phase 3: Dispatch by EtherType
    match eth.ethertype {
        ETHERTYPE_ARP => {
            arp::process_arp_frame(&eth.payload);
        }
        ETHERTYPE_IPV4 => {
            // Parse IP header from Ethernet payload
            let ip_hdr = match Ipv4Header::from_bytes(&eth.payload) {
                Some(h) => h,
                None => return true,
            };

            let ip_payload_start = Ipv4Header::SIZE;
            if ip_payload_start >= eth.payload.len() {
                return true;
            }
            let ip_payload = &eth.payload[ip_payload_start..];

            // Check if this is a DNS response (UDP port 53 → our DNS port)
            // Capture it in DNS_RESPONSE buffer for dns_resolve()
            if ip_hdr.protocol == 17 {
                // UDP
                if let Some(udp_hdr) = super::udp::UdpHeader::from_bytes(ip_payload) {
                    if udp_hdr.src_port == 53 {
                        // DNS response — capture it
                        let udp_payload_start = super::udp::UdpHeader::SIZE;
                        let udp_data_len = (udp_hdr.length as usize).saturating_sub(super::udp::UdpHeader::SIZE);
                        if udp_payload_start + udp_data_len <= ip_payload.len() {
                            let dns_data = &ip_payload[udp_payload_start..udp_payload_start + udp_data_len];
                            let mut dns_buf = DNS_RESPONSE.lock();
                            let copy_len = dns_data.len().min(dns_buf.data.len());
                            dns_buf.data[..copy_len].copy_from_slice(&dns_data[..copy_len]);
                            dns_buf.len = copy_len;
                            dns_buf.ready = true;
                        }
                        return true;
                    }
                }
            }

            // Dispatch to TCP or UDP via socket layer
            let mut table = SOCKETS.lock();
            match ip_hdr.protocol {
                6 => tcp::tcp_receive_packet(&ip_hdr, ip_payload, &mut table),
                17 => udp::udp_receive_packet(&ip_hdr, ip_payload, &mut table),
                _ => {} // Unknown protocol, drop
            }
        }
        _ => {} // Unknown EtherType, drop
    }

    true
}
