//! Read-only monitor taps for the message bus.
//!
//! Monitors observe message traffic matching a filter without ability to inject.
//! Up to 4 taps can be registered, each with a 64-entry circular buffer.

#![allow(dead_code)]

use fabric_types::{ProcessId, TypeId};
use super::queue::Envelope;

/// Maximum number of registered monitor taps.
const MAX_MONITORS: usize = 4;

/// Maximum events buffered per monitor tap.
const MONITOR_BUFFER_SIZE: usize = 64;

/// Filter criteria for a monitor tap. None means "match all".
#[derive(Clone, Copy, Debug)]
pub struct MonitorFilter {
    pub sender:   Option<ProcessId>,
    pub receiver: Option<ProcessId>,
    pub msg_type: Option<TypeId>,
}

impl MonitorFilter {
    /// Check if an envelope matches this filter.
    pub fn matches(&self, env: &Envelope) -> bool {
        if let Some(s) = self.sender {
            if env.header.sender != s {
                return false;
            }
        }
        if let Some(r) = self.receiver {
            if env.header.receiver != r {
                return false;
            }
        }
        if let Some(t) = self.msg_type {
            if env.header.msg_type != t {
                return false;
            }
        }
        true
    }
}

/// A single monitor tap with its own circular buffer.
struct MonitorTap {
    id: u32,
    filter: MonitorFilter,
    buffer: [Option<Envelope>; MONITOR_BUFFER_SIZE],
    head: usize,
    tail: usize,
    count: usize,
}

impl MonitorTap {
    fn new(id: u32, filter: MonitorFilter) -> Self {
        const NONE: Option<Envelope> = None;
        Self {
            id,
            filter,
            buffer: [NONE; MONITOR_BUFFER_SIZE],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    fn push(&mut self, env: Envelope) {
        if self.count >= MONITOR_BUFFER_SIZE {
            // Overwrite oldest
            self.head = (self.head + 1) % MONITOR_BUFFER_SIZE;
            self.count -= 1;
        }
        self.buffer[self.tail] = Some(env);
        self.tail = (self.tail + 1) % MONITOR_BUFFER_SIZE;
        self.count += 1;
    }

    fn pop(&mut self) -> Option<Envelope> {
        if self.count == 0 {
            return None;
        }
        let env = self.buffer[self.head].take();
        self.head = (self.head + 1) % MONITOR_BUFFER_SIZE;
        self.count -= 1;
        env
    }

    fn pending(&self) -> usize {
        self.count
    }
}

/// Monitor tap registry.
pub struct MonitorRegistry {
    taps: [Option<MonitorTap>; MAX_MONITORS],
    next_id: u32,
}

impl MonitorRegistry {
    pub const fn new() -> Self {
        const NONE: Option<MonitorTap> = None;
        Self {
            taps: [NONE; MAX_MONITORS],
            next_id: 1,
        }
    }

    /// Register a new monitor tap with the given filter.
    /// Returns the tap ID on success, or None if limit reached.
    pub fn register(&mut self, filter: MonitorFilter) -> Option<u32> {
        for slot in self.taps.iter_mut() {
            if slot.is_none() {
                let id = self.next_id;
                self.next_id += 1;
                *slot = Some(MonitorTap::new(id, filter));
                return Some(id);
            }
        }
        None
    }

    /// Unregister a monitor tap by ID.
    pub fn unregister(&mut self, tap_id: u32) -> bool {
        for slot in self.taps.iter_mut() {
            if let Some(tap) = slot {
                if tap.id == tap_id {
                    *slot = None;
                    return true;
                }
            }
        }
        false
    }

    /// Notify all matching monitors of a message (copies envelope).
    pub fn notify(&mut self, envelope: &Envelope) {
        for slot in self.taps.iter_mut() {
            if let Some(tap) = slot {
                if tap.filter.matches(envelope) {
                    tap.push(*envelope);
                }
            }
        }
    }

    /// Drain one buffered event from a monitor tap.
    pub fn drain(&mut self, tap_id: u32) -> Option<Envelope> {
        for slot in self.taps.iter_mut() {
            if let Some(tap) = slot {
                if tap.id == tap_id {
                    return tap.pop();
                }
            }
        }
        None
    }

    /// Get the number of buffered events for a monitor.
    pub fn pending_count(&self, tap_id: u32) -> usize {
        for slot in &self.taps {
            if let Some(tap) = slot {
                if tap.id == tap_id {
                    return tap.pending();
                }
            }
        }
        0
    }

    /// Clear all taps.
    pub fn clear(&mut self) {
        for slot in self.taps.iter_mut() {
            *slot = None;
        }
        self.next_id = 1;
    }
}
