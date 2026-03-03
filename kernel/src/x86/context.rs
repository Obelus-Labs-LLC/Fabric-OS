//! Saved CPU context for interrupt/syscall frames and context switching.
//!
//! SavedContext matches the exact stack layout pushed by isr_common in idt.rs:
//! first GPRs (pushed by stub), then vector+error_code, then CPU-pushed frame.
//! This struct can be overlaid on the stack pointer after all pushes.

#![allow(dead_code)]

/// Saved CPU register context — matches the interrupt/syscall stack frame.
///
/// Layout on the stack (low address → high address):
///   [RSP points here]
///   rax, rbx, rcx, rdx, rsi, rdi, rbp, r8..r15  (pushed by isr_common)
///   vector, error_code                             (pushed by ISR stub)
///   rip, cs, rflags, rsp, ss                       (pushed by CPU)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SavedContext {
    // Pushed by isr_common (in push order: rax first = lowest address)
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    // Pushed by ISR stub
    pub vector: u64,
    pub error_code: u64,
    // Pushed by CPU on interrupt/exception
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl SavedContext {
    /// Size in bytes of the full context frame.
    pub const SIZE: usize = core::mem::size_of::<Self>();

    /// Create a zeroed context.
    pub const fn zero() -> Self {
        Self {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, rbp: 0,
            r8: 0, r9: 0, r10: 0, r11: 0,
            r12: 0, r13: 0, r14: 0, r15: 0,
            vector: 0, error_code: 0,
            rip: 0, cs: 0, rflags: 0, rsp: 0, ss: 0,
        }
    }

    /// Build an initial context for first-time Ring 3 entry.
    ///
    /// When the scheduler picks this process, isr_common will pop this
    /// fake frame and IRETQ into userspace.
    pub fn for_user_entry(
        entry_rip: u64,
        user_stack_top: u64,
        user_cs: u16,
        user_ss: u16,
    ) -> Self {
        Self {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, rbp: 0,
            r8: 0, r9: 0, r10: 0, r11: 0,
            r12: 0, r13: 0, r14: 0, r15: 0,
            vector: 0,
            error_code: 0,
            rip: entry_rip,
            cs: user_cs as u64,
            rflags: 0x202, // IF=1 (interrupts enabled in userspace)
            rsp: user_stack_top,
            ss: user_ss as u64,
        }
    }
}

/// Write a SavedContext onto a kernel stack and return the new RSP.
///
/// The context is placed at the top of the stack (stack_top - SIZE),
/// and the returned value is the address that should be loaded into RSP
/// so that isr_common's pop sequence restores it correctly.
pub fn place_initial_context(
    kernel_stack_top: u64,
    ctx: &SavedContext,
) -> u64 {
    let frame_addr = kernel_stack_top - SavedContext::SIZE as u64;
    unsafe {
        let dst = frame_addr as *mut SavedContext;
        core::ptr::write(dst, *ctx);
    }
    frame_addr
}

/// Read the current CR3 (page table physical base).
pub fn read_cr3() -> u64 {
    let cr3: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack));
    }
    cr3 & 0x000F_FFFF_FFFF_F000
}

/// Write CR3 (switch address space). Flushes TLB.
pub fn write_cr3(phys: u64) {
    unsafe {
        core::arch::asm!("mov cr3, {}", in(reg) phys, options(nomem, nostack));
    }
}
