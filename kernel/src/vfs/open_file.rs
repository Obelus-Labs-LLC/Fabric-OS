//! Open file table — system-wide table tracking all open file descriptors.
//!
//! Each open file entry tracks the inode, current offset, and access flags.
//! Per-process file descriptors are mapped through the HandleTable in the PCB.

#![allow(dead_code)]

use super::inode::InodeId;

/// Maximum number of system-wide open files.
pub const MAX_OPEN_FILES: usize = 4096;

/// Open file identifier. 0 is reserved as invalid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpenFileId(pub u32);

impl OpenFileId {
    pub const INVALID: Self = Self(0);

    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

/// File open flags (bitflags).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpenFlags(pub u32);

impl OpenFlags {
    pub const NONE:   Self = Self(0);
    pub const READ:   Self = Self(1);
    pub const WRITE:  Self = Self(2);
    pub const APPEND: Self = Self(4);
    pub const CREATE: Self = Self(8);

    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 != 0
    }

    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

impl core::ops::BitOr for OpenFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// A single open file entry.
#[derive(Clone, Copy)]
pub struct OpenFile {
    /// The inode this file refers to.
    pub inode_id: InodeId,
    /// Current read/write offset.
    pub offset: u64,
    /// Access flags.
    pub flags: OpenFlags,
    /// Reference count (for future dup/fork; always 1 in Phase 8).
    pub ref_count: u16,
    /// Whether this slot is in use.
    pub active: bool,
}

impl OpenFile {
    const fn empty() -> Self {
        Self {
            inode_id: InodeId::INVALID,
            offset: 0,
            flags: OpenFlags::NONE,
            ref_count: 0,
            active: false,
        }
    }
}

/// Errors from open file operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFileError {
    TableFull,
    NotFound,
    NotActive,
}

/// System-wide open file table. Fixed 4096-slot slab.
pub struct OpenFileTable {
    entries: [OpenFile; MAX_OPEN_FILES],
    count: usize,
}

impl OpenFileTable {
    pub const fn new() -> Self {
        Self {
            entries: [OpenFile::empty(); MAX_OPEN_FILES],
            count: 0,
        }
    }

    /// Allocate an open file entry. Returns the OpenFileId (1-based index).
    pub fn alloc(
        &mut self,
        inode_id: InodeId,
        flags: OpenFlags,
    ) -> Result<OpenFileId, OpenFileError> {
        // Start from index 1 (0 reserved as INVALID)
        for i in 1..MAX_OPEN_FILES {
            if !self.entries[i].active {
                self.entries[i].inode_id = inode_id;
                self.entries[i].offset = 0;
                self.entries[i].flags = flags;
                self.entries[i].ref_count = 1;
                self.entries[i].active = true;
                self.count += 1;

                return Ok(OpenFileId::new(i as u32));
            }
        }
        Err(OpenFileError::TableFull)
    }

    /// Get an open file entry by ID (immutable).
    pub fn get(&self, id: OpenFileId) -> Option<&OpenFile> {
        let idx = id.0 as usize;
        if idx < MAX_OPEN_FILES && self.entries[idx].active {
            Some(&self.entries[idx])
        } else {
            None
        }
    }

    /// Get an open file entry by ID (mutable).
    pub fn get_mut(&mut self, id: OpenFileId) -> Option<&mut OpenFile> {
        let idx = id.0 as usize;
        if idx < MAX_OPEN_FILES && self.entries[idx].active {
            Some(&mut self.entries[idx])
        } else {
            None
        }
    }

    /// Release an open file entry.
    pub fn release(&mut self, id: OpenFileId) -> Result<(), OpenFileError> {
        let idx = id.0 as usize;
        if idx >= MAX_OPEN_FILES {
            return Err(OpenFileError::NotFound);
        }
        if !self.entries[idx].active {
            return Err(OpenFileError::NotActive);
        }

        self.entries[idx].ref_count -= 1;
        if self.entries[idx].ref_count == 0 {
            self.entries[idx].active = false;
            self.entries[idx].inode_id = InodeId::INVALID;
            self.count -= 1;
        }

        Ok(())
    }

    /// Number of active open files.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Clear all entries (for testing/cleanup).
    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            *entry = OpenFile::empty();
        }
        self.count = 0;
    }
}
