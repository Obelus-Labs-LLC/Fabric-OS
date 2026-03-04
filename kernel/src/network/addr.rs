//! Network address types for the Fabric OS network stack.
//!
//! IPv4 addresses, socket addresses, and protocol identifiers.
//! All types are `repr(C)` or `repr(u8)` for stable layout.

#![allow(dead_code)]

/// IPv4 address — 4 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    pub const UNSPECIFIED: Self = Self([0, 0, 0, 0]);
    pub const LOOPBACK: Self = Self([127, 0, 0, 1]);
    pub const BROADCAST: Self = Self([255, 255, 255, 255]);

    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }

    /// Check if this is the loopback address (127.0.0.1).
    pub const fn is_loopback(&self) -> bool {
        self.0[0] == 127 && self.0[1] == 0 && self.0[2] == 0 && self.0[3] == 1
    }

    /// Check if this is the unspecified address (0.0.0.0).
    pub const fn is_unspecified(&self) -> bool {
        self.0[0] == 0 && self.0[1] == 0 && self.0[2] == 0 && self.0[3] == 0
    }

    /// Convert to big-endian u32 (network byte order).
    pub const fn to_u32(&self) -> u32 {
        (self.0[0] as u32) << 24
            | (self.0[1] as u32) << 16
            | (self.0[2] as u32) << 8
            | (self.0[3] as u32)
    }

    /// Create from big-endian u32.
    pub const fn from_u32(v: u32) -> Self {
        Self([
            (v >> 24) as u8,
            (v >> 16) as u8,
            (v >> 8) as u8,
            v as u8,
        ])
    }
}

impl core::fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

/// Socket address — IPv4 address + port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct SocketAddr {
    pub addr: Ipv4Addr,
    pub port: u16,
}

impl SocketAddr {
    pub const UNSPECIFIED: Self = Self {
        addr: Ipv4Addr::UNSPECIFIED,
        port: 0,
    };

    pub const fn new(addr: Ipv4Addr, port: u16) -> Self {
        Self { addr, port }
    }

    /// Check if this is completely unspecified (0.0.0.0:0).
    pub const fn is_unspecified(&self) -> bool {
        self.addr.is_unspecified() && self.port == 0
    }
}

impl core::fmt::Display for SocketAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}", self.addr, self.port)
    }
}

/// Address family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AddressFamily {
    Inet = 2,  // AF_INET (IPv4)
}

/// Socket type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SocketType {
    Stream   = 1,  // SOCK_STREAM (TCP)
    Datagram = 2,  // SOCK_DGRAM (UDP)
}

/// Protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Protocol {
    Tcp = 6,   // IPPROTO_TCP
    Udp = 17,  // IPPROTO_UDP
}
