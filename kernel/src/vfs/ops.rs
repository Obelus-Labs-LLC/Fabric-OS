//! VFS operations — dispatches file operations to the appropriate filesystem.
//!
//! These functions are called from syscall handlers. They handle path resolution,
//! mount traversal, and dispatch to tmpfs or devfs as appropriate.

#![allow(dead_code)]

use super::inode::{InodeId, InodeType};
use super::mount::FsType;
use super::open_file::{OpenFileId, OpenFlags};
use super::{INODES, MOUNTS, OPEN_FILES, TMPFS, DEVFS};

/// VFS operation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotFound,
    NotAFile,
    NotADirectory,
    PermissionDenied,
    InvalidPath,
    NoSpace,
    BadFileDescriptor,
    IoError,
}

/// Stat result structure (returned to userspace).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct StatBuf {
    pub inode_id: u32,
    pub inode_type: u8,  // 0=file, 1=dir, 2=chardev
    pub size: u64,
    pub fs_id: u16,
    pub _pad: [u8; 5],
}

impl StatBuf {
    pub const fn zeroed() -> Self {
        Self {
            inode_id: 0,
            inode_type: 0,
            size: 0,
            fs_id: 0,
            _pad: [0; 5],
        }
    }
}

/// Directory entry structure (returned by getdents).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DirentBuf {
    pub inode_id: u32,
    pub name_len: u16,
    pub inode_type: u8,
    pub _pad: u8,
    pub name: [u8; 56], // Fixed size for simplicity
}

impl DirentBuf {
    pub const SIZE: usize = core::mem::size_of::<Self>();
}

/// Resolve a path to an InodeId by traversing mount table and filesystem lookups.
pub fn resolve_path(path: &[u8]) -> Result<InodeId, VfsError> {
    if path.is_empty() || path[0] != b'/' {
        return Err(VfsError::InvalidPath);
    }

    // Find the mount point
    let mounts = MOUNTS.lock();
    let resolve = mounts.resolve(path).ok_or(VfsError::NotFound)?;
    let mount_entry = mounts.get(resolve.mount_index).ok_or(VfsError::NotFound)?;
    let fs_type = mount_entry.fs_type;
    let root_inode = mount_entry.root_inode;
    drop(mounts);

    // If no remaining path, return the mount root inode
    if resolve.remaining_len == 0 {
        return Ok(root_inode);
    }

    let remaining = &resolve.remaining[..resolve.remaining_len];

    // Look up in the appropriate filesystem
    match fs_type {
        FsType::Tmpfs => {
            // Walk path components through tmpfs directories
            let tmpfs = TMPFS.lock();
            let mut current = root_inode;

            for component in remaining.split(|&b| b == b'/') {
                if component.is_empty() {
                    continue;
                }
                current = tmpfs.lookup(current, component).ok_or(VfsError::NotFound)?;
            }

            Ok(current)
        }
        FsType::Devfs => {
            // devfs is flat: just look up the device name
            let devfs = DEVFS.lock();
            devfs.lookup(remaining).ok_or(VfsError::NotFound)
        }
    }
}

/// Open a file by path. Returns an OpenFileId.
pub fn vfs_open(path: &[u8], flags: OpenFlags) -> Result<OpenFileId, VfsError> {
    let inode_id = resolve_path(path)?;

    // Verify it's a file or device (not a directory for read/write)
    let inodes = INODES.lock();
    let inode = inodes.get(inode_id).ok_or(VfsError::NotFound)?;
    // Allow opening directories for getdents
    let _ = inode;
    drop(inodes);

    // Allocate an open file entry
    let mut open_files = OPEN_FILES.lock();
    let open_file_id = open_files.alloc(inode_id, flags)
        .map_err(|_| VfsError::NoSpace)?;

    Ok(open_file_id)
}

