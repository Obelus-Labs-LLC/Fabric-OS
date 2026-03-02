//! Device and driver wire types for the Fabric OS HAL.
//!
//! Shared between kernel and simulated userspace drivers.
//! All types are `repr(C)` or `repr(u8)` for stable layout.

#![allow(dead_code)]

/// Identifies a device class for HAL dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DeviceClass {
    Serial       = 0,
    Timer        = 1,
    BlockStorage = 2,
    Framebuffer  = 3,
}

/// Operations a driver can handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DriverOp {
    Init      = 0,
    Read      = 1,
    Write     = 2,
    Status    = 3,
    Interrupt = 4,
    Shutdown  = 5,
}

/// Status returned by driver operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DriverStatus {
    Ok              = 0,
    Error           = 1,
    DeviceNotReady  = 2,
    InvalidRequest  = 3,
    PermissionDenied = 4,
}

/// Compact driver request descriptor — fits in bus payload.
/// 32 bytes total with explicit padding.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct DriverRequest {
    pub operation:    DriverOp,    // 1 byte
    pub device_class: DeviceClass, // 1 byte
    pub _pad:         [u8; 2],     // 2 bytes alignment
    pub offset:       u32,         // 4 bytes — block/register offset
    pub length:       u32,         // 4 bytes — byte length of data
    pub flags:        u32,         // 4 bytes — reserved
    pub _reserved:    [u8; 16],    // 16 bytes padding to 32
}

impl DriverRequest {
    pub const SIZE: usize = 32;

    pub const fn zeroed() -> Self {
        Self {
            operation: DriverOp::Status,
            device_class: DeviceClass::Serial,
            _pad: [0; 2],
            offset: 0,
            length: 0,
            flags: 0,
            _reserved: [0; 16],
        }
    }

    /// Serialize to bytes (safe transmute for repr(C) type).
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        unsafe { core::mem::transmute_copy(self) }
    }

    /// Deserialize from bytes. Returns None if slice is too short.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let mut buf = [0u8; Self::SIZE];
        buf.copy_from_slice(&bytes[..Self::SIZE]);
        Some(unsafe { core::mem::transmute(buf) })
    }
}

/// Driver response descriptor — fits in bus payload.
/// 16 bytes total with explicit padding.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct DriverResponse {
    pub status:     DriverStatus, // 1 byte
    pub _pad:       [u8; 3],      // 3 bytes alignment
    pub bytes_xfer: u32,          // 4 bytes — bytes transferred
    pub _reserved:  [u8; 8],      // 8 bytes padding to 16
}

impl DriverResponse {
    pub const SIZE: usize = 16;

    pub const fn ok(bytes_xfer: u32) -> Self {
        Self {
            status: DriverStatus::Ok,
            _pad: [0; 3],
            bytes_xfer,
            _reserved: [0; 8],
        }
    }

    pub const fn error(status: DriverStatus) -> Self {
        Self {
            status,
            _pad: [0; 3],
            bytes_xfer: 0,
            _reserved: [0; 8],
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        unsafe { core::mem::transmute_copy(self) }
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let mut buf = [0u8; Self::SIZE];
        buf.copy_from_slice(&bytes[..Self::SIZE]);
        Some(unsafe { core::mem::transmute(buf) })
    }
}
