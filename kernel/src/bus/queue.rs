//! Per-process bounded inbox queue.
//!
//! Each registered ProcessId gets a circular buffer of Envelopes.
//! When full, sends to this process return BusError::ReceiverQueueFull.

#![allow(dead_code)]

use fabric_types::MessageHeader;
use super::arena::ArenaSlice;

/// Maximum messages per inbox.
pub const QUEUE_CAPACITY: usize = 32;

/// A message envelope as stored in the inbox queue.
#[derive(Clone, Copy)]
pub struct Envelope {
    pub header:  MessageHeader,       // 64 bytes — the wire header
    pub hmac:    [u8; 32],            // HMAC-SHA3-256 of active_bytes + payload
    pub payload: Option<ArenaSlice>,  // Reference into payload arena (None = no payload)
}

impl Envelope {
    pub const fn zeroed() -> Self {
        Self {
            header: MessageHeader::zeroed(),
            hmac: [0u8; 32],
            payload: None,
        }
    }
}

/// Bounded circular buffer of envelopes for a single process.
pub struct InboxQueue {
    entries: [Option<Envelope>; QUEUE_CAPACITY],
    head: usize,   // next read position
    tail: usize,   // next write position
    count: usize,  // current occupancy
}

impl InboxQueue {
    pub const fn new() -> Self {
        const NONE: Option<Envelope> = None;
        Self {
            entries: [NONE; QUEUE_CAPACITY],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    /// Push an envelope into the queue. Returns false if full.
    pub fn push(&mut self, env: Envelope) -> bool {
        if self.count >= QUEUE_CAPACITY {
            return false;
        }
        self.entries[self.tail] = Some(env);
        self.tail = (self.tail + 1) % QUEUE_CAPACITY;
        self.count += 1;
        true
    }

    /// Pop the oldest envelope from the queue.
    pub fn pop(&mut self) -> Option<Envelope> {
        if self.count == 0 {
            return None;
        }
        let env = self.entries[self.head].take();
        self.head = (self.head + 1) % QUEUE_CAPACITY;
        self.count -= 1;
        env
    }

    /// Peek at the oldest envelope without removing it.
    pub fn peek(&self) -> Option<&Envelope> {
        if self.count == 0 {
            return None;
        }
        self.entries[self.head].as_ref()
    }

    /// Number of pending messages.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Is the queue full?
    pub fn is_full(&self) -> bool {
        self.count >= QUEUE_CAPACITY
    }

    /// Is the queue empty?
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        for entry in self.entries.iter_mut() {
            *entry = None;
        }
        self.head = 0;
        self.tail = 0;
        self.count = 0;
    }
}