/// Read from an open file. Returns bytes read.
pub fn vfs_read(open_file_id: OpenFileId, buf: &mut [u8]) -> Result<usize, VfsError> {
    // Get the open file entry
    let open_files = OPEN_FILES.lock();
    let open_file = open_files.get(open_file_id).ok_or(VfsError::BadFileDescriptor)?;
    let inode_id = open_file.inode_id;
    let offset = open_file.offset;
    drop(open_files);

    // Get the inode to determine filesystem
    let inodes = INODES.lock();
    let inode = inodes.get(inode_id).ok_or(VfsError::NotFound)?;
    let fs_id = inode.fs_id;
    let inode_type = inode.inode_type;
    drop(inodes);

    // Dispatch to the appropriate filesystem
    let bytes_read = match inode_type {
        InodeType::File | InodeType::Directory => {
            let tmpfs = TMPFS.lock();
            tmpfs.read(inode_id, offset, buf)
        }
        InodeType::CharDevice => {
            let mut devfs = DEVFS.lock();
            if devfs.fs_id() == fs_id {
                devfs.read(inode_id, buf)
            } else {
                0
            }
        }
    };

    // Update offset
    let mut open_files = OPEN_FILES.lock();
    if let Some(of) = open_files.get_mut(open_file_id) {
        of.offset += bytes_read as u64;
    }

    Ok(bytes_read)
}

/// Write to an open file. Returns bytes written.
pub fn vfs_write(open_file_id: OpenFileId, data: &[u8]) -> Result<usize, VfsError> {
    // Get the open file entry
    let open_files = OPEN_FILES.lock();
    let open_file = open_files.get(open_file_id).ok_or(VfsError::BadFileDescriptor)?;
    let inode_id = open_file.inode_id;
    let offset = open_file.offset;
    let flags = open_file.flags;
    drop(open_files);

    // Get the inode
    let inodes = INODES.lock();
    let inode = inodes.get(inode_id).ok_or(VfsError::NotFound)?;
    let fs_id = inode.fs_id;
    let inode_type = inode.inode_type;
    drop(inodes);

    let bytes_written = match inode_type {
        InodeType::File => {
            let write_offset = if flags.contains(OpenFlags::APPEND) {
                let tmpfs = TMPFS.lock();
                tmpfs.file_size(inode_id)
            } else {
                offset
            };
            let mut tmpfs = TMPFS.lock();
            let written = tmpfs.write(inode_id, write_offset, data);

            // Update inode size
            let new_size = tmpfs.file_size(inode_id);
            drop(tmpfs);
            let mut inodes = INODES.lock();
            if let Some(inode) = inodes.get_mut(inode_id) {
                inode.size = new_size;
            }

            written
        }
        InodeType::CharDevice => {
            let devfs = DEVFS.lock();
            if devfs.fs_id() == fs_id {
                devfs.write(inode_id, data.len())
            } else {
                0
            }
        }
        InodeType::Directory => return Err(VfsError::NotAFile),
    };

    // Update offset
    let mut open_files = OPEN_FILES.lock();
    if let Some(of) = open_files.get_mut(open_file_id) {
        of.offset += bytes_written as u64;
    }

    Ok(bytes_written)
}

/// Close an open file.
pub fn vfs_close(open_file_id: OpenFileId) -> Result<(), VfsError> {
    let mut open_files = OPEN_FILES.lock();
    open_files.release(open_file_id).map_err(|_| VfsError::BadFileDescriptor)
}

/// Stat a file by path.
pub fn vfs_stat(path: &[u8]) -> Result<StatBuf, VfsError> {
    let inode_id = resolve_path(path)?;
    vfs_stat_inode(inode_id)
}

/// Stat by inode (used for both stat and fstat).
pub fn vfs_stat_inode(inode_id: InodeId) -> Result<StatBuf, VfsError> {
    let inodes = INODES.lock();
    let inode = inodes.get(inode_id).ok_or(VfsError::NotFound)?;

    Ok(StatBuf {
        inode_id: inode.inode_id.0,
        inode_type: inode.inode_type as u8,
        size: inode.size,
        fs_id: inode.fs_id,
        _pad: [0; 5],
    })
}

/// Fstat an open file descriptor.
pub fn vfs_fstat(open_file_id: OpenFileId) -> Result<StatBuf, VfsError> {
    let open_files = OPEN_FILES.lock();
    let open_file = open_files.get(open_file_id).ok_or(VfsError::BadFileDescriptor)?;
    let inode_id = open_file.inode_id;
    drop(open_files);

    vfs_stat_inode(inode_id)
}

