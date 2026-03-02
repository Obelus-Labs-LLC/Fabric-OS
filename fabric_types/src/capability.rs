//! Capability token wire format — 64-byte cache-line aligned.
//!
//! See INTERFACE_CONTRACT.md for the authoritative wire format specification.

#![allow(dead_code)]

use core::fmt;
use crate::ids::{CapabilityId, ResourceId, ProcessId};

/// Permission bitflags (u16). 5 defined, 11 reserved for future phases.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Perm(pub u16);

impl Perm {
    pub const NONE:    Self = Self(0);
    pub const READ:    Self = Self(1 << 0);
    pub const WRITE:   Self = Self(1 << 1);
    pub const EXECUTE: Self = Self(1 << 2);
    pub const GRANT:   Self = Self(1 << 3);
    pub const REVOKE:  Self = Self(1 << 4);
    // Bits 5-15 reserved for future Estate agent permissions

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_subset_of(self, parent: Self) -> bool {
        self.0 & !parent.0 == 0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::ops::BitOr for Perm {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitAnd for Perm {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl fmt::Debug for Perm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Perm(")?;
        let mut first = true;
        let flags: [(&str, Perm); 5] = [
            ("R", Self::READ), ("W", Self::WRITE), ("X", Self::EXECUTE),
            ("G", Self::GRANT), ("V", Self::REVOKE),
        ];
        for &(name, p) in &flags {
            if self.contains(p) {
                if !first { write!(f, "|")?; }
                write!(f, "{}", name)?;
                first = false;
            }
        }
        if first { write!(f, "NONE")?; }
        write!(f, ")")
    }
}

/// Rate-limiting budget configuration.
/// Stored kernel-side (not in the wire token), but defined here for shared use.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub max_uses: u32,
    pub interval_ticks: u64,
}

/// Capability token — 64-byte cache-line-aligned wire format.
///
/// Layout: 40 bytes active fields + 24 bytes reserved (including extension_ptr).
/// HMAC is stored kernel-side, NOT in this struct.
#[derive(Clone, Copy)]
#[repr(C, align(64))]
pub struct CapabilityToken {
    // --- Active fields (40 bytes) ---
    pub version:        u16,        //  0..2
    pub permissions:    Perm,       //  2..4   (repr(transparent) u16)
    pub owner:          ProcessId,  //  4..8   (repr(transparent) u32)
    pub id:             CapabilityId, // 8..16 (repr(transparent) u64)
    pub resource:       ResourceId, // 16..24  (repr(transparent) u64)
    pub delegated_from: u64,        // 24..32  (0 = root token)
    pub nonce:          u32,        // 32..36
    pub expires:        u32,        // 36..40  (ticks from creation, 0 = never)

    // --- Reserved (24 bytes) ---
    pub extension_ptr:  u64,        // 40..48  (pointer to extended metadata, 0 = none)
    pub _reserved:      [u8; 16],   // 48..64  (future: budget params, Estate agent tags)
}

impl CapabilityToken {
    pub const VERSION: u16 = 1;

    /// Create a zeroed token (invalid until fields are set and signed).
    pub const fn zeroed() -> Self {
        Self {
            version: 0,
            permissions: Perm::NONE,
            owner: ProcessId(0),
            id: CapabilityId(0),
            resource: ResourceId(0),
            delegated_from: 0,
            nonce: 0,
            expires: 0,
            extension_ptr: 0,
            _reserved: [0u8; 16],
        }
    }

    /// Serialize the 40 active bytes into a fixed-size buffer for HMAC signing.
    /// The layout matches the `#[repr(C)]` struct layout exactly.
    pub fn active_bytes(&self) -> [u8; 40] {
        let mut buf = [0u8; 40];
        buf[0..2].copy_from_slice(&self.version.to_le_bytes());
        buf[2..4].copy_from_slice(&self.permissions.0.to_le_bytes());
        buf[4..8].copy_from_slice(&self.owner.0.to_le_bytes());
        buf[8..16].copy_from_slice(&self.id.0.to_le_bytes());
        buf[16..24].copy_from_slice(&self.resource.0.to_le_bytes());
        buf[24..32].copy_from_slice(&self.delegated_from.to_le_bytes());
        buf[32..36].copy_from_slice(&self.nonce.to_le_bytes());
        buf[36..40].copy_from_slice(&self.expires.to_le_bytes());
        buf
    }

    pub fn is_root(&self) -> bool {
        self.delegated_from == 0
    }
}

impl fmt::Debug for CapabilityToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapabilityToken")
            .field("v", &self.version)
            .field("id", &self.id)
            .field("resource", &self.resource)
            .field("owner", &self.owner)
            .field("perm", &self.permissions)
            .field("nonce", &self.nonce)
            .field("expires", &self.expires)
            .field("parent", &self.delegated_from)
            .finish()
    }
}

// Compile-time size assertion
const _: () = assert!(core::mem::size_of::<CapabilityToken>() == 64);
const _: () = assert!(core::mem::align_of::<CapabilityToken>() == 64);
