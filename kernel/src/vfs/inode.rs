//! Inode table — fixed-size slab for VFS inode management.
//!
//! Each inode represents a file, directory, or device in the virtual
//! filesystem. The inode table holds up to 1024 inodes with generation
//! counters to prevent stale references.

#![allow(dead_code)]

/// Maximum number of inodes in the system.
pub const MAX_INODES: usize = 1024;

/// Inode identifier. 0 is reserved as invalid/unallocated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InodeId(pub u32);

impl InodeId {
    pub const INVALID: Self = Self(0);

    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

/// Type of filesystem object an inode represents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum InodeType {
    File       = 0,
    Directory  = 1,
    CharDevice = 2,
}

/// A single inode entry in the inode table.
#[derive(Clone, Copy)]
pub struct Inode {
    /// Unique inode identifier.
    pub inode_id: InodeId,
    /// Type of this inode (file, directory, char device).
    pub inode_type: InodeType,
    /// Size in bytes (for files), 0 for dirs/devices.
    pub size: u64,
    /// Which mounted filesystem owns this inode.
    pub fs_id: u16,
    /// Filesystem-private data (index into fs-specific storage).
    pub fs_data: u64,
    /// Whether this slot is in use.
    pub active: bool,
    /// Generation counter for stale reference detection.
    pub generation: u16,
}

impl Inode {
    const fn empty() -> Self {
        Self {
            inode_id: InodeId::INVALID,
            inode_type: InodeType::File,
            size: 0,
            fs_id: 0,
            fs_data: 0,
            active: false,
            generation: 0,
        }
    }
}

/// Errors from inode operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InodeError {
    TableFull,
    NotFound,
    NotActive,
    StaleGeneration,
}

/// System-wide inode table. Fixed 1024-slot slab.
pub struct InodeTable {
    entries: [Inode; MAX_INODES],
    count: usize,
    next_id: u32,
}

impl InodeTable {
    pub const fn new() -> Self {
        Self {
            entries: [Inode::empty(); MAX_INODES],
            count: 0,
            next_id: 1, // 0 is reserved as INVALID
        }
    }

    /// Allocate a new inode. Returns the InodeId.
    pub fn alloc(&mut self, inode_type: InodeType, fs_id: u16, fs_data: u64) -> Result<InodeId, InodeError> {
        for i in 0..MAX_INODES {
            if !self.entries[i].active {
                let id = InodeId::new(self.next_id);
                self.next_id += 1;

                self.entries[i].inode_id = id;
                self.entries[i].inode_type = inode_type;
                self.entries[i].size = 0;
                self.entries[i].fs_id = fs_id;
                self.entries[i].fs_data = fs_data;
                self.entries[i].active = true;
                // generation already set from previous release (or 0 for fresh)
                self.count += 1;

                return Ok(id);
            }
        }
        Err(InodeError::TableFull)
    }

    /// Get an inode by ID (immutable).
    pub fn get(&self, id: InodeId) -> Option<&Inode> {
        for i in 0..MAX_INODES {
            if self.entries[i].active && self.entries[i].inode_id == id {
                return Some(&self.entries[i]);
            }
        }
        None
    }

    /// Get an inode by ID (mutable).
    pub fn get_mut(&mut self, id: InodeId) -> Option<&mut Inode> {
        for i in 0..MAX_INODES {
            if self.entries[i].active && self.entries[i].inode_id == id {
                return Some(&mut self.entries[i]);
            }
        }
        None
    }

    /// Release an inode, making the slot available for reuse.
    pub fn release(&mut self, id: InodeId) -> Result<(), InodeError> {
        for i in 0..MAX_INODES {
            if self.entries[i].active && self.entries[i].inode_id == id {
                self.entries[i].active = false;
                self.entries[i].generation = self.entries[i].generation.wrapping_add(1);
                self.count -= 1;
                return Ok(());
            }
        }
        Err(InodeError::NotFound)
    }

    /// Number of active inodes.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Clear all inodes (for testing/cleanup).
    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            *entry = Inode::empty();
        }
        self.count = 0;
        self.next_id = 1;
    }
}