/// Read directory entries from an open directory.
pub fn vfs_readdir(open_file_id: OpenFileId, buf: &mut [u8]) -> Result<usize, VfsError> {
    // Get the open file entry
    let open_files = OPEN_FILES.lock();
    let open_file = open_files.get(open_file_id).ok_or(VfsError::BadFileDescriptor)?;
    let inode_id = open_file.inode_id;
    let offset = open_file.offset as usize;
    drop(open_files);

    // Verify it's a directory
    let inodes = INODES.lock();
    let inode = inodes.get(inode_id).ok_or(VfsError::NotFound)?;
    if inode.inode_type != InodeType::Directory {
        return Err(VfsError::NotADirectory);
    }
    let fs_id = inode.fs_id;
    drop(inodes);

    // Get directory entries from the filesystem
    let tmpfs = TMPFS.lock();
    let entries = match tmpfs.readdir(inode_id) {
        Some(entries) => entries,
        None => {
            // Try devfs
            drop(tmpfs);
            // For devfs, we need to build the directory listing manually
            let devfs = DEVFS.lock();
            if devfs.fs_id() == fs_id {
                // Build devfs directory entries
                let dev_names: [(&str, InodeId); 3] = [
                    ("null", devfs.null_inode()),
                    ("zero", devfs.zero_inode()),
                    ("random", devfs.random_inode()),
                ];
                drop(devfs);

                let entry_size = DirentBuf::SIZE;
                let mut bytes_written = 0;
                let mut entry_index = 0;

                for (name, inode_id) in &dev_names {
                    if entry_index < offset {
                        entry_index += 1;
                        continue;
                    }
                    if bytes_written + entry_size > buf.len() {
                        break;
                    }

                    let mut dirent = DirentBuf {
                        inode_id: inode_id.0,
                        name_len: name.len() as u16,
                        inode_type: InodeType::CharDevice as u8,
                        _pad: 0,
                        name: [0; 56],
                    };
                    let name_bytes = name.as_bytes();
                    let copy_len = name_bytes.len().min(56);
                    dirent.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

                    let dirent_bytes = unsafe {
                        core::slice::from_raw_parts(
                            &dirent as *const DirentBuf as *const u8,
                            entry_size,
                        )
                    };
                    buf[bytes_written..bytes_written + entry_size].copy_from_slice(dirent_bytes);
                    bytes_written += entry_size;
                    entry_index += 1;
                }

                // Update offset
                let mut open_files = OPEN_FILES.lock();
                if let Some(of) = open_files.get_mut(open_file_id) {
                    of.offset = entry_index as u64;
                }

                return Ok(bytes_written);
            }
            return Err(VfsError::NotFound);
        }
    };

    let entry_size = DirentBuf::SIZE;
    let mut bytes_written = 0;
    let mut entry_index = 0;

    for entry in entries {
        if entry_index < offset {
            entry_index += 1;
            continue;
        }
        if bytes_written + entry_size > buf.len() {
            break;
        }

        // Look up inode type
        let inodes = INODES.lock();
        let itype = inodes.get(entry.inode_id)
            .map(|i| i.inode_type as u8)
            .unwrap_or(0);
        drop(inodes);

        let mut dirent = DirentBuf {
            inode_id: entry.inode_id.0,
            name_len: entry.name.len() as u16,
            inode_type: itype,
            _pad: 0,
            name: [0; 56],
        };
        let name_bytes = entry.name.as_bytes();
        let copy_len = name_bytes.len().min(56);
        dirent.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);

        let dirent_bytes = unsafe {
            core::slice::from_raw_parts(
                &dirent as *const DirentBuf as *const u8,
                entry_size,
            )
        };
        buf[bytes_written..bytes_written + entry_size].copy_from_slice(dirent_bytes);
        bytes_written += entry_size;
        entry_index += 1;
    }

    drop(tmpfs);

    // Update offset
    let mut open_files = OPEN_FILES.lock();
    if let Some(of) = open_files.get_mut(open_file_id) {
        of.offset = entry_index as u64;
    }

    Ok(bytes_written)
}
