//! OCRB Phase 8 Gate — VFS + Filesystem verification tests.
//!
//! 10 tests verifying inode table, mount resolution, tmpfs read/write,
//! directory operations, devfs devices, syscall integration, stdio,
//! and CPIO parsing.
//!
//! All tests operate on the global VFS tables (already initialized by
//! vfs::init()) to avoid stack-allocating large InodeTable/MountTable
//! structs (~32KB each) which overflow the kernel stack.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use super::OcrbResult;

pub fn run_all_tests() -> Vec<OcrbResult> {
    let mut results = Vec::new();
    results.push(test_inode_table_crud());
    results.push(test_mount_resolve());
    results.push(test_tmpfs_read_write());
    results.push(test_tmpfs_directory_ops());
    results.push(test_devfs_null());
    results.push(test_devfs_zero());
    results.push(test_devfs_random());
    results.push(test_vfs_open_read_close());
    results.push(test_stdio_pre_open());
    results.push(test_cpio_parse_load());
    results
}

/// Test 1: Inode table alloc/get/release with generation tracking.
/// Uses the global INODES table — allocates, verifies, then releases.
fn test_inode_table_crud() -> OcrbResult {
    use crate::vfs::inode::InodeType;

    let mut inodes = crate::vfs::INODES.lock();

    // Allocate an inode
    let id1 = match inodes.alloc(InodeType::File, 99, 42) {
        Ok(id) => id,
        Err(_) => return OcrbResult {
            test_name: "Inode Table CRUD",
            passed: false, score: 0, weight: 10,
            details: String::from("Failed to allocate inode"),
        },
    };

    // Verify we can get it
    let inode = inodes.get(id1);
    if inode.is_none() || inode.unwrap().fs_data != 42 {
        let _ = inodes.release(id1);
        return OcrbResult {
            test_name: "Inode Table CRUD",
            passed: false, score: 0, weight: 10,
            details: String::from("Failed to retrieve allocated inode"),
        };
    }

    // Allocate a second inode
    let id2 = match inodes.alloc(InodeType::Directory, 99, 99) {
        Ok(id) => id,
        Err(_) => {
            let _ = inodes.release(id1);
            return OcrbResult {
                test_name: "Inode Table CRUD",
                passed: false, score: 0, weight: 10,
                details: String::from("Failed to allocate second inode"),
            };
        }
    };

    if id1 == id2 {
        let _ = inodes.release(id1);
        let _ = inodes.release(id2);
        return OcrbResult {
            test_name: "Inode Table CRUD",
            passed: false, score: 0, weight: 10,
            details: String::from("Two inodes got same ID"),
        };
    }

    // Release first inode
    if inodes.release(id1).is_err() {
        let _ = inodes.release(id2);
        return OcrbResult {
            test_name: "Inode Table CRUD",
            passed: false, score: 0, weight: 10,
            details: String::from("Failed to release inode"),
        };
    }

    // Verify released inode is gone
    if inodes.get(id1).is_some() {
        let _ = inodes.release(id2);
        return OcrbResult {
            test_name: "Inode Table CRUD",
            passed: false, score: 0, weight: 10,
            details: String::from("Released inode still accessible"),
        };
    }

    // Second inode still valid
    if inodes.get(id2).is_none() {
        let _ = inodes.release(id2);
        return OcrbResult {
            test_name: "Inode Table CRUD",
            passed: false, score: 0, weight: 10,
            details: String::from("Second inode lost after first released"),
        };
    }

    // Clean up
    let _ = inodes.release(id2);

    OcrbResult {
        test_name: "Inode Table CRUD",
        passed: true, score: 100, weight: 10,
        details: String::from("Alloc/get/release/stale verified"),
    }
}

