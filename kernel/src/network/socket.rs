//! Socket table and socket state management.
//!
//! 256-slot table with generation-counted SocketId (slot bits 0-7, gen bits 8-23).
//! Each socket has a type, protocol, local/remote address, state, owner PID,
//! and RX/TX ring buffers.

#![allow(dead_code)]

extern crate alloc;
use alloc::boxed::Box;
use fabric_types::ProcessId;
use super::addr::{SocketAddr, SocketType, Protocol};
use super::buffer::RingBuffer;
use super::tcp_timer::RetransmitQueue;

/// Maximum number of sockets.
pub const MAX_SOCKETS: usize = 256;

/// Maximum listen backlog.
pub const MAX_BACKLOG: usize = 8;

/// Socket identifier — encodes slot index (bits 0-7) and generation (bits 8-23).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketId(pub u32);

impl SocketId {
    pub const INVALID: Self = Self(0xFFFF_FFFF);

    pub const fn slot(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    pub const fn generation(self) -> u16 {
        ((self.0 >> 8) & 0xFFFF) as u16
    }

    pub const fn pack(slot: u8, gen: u16) -> Self {
        Self((slot as u32) | ((gen as u32) << 8))
    }
}

/// Socket state (TCP and general).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SocketState {
    Closed       = 0,
    Bound        = 1,
    Listen       = 2,
    SynSent      = 3,
    SynReceived  = 4,
    Established  = 5,
    FinWait1     = 6,
    FinWait2     = 7,
    CloseWait    = 8,
    LastAck      = 9,
    TimeWait     = 10,
    Closing      = 11,
}

/// Socket error codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketError {
    NotFound,
    InvalidState,
    AddressInUse,
    ConnectionRefused,
    NotConnected,
    WouldBlock,
    BufferFull,
    TooManySockets,
    InvalidArgument,
    NotBound,
}

/// A single socket.
pub struct Socket {
    /// Whether this slot is active.
    pub active: bool,
    /// Generation counter for stale detection.
    pub generation: u16,
    /// Socket type (Stream / Datagram).
    pub sock_type: SocketType,
    /// Protocol (TCP / UDP).
    pub protocol: Protocol,
    /// Local address (bound address + port).
    pub local_addr: SocketAddr,
    /// Remote address (connected peer).
    pub remote_addr: SocketAddr,
    /// Socket state.
    pub state: SocketState,
    /// Owning process.
    pub owner: ProcessId,

    // -- Buffers --
    /// Receive buffer.
    pub rx: RingBuffer,
    /// Transmit buffer.
    pub tx: RingBuffer,

    // -- TCP state --
    /// TCP: next sequence number to send.
    pub send_seq: u32,
    /// TCP: last acknowledged sequence from peer.
    pub send_ack: u32,
    /// TCP: next expected sequence from peer.
    pub recv_seq: u32,
    /// TCP: receive window size.
    pub recv_window: u16,
    /// TCP: send window (advertised by peer).
    pub send_window: u16,

    // -- TCP retransmission --
    /// Retransmit queue (TCP only, None for UDP).
    pub retransmit: Option<Box<RetransmitQueue>>,

    // -- Listen backlog --
    /// Pending connections for listening sockets.
    pub backlog: [SocketId; MAX_BACKLOG],
    /// Number of pending connections.
    pub backlog_count: usize,
}

impl Socket {
    pub const fn empty() -> Self {
        Self {
            active: false,
            generation: 0,
            sock_type: SocketType::Stream,
            protocol: Protocol::Tcp,
            local_addr: SocketAddr::UNSPECIFIED,
            remote_addr: SocketAddr::UNSPECIFIED,
            state: SocketState::Closed,
            owner: ProcessId::KERNEL,
            rx: RingBuffer::new(),
            tx: RingBuffer::new(),
            send_seq: 0,
            send_ack: 0,
            recv_seq: 0,
            recv_window: 4096,
            send_window: 4096,
            retransmit: None,
            backlog: [SocketId::INVALID; MAX_BACKLOG],
            backlog_count: 0,
        }
    }

    /// Reset a socket to its initial state (preserving generation).
    pub fn reset(&mut self) {
        let gen = self.generation;
        *self = Self::empty();
        self.generation = gen;
    }
}

/// Socket table — 256 slots with generation-counted allocation.
pub struct SocketTable {
    pub sockets: [Socket; MAX_SOCKETS],
}

impl SocketTable {
    pub const fn new() -> Self {
        // const array init with const fn
        const EMPTY: Socket = Socket::empty();
        Self {
            sockets: [EMPTY; MAX_SOCKETS],
        }
    }

