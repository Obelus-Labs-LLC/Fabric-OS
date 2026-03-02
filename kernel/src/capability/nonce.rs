//! Nonce tracking for replay prevention.
//!
//! Each capability has a monotonic nonce sequence. Validation requires presenting
//! a nonce strictly greater than the last accepted value. This prevents replay
//! of old validate() calls.

#![allow(dead_code)]

use alloc::collections::BTreeMap;

/// Tracks the highest accepted nonce per capability ID.
pub struct NonceTracker {
    last_seen: BTreeMap<u64, u32>,
}

impl NonceTracker {
    pub const fn new() -> Self {
        Self {
            last_seen: BTreeMap::new(),
        }
    }

    /// Check if a nonce is valid (strictly greater than last seen) and advance.
    /// Returns true if accepted, false if replay detected.
    pub fn check_and_advance(&mut self, cap_id: u64, nonce: u32) -> bool {
        let entry = self.last_seen.entry(cap_id).or_insert(0);
        if nonce > *entry {
            *entry = nonce;
            true
        } else {
            false
        }
    }

    /// Get the next expected nonce for a capability.
    pub fn next_expected(&self, cap_id: u64) -> u32 {
        self.last_seen.get(&cap_id).map(|n| n + 1).unwrap_or(1)
    }

    /// Remove tracking for a revoked capability.
    pub fn remove(&mut self, cap_id: u64) {
        self.last_seen.remove(&cap_id);
    }

    /// Clear all tracking state.
    pub fn clear(&mut self) {
        self.last_seen.clear();
    }
}