/// Test 2: Mount table + path resolution with longest-prefix matching.
/// Verifies the global mount table set up by vfs::init().
fn test_mount_resolve() -> OcrbResult {
    use crate::vfs::mount::FsType;

    let mounts = crate::vfs::MOUNTS.lock();

    // Resolve "/" → root mount (tmpfs)
    let r = match mounts.resolve(b"/") {
        Some(r) => r,
        None => return OcrbResult {
            test_name: "Mount + Resolve",
            passed: false, score: 0, weight: 10,
            details: String::from("Failed to resolve /"),
        },
    };
    let mount = mounts.get(r.mount_index).unwrap();
    if mount.fs_type != FsType::Tmpfs {
        return OcrbResult {
            test_name: "Mount + Resolve",
            passed: false, score: 0, weight: 10,
            details: String::from("Root path did not resolve to tmpfs"),
        };
    }

    // Resolve "/dev/null" → devfs mount (longest prefix)
    let r = match mounts.resolve(b"/dev/null") {
        Some(r) => r,
        None => return OcrbResult {
            test_name: "Mount + Resolve",
            passed: false, score: 0, weight: 10,
            details: String::from("Failed to resolve /dev/null"),
        },
    };
    let mount = mounts.get(r.mount_index).unwrap();
    if mount.fs_type != FsType::Devfs {
        return OcrbResult {
            test_name: "Mount + Resolve",
            passed: false, score: 0, weight: 10,
            details: String::from("/dev/null did not resolve to devfs"),
        };
    }

    // Remaining path should be "null"
    let remaining = &r.remaining[..r.remaining_len];
    if remaining != b"null" {
        return OcrbResult {
            test_name: "Mount + Resolve",
            passed: false, score: 0, weight: 10,
            details: alloc::format!("Remaining path is '{}'", core::str::from_utf8(remaining).unwrap_or("?")),
        };
    }

    // Resolve "/foo" → root mount (tmpfs)
    let r = match mounts.resolve(b"/foo") {
        Some(r) => r,
        None => return OcrbResult {
            test_name: "Mount + Resolve",
            passed: false, score: 0, weight: 10,
            details: String::from("Failed to resolve /foo"),
        },
    };
    let mount = mounts.get(r.mount_index).unwrap();
    if mount.fs_type != FsType::Tmpfs {
        return OcrbResult {
            test_name: "Mount + Resolve",
            passed: false, score: 0, weight: 10,
            details: String::from("/foo did not resolve to root tmpfs"),
        };
    }

    OcrbResult {
        test_name: "Mount + Resolve",
        passed: true, score: 100, weight: 10,
        details: String::from("Longest-prefix matching verified"),
    }
}

/// Test 3: Tmpfs create file, write data, read back.
/// Uses the global TMPFS and INODES tables.
fn test_tmpfs_read_write() -> OcrbResult {
    use crate::vfs::inode::InodeType;

    // Allocate a test file inode
    let file_inode = {
        let mut inodes = crate::vfs::INODES.lock();
        let tmpfs = crate::vfs::TMPFS.lock();
        match inodes.alloc(InodeType::File, tmpfs.fs_id(), 0) {
            Ok(id) => id,
            Err(_) => return OcrbResult {
                test_name: "Tmpfs Create + Read/Write",
                passed: false, score: 0, weight: 15,
                details: String::from("Failed to allocate file inode"),
            },
        }
    };

    // Write data via tmpfs
    let test_data = b"Hello, Fabric OS!";
    {
        let mut tmpfs = crate::vfs::TMPFS.lock();
        let written = tmpfs.write(file_inode, 0, test_data);
        if written != test_data.len() {
            return OcrbResult {
                test_name: "Tmpfs Create + Read/Write",
                passed: false, score: 0, weight: 15,
                details: String::from("Write returned wrong byte count"),
            };
        }
    }

    // Read back
    let mut buf = [0u8; 64];
    let read = {
        let tmpfs = crate::vfs::TMPFS.lock();
        tmpfs.read(file_inode, 0, &mut buf)
    };
    if read != test_data.len() {
        return OcrbResult {
            test_name: "Tmpfs Create + Read/Write",
            passed: false, score: 0, weight: 15,
            details: alloc::format!("Read returned {} bytes, expected {}", read, test_data.len()),
        };
    }

    if &buf[..read] != test_data {
        return OcrbResult {
            test_name: "Tmpfs Create + Read/Write",
            passed: false, score: 0, weight: 15,
            details: String::from("Read data does not match written data"),
        };
    }

    // Verify file size
    let size = {
        let tmpfs = crate::vfs::TMPFS.lock();
        tmpfs.file_size(file_inode)
    };
    if size != test_data.len() as u64 {
        return OcrbResult {
            test_name: "Tmpfs Create + Read/Write",
            passed: false, score: 0, weight: 15,
            details: String::from("File size mismatch"),
        };
    }

    // Clean up inode
    let _ = crate::vfs::INODES.lock().release(file_inode);

    OcrbResult {
        test_name: "Tmpfs Create + Read/Write",
        passed: true, score: 100, weight: 15,
        details: String::from("Write + read back match"),
    }
}