    /// Allocate a new socket. Returns SocketId or TooManySockets.
    pub fn alloc(
        &mut self,
        sock_type: SocketType,
        protocol: Protocol,
        owner: ProcessId,
    ) -> Result<SocketId, SocketError> {
        for slot in 0..MAX_SOCKETS {
            if !self.sockets[slot].active {
                let gen = self.sockets[slot].generation.wrapping_add(1);
                self.sockets[slot] = Socket::empty();
                self.sockets[slot].active = true;
                self.sockets[slot].generation = gen;
                self.sockets[slot].sock_type = sock_type;
                self.sockets[slot].protocol = protocol;
                self.sockets[slot].owner = owner;
                // TCP sockets get a retransmit queue
                if protocol == Protocol::Tcp {
                    self.sockets[slot].retransmit = Some(Box::new(RetransmitQueue::new()));
                }
                return Ok(SocketId::pack(slot as u8, gen));
            }
        }
        Err(SocketError::TooManySockets)
    }

    /// Get a socket by SocketId. Validates generation.
    pub fn get(&self, id: SocketId) -> Option<&Socket> {
        let slot = id.slot() as usize;
        if slot >= MAX_SOCKETS {
            return None;
        }
        let sock = &self.sockets[slot];
        if sock.active && sock.generation == id.generation() {
            Some(sock)
        } else {
            None
        }
    }

    /// Get a mutable socket by SocketId. Validates generation.
    pub fn get_mut(&mut self, id: SocketId) -> Option<&mut Socket> {
        let slot = id.slot() as usize;
        if slot >= MAX_SOCKETS {
            return None;
        }
        let sock = &mut self.sockets[slot];
        if sock.active && sock.generation == id.generation() {
            Some(sock)
        } else {
            None
        }
    }

    /// Release a socket. Increments generation to invalidate stale references.
    pub fn release(&mut self, id: SocketId) -> Result<(), SocketError> {
        let slot = id.slot() as usize;
        if slot >= MAX_SOCKETS {
            return Err(SocketError::NotFound);
        }
        let sock = &mut self.sockets[slot];
        if !sock.active || sock.generation != id.generation() {
            return Err(SocketError::NotFound);
        }
        sock.active = false;
        // Generation stays incremented so stale SocketIds fail validation
        Ok(())
    }

    /// Find a socket by local address (for incoming packet delivery).
    pub fn find_by_local(&self, addr: &SocketAddr, protocol: Protocol) -> Option<SocketId> {
        for slot in 0..MAX_SOCKETS {
            let sock = &self.sockets[slot];
            if sock.active && sock.protocol == protocol {
                // Exact match
                if sock.local_addr == *addr {
                    return Some(SocketId::pack(slot as u8, sock.generation));
                }
                // Wildcard match (bound to 0.0.0.0)
                if sock.local_addr.addr.is_unspecified()
                    && sock.local_addr.port == addr.port
                {
                    return Some(SocketId::pack(slot as u8, sock.generation));
                }
            }
        }
        None
    }

    /// Find a TCP socket by the 4-tuple (src, dst).
    pub fn find_tcp_connection(
        &self,
        local: &SocketAddr,
        remote: &SocketAddr,
    ) -> Option<SocketId> {
        for slot in 0..MAX_SOCKETS {
            let sock = &self.sockets[slot];
            if sock.active && sock.protocol == Protocol::Tcp {
                // Check 4-tuple match (allowing wildcard local addr)
                let local_match = sock.local_addr == *local
                    || (sock.local_addr.addr.is_unspecified()
                        && sock.local_addr.port == local.port);
                let remote_match = sock.remote_addr == *remote;
                if local_match && remote_match {
                    return Some(SocketId::pack(slot as u8, sock.generation));
                }
            }
        }
        None
    }

    /// Find a listening socket by local port (for SYN delivery).
    pub fn find_listener(&self, port: u16) -> Option<SocketId> {
        for slot in 0..MAX_SOCKETS {
            let sock = &self.sockets[slot];
            if sock.active
                && sock.protocol == Protocol::Tcp
                && sock.state == SocketState::Listen
                && sock.local_addr.port == port
            {
                return Some(SocketId::pack(slot as u8, sock.generation));
            }
        }
        None
    }

    /// Clean up all sockets owned by a process.
    pub fn cleanup_by_owner(&mut self, pid: ProcessId) {
        for slot in 0..MAX_SOCKETS {
            if self.sockets[slot].active && self.sockets[slot].owner == pid {
                self.sockets[slot].active = false;
            }
        }
    }

    /// Check if a local port is already in use.
    pub fn port_in_use(&self, port: u16, protocol: Protocol) -> bool {
        for slot in 0..MAX_SOCKETS {
            let sock = &self.sockets[slot];
            if sock.active && sock.protocol == protocol && sock.local_addr.port == port {
                return true;
            }
        }
        false
    }

    /// Count of active sockets.
    pub fn count(&self) -> usize {
        self.sockets.iter().filter(|s| s.active).count()
    }
}
