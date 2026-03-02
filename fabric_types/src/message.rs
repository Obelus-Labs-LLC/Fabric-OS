//! Message header wire format — 64-byte cache-line aligned.
//!
//! See INTERFACE_CONTRACT.md for the authoritative wire format specification.
//! Phase 2 (IPC Bus) implements the full message routing; this defines the header layout.

#![allow(dead_code)]

use core::fmt;
use crate::ids::{ProcessId, TypeId, Timestamp};

/// Message header — 64-byte cache-line-aligned wire format.
///
/// Layout: 40 bytes active fields + 24 bytes reserved (including extension_ptr).
/// Payload data and HMAC are stored in extension area (pointed to by extension_ptr).
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct MessageHeader {
    // --- Active fields (40 bytes) ---
    pub version:       u16,         //  0..2
    pub msg_type:      TypeId,      //  2..4   (repr(transparent) u16)
    pub sender:        ProcessId,   //  4..8   (repr(transparent) u32)
    pub receiver:      ProcessId,   //  8..12  (repr(transparent) u32)
    pub payload_len:   u32,         // 12..16  (byte length, 0 = no payload)
    pub capability_id: u64,         // 16..24  (reference into capability store)
    pub sequence:      u64,         // 24..32  (monotonic per-sender, gap = attack)
    pub timestamp:     Timestamp,   // 32..40  (repr(transparent) u64)

    // --- Reserved (24 bytes) ---
    pub extension_ptr: u64,         // 40..48  (pointer to payload + intent + HMAC)
    pub _reserved:     [u8; 16],    // 48..64  (future: intent_category, priority, energy_class)
}

impl MessageHeader {
    pub const VERSION: u16 = 1;

    pub const fn zeroed() -> Self {
        Self {
            version: 0,
            msg_type: TypeId(0),
            sender: ProcessId(0),
            receiver: ProcessId(0),
            payload_len: 0,
            capability_id: 0,
            sequence: 0,
            timestamp: Timestamp(0),
            extension_ptr: 0,
            _reserved: [0u8; 16],
        }
    }

    /// Serialize the 40 active bytes for HMAC computation.
    pub fn active_bytes(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..2].copy_from_slice(&self.version.to_le_bytes());
        buf[2..4].copy_from_slice(&self.msg_type.0.to_le_bytes());
        buf[4..8].copy_from_slice(&self.sender.0.to_le_bytes());
        buf[8..12].copy_from_slice(&self.receiver.0.to_le_bytes());
        buf[12..16].copy_from_slice(&self.payload_len.to_le_bytes());
        buf[16..24].copy_from_slice(&self.capability_id.to_le_bytes());
        buf[24..32].copy_from_slice(&self.sequence.to_le_bytes());
        buf[32..40].copy_from_slice(&self.timestamp.0.to_le_bytes());
        buf
    }

    pub fn has_payload(&self) -> bool {
        self.payload_len > 0
    }
}

impl fmt::Debug for MessageHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageHeader")
            .field("v", &self.version)
            .field("type", &self.msg_type)
            .field("sender", &self.sender)
            .field("receiver", &self.receiver)
            .field("cap", &self.capability_id)
            .field("seq", &self.sequence)
            .field("payload_len", &self.payload_len)
            .finish()
    }
}

// Compile-time size assertion
const _: () = assert!(core::mem::size_of::<MessageHeader>() == 64);
const _: () = assert!(core::mem::align_of::<MessageHeader>() == 64);
