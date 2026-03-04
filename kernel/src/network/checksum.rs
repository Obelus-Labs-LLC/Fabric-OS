//! RFC 1071 internet checksum.
//!
//! Used for IP, UDP, and TCP header checksums. Computes a 16-bit one's
//! complement sum over the data, then returns the one's complement of the result.

#![allow(dead_code)]

/// Compute the internet checksum over a byte slice.
/// Returns the 16-bit one's complement checksum in network byte order.
pub fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;

    // Sum 16-bit words
    while i + 1 < data.len() {
        let word = (data[i] as u32) << 8 | data[i + 1] as u32;
        sum += word;
        i += 2;
    }

    // Handle odd byte
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }

    // Fold 32-bit sum to 16 bits
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    // One's complement
    !(sum as u16)
}

/// Verify a checksum — returns true if valid (sum including checksum == 0).
pub fn verify_checksum(data: &[u8]) -> bool {
    internet_checksum(data) == 0
}

/// Compute checksum with a pseudo-header for UDP/TCP.
/// The pseudo-header covers: src IP, dst IP, zero, protocol, length.
pub fn pseudo_header_checksum(
    src: &[u8; 4],
    dst: &[u8; 4],
    protocol: u8,
    length: u16,
    payload: &[u8],
) -> u16 {
    let mut sum: u32 = 0;

    // Source IP (2 x 16-bit words)
    sum += (src[0] as u32) << 8 | src[1] as u32;
    sum += (src[2] as u32) << 8 | src[3] as u32;

    // Destination IP
    sum += (dst[0] as u32) << 8 | dst[1] as u32;
    sum += (dst[2] as u32) << 8 | dst[3] as u32;

    // Zero + Protocol
    sum += protocol as u32;

    // Length
    sum += length as u32;

    // Payload (the actual UDP/TCP header + data)
    let mut i = 0;
    while i + 1 < payload.len() {
        let word = (payload[i] as u32) << 8 | payload[i + 1] as u32;
        sum += word;
        i += 2;
    }
    if i < payload.len() {
        sum += (payload[i] as u32) << 8;
    }

    // Fold and complement
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    !(sum as u16)
}
