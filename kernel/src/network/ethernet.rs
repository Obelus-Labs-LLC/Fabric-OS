//! Ethernet Frame — build and parse IEEE 802.3 / Ethernet II frames.
//!
//! Provides frame construction for transmitting via virtio-net and
//! frame parsing for received packets.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

/// EtherType constants (big-endian on the wire).
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;
pub const ETHERTYPE_IPV6: u16 = 0x86DD;

/// Broadcast MAC address.
pub const BROADCAST_MAC: [u8; 6] = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

/// Ethernet II frame header (14 bytes).
#[derive(Clone, Debug)]
pub struct EthernetFrame {
    pub dst_mac: [u8; 6],
    pub src_mac: [u8; 6],
    pub ethertype: u16,
    pub payload: Vec<u8>,
}

impl EthernetFrame {
    /// Build a raw Ethernet frame ready for transmission.
    ///
    /// Returns the complete frame bytes: dst(6) + src(6) + ethertype(2) + payload.
    pub fn build(dst_mac: [u8; 6], src_mac: [u8; 6], ethertype: u16, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(14 + payload.len());
        frame.extend_from_slice(&dst_mac);
        frame.extend_from_slice(&src_mac);
        frame.push((ethertype >> 8) as u8);
        frame.push(ethertype as u8);
        frame.extend_from_slice(payload);
        frame
    }

    /// Parse a raw Ethernet frame from received bytes.
    ///
    /// Returns None if the frame is too short (< 14 bytes header).
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 14 {
            return None;
        }

        let mut dst_mac = [0u8; 6];
        let mut src_mac = [0u8; 6];
        dst_mac.copy_from_slice(&data[0..6]);
        src_mac.copy_from_slice(&data[6..12]);
        let ethertype = (data[12] as u16) << 8 | data[13] as u16;
        let payload = data[14..].to_vec();

        Some(Self {
            dst_mac,
            src_mac,
            ethertype,
            payload,
        })
    }

    /// Get the header bytes (14 bytes) without payload.
    pub fn header_bytes(&self) -> [u8; 14] {
        let mut hdr = [0u8; 14];
        hdr[0..6].copy_from_slice(&self.dst_mac);
        hdr[6..12].copy_from_slice(&self.src_mac);
        hdr[12] = (self.ethertype >> 8) as u8;
        hdr[13] = self.ethertype as u8;
        hdr
    }
}
