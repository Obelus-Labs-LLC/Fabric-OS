//! Hash-chained audit log for the message bus.
//!
//! Each entry's hash = SHA3-256(prev_hash || entry_active_bytes).
//! The chain is verifiable: any tampering breaks the hash linkage.

#![allow(dead_code)]

use sha3::{Sha3_256, Digest};
use fabric_types::{ProcessId, TypeId, Timestamp};
use fabric_types::audit::{AuditAction, AuditEntry};

/// Audit log ring buffer capacity.
const AUDIT_CAPACITY: usize = 512;

/// Hash-chained audit log.
pub struct AuditLog {
    entries: [Option<AuditEntry>; AUDIT_CAPACITY],
    write_pos: usize,
    total_count: u64,         // total entries ever written
    prev_hash: [u8; 32],     // hash of the most recent entry
}

impl AuditLog {
    pub const fn new() -> Self {
        const NONE: Option<AuditEntry> = None;
        Self {
            entries: [NONE; AUDIT_CAPACITY],
            write_pos: 0,
            total_count: 0,
            prev_hash: [0u8; 32],
        }
    }

    /// Append a new audit entry with automatic hash chaining.
    pub fn append(
        &mut self,
        actor: ProcessId,
        action: AuditAction,
        target: ProcessId,
        msg_type: TypeId,
        capability_id: u64,
        msg_sequence: u64,
        timestamp: Timestamp,
    ) {
        let mut entry = AuditEntry::zeroed();
        entry.sequence = self.total_count;
        entry.timestamp = timestamp;
        entry.actor = actor;
        entry.action = action;
        entry.target = target;
        entry.msg_type = msg_type;
        entry.capability_id = capability_id;
        entry.msg_sequence = msg_sequence;
        entry.prev_hash = self.prev_hash;

        // Compute hash: SHA3-256(hashable_bytes) where hashable_bytes includes prev_hash
        let hashable = entry.hashable_bytes();
        let mut hasher = Sha3_256::new();
        hasher.update(&hashable);
        // Also include the variable fields not in hashable_bytes
        hasher.update(&capability_id.to_le_bytes());
        hasher.update(&msg_sequence.to_le_bytes());
        let result = hasher.finalize();
        entry.hash.copy_from_slice(&result);

        self.prev_hash = entry.hash;
        self.entries[self.write_pos] = Some(entry);
        self.write_pos = (self.write_pos + 1) % AUDIT_CAPACITY;
        self.total_count += 1;
    }

    /// Verify the hash chain integrity for all entries in the ring.
    /// Returns (verified_count, is_valid).
    pub fn verify_chain(&self) -> (usize, bool) {
        if self.total_count == 0 {
            return (0, true);
        }

        let ring_count = self.ring_count();
        if ring_count == 0 {
            return (0, true);
        }

        // Find the oldest entry in the ring
        let start = if self.total_count <= AUDIT_CAPACITY as u64 {
            0
        } else {
            self.write_pos // oldest is at write_pos (about to be overwritten)
        };

        let mut verified = 0;
        let mut prev_hash = [0u8; 32];
        let mut first = true;

        for i in 0..ring_count {
            let idx = (start + i) % AUDIT_CAPACITY;
            if let Some(entry) = &self.entries[idx] {
                if first {
                    // First entry in the ring — we trust its prev_hash
                    prev_hash = entry.prev_hash;
                    first = false;
                }

                // Verify this entry's prev_hash matches what we expect
                if entry.prev_hash != prev_hash {
                    return (verified, false);
                }

                // Recompute the hash
                let hashable = entry.hashable_bytes();
                let mut hasher = Sha3_256::new();
                hasher.update(&hashable);
                hasher.update(&entry.capability_id.to_le_bytes());
                hasher.update(&entry.msg_sequence.to_le_bytes());
                let result = hasher.finalize();

                let mut expected = [0u8; 32];
                expected.copy_from_slice(&result);

                if entry.hash != expected {
                    return (verified, false);
                }

                prev_hash = entry.hash;
                verified += 1;
            }
        }

        (verified, true)
    }

    /// Total entries ever appended (including overwritten).
    pub fn total_count(&self) -> u64 {
        self.total_count
    }

    /// Entries currently in the ring.
    pub fn ring_count(&self) -> usize {
        if self.total_count <= AUDIT_CAPACITY as u64 {
            self.total_count as usize
        } else {
            AUDIT_CAPACITY
        }
    }

    /// Get a mutable reference to entries (for testing tamper detection).
    pub fn entries_mut(&mut self) -> &mut [Option<AuditEntry>; AUDIT_CAPACITY] {
        &mut self.entries
    }

    /// Get the write position (for testing).
    pub fn write_pos(&self) -> usize {
        self.write_pos
    }

    /// Clear all state.
    pub fn clear(&mut self) {
        for entry in self.entries.iter_mut() {
            *entry = None;
        }
        self.write_pos = 0;
        self.total_count = 0;
        self.prev_hash = [0u8; 32];
    }
}
