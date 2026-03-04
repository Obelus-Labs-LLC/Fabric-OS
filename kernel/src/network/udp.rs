//! UDP protocol — User Datagram Protocol.
//!
//! Provides connectionless, unreliable datagram delivery.
//! Used over loopback so delivery is lossless in practice.

#![allow(dead_code)]

use super::addr::{Ipv4Addr, SocketAddr, Protocol};
use super::checksum::pseudo_header_checksum;
use super::ip::Ipv4Header;
use super::socket::{SocketId, SocketState, SocketError};

/// UDP header — 8 bytes.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct UdpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,   // Header + data length
    pub checksum: u16,
}

impl UdpHeader {
    pub const SIZE: usize = 8;

    pub fn new(src_port: u16, dst_port: u16, data_len: u16) -> Self {
        Self {
            src_port,
            dst_port,
            length: Self::SIZE as u16 + data_len,
            checksum: 0, // Checksum computed later
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        [
            (self.src_port >> 8) as u8,
            self.src_port as u8,
            (self.dst_port >> 8) as u8,
            self.dst_port as u8,
            (self.length >> 8) as u8,
            self.length as u8,
            (self.checksum >> 8) as u8,
            self.checksum as u8,
        ]
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            src_port: u16::from_be_bytes([data[0], data[1]]),
            dst_port: u16::from_be_bytes([data[2], data[3]]),
            length: u16::from_be_bytes([data[4], data[5]]),
            checksum: u16::from_be_bytes([data[6], data[7]]),
        })
    }
}

/// Build a UDP packet (IP header + UDP header + data).
/// Returns the total packet bytes and length.
pub fn build_udp_packet(
    src: SocketAddr,
    dst: SocketAddr,
    data: &[u8],
    buf: &mut [u8],
) -> usize {
    let data_len = data.len() as u16;
    let udp_len = UdpHeader::SIZE as u16 + data_len;
    let total = Ipv4Header::SIZE + udp_len as usize;

    if buf.len() < total {
        return 0;
    }

    // Build IP header
    let ip = Ipv4Header::new(src.addr, dst.addr, 17, udp_len); // 17 = UDP
    let ip_bytes = ip.to_bytes();
    buf[..Ipv4Header::SIZE].copy_from_slice(&ip_bytes);

    // Build UDP header
    let mut udp = UdpHeader::new(src.port, dst.port, data_len);

    // Build UDP segment (header + data) for checksum
    let udp_hdr_bytes = udp.to_bytes();
    let udp_start = Ipv4Header::SIZE;
    buf[udp_start..udp_start + UdpHeader::SIZE].copy_from_slice(&udp_hdr_bytes);
    buf[udp_start + UdpHeader::SIZE..total].copy_from_slice(data);

    // Compute checksum over pseudo-header + UDP segment
    let chk = pseudo_header_checksum(
        &src.addr.0,
        &dst.addr.0,
        17,
        udp_len,
        &buf[udp_start..total],
    );
    udp.checksum = chk;

    // Re-write UDP header with checksum
    let udp_bytes = udp.to_bytes();
    buf[udp_start..udp_start + UdpHeader::SIZE].copy_from_slice(&udp_bytes);

    total
}

/// Process a received UDP packet. Writes data into the destination socket's RX buffer.
/// Called from deliver path with SOCKETS lock held.
pub fn udp_receive_packet(
    ip_hdr: &Ipv4Header,
    udp_data: &[u8],
    sockets: &mut super::socket::SocketTable,
) {
    let udp_hdr = match UdpHeader::from_bytes(udp_data) {
        Some(h) => h,
        None => return,
    };

    let dst_addr = SocketAddr::new(Ipv4Addr(ip_hdr.dst_addr), udp_hdr.dst_port);

    // Find the socket bound to this destination
    let sock_id = match sockets.find_by_local(&dst_addr, Protocol::Udp) {
        Some(id) => id,
        None => return, // No socket listening, drop
    };

    let payload_start = UdpHeader::SIZE;
    let payload_len = (udp_hdr.length as usize).saturating_sub(UdpHeader::SIZE);
    if payload_start + payload_len > udp_data.len() {
        return;
    }

    let payload = &udp_data[payload_start..payload_start + payload_len];

    // Write the source address info + payload into RX buffer
    // Format: [src_port:2][src_ip:4][len:2][data:N]
    if let Some(sock) = sockets.get_mut(sock_id) {
        let src_port_bytes = udp_hdr.src_port.to_be_bytes();
        let data_len_bytes = (payload.len() as u16).to_be_bytes();
        sock.rx.write(&src_port_bytes);
        sock.rx.write(&ip_hdr.src_addr);
        sock.rx.write(&data_len_bytes);
        sock.rx.write(payload);
    }
}
