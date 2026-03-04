//! Window Manager Events — typed event enum with serialization and ring buffer.
//!
//! Events are delivered to per-window queues by the keyboard IRQ handler
//! and window manager. Userspace reads events via SYS_WM_EVENT (syscall 34).

#![allow(dead_code)]

/// Window manager event types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WmEvent {
    /// Key pressed (data = ASCII code).
    KeyPress(u8) = 1,
    /// Key released (data = ASCII code).
    KeyRelease(u8) = 2,
    /// Window close requested (Alt+F4 or close button).
    WindowClose = 3,
    /// Window gained focus.
    WindowFocus = 4,
    /// Window lost focus.
    WindowBlur = 5,
}

/// Serialized event size in bytes: [type, data, 0, 0].
pub const SERIALIZED_SIZE: usize = 4;

impl WmEvent {
    /// Serialize event to a 4-byte array: [type_tag, data, 0, 0].
    pub fn to_bytes(&self) -> [u8; SERIALIZED_SIZE] {
        match *self {
            WmEvent::KeyPress(ascii) => [1, ascii, 0, 0],
            WmEvent::KeyRelease(ascii) => [2, ascii, 0, 0],
            WmEvent::WindowClose => [3, 0, 0, 0],
            WmEvent::WindowFocus => [4, 0, 0, 0],
            WmEvent::WindowBlur => [5, 0, 0, 0],
        }
    }
}

// ── Event Queue ─────────────────────────────────────────────────────

/// Ring buffer size for per-window event queues.
const EVENT_QUEUE_SIZE: usize = 64;

/// Per-window event ring buffer (64 entries, FIFO).
pub struct WmEventQueue {
    buf: [Option<WmEvent>; EVENT_QUEUE_SIZE],
    read_idx: usize,
    write_idx: usize,
}

impl WmEventQueue {
    pub const fn new() -> Self {
        // const-init: 64 x None (manually, since [None; N] works for Copy types)
        Self {
            buf: [None; EVENT_QUEUE_SIZE],
            read_idx: 0,
            write_idx: 0,
        }
    }

    /// Push an event into the queue. Drops if full.
    pub fn push(&mut self, event: WmEvent) {
        let next_write = (self.write_idx + 1) % EVENT_QUEUE_SIZE;
        if next_write != self.read_idx {
            self.buf[self.write_idx] = Some(event);
            self.write_idx = next_write;
        }
        // else: queue full, drop the event
    }

    /// Pop the oldest event from the queue.
    pub fn pop(&mut self) -> Option<WmEvent> {
        if self.read_idx == self.write_idx {
            return None;
        }
        let event = self.buf[self.read_idx].take();
        self.read_idx = (self.read_idx + 1) % EVENT_QUEUE_SIZE;
        event
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.read_idx == self.write_idx
    }

    /// Number of events available.
    pub fn len(&self) -> usize {
        if self.write_idx >= self.read_idx {
            self.write_idx - self.read_idx
        } else {
            EVENT_QUEUE_SIZE - self.read_idx + self.write_idx
        }
    }
}
