//! ARP — Address Resolution Protocol for IPv4 over Ethernet.
//!
//! Builds ARP request/reply packets, maintains a simple ARP table
//! mapping IPv4 addresses to MAC addresses.

#![allow(dead_code)]

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;
use crate::serial_println;
use super::ethernet::{EthernetFrame, ETHERTYPE_ARP, BROADCAST_MAC};

/// ARP hardware type: Ethernet = 1.
const ARP_HW_ETHERNET: u16 = 1;
/// ARP protocol type: IPv4 = 0x0800.
const ARP_PROTO_IPV4: u16 = 0x0800;
/// ARP opcodes.
const ARP_OP_REQUEST: u16 = 1;
const ARP_OP_REPLY: u16 = 2;

/// ARP packet (28 bytes for IPv4-over-Ethernet).
#[derive(Clone, Debug)]
pub struct ArpPacket {
    pub hw_type: u16,
    pub proto_type: u16,
    pub hw_len: u8,
    pub proto_len: u8,
    pub opcode: u16,
    pub sender_mac: [u8; 6],
    pub sender_ip: [u8; 4],
    pub target_mac: [u8; 6],
    pub target_ip: [u8; 4],
}

/// Global ARP table: IPv4 (as [u8;4]) -> MAC address.
pub static ARP_TABLE: Mutex<BTreeMap<[u8; 4], [u8; 6]>> = Mutex::new(BTreeMap::new());

impl ArpPacket {
    /// Build an ARP request: "Who has `target_ip`? Tell `sender_ip`."
    pub fn build_request(
        sender_mac: [u8; 6],
        sender_ip: [u8; 4],
        target_ip: [u8; 4],
    ) -> Self {
        Self {
            hw_type: ARP_HW_ETHERNET,
            proto_type: ARP_PROTO_IPV4,
            hw_len: 6,
            proto_len: 4,
            opcode: ARP_OP_REQUEST,
            sender_mac,
            sender_ip,
            target_mac: [0x00; 6],
            target_ip,
        }
    }

    /// Build an ARP reply.
    pub fn build_reply(
        sender_mac: [u8; 6],
        sender_ip: [u8; 4],
        target_mac: [u8; 6],
        target_ip: [u8; 4],
    ) -> Self {
        Self {
            hw_type: ARP_HW_ETHERNET,
            proto_type: ARP_PROTO_IPV4,
            hw_len: 6,
            proto_len: 4,
            opcode: ARP_OP_REPLY,
            sender_mac,
            sender_ip,
            target_mac,
            target_ip,
        }
    }

    /// Serialize to wire format (28 bytes).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(28);
        buf.push((self.hw_type >> 8) as u8);
        buf.push(self.hw_type as u8);
        buf.push((self.proto_type >> 8) as u8);
        buf.push(self.proto_type as u8);
        buf.push(self.hw_len);
        buf.push(self.proto_len);
        buf.push((self.opcode >> 8) as u8);
        buf.push(self.opcode as u8);
        buf.extend_from_slice(&self.sender_mac);
        buf.extend_from_slice(&self.sender_ip);
        buf.extend_from_slice(&self.target_mac);
        buf.extend_from_slice(&self.target_ip);
        buf
    }

    /// Parse from wire bytes (>= 28 bytes required).
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 28 {
            return None;
        }
        let hw_type = (data[0] as u16) << 8 | data[1] as u16;
        let proto_type = (data[2] as u16) << 8 | data[3] as u16;
        let hw_len = data[4];
        let proto_len = data[5];
        let opcode = (data[6] as u16) << 8 | data[7] as u16;

        let mut sender_mac = [0u8; 6];
        let mut sender_ip = [0u8; 4];
        let mut target_mac = [0u8; 6];
        let mut target_ip = [0u8; 4];

        sender_mac.copy_from_slice(&data[8..14]);
        sender_ip.copy_from_slice(&data[14..18]);
        target_mac.copy_from_slice(&data[18..24]);
        target_ip.copy_from_slice(&data[24..28]);

        Some(Self {
            hw_type,
            proto_type,
            hw_len,
            proto_len,
            opcode,
            sender_mac,
            sender_ip,
            target_mac,
            target_ip,
        })
    }

    /// Build a complete Ethernet frame containing this ARP packet.
    pub fn to_ethernet_frame(&self, src_mac: [u8; 6]) -> Vec<u8> {
        let dst_mac = if self.opcode == ARP_OP_REQUEST {
            BROADCAST_MAC
        } else {
            self.target_mac
        };
        EthernetFrame::build(dst_mac, src_mac, ETHERTYPE_ARP, &self.to_bytes())
    }
}

