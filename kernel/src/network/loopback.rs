//! Loopback network interface.
//!
//! 64-slot circular packet queue, MTU=1500. Packets enqueued here are
//! immediately available for delivery to the receive path.

#![allow(dead_code)]

/// Maximum transmission unit for loopback.
pub const LOOPBACK_MTU: usize = 1500;

/// Number of packet slots in the loopback queue.
pub const LOOPBACK_QUEUE_SIZE: usize = 64;

/// A single packet in the loopback queue.
pub struct LoopbackPacket {
    pub data: [u8; LOOPBACK_MTU],
    pub len: usize,
}

impl LoopbackPacket {
    pub const fn empty() -> Self {
        Self {
            data: [0u8; LOOPBACK_MTU],
            len: 0,
        }
    }
}

/// Loopback interface — circular queue of packets.
pub struct Loopback {
    packets: [LoopbackPacket; LOOPBACK_QUEUE_SIZE],
    head: usize,
    tail: usize,
    count: usize,
}

impl Loopback {
    pub const fn new() -> Self {
        const EMPTY_PKT: LoopbackPacket = LoopbackPacket::empty();
        Self {
            packets: [EMPTY_PKT; LOOPBACK_QUEUE_SIZE],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    /// Enqueue a packet into the loopback queue.
    /// Returns false if the queue is full.
    pub fn enqueue(&mut self, data: &[u8]) -> bool {
        if self.count >= LOOPBACK_QUEUE_SIZE || data.len() > LOOPBACK_MTU {
            return false;
        }
        let slot = &mut self.packets[self.tail];
        slot.data[..data.len()].copy_from_slice(data);
        slot.len = data.len();
        self.tail = (self.tail + 1) % LOOPBACK_QUEUE_SIZE;
        self.count += 1;
        true
    }

    /// Dequeue a packet from the loopback queue.
    /// Returns a stack-allocated copy of the packet data and its length.
    pub fn dequeue(&mut self) -> Option<([u8; LOOPBACK_MTU], usize)> {
        if self.count == 0 {
            return None;
        }
        let slot = &self.packets[self.head];
        let mut buf = [0u8; LOOPBACK_MTU];
        buf[..slot.len].copy_from_slice(&slot.data[..slot.len]);
        let len = slot.len;
        self.head = (self.head + 1) % LOOPBACK_QUEUE_SIZE;
        self.count -= 1;
        Some((buf, len))
    }

    /// Number of packets queued.
    pub fn queued(&self) -> usize {
        self.count
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Whether the queue is full.
    pub fn is_full(&self) -> bool {
        self.count >= LOOPBACK_QUEUE_SIZE
    }
}
