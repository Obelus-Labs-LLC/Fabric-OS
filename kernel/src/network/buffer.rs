//! Ring buffer for network socket data.
//!
//! Fixed 4KB circular buffer with wraparound read/write.
//! Used for both RX and TX socket buffers.

#![allow(dead_code)]

/// Ring buffer size — 4KB.
pub const RING_BUF_SIZE: usize = 4096;

/// Fixed-size circular ring buffer.
pub struct RingBuffer {
    data: [u8; RING_BUF_SIZE],
    read_pos: usize,
    write_pos: usize,
    len: usize,
}

impl RingBuffer {
    /// Create a new empty ring buffer.
    pub const fn new() -> Self {
        Self {
            data: [0u8; RING_BUF_SIZE],
            read_pos: 0,
            write_pos: 0,
            len: 0,
        }
    }

    /// Number of bytes currently in the buffer.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the buffer is empty.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the buffer is full.
    pub const fn is_full(&self) -> bool {
        self.len == RING_BUF_SIZE
    }

    /// Available space for writing.
    pub const fn available(&self) -> usize {
        RING_BUF_SIZE - self.len
    }

    /// Write bytes into the ring buffer. Returns number of bytes written.
    /// Writes as many bytes as fit; discards the rest.
    pub fn write(&mut self, data: &[u8]) -> usize {
        let to_write = data.len().min(self.available());
        for i in 0..to_write {
            self.data[self.write_pos] = data[i];
            self.write_pos = (self.write_pos + 1) % RING_BUF_SIZE;
        }
        self.len += to_write;
        to_write
    }

    /// Read bytes from the ring buffer into `buf`. Returns number of bytes read.
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let to_read = buf.len().min(self.len);
        for i in 0..to_read {
            buf[i] = self.data[self.read_pos];
            self.read_pos = (self.read_pos + 1) % RING_BUF_SIZE;
        }
        self.len -= to_read;
        to_read
    }

    /// Peek at the first `n` bytes without consuming them.
    pub fn peek(&self, buf: &mut [u8]) -> usize {
        let to_peek = buf.len().min(self.len);
        let mut pos = self.read_pos;
        for i in 0..to_peek {
            buf[i] = self.data[pos];
            pos = (pos + 1) % RING_BUF_SIZE;
        }
        to_peek
    }

    /// Discard `n` bytes from the read position.
    pub fn discard(&mut self, n: usize) {
        let to_discard = n.min(self.len);
        self.read_pos = (self.read_pos + to_discard) % RING_BUF_SIZE;
        self.len -= to_discard;
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
        self.len = 0;
    }
}
