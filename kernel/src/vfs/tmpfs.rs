//! tmpfs — RAM-backed filesystem for temporary files and directories.
//!
//! Data is stored in heap-allocated BTreeMaps. Each file's content is
//! a Vec<u8>, and each directory maintains a Vec of (name, InodeId) entries.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use super::inode::InodeId;

/// A directory entry: name → inode.
#[derive(Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub inode_id: InodeId,
}

/// Directory storage for tmpfs.
pub struct TmpfsDir {
    pub entries: Vec<DirEntry>,
}

impl TmpfsDir {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

/// File storage for tmpfs.
pub struct TmpfsFile {
    pub data: Vec<u8>,
}

impl TmpfsFile {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
        }
    }

    pub fn with_data(data: &[u8]) -> Self {
        Self {
            data: Vec::from(data),
        }
    }
}

/// The tmpfs filesystem instance.
pub struct Tmpfs {
    fs_id: u16,
    root_inode: InodeId,
    dirs: BTreeMap<u32, TmpfsDir>,
    files: BTreeMap<u32, TmpfsFile>,
    initialized: bool,
}

impl Tmpfs {
    pub const fn new() -> Self {
        Self {
            fs_id: 0,
            root_inode: InodeId::INVALID,
            dirs: BTreeMap::new(),
            files: BTreeMap::new(),
            initialized: false,
        }
    }

    /// Initialize with the assigned fs_id and root inode.
    pub fn init(&mut self, fs_id: u16, root_inode: InodeId) {
        self.fs_id = fs_id;
        self.root_inode = root_inode;
        self.dirs.insert(root_inode.0, TmpfsDir::new());
        self.initialized = true;
    }

    pub fn fs_id(&self) -> u16 {
        self.fs_id
    }

    pub fn root_inode(&self) -> InodeId {
        self.root_inode
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Register a new directory (just creates the empty dir entry list).
    pub fn register_dir(&mut self, inode_id: InodeId) {
        self.dirs.insert(inode_id.0, TmpfsDir::new());
    }

    /// Add a directory entry to a parent directory.
    pub fn add_dir_entry(&mut self, parent: InodeId, name: &[u8], child: InodeId) {
        if let Some(dir) = self.dirs.get_mut(&parent.0) {
            let name_str = core::str::from_utf8(name).unwrap_or("?");
            dir.entries.push(DirEntry {
                name: String::from(name_str),
                inode_id: child,
            });
        }
    }

    /// Create a file with initial data.
    pub fn create_file_with_data(&mut self, inode_id: InodeId, data: &[u8]) {
        self.files.insert(inode_id.0, TmpfsFile::with_data(data));
    }

    /// Look up a name in a directory. Returns the child InodeId.
    pub fn lookup(&self, dir_inode: InodeId, name: &[u8]) -> Option<InodeId> {
        let name_str = core::str::from_utf8(name).ok()?;
        if let Some(dir) = self.dirs.get(&dir_inode.0) {
            for entry in &dir.entries {
                if entry.name == name_str {
                    return Some(entry.inode_id);
                }
            }
        }
        None
    }

    /// Read file data starting at offset, up to buf_len bytes.
    /// Returns the number of bytes read.
    pub fn read(&self, inode_id: InodeId, offset: u64, buf: &mut [u8]) -> usize {
        if let Some(file) = self.files.get(&inode_id.0) {
            let offset = offset as usize;
            if offset >= file.data.len() {
                return 0; // EOF
            }
            let available = file.data.len() - offset;
            let to_read = buf.len().min(available);
            buf[..to_read].copy_from_slice(&file.data[offset..offset + to_read]);
            to_read
        } else {
            0
        }
    }

    /// Write data to a file at offset. Extends the file if needed.
    /// Returns the number of bytes written.
    pub fn write(&mut self, inode_id: InodeId, offset: u64, data: &[u8]) -> usize {
        let file = self.files.entry(inode_id.0).or_insert_with(TmpfsFile::new);
        let offset = offset as usize;

        // Extend file if needed
        if offset + data.len() > file.data.len() {
            file.data.resize(offset + data.len(), 0);
        }

        file.data[offset..offset + data.len()].copy_from_slice(data);
        data.len()
    }

    /// Get file size.
    pub fn file_size(&self, inode_id: InodeId) -> u64 {
        self.files
            .get(&inode_id.0)
            .map(|f| f.data.len() as u64)
            .unwrap_or(0)
    }

    /// List directory entries.
    pub fn readdir(&self, dir_inode: InodeId) -> Option<&[DirEntry]> {
        self.dirs.get(&dir_inode.0).map(|d| d.entries.as_slice())
    }

    /// Check if an inode is a known directory.
    pub fn is_dir(&self, inode_id: InodeId) -> bool {
        self.dirs.contains_key(&inode_id.0)
    }

    /// Check if an inode is a known file.
    pub fn is_file(&self, inode_id: InodeId) -> bool {
        self.files.contains_key(&inode_id.0)
    }

    /// Clear all data (for testing/cleanup).
    pub fn clear(&mut self) {
        self.dirs.clear();
        self.files.clear();
        self.initialized = false;
        self.fs_id = 0;
        self.root_inode = InodeId::INVALID;
    }
}
