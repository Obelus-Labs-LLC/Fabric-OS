//! Virtual Filesystem Layer — Phase 8 of Fabric OS.
//!
//! Provides a unified file abstraction over multiple filesystem types:
//! - tmpfs: RAM-backed files and directories
//! - devfs: Device files (/dev/null, /dev/zero, /dev/random)
//!
//! The VFS manages inodes, mount points, and open file descriptors.
//! File descriptors are mapped through the per-process HandleTable.

#![allow(dead_code)]

pub mod inode;
pub mod mount;
pub mod open_file;
pub mod tmpfs;
pub mod devfs;
pub mod cpio;
pub mod ops;
pub mod stdio;

use crate::sync::OrderedMutex;
use crate::serial_println;

use inode::InodeTable;
use mount::MountTable;
use open_file::OpenFileTable;
use tmpfs::Tmpfs;
use devfs::Devfs;

/// Global inode table.
pub static INODES: OrderedMutex<InodeTable, { crate::sync::levels::VFS }> =
    OrderedMutex::new(InodeTable::new());

/// Global mount table.
pub static MOUNTS: OrderedMutex<MountTable, { crate::sync::levels::VFS }> =
    OrderedMutex::new(MountTable::new());

/// Global open file table.
pub static OPEN_FILES: OrderedMutex<OpenFileTable, { crate::sync::levels::VFS }> =
    OrderedMutex::new(OpenFileTable::new());

/// Global tmpfs instance.
pub static TMPFS: OrderedMutex<Tmpfs, { crate::sync::levels::VFS }> =
    OrderedMutex::new(Tmpfs::new());

/// Global devfs instance.
pub static DEVFS: OrderedMutex<Devfs, { crate::sync::levels::VFS }> =
    OrderedMutex::new(Devfs::new());

/// Initialize the VFS subsystem:
/// 1. Mount tmpfs at "/"
/// 2. Mount devfs at "/dev"
/// 3. Create /dev device files
pub fn init() {
    // Mount tmpfs at root "/"
    let tmpfs_root = {
        let mut inodes = INODES.lock();
        let root_inode = inodes.alloc(
            inode::InodeType::Directory,
            0, // fs_id will be set after mount
            0, // fs_data
        ).expect("[VFS] Failed to allocate tmpfs root inode");

        let mut mounts = MOUNTS.lock();
        let fs_id = mounts.mount(b"/", mount::FsType::Tmpfs, root_inode)
            .expect("[VFS] Failed to mount tmpfs at /");

        // Update inode's fs_id
        if let Some(inode) = inodes.get_mut(root_inode) {
            inode.fs_id = fs_id;
        }

        // Initialize tmpfs with its root inode
        let mut tmpfs = TMPFS.lock();
        tmpfs.init(fs_id, root_inode);

        serial_println!("[VFS] Mounted tmpfs at / (fs_id={})", fs_id);
        root_inode
    };

    // Mount devfs at "/dev"
    {
        // First create a /dev directory in tmpfs
        let dev_dir_inode = {
            let mut inodes = INODES.lock();
            let dev_inode = inodes.alloc(
                inode::InodeType::Directory,
                0, // will be updated
                0,
            ).expect("[VFS] Failed to allocate /dev directory inode");

            // Register /dev in tmpfs root directory
            let mut tmpfs = TMPFS.lock();
            tmpfs.add_dir_entry(tmpfs_root, b"dev", dev_inode);

            dev_inode
        };

        // Mount devfs over /dev
        let mut mounts = MOUNTS.lock();
        let fs_id = mounts.mount(b"/dev", mount::FsType::Devfs, dev_dir_inode)
            .expect("[VFS] Failed to mount devfs at /dev");

        // Initialize devfs and create device inodes
        let mut inodes = INODES.lock();
        let mut devfs = DEVFS.lock();
        devfs.init(fs_id, dev_dir_inode, &mut inodes);

        serial_println!("[VFS] Mounted devfs at /dev (fs_id={})", fs_id);
    }

    serial_println!("[VFS] Virtual filesystem initialized");
}

/// Load an initramfs CPIO archive into tmpfs.
pub fn load_initramfs(archive: &[u8]) {
    let entries = match cpio::parse_cpio(archive) {
        Ok(entries) => entries,
        Err(e) => {
            serial_println!("[VFS] Failed to parse initramfs CPIO: {:?}", e);
            return;
        }
    };

    serial_println!("[VFS] Initramfs: {} entries found", entries.len());

    let mut inodes = INODES.lock();
    let mut tmpfs = TMPFS.lock();

    for entry in &entries {
        let path = &entry.name;
        if path.is_empty() || path == b"." || path == b"./" {
            continue;
        }

        // Strip leading "./" if present
        let clean_path = if path.starts_with(b"./") {
            &path[2..]
        } else {
            path
        };

        if clean_path.is_empty() {
            continue;
        }

        if entry.is_directory {
            // Create directory in tmpfs
            if let Ok(dir_inode) = inodes.alloc(
                inode::InodeType::Directory,
                tmpfs.fs_id(),
                0,
            ) {
                tmpfs.register_dir(dir_inode);
                // Add to parent (simplified: add to root for now)
                let root = tmpfs.root_inode();
                tmpfs.add_dir_entry(root, clean_path, dir_inode);
                serial_println!("[VFS] initramfs: created dir /{}",
                    core::str::from_utf8(clean_path).unwrap_or("?"));
            }
        } else {
            // Create file in tmpfs
            if let Ok(file_inode) = inodes.alloc(
                inode::InodeType::File,
                tmpfs.fs_id(),
                0,
            ) {
                // Set file size
                if let Some(inode) = inodes.get_mut(file_inode) {
                    inode.size = entry.data.len() as u64;
                }
                tmpfs.create_file_with_data(file_inode, entry.data);
                // Find or use parent dir — simplified: add to root
                let root = tmpfs.root_inode();
                tmpfs.add_dir_entry(root, clean_path, file_inode);
                serial_println!("[VFS] initramfs: created file /{} ({} bytes)",
                    core::str::from_utf8(clean_path).unwrap_or("?"),
                    entry.data.len());
            }
        }
    }
}
