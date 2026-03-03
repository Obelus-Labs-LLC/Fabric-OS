//! Handle manager — per-process handle allocation and resolution.
//!
//! Handles are the Phase 6 syscall ABI primitive: opaque u64 indices
//! that map to capability IDs in the kernel. Wasm-compatible (no pointers).
//!
//! Each process owns its own HandleTable (stored in the PCB).
//! The module-level API here provides convenience wrappers that
//! operate on a specific process's handle table via the process table.

#![allow(dead_code)]

pub mod table;

pub use table::{HandleTable, HandleError, MAX_HANDLES};
use fabric_types::{HandleId, ProcessId};
use crate::process;
use crate::serial_println;

/// Initialize the handle subsystem (currently a no-op; tables are per-PCB).
pub fn init() {
    serial_println!("[HANDLE] Handle table subsystem initialized (per-process, {} slots)", MAX_HANDLES);
}

/// Allocate a handle for a process. Returns the packed HandleId.
pub fn alloc_handle(pid: ProcessId, cap_id: u64) -> Result<HandleId, HandleError> {
    let mut table = process::TABLE.lock();
    let pcb = table.get_mut(pid).ok_or(HandleError::NotActive)?;
    pcb.handle_table.alloc(cap_id)
}

/// Resolve a handle for a process to its capability ID.
pub fn resolve_handle(pid: ProcessId, handle: HandleId) -> Result<u64, HandleError> {
    let table = process::TABLE.lock();
    let pcb = table.get(pid).ok_or(HandleError::NotActive)?;
    pcb.handle_table.resolve(handle)
}

/// Release a handle for a process.
pub fn release_handle(pid: ProcessId, handle: HandleId) -> Result<(), HandleError> {
    let mut table = process::TABLE.lock();
    let pcb = table.get_mut(pid).ok_or(HandleError::NotActive)?;
    pcb.handle_table.release(handle)
}

/// Count active handles for a process.
pub fn handle_count(pid: ProcessId) -> usize {
    let table = process::TABLE.lock();
    table.get(pid).map(|pcb| pcb.handle_table.count()).unwrap_or(0)
}
