//! IPv4 header — 20 bytes (no options).
//!
//! Minimal IP header for loopback-only networking. No fragmentation,
//! no options, no TTL decrement — just enough structure for protocol dispatch.

#![allow(dead_code)]

use super::addr::Ipv4Addr;
use super::checksum::internet_checksum;

/// IPv4 header — 20 bytes, no options.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Ipv4Header {
    pub version_ihl: u8,   // Version (4) + IHL (5) = 0x45
    pub tos: u8,           // Type of service
    pub total_length: u16, // Total packet length (header + payload), big-endian
    pub identification: u16,
    pub flags_fragment: u16,
    pub ttl: u8,
    pub protocol: u8,      // 6 = TCP, 17 = UDP
    pub checksum: u16,
    pub src_addr: [u8; 4],
    pub dst_addr: [u8; 4],
}

impl Ipv4Header {
    pub const SIZE: usize = 20;

    /// Create a new IPv4 header for a loopback packet.
    pub fn new(
        src: Ipv4Addr,
        dst: Ipv4Addr,
        protocol: u8,
        payload_len: u16,
    ) -> Self {
        let total_length = (Self::SIZE as u16) + payload_len;
        let mut hdr = Self {
            version_ihl: 0x45, // IPv4, IHL=5 (20 bytes)
            tos: 0,
            total_length: total_length.to_be(),
            identification: 0,
            flags_fragment: 0x4000u16.to_be(), // DF flag
            ttl: 64,
            protocol,
            checksum: 0,
            src_addr: src.0,
            dst_addr: dst.0,
        };
        hdr.checksum = hdr.compute_checksum();
        hdr
    }

    /// Compute the header checksum.
    pub fn compute_checksum(&self) -> u16 {
        let bytes = self.to_bytes_no_checksum();
        internet_checksum(&bytes)
    }

    /// Serialize the header to bytes (with checksum field zeroed for computation).
    fn to_bytes_no_checksum(&self) -> [u8; Self::SIZE] {
        let mut buf = self.to_bytes();
        buf[10] = 0;
        buf[11] = 0;
        buf
    }

    /// Serialize header to bytes.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let tl = self.total_length.to_be_bytes();
        let id = self.identification.to_be_bytes();
        let ff = self.flags_fragment.to_be_bytes();
        let cs = self.checksum.to_be_bytes();
        [
            self.version_ihl,
            self.tos,
            // total_length is already in big-endian
            (self.total_length >> 8) as u8,
            self.total_length as u8,
            id[0], id[1],
            ff[0], ff[1],
            self.ttl,
            self.protocol,
            cs[0], cs[1],
            self.src_addr[0], self.src_addr[1], self.src_addr[2], self.src_addr[3],
            self.dst_addr[0], self.dst_addr[1], self.dst_addr[2], self.dst_addr[3],
        ]
    }

    /// Parse from a byte slice. Returns None if too short.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            version_ihl: data[0],
            tos: data[1],
            total_length: u16::from_be_bytes([data[2], data[3]]),
            identification: u16::from_be_bytes([data[4], data[5]]),
            flags_fragment: u16::from_be_bytes([data[6], data[7]]),
            ttl: data[8],
            protocol: data[9],
            checksum: u16::from_be_bytes([data[10], data[11]]),
            src_addr: [data[12], data[13], data[14], data[15]],
            dst_addr: [data[16], data[17], data[18], data[19]],
        })
    }

    /// Get total packet length from the header.
    pub fn total_len(&self) -> u16 {
        self.total_length
    }

    /// Get payload length (total - header).
    pub fn payload_len(&self) -> u16 {
        self.total_length.saturating_sub(Self::SIZE as u16)
    }

    /// Get source address.
    pub fn src(&self) -> Ipv4Addr {
        Ipv4Addr(self.src_addr)
    }

    /// Get destination address.
    pub fn dst(&self) -> Ipv4Addr {
        Ipv4Addr(self.dst_addr)
    }
}