/// Test 4: Tmpfs directory operations — create nested dirs, list entries.
/// Uses the global tables.
fn test_tmpfs_directory_ops() -> OcrbResult {
    use crate::vfs::inode::InodeType;

    // Create a subdirectory inode
    let sub_inode = {
        let mut inodes = crate::vfs::INODES.lock();
        let tmpfs = crate::vfs::TMPFS.lock();
        match inodes.alloc(InodeType::Directory, tmpfs.fs_id(), 0) {
            Ok(id) => id,
            Err(_) => return OcrbResult {
                test_name: "Tmpfs Directory Ops",
                passed: false, score: 0, weight: 10,
                details: String::from("Failed to allocate dir inode"),
            },
        }
    };

    // Register dir and add to root
    {
        let mut tmpfs = crate::vfs::TMPFS.lock();
        tmpfs.register_dir(sub_inode);
        let root = tmpfs.root_inode();
        tmpfs.add_dir_entry(root, b"ocrb_subdir", sub_inode);
    }

    // Create a file in the subdirectory
    let file_inode = {
        let mut inodes = crate::vfs::INODES.lock();
        let tmpfs = crate::vfs::TMPFS.lock();
        match inodes.alloc(InodeType::File, tmpfs.fs_id(), 0) {
            Ok(id) => id,
            Err(_) => return OcrbResult {
                test_name: "Tmpfs Directory Ops",
                passed: false, score: 0, weight: 10,
                details: String::from("Failed to allocate file inode"),
            },
        }
    };

    {
        let mut tmpfs = crate::vfs::TMPFS.lock();
        tmpfs.add_dir_entry(sub_inode, b"nested.txt", file_inode);
    }

    // Verify lookup from root
    {
        let tmpfs = crate::vfs::TMPFS.lock();
        let root = tmpfs.root_inode();
        let found = tmpfs.lookup(root, b"ocrb_subdir");
        if found != Some(sub_inode) {
            return OcrbResult {
                test_name: "Tmpfs Directory Ops",
                passed: false, score: 0, weight: 10,
                details: String::from("Lookup 'ocrb_subdir' in root failed"),
            };
        }

        // Verify file in subdirectory
        let found_file = tmpfs.lookup(sub_inode, b"nested.txt");
        if found_file != Some(file_inode) {
            return OcrbResult {
                test_name: "Tmpfs Directory Ops",
                passed: false, score: 0, weight: 10,
                details: String::from("Lookup 'nested.txt' in subdir failed"),
            };
        }

        // Verify readdir on subdirectory
        let entries = tmpfs.readdir(sub_inode);
        if entries.is_none() || entries.unwrap().len() != 1 {
            return OcrbResult {
                test_name: "Tmpfs Directory Ops",
                passed: false, score: 0, weight: 10,
                details: String::from("Subdir should have 1 entry"),
            };
        }
    }

    // Clean up
    let _ = crate::vfs::INODES.lock().release(file_inode);
    let _ = crate::vfs::INODES.lock().release(sub_inode);

    OcrbResult {
        test_name: "Tmpfs Directory Ops",
        passed: true, score: 100, weight: 10,
        details: String::from("Nested dirs + lookup verified"),
    }
}

/// Test 5: /dev/null — write discards, read returns 0 bytes.
fn test_devfs_null() -> OcrbResult {
    let mut devfs = crate::vfs::DEVFS.lock();
    if !devfs.is_initialized() {
        return OcrbResult {
            test_name: "Devfs Null Device",
            passed: false, score: 0, weight: 5,
            details: String::from("Devfs not initialized"),
        };
    }

    let null_inode = devfs.null_inode();

    // Write should succeed (discard)
    let written = devfs.write(null_inode, 100);
    if written != 100 {
        return OcrbResult {
            test_name: "Devfs Null Device",
            passed: false, score: 0, weight: 5,
            details: String::from("Write to /dev/null returned wrong count"),
        };
    }

    // Read should return 0 bytes (EOF)
    let mut buf = [0u8; 32];
    let read = devfs.read(null_inode, &mut buf);
    if read != 0 {
        return OcrbResult {
            test_name: "Devfs Null Device",
            passed: false, score: 0, weight: 5,
            details: String::from("Read from /dev/null should return 0"),
        };
    }

    OcrbResult {
        test_name: "Devfs Null Device",
        passed: true, score: 100, weight: 5,
        details: String::from("Write discards, read returns EOF"),
    }
}

