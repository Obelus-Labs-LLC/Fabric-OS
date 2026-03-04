//! stdio pre-opening — sets up fd 0/1/2 for user processes.
//!
//! - fd 0 (stdin):  /dev/null — reads return EOF
//! - fd 1 (stdout): serial output (special SERIAL_INODE sentinel)
//! - fd 2 (stderr): serial output (same mechanism)
//!
//! The serial-backed fds use a special sentinel inode that is recognized
//! by the write path to direct output to the serial port.

#![allow(dead_code)]

use fabric_types::ProcessId;
use super::inode::InodeId;
use super::open_file::OpenFlags;
use super::{OPEN_FILES, DEVFS};

/// Sentinel inode ID for serial-backed stdout/stderr.
/// This is a special value that the write syscall recognizes.
pub const SERIAL_INODE_ID: InodeId = InodeId(0xFFFF_FFFE);

/// Check if an inode is the serial sentinel.
pub fn is_serial_inode(id: InodeId) -> bool {
    id == SERIAL_INODE_ID
}

/// Set up stdio file descriptors for a newly spawned process.
///
/// Allocates handles at slots 0, 1, 2 in the process's HandleTable:
/// - fd 0: /dev/null (stdin)
/// - fd 1: serial stdout (SERIAL_INODE sentinel)
/// - fd 2: serial stderr (SERIAL_INODE sentinel)
///
/// Must be called while holding a lock on TABLE (or after PCB is created).
pub fn setup_stdio(pid: ProcessId) {
    // fd 0: stdin → /dev/null
    let stdin_inode = {
        let devfs = DEVFS.lock();
        devfs.null_inode()
    };

    let stdin_of = {
        let mut open_files = OPEN_FILES.lock();
        open_files.alloc(stdin_inode, OpenFlags::READ)
            .expect("[STDIO] Failed to allocate stdin open file")
    };

    // fd 1: stdout → serial sentinel
    let stdout_of = {
        let mut open_files = OPEN_FILES.lock();
        open_files.alloc(SERIAL_INODE_ID, OpenFlags::WRITE)
            .expect("[STDIO] Failed to allocate stdout open file")
    };

    // fd 2: stderr → serial sentinel
    let stderr_of = {
        let mut open_files = OPEN_FILES.lock();
        open_files.alloc(SERIAL_INODE_ID, OpenFlags::WRITE)
            .expect("[STDIO] Failed to allocate stderr open file")
    };

    // Install into the process's handle table at slots 0, 1, 2
    let mut table = crate::process::TABLE.lock();
    if let Some(pcb) = table.get_mut(pid) {
        // Allocate handles — they go to first available slots (0, 1, 2)
        let h0 = pcb.handle_table.alloc(stdin_of.0 as u64);
        let h1 = pcb.handle_table.alloc(stdout_of.0 as u64);
        let h2 = pcb.handle_table.alloc(stderr_of.0 as u64);

        if h0.is_err() || h1.is_err() || h2.is_err() {
            crate::serial_println!("[STDIO] Warning: failed to allocate stdio handles for pid:{}", pid.0);
        }
    }
}
