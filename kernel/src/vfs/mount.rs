//! Mount table — manages filesystem mount points and path resolution.
//!
//! The mount table maps directory paths to mounted filesystems.
//! Path resolution uses longest-prefix matching to find the correct
//! mount point for any given path.

#![allow(dead_code)]

use super::inode::InodeId;

/// Maximum number of simultaneous mounts.
pub const MAX_MOUNTS: usize = 64;

/// Maximum path length for a mount point.
pub const MAX_MOUNT_PATH: usize = 64;

/// Filesystem type identifier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum FsType {
    Tmpfs  = 0,
    Devfs  = 1,
}

/// A single mount table entry.
#[derive(Clone, Copy)]
pub struct MountEntry {
    /// Mount point path (e.g., "/", "/dev").
    pub mount_path: [u8; MAX_MOUNT_PATH],
    /// Length of the mount path.
    pub path_len: usize,
    /// Unique filesystem ID (assigned at mount time).
    pub fs_id: u16,
    /// Type of filesystem mounted here.
    pub fs_type: FsType,
    /// Root inode of the mounted filesystem.
    pub root_inode: InodeId,
    /// Whether this slot is in use.
    pub active: bool,
}

impl MountEntry {
    const fn empty() -> Self {
        Self {
            mount_path: [0; MAX_MOUNT_PATH],
            path_len: 0,
            fs_id: 0,
            fs_type: FsType::Tmpfs,
            root_inode: InodeId::INVALID,
            active: false,
        }
    }

    /// Get the mount path as a byte slice.
    pub fn path(&self) -> &[u8] {
        &self.mount_path[..self.path_len]
    }
}

/// Errors from mount operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountError {
    TableFull,
    PathTooLong,
    NotFound,
}

/// Result of path resolution: the matched mount entry index and remaining path.
pub struct ResolveResult {
    /// Index into the mount table.
    pub mount_index: usize,
    /// The remaining path after stripping the mount prefix.
    /// For "/dev/null" with mount at "/dev", this is "null".
    pub remaining: [u8; MAX_MOUNT_PATH],
    pub remaining_len: usize,
}

/// System-wide mount table. Fixed 64-slot slab.
pub struct MountTable {
    entries: [MountEntry; MAX_MOUNTS],
    count: usize,
    next_fs_id: u16,
}

impl MountTable {
    pub const fn new() -> Self {
        Self {
            entries: [MountEntry::empty(); MAX_MOUNTS],
            count: 0,
            next_fs_id: 1,
        }
    }

    /// Mount a filesystem at the given path. Returns the assigned fs_id.
    pub fn mount(
        &mut self,
        path: &[u8],
        fs_type: FsType,
        root_inode: InodeId,
    ) -> Result<u16, MountError> {
        if path.len() > MAX_MOUNT_PATH {
            return Err(MountError::PathTooLong);
        }

        for i in 0..MAX_MOUNTS {
            if !self.entries[i].active {
                let fs_id = self.next_fs_id;
                self.next_fs_id += 1;

                self.entries[i].mount_path[..path.len()].copy_from_slice(path);
                self.entries[i].path_len = path.len();
                self.entries[i].fs_id = fs_id;
                self.entries[i].fs_type = fs_type;
                self.entries[i].root_inode = root_inode;
                self.entries[i].active = true;
                self.count += 1;

                return Ok(fs_id);
            }
        }
        Err(MountError::TableFull)
    }

    /// Resolve a path to a mount entry using longest-prefix matching.
    pub fn resolve(&self, path: &[u8]) -> Option<ResolveResult> {
        let mut best_index: Option<usize> = None;
        let mut best_len: usize = 0;

        for i in 0..MAX_MOUNTS {
            if !self.entries[i].active {
                continue;
            }

            let mount_path = self.entries[i].path();

            // Check if the path starts with this mount point
            if path.len() >= mount_path.len() && &path[..mount_path.len()] == mount_path {
                // For non-root mounts, ensure we match at a path boundary
                if mount_path.len() == 1 && mount_path[0] == b'/' {
                    // Root mount always matches
                    if mount_path.len() > best_len {
                        best_len = mount_path.len();
                        best_index = Some(i);
                    }
                } else if path.len() == mount_path.len()
                    || path[mount_path.len()] == b'/'
                {
                    if mount_path.len() > best_len {
                        best_len = mount_path.len();
                        best_index = Some(i);
                    }
                }
            }
        }

        best_index.map(|idx| {
            let mount_path_len = self.entries[idx].path_len;
            let mut remaining = [0u8; MAX_MOUNT_PATH];
            let remaining_len;

            if path.len() <= mount_path_len {
                remaining_len = 0;
            } else {
                // Strip mount prefix and leading slash
                let start = if path[mount_path_len] == b'/' {
                    mount_path_len + 1
                } else {
                    mount_path_len
                };
                remaining_len = path.len() - start;
                if remaining_len > 0 {
                    remaining[..remaining_len].copy_from_slice(&path[start..]);
                }
            }

            ResolveResult {
                mount_index: idx,
                remaining,
                remaining_len,
            }
        })
    }

    /// Get a mount entry by index.
    pub fn get(&self, index: usize) -> Option<&MountEntry> {
        if index < MAX_MOUNTS && self.entries[index].active {
            Some(&self.entries[index])
        } else {
            None
        }
    }

    /// Get a mount entry by fs_id.
    pub fn get_by_fs_id(&self, fs_id: u16) -> Option<&MountEntry> {
        for i in 0..MAX_MOUNTS {
            if self.entries[i].active && self.entries[i].fs_id == fs_id {
                return Some(&self.entries[i]);
            }
        }
        None
    }

    /// Number of active mounts.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Clear all mounts (for testing/cleanup).
    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            *entry = MountEntry::empty();
        }
        self.count = 0;
        self.next_fs_id = 1;
    }
}