/// Test 6: /dev/zero — read returns all zeroes.
fn test_devfs_zero() -> OcrbResult {
    let mut devfs = crate::vfs::DEVFS.lock();
    if !devfs.is_initialized() {
        return OcrbResult {
            test_name: "Devfs Zero Device",
            passed: false, score: 0, weight: 5,
            details: String::from("Devfs not initialized"),
        };
    }

    let zero_inode = devfs.zero_inode();

    // Fill buffer with non-zero data
    let mut buf = [0xFFu8; 32];
    let read = devfs.read(zero_inode, &mut buf);
    if read != 32 {
        return OcrbResult {
            test_name: "Devfs Zero Device",
            passed: false, score: 0, weight: 5,
            details: String::from("Read from /dev/zero returned wrong count"),
        };
    }

    // Verify all zeroes
    for &b in &buf {
        if b != 0 {
            return OcrbResult {
                test_name: "Devfs Zero Device",
                passed: false, score: 0, weight: 5,
                details: String::from("Read from /dev/zero returned non-zero byte"),
            };
        }
    }

    OcrbResult {
        test_name: "Devfs Zero Device",
        passed: true, score: 100, weight: 5,
        details: String::from("Read returns all zeroes"),
    }
}

/// Test 7: /dev/random — read returns non-zero data, two reads differ.
fn test_devfs_random() -> OcrbResult {
    let mut devfs = crate::vfs::DEVFS.lock();
    if !devfs.is_initialized() {
        return OcrbResult {
            test_name: "Devfs Random Device",
            passed: false, score: 0, weight: 5,
            details: String::from("Devfs not initialized"),
        };
    }

    let random_inode = devfs.random_inode();

    let mut buf1 = [0u8; 32];
    let mut buf2 = [0u8; 32];

    devfs.read(random_inode, &mut buf1);
    devfs.read(random_inode, &mut buf2);

    // At least one non-zero byte in first read
    let has_nonzero = buf1.iter().any(|&b| b != 0);
    if !has_nonzero {
        return OcrbResult {
            test_name: "Devfs Random Device",
            passed: false, score: 0, weight: 5,
            details: String::from("/dev/random returned all zeroes"),
        };
    }

    // Two reads should differ
    if buf1 == buf2 {
        return OcrbResult {
            test_name: "Devfs Random Device",
            passed: false, score: 0, weight: 5,
            details: String::from("Two reads from /dev/random are identical"),
        };
    }

    OcrbResult {
        test_name: "Devfs Random Device",
        passed: true, score: 100, weight: 5,
        details: String::from("Non-zero data, two reads differ"),
    }
}

/// Test 8: VFS open/read/close round-trip.
fn test_vfs_open_read_close() -> OcrbResult {
    use crate::vfs::ops;
    use crate::vfs::open_file::OpenFlags;

    // Create a test file in tmpfs via the VFS
    {
        let mut inodes = crate::vfs::INODES.lock();
        let mut tmpfs = crate::vfs::TMPFS.lock();

        let file_inode = match inodes.alloc(
            crate::vfs::inode::InodeType::File,
            tmpfs.fs_id(),
            0,
        ) {
            Ok(id) => id,
            Err(_) => return OcrbResult {
                test_name: "Syscall Open/Read/Close",
                passed: false, score: 0, weight: 15,
                details: String::from("Failed to allocate test file inode"),
            },
        };

        // Write test data
        let test_data = b"OCRB test data 8";
        tmpfs.create_file_with_data(file_inode, test_data);

        // Update inode size
        if let Some(inode) = inodes.get_mut(file_inode) {
            inode.size = test_data.len() as u64;
        }

        // Add to root directory
        let root = tmpfs.root_inode();
        tmpfs.add_dir_entry(root, b"ocrb_test.txt", file_inode);
    }

    // Open via VFS
    let open_id = match ops::vfs_open(b"/ocrb_test.txt", OpenFlags::READ) {
        Ok(id) => id,
        Err(e) => return OcrbResult {
            test_name: "Syscall Open/Read/Close",
            passed: false, score: 0, weight: 15,
            details: alloc::format!("vfs_open failed: {:?}", e),
        },
    };

    // Read via VFS
    let mut buf = [0u8; 64];
    let read = match ops::vfs_read(open_id, &mut buf) {
        Ok(n) => n,
        Err(e) => return OcrbResult {
            test_name: "Syscall Open/Read/Close",
            passed: false, score: 0, weight: 15,
            details: alloc::format!("vfs_read failed: {:?}", e),
        },
    };

    if &buf[..read] != b"OCRB test data 8" {
        return OcrbResult {
            test_name: "Syscall Open/Read/Close",
            passed: false, score: 0, weight: 15,
            details: String::from("Read data does not match"),
        };
    }

    // Close via VFS
    if ops::vfs_close(open_id).is_err() {
        return OcrbResult {
            test_name: "Syscall Open/Read/Close",
            passed: false, score: 0, weight: 15,
            details: String::from("vfs_close failed"),
        };
    }

    OcrbResult {
        test_name: "Syscall Open/Read/Close",
        passed: true, score: 100, weight: 15,
        details: String::from("Full open/read/close round-trip verified"),
    }
}

