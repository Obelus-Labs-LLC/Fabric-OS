//! Process wire types for the Fabric OS scheduler.
//!
//! Shared between kernel and future userspace. Kernel-internal fields
//! (BehavioralProfile, supervision tree state) are NOT here.

#![allow(dead_code)]

use crate::ids::Timestamp;
use core::fmt;

/// Opaque handle index for capability access (u64, Wasm-compatible).
/// Encodes slot index (bits 0-7) and generation counter (bits 8-23).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct HandleId(pub u64);

impl HandleId {
    pub const INVALID: Self = Self(0xFFFF_FFFF_FFFF_FFFF);

    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Extract the slot index (bits 0-7).
    pub const fn slot(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// Extract the generation counter (bits 8-23).
    pub const fn generation(self) -> u16 {
        ((self.0 >> 8) & 0xFFFF) as u16
    }

    /// Pack a slot index and generation into a HandleId.
    pub const fn pack(slot: u8, generation: u16) -> Self {
        Self((slot as u64) | ((generation as u64) << 8))
    }
}

impl fmt::Debug for HandleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle(slot:{}, gen:{})", self.slot(), self.generation())
    }
}

impl fmt::Display for HandleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "h:{}:{}", self.slot(), self.generation())
    }
}

/// Intent category — what kind of work a process is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum IntentCategory {
    Compute = 0,
    Io      = 1,
    Network = 2,
    Storage = 3,
    Display = 4,
    Ai      = 5,
}

/// Process priority level. Ordered: Background < Low < Normal < High < Critical.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Priority {
    Background = 0,
    Low        = 1,
    Normal     = 2,
    High       = 3,
    Critical   = 4,
}

impl Priority {
    /// Convert from u8 (clamped to valid range).
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Background,
            1 => Self::Low,
            2 => Self::Normal,
            3 => Self::High,
            _ => Self::Critical,
        }
    }
}

/// Process lifecycle state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ProcessState {
    Ready      = 0,
    Running    = 1,
    Blocked    = 2,
    Suspended  = 3,
    Terminated = 4,
}

/// Energy class hint for the scheduler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EnergyClass {
    Battery     = 0,
    Balanced    = 1,
    Performance = 2,
}

/// Supervision strategy for a supervisor process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SupervisionStrategy {
    OneForOne  = 0,
    OneForAll  = 1,
    RestForOne = 2,
}

/// Compact intent descriptor — 16 bytes.
/// Human-readable description is stored kernel-side (heap-allocated String).
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Intent {
    pub category: IntentCategory,
    pub priority: Priority,
    pub energy_class: EnergyClass,
    pub _pad: u8,
    pub _reserved: u32,
    pub deadline: Timestamp,
}

impl Intent {
    pub const fn default() -> Self {
        Self {
            category: IntentCategory::Compute,
            priority: Priority::Normal,
            energy_class: EnergyClass::Balanced,
            _pad: 0,
            _reserved: 0,
            deadline: Timestamp::ZERO,
        }
    }
}
