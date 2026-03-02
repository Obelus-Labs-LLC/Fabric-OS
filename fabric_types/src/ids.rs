//! Core identity types for the Fabric OS wire protocol.
//!
//! All IDs are newtype wrappers providing type safety at zero cost.

#![allow(dead_code)]

use core::fmt;

/// Globally unique capability token identifier.
/// Sequential within a single kernel instance. Starts at 1; 0 is reserved (means "none").
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CapabilityId(pub u64);

/// Identifies a kernel-managed resource.
/// Encoding: upper 16 bits = resource kind, lower 48 bits = resource-specific ID.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ResourceId(pub u64);

/// Process identity placeholder. Phase 3 defines the full process lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ProcessId(pub u32);

/// Typed message identifier for IPC payload schema.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TypeId(pub u16);

/// Monotonic tick counter. No real-time clock yet; ticks are advanced by the kernel.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Timestamp(pub u64);

// --- Resource kind constants ---

impl ResourceId {
    pub const KIND_MEMORY: u64   = 0x0001_0000_0000_0000;
    pub const KIND_IPC:    u64   = 0x0002_0000_0000_0000;
    pub const KIND_DEVICE: u64   = 0x0003_0000_0000_0000;
    pub const KIND_FS:     u64   = 0x0004_0000_0000_0000;
    pub const KIND_NET:    u64   = 0x0005_0000_0000_0000;

    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn kind(self) -> u16 {
        (self.0 >> 48) as u16
    }

    pub const fn specific(self) -> u64 {
        self.0 & 0x0000_FFFF_FFFF_FFFF
    }
}

impl CapabilityId {
    pub const NONE: Self = Self(0);

    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn is_none(self) -> bool {
        self.0 == 0
    }
}

impl ProcessId {
    pub const KERNEL: Self = Self(0);

    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }
}

impl TypeId {
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }
}

impl Timestamp {
    pub const ZERO: Self = Self(0);

    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }
}

// --- Debug / Display impls ---

impl fmt::Debug for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CapId({})", self.0)
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cap:{}", self.0)
    }
}

impl fmt::Debug for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResId(0x{:016x})", self.0)
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "res:0x{:04x}:{:012x}", self.kind(), self.specific())
    }
}

impl fmt::Debug for ProcessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pid({})", self.0)
    }
}

impl fmt::Display for ProcessId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pid:{}", self.0)
    }
}

impl fmt::Debug for TypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TypeId({})", self.0)
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tick({})", self.0)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "t:{}", self.0)
    }
}