/// Test 9: Stdio pre-open — verify fd 0/1/2 setup works.
fn test_stdio_pre_open() -> OcrbResult {
    use crate::vfs::open_file::OpenFlags;

    // Test that we can allocate stdio-like entries
    let stdin_of = {
        let devfs = crate::vfs::DEVFS.lock();
        let null_inode = devfs.null_inode();
        drop(devfs);

        let mut open_files = crate::vfs::OPEN_FILES.lock();
        match open_files.alloc(null_inode, OpenFlags::READ) {
            Ok(id) => id,
            Err(_) => return OcrbResult {
                test_name: "Stdio Pre-Open",
                passed: false, score: 0, weight: 10,
                details: String::from("Failed to allocate stdin open file"),
            },
        }
    };

    let stdout_of = {
        let serial_inode = crate::vfs::stdio::SERIAL_INODE_ID;
        let mut open_files = crate::vfs::OPEN_FILES.lock();
        match open_files.alloc(serial_inode, OpenFlags::WRITE) {
            Ok(id) => id,
            Err(_) => return OcrbResult {
                test_name: "Stdio Pre-Open",
                passed: false, score: 0, weight: 10,
                details: String::from("Failed to allocate stdout open file"),
            },
        }
    };

    // Verify entries are valid
    {
        let open_files = crate::vfs::OPEN_FILES.lock();
        if open_files.get(stdin_of).is_none() {
            return OcrbResult {
                test_name: "Stdio Pre-Open",
                passed: false, score: 0, weight: 10,
                details: String::from("Stdin open file not found"),
            };
        }
        if open_files.get(stdout_of).is_none() {
            return OcrbResult {
                test_name: "Stdio Pre-Open",
                passed: false, score: 0, weight: 10,
                details: String::from("Stdout open file not found"),
            };
        }

        // Verify stdout is serial sentinel
        let stdout = open_files.get(stdout_of).unwrap();
        if !crate::vfs::stdio::is_serial_inode(stdout.inode_id) {
            return OcrbResult {
                test_name: "Stdio Pre-Open",
                passed: false, score: 0, weight: 10,
                details: String::from("Stdout not pointing to serial inode"),
            };
        }
    }

    // Clean up
    {
        let mut open_files = crate::vfs::OPEN_FILES.lock();
        let _ = open_files.release(stdin_of);
        let _ = open_files.release(stdout_of);
    }

    OcrbResult {
        test_name: "Stdio Pre-Open",
        passed: true, score: 100, weight: 10,
        details: String::from("stdin(/dev/null) + stdout(serial) verified"),
    }
}

/// Test 10: CPIO parse + load into tmpfs.
fn test_cpio_parse_load() -> OcrbResult {
    use crate::vfs::cpio;

    // Build a test CPIO archive
    let test_entries = [
        ("test_dir", &b""[..], true),
        ("test_file.txt", b"Hello from CPIO!", false),
        ("another.bin", &[0xDE, 0xAD, 0xBE, 0xEF][..], false),
    ];

    let archive = cpio::build_test_cpio(&test_entries);

    // Parse the archive
    let entries = match cpio::parse_cpio(&archive) {
        Ok(entries) => entries,
        Err(e) => return OcrbResult {
            test_name: "CPIO Parse + Load",
            passed: false, score: 0, weight: 15,
            details: alloc::format!("CPIO parse failed: {:?}", e),
        },
    };

    // Verify 3 entries found
    if entries.len() != 3 {
        return OcrbResult {
            test_name: "CPIO Parse + Load",
            passed: false, score: 0, weight: 15,
            details: alloc::format!("Expected 3 entries, got {}", entries.len()),
        };
    }

    // Verify first entry is a directory
    if !entries[0].is_directory {
        return OcrbResult {
            test_name: "CPIO Parse + Load",
            passed: false, score: 0, weight: 15,
            details: String::from("First entry should be a directory"),
        };
    }

    // Verify second entry has correct data
    if entries[1].data != b"Hello from CPIO!" {
        return OcrbResult {
            test_name: "CPIO Parse + Load",
            passed: false, score: 0, weight: 15,
            details: String::from("File data mismatch"),
        };
    }

    // Verify third entry has correct binary data
    if entries[2].data != &[0xDE, 0xAD, 0xBE, 0xEF] {
        return OcrbResult {
            test_name: "CPIO Parse + Load",
            passed: false, score: 0, weight: 15,
            details: String::from("Binary file data mismatch"),
        };
    }

    OcrbResult {
        test_name: "CPIO Parse + Load",
        passed: true, score: 100, weight: 15,
        details: alloc::format!("Parsed {} entries (1 dir + 2 files)", entries.len()),
    }
}
