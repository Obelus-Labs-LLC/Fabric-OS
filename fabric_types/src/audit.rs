//! Audit log entry wire format — hash-chained for tamper detection.
//!
//! Each AuditEntry records a bus event (send, deliver, reject, etc.) and
//! includes a SHA-3-256 hash of itself chained to the previous entry's hash.
//! This creates an immutable, verifiable audit trail.

#![allow(dead_code)]

use crate::ids::{ProcessId, TypeId, Timestamp};

/// Action types for audit log entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AuditAction {
    MessageSent       = 1,
    MessageDelivered  = 2,
    MessageRejected   = 3,
    CapValidated      = 4,
    CapDenied         = 5,
    SequenceViolation = 6,
    HmacFailure       = 7,
    QueueFull         = 8,
    MonitorNotify     = 9,
    PolicyViolation   = 10,
}

/// Hash-chained audit log entry.
///
/// Each entry's `hash` = SHA3-256(prev_hash || active_bytes_of_this_entry).
/// The kernel computes the hash; wire format carries both prev_hash and hash
/// so verifiers can check the chain without re-hashing from genesis.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct AuditEntry {
    pub sequence:      u64,        // Monotonic within audit log
    pub timestamp:     Timestamp,  // Tick at time of event
    pub actor:         ProcessId,  // Who caused this event
    pub action:        AuditAction,// What happened
    pub _pad:          [u8; 3],    // Alignment padding
    pub target:        ProcessId,  // Receiver or affected party
    pub msg_type:      TypeId,     // Message type (if applicable)
    pub _pad2:         [u8; 2],    // Alignment padding
    pub capability_id: u64,        // Capability used (if applicable)
    pub msg_sequence:  u64,        // Message sequence number (if applicable)
    pub prev_hash:     [u8; 32],   // SHA3-256 of previous entry
    pub hash:          [u8; 32],   // SHA3-256 of this entry (including prev_hash)
}

impl AuditEntry {
    pub const fn zeroed() -> Self {
        Self {
            sequence: 0,
            timestamp: Timestamp(0),
            actor: ProcessId(0),
            action: AuditAction::MessageSent,
            _pad: [0; 3],
            target: ProcessId(0),
            msg_type: TypeId(0),
            _pad2: [0; 2],
            capability_id: 0,
            msg_sequence: 0,
            prev_hash: [0u8; 32],
            hash: [0u8; 32],
        }
    }

    /// Serialize the fields (excluding hash) into bytes for hashing.
    /// This includes prev_hash so the chain is cryptographically linked.
    pub fn hashable_bytes(&self) -> [u8; 64] {
        let mut buf = [0u8; 64];
        buf[0..8].copy_from_slice(&self.sequence.to_le_bytes());
        buf[8..16].copy_from_slice(&self.timestamp.0.to_le_bytes());
        buf[16..20].copy_from_slice(&self.actor.0.to_le_bytes());
        buf[20] = self.action as u8;
        buf[21..24].copy_from_slice(&[0u8; 3]); // pad
        buf[24..28].copy_from_slice(&self.target.0.to_le_bytes());
        buf[28..30].copy_from_slice(&self.msg_type.0.to_le_bytes());
        buf[30..32].copy_from_slice(&[0u8; 2]); // pad
        // prev_hash is included so chain is linked
        buf[32..64].copy_from_slice(&self.prev_hash);
        buf
    }
}

impl core::fmt::Debug for AuditEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AuditEntry")
            .field("seq", &self.sequence)
            .field("action", &self.action)
            .field("actor", &self.actor)
            .field("target", &self.target)
            .finish()
    }
}