/// Handle an incoming ARP reply: update the ARP table.
pub fn handle_arp_reply(packet: &ArpPacket) {
    if packet.opcode == ARP_OP_REPLY {
        let mut table = ARP_TABLE.lock();
        table.insert(packet.sender_ip, packet.sender_mac);
        serial_println!(
            "[ARP] Learned {}.{}.{}.{} -> {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            packet.sender_ip[0], packet.sender_ip[1],
            packet.sender_ip[2], packet.sender_ip[3],
            packet.sender_mac[0], packet.sender_mac[1], packet.sender_mac[2],
            packet.sender_mac[3], packet.sender_mac[4], packet.sender_mac[5],
        );
    }
}

/// Look up a MAC address in the ARP table.
pub fn arp_lookup(ip: [u8; 4]) -> Option<[u8; 6]> {
    let table = ARP_TABLE.lock();
    table.get(&ip).copied()
}

/// Resolve an IP address to a MAC address, blocking until resolved or timeout.
///
/// Checks ARP table first. If miss, sends ARP request and polls NIC RX
/// for the reply. Returns None if resolution times out.
pub fn arp_resolve(target_ip: [u8; 4]) -> Option<[u8; 6]> {
    // Check table first
    if let Some(mac) = arp_lookup(target_ip) {
        return Some(mac);
    }

    // Send ARP request
    arp_request(target_ip);

    // Poll NIC RX for ARP reply with timeout
    for _ in 0..50_000 {
        super::nic_dispatch::nic_receive_one();

        if let Some(mac) = arp_lookup(target_ip) {
            return Some(mac);
        }

        core::hint::spin_loop();
    }

    serial_println!(
        "[ARP] Timeout resolving {}.{}.{}.{}",
        target_ip[0], target_ip[1], target_ip[2], target_ip[3]
    );
    None
}

/// Send an ARP request for the given IP address via the NIC.
pub fn arp_request(target_ip: [u8; 4]) {
    // Get our NIC's MAC and IP
    let mut nic_guard = crate::virtio::net::NIC.lock();
    if let Some(ref mut nic) = *nic_guard {
        let src_mac = nic.mac;
        let src_ip: [u8; 4] = [10, 0, 2, 15]; // QEMU user-mode guest IP

        let arp = ArpPacket::build_request(src_mac, src_ip, target_ip);
        let frame = arp.to_ethernet_frame(src_mac);

        serial_println!(
            "[ARP] Sending who-has {}.{}.{}.{}",
            target_ip[0], target_ip[1], target_ip[2], target_ip[3]
        );

        nic.send_packet(&frame);
    }
}

/// Process a received Ethernet frame that contains ARP.
pub fn process_arp_frame(payload: &[u8]) {
    if let Some(arp) = ArpPacket::parse(payload) {
        match arp.opcode {
            ARP_OP_REPLY => handle_arp_reply(&arp),
            ARP_OP_REQUEST => {
                // We could respond to ARP requests for our IP, but for now just log
                serial_println!(
                    "[ARP] Request: who has {}.{}.{}.{}?",
                    arp.target_ip[0], arp.target_ip[1],
                    arp.target_ip[2], arp.target_ip[3]
                );
            }
            _ => {}
        }
    }
}
