//! Task State Segment — kernel stack pointers for Ring 3→0 transitions.
//!
//! Single static TSS for the BSP. RSP0 is updated on every context switch
//! to point to the current process's kernel stack top. IST1 provides a
//! dedicated stack for Double Fault handling.

#![allow(dead_code)]
#![allow(static_mut_refs)]

use crate::memory::PAGE_SIZE;
use crate::memory::frame;
use crate::serial_println;
use super::gdt;

/// Task State Segment for x86_64 (104 bytes).
#[repr(C, packed)]
pub struct TaskStateSegment {
    _reserved0: u32,
    /// Stack pointers for privilege level transitions.
    /// RSP0 = kernel stack loaded on Ring 3→0 transition.
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    _reserved1: u64,
    /// Interrupt Stack Table entries (IST1-IST7).
    /// IST1 = dedicated double-fault stack.
    pub ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    /// I/O permission bitmap base offset.
    pub iomap_base: u16,
}

impl TaskStateSegment {
    /// Create a zeroed TSS.
    pub const fn new() -> Self {
        Self {
            _reserved0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            _reserved1: 0,
            ist: [0; 7],
            _reserved2: 0,
            _reserved3: 0,
            iomap_base: core::mem::size_of::<Self>() as u16,
        }
    }
}

/// Static TSS instance (single CPU).
static mut TSS: TaskStateSegment = TaskStateSegment::new();

/// Initialize the TSS:
/// 1. Allocate a 4KB stack frame for IST1 (double fault).
/// 2. Set IST1 to top of that stack.
/// 3. Write TSS descriptor into GDT entries 5-6.
/// 4. Load TSS register (LTR).
pub fn init() {
    // Allocate IST1 stack (4KB for double fault handler)
    let ist1_frame = frame::allocate_frame()
        .expect("[TSS] Failed to allocate IST1 stack frame");
    let ist1_top = ist1_frame.to_virt().as_u64() + PAGE_SIZE as u64;

    unsafe {
        TSS.ist[0] = ist1_top; // IST1
        TSS.iomap_base = core::mem::size_of::<TaskStateSegment>() as u16;
    }

    // Write TSS descriptor into GDT
    let tss_addr = unsafe { &TSS as *const TaskStateSegment as u64 };
    let tss_size = core::mem::size_of::<TaskStateSegment>() as u16;
    gdt::set_tss_entry(tss_addr, tss_size);

    // Load TSS register
    gdt::load_tss();

    serial_println!(
        "[TSS] Initialized (IST1 stack at 0x{:x}, size {}B)",
        ist1_top, tss_size
    );
}

/// Update RSP0 in the TSS (called on context switch).
/// RSP0 = kernel stack top for the next process.
pub fn set_rsp0(kernel_stack_top: u64) {
    unsafe {
        TSS.rsp0 = kernel_stack_top;
    }
}

/// Get current RSP0 value (for STRESS testing).
pub fn get_rsp0() -> u64 {
    unsafe { TSS.rsp0 }
}

/// Get IST1 value (for STRESS testing).
pub fn get_ist1() -> u64 {
    unsafe { TSS.ist[0] }
}

/// Get a pointer to the TSS (for STRESS testing).
pub fn tss_address() -> u64 {
    unsafe { &TSS as *const TaskStateSegment as u64 }
}
