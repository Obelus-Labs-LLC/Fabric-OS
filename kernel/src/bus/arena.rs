//! Fixed-size payload arena for zero-copy message passing.
//!
//! The arena is a heap-allocated ring buffer. Senders copy payload data in once;
//! receivers read directly from the arena without additional copies.

#![allow(dead_code)]

use alloc::boxed::Box;

/// Arena capacity: 256 KiB.
const ARENA_CAPACITY: usize = 256 * 1024;

/// Maximum single payload size: 4 KiB.
pub const MAX_PAYLOAD_SIZE: usize = 4096;

/// A reference to a payload region within the arena.
#[derive(Clone, Copy, Debug)]
pub struct ArenaSlice {
    pub offset: u32,
    pub len: u32,
}

/// Fixed-size ring buffer arena for zero-copy payload storage.
///
/// Initialized to `None` at const time; heap-allocated during `init()`.
pub struct PayloadArena {
    buffer: Option<Box<[u8; ARENA_CAPACITY]>>,
    write_cursor: usize,
}

impl PayloadArena {
    pub const fn new() -> Self {
        Self {
            buffer: None,
            write_cursor: 0,
        }
    }

    /// Heap-allocate the arena buffer. Must be called during bus::init().
    pub fn init(&mut self) {
        self.buffer = Some(Box::new([0u8; ARENA_CAPACITY]));
        self.write_cursor = 0;
    }

    /// Allocate space and copy payload data into the arena.
    pub fn allocate(&mut self, data: &[u8]) -> Option<ArenaSlice> {
        if data.is_empty() {
            return None;
        }
        if data.len() > MAX_PAYLOAD_SIZE {
            return None;
        }

        let buf = self.buffer.as_mut()?;
        let offset = self.write_cursor;

        // Wrap around if we'd exceed capacity
        if offset + data.len() > ARENA_CAPACITY {
            // Start from the beginning (simple wrap, overwrites old data)
            self.write_cursor = 0;
            let slice = ArenaSlice {
                offset: 0,
                len: data.len() as u32,
            };
            buf[0..data.len()].copy_from_slice(data);
            self.write_cursor = data.len();
            return Some(slice);
        }

        buf[offset..offset + data.len()].copy_from_slice(data);
        let slice = ArenaSlice {
            offset: offset as u32,
            len: data.len() as u32,
        };
        self.write_cursor = offset + data.len();
        Some(slice)
    }

    /// Get a read-only slice of payload data.
    pub fn get(&self, slice: ArenaSlice) -> &[u8] {
        let buf = self.buffer.as_ref().expect("arena not initialized");
        let start = slice.offset as usize;
        let end = start + slice.len as usize;
        &buf[start..end]
    }

    /// Reset the arena (for testing).
    pub fn clear(&mut self) {
        self.write_cursor = 0;
    }

    /// Available bytes before wrap-around.
    pub fn available(&self) -> usize {
        ARENA_CAPACITY - self.write_cursor
    }

    /// Whether the arena has been initialized.
    pub fn is_initialized(&self) -> bool {
        self.buffer.is_some()
    }
}
