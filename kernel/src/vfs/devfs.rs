//! devfs — Device filesystem providing /dev/null, /dev/zero, /dev/random.
//!
//! Devices are hardcoded character devices with simple read/write semantics:
//! - /dev/null:   write discards data, read returns EOF (0 bytes)
//! - /dev/zero:   read returns zeroes, write discards
//! - /dev/random: read returns pseudo-random bytes (LFSR), write discards

#![allow(dead_code)]

use super::inode::{InodeId, InodeType, InodeTable};

/// Device type identifiers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum DeviceType {
    Null   = 0,
    Zero   = 1,
    Random = 2,
}

/// The devfs filesystem instance.
pub struct Devfs {
    fs_id: u16,
    root_inode: InodeId,
    null_inode: InodeId,
    zero_inode: InodeId,
    random_inode: InodeId,
    /// LFSR state for /dev/random.
    lfsr_state: u64,
    initialized: bool,
}

impl Devfs {
    pub const fn new() -> Self {
        Self {
            fs_id: 0,
            root_inode: InodeId::INVALID,
            null_inode: InodeId::INVALID,
            zero_inode: InodeId::INVALID,
            random_inode: InodeId::INVALID,
            lfsr_state: 0xDEAD_BEEF_CAFE_1234, // Non-zero seed
            initialized: false,
        }
    }

    /// Initialize devfs: allocate inodes for null, zero, random.
    pub fn init(&mut self, fs_id: u16, root_inode: InodeId, inodes: &mut InodeTable) {
        self.fs_id = fs_id;
        self.root_inode = root_inode;

        // Allocate /dev/null inode
        self.null_inode = inodes.alloc(
            InodeType::CharDevice,
            fs_id,
            DeviceType::Null as u64,
        ).expect("[DEVFS] Failed to allocate /dev/null inode");

        // Allocate /dev/zero inode
        self.zero_inode = inodes.alloc(
            InodeType::CharDevice,
            fs_id,
            DeviceType::Zero as u64,
        ).expect("[DEVFS] Failed to allocate /dev/zero inode");

        // Allocate /dev/random inode
        self.random_inode = inodes.alloc(
            InodeType::CharDevice,
            fs_id,
            DeviceType::Random as u64,
        ).expect("[DEVFS] Failed to allocate /dev/random inode");

        self.initialized = true;
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub fn fs_id(&self) -> u16 {
        self.fs_id
    }

    pub fn root_inode(&self) -> InodeId {
        self.root_inode
    }

    pub fn null_inode(&self) -> InodeId {
        self.null_inode
    }

    pub fn zero_inode(&self) -> InodeId {
        self.zero_inode
    }

    pub fn random_inode(&self) -> InodeId {
        self.random_inode
    }

    /// Look up a device name. Returns the device's InodeId.
    pub fn lookup(&self, name: &[u8]) -> Option<InodeId> {
        match name {
            b"null" => Some(self.null_inode),
            b"zero" => Some(self.zero_inode),
            b"random" => Some(self.random_inode),
            _ => None,
        }
    }

    /// Get the device type for an inode.
    pub fn device_type(&self, inode_id: InodeId) -> Option<DeviceType> {
        if inode_id == self.null_inode {
            Some(DeviceType::Null)
        } else if inode_id == self.zero_inode {
            Some(DeviceType::Zero)
        } else if inode_id == self.random_inode {
            Some(DeviceType::Random)
        } else {
            None
        }
    }

    /// Read from a device into buf. Returns bytes read.
    pub fn read(&mut self, inode_id: InodeId, buf: &mut [u8]) -> usize {
        match self.device_type(inode_id) {
            Some(DeviceType::Null) => 0, // EOF
            Some(DeviceType::Zero) => {
                for byte in buf.iter_mut() {
                    *byte = 0;
                }
                buf.len()
            }
            Some(DeviceType::Random) => {
                for byte in buf.iter_mut() {
                    *byte = self.next_random_byte();
                }
                buf.len()
            }
            None => 0,
        }
    }

    /// Write to a device. Returns bytes "written" (always discarded).
    pub fn write(&self, inode_id: InodeId, len: usize) -> usize {
        match self.device_type(inode_id) {
            Some(DeviceType::Null) | Some(DeviceType::Zero) | Some(DeviceType::Random) => len,
            None => 0,
        }
    }

    /// Generate a pseudo-random byte using a 64-bit LFSR (Galois feedback).
    fn next_random_byte(&mut self) -> u8 {
        // 64-bit maximal-length LFSR taps: 64,63,61,60
        let bit = self.lfsr_state & 1;
        self.lfsr_state >>= 1;
        if bit != 0 {
            self.lfsr_state ^= 0xD800_0000_0000_0000;
        }
        (self.lfsr_state & 0xFF) as u8
    }

    /// List device names (for readdir on /dev).
    pub fn list_devices(&self) -> &[(&str, InodeId)] {
        // Can't return dynamic data easily without alloc in const context,
        // so we use a fixed approach in readdir
        &[]
    }

    /// Clear state (for testing/cleanup).
    pub fn clear(&mut self) {
        *self = Self::new();
    }
}
