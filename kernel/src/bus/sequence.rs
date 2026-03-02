//! Per-sender monotonic sequence number tracking.
//!
//! Each sender must present strictly sequential sequence numbers.
//! Gaps indicate message loss or attack; replays indicate duplication.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use fabric_types::ProcessId;

/// Tracks the last accepted sequence number per sender ProcessId.
pub struct SequenceTracker {
    last_seen: BTreeMap<u32, u64>,
}

impl SequenceTracker {
    pub const fn new() -> Self {
        Self {
            last_seen: BTreeMap::new(),
        }
    }

    /// Check and record a sequence number.
    ///
    /// Returns:
    /// - `Ok(())` if sequence is exactly `last_seen + 1` (or first message with seq 1)
    /// - `Err(true)` if sequence <= last_seen (replay)
    /// - `Err(false)` if sequence > last_seen + 1 (gap)
    pub fn check(&mut self, sender: ProcessId, sequence: u64) -> Result<(), SequenceError> {
        let expected = self.next_expected(sender);

        if sequence == expected {
            self.last_seen.insert(sender.0, sequence);
            Ok(())
        } else if sequence <= self.last_seen.get(&sender.0).copied().unwrap_or(0) {
            Err(SequenceError::Replay)
        } else {
            Err(SequenceError::Gap {
                expected,
                got: sequence,
            })
        }
    }

    /// Get the next expected sequence number for a sender.
    pub fn next_expected(&self, sender: ProcessId) -> u64 {
        self.last_seen
            .get(&sender.0)
            .map(|n| n + 1)
            .unwrap_or(1)
    }

    /// Remove tracking for a process.
    pub fn remove(&mut self, sender: ProcessId) {
        self.last_seen.remove(&sender.0);
    }

    /// Clear all tracking state.
    pub fn clear(&mut self) {
        self.last_seen.clear();
    }
}

/// Errors from sequence validation.
#[derive(Debug)]
pub enum SequenceError {
    /// Sequence number already seen (replay attack).
    Replay,
    /// Sequence number skipped (possible message loss or attack).
    Gap { expected: u64, got: u64 },
}
