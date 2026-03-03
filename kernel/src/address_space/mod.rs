//! Address space manager — per-process virtual memory isolation.
//!
//! Phase 6 creates per-process PML4 page tables with the kernel upper half
//! cloned from the boot PML4. CR3 is stored in the PCB but not yet loaded
//! (no ring 3 switch until Phase 7).

#![allow(dead_code)]

pub mod per_process;

pub use per_process::{AddressSpace, AddressSpaceError};
use crate::serial_println;

/// Initialize the address space subsystem.
pub fn init() {
    serial_println!("[ADDR] Address space subsystem initialized (per-process PML4)");
}
