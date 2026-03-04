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

/// Syscall numbers for the Fabric OS kernel ABI.
/// Convention: RAX = syscall number, args in RDI, RSI, RDX, R10, R8, R9.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum SyscallNumber {
    /// Exit the current process. RDI = exit code.
    Exit   = 0,
    /// Yield the current time slice voluntarily.
    Yield  = 1,
    /// Write bytes to a handle. RDI = handle, RSI = buf_ptr, RDX = len.
    Write  = 2,
    /// Get current process ID. Returns PID in RAX.
    GetPid  = 3,
    /// Open a file by path. RDI = path_ptr, RSI = path_len, RDX = flags. Returns fd.
    Open    = 4,
    /// Read from a file descriptor. RDI = fd, RSI = buf_ptr, RDX = len. Returns bytes read.
    Read    = 5,
    /// Close a file descriptor. RDI = fd. Returns 0 on success.
    Close   = 6,
    /// Stat a file by path. RDI = path_ptr, RSI = path_len, RDX = stat_buf. Returns 0.
    Stat    = 7,
    /// Fstat an open fd. RDI = fd, RSI = stat_buf. Returns 0.
    Fstat   = 8,
    /// Read directory entries. RDI = fd, RSI = buf_ptr, RDX = len. Returns bytes read.
    Getdents = 9,
    /// Create a socket. RDI = type (1=stream,2=dgram), RSI = protocol (6=tcp,17=udp). Returns socket fd.
    Socket   = 10,
    /// Bind a socket. RDI = fd, RSI = addr (u32 IPv4), RDX = port (u16). Returns 0.
    Bind     = 11,
    /// Listen on a socket. RDI = fd. Returns 0.
    Listen   = 12,
    /// Accept a connection. RDI = fd. Returns new socket fd.
    Accept   = 13,
    /// Connect a socket. RDI = fd, RSI = addr (u32 IPv4), RDX = port (u16). Returns 0.
    Connect  = 14,
    /// Send data on socket. RDI = fd, RSI = buf_ptr, RDX = len. Returns bytes sent.
    Send     = 15,
    /// Receive data from socket. RDI = fd, RSI = buf_ptr, RDX = len. Returns bytes received.
    Recv     = 16,
    /// Shutdown a socket. RDI = fd. Returns 0.
    Shutdown = 17,
}

impl SyscallNumber {
    /// Convert from raw u64 syscall number.
    pub const fn from_u64(v: u64) -> Option<Self> {
        match v {
            0 => Some(Self::Exit),
            1 => Some(Self::Yield),
            2 => Some(Self::Write),
            3 => Some(Self::GetPid),
            4 => Some(Self::Open),
            5 => Some(Self::Read),
            6 => Some(Self::Close),
            7 => Some(Self::Stat),
            8 => Some(Self::Fstat),
            9 => Some(Self::Getdents),
            10 => Some(Self::Socket),
            11 => Some(Self::Bind),
            12 => Some(Self::Listen),
            13 => Some(Self::Accept),
            14 => Some(Self::Connect),
            15 => Some(Self::Send),
            16 => Some(Self::Recv),
            17 => Some(Self::Shutdown),
            _ => None,
        }
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
