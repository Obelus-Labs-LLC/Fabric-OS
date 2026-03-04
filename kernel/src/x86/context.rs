//! Saved CPU context for interrupt/syscall frames and context switching.
//!
//! SavedContext matches the exact stack layout pushed by isr_common in idt.rs:
//! GPRs are pushed rax-first (highest address) through r15-last (lowest address),
//! then vector+error_code (pushed by ISR stub), then CPU-pushed frame.
//! This struct can be overlaid on the stack pointer after all pushes.

#![allow(dead_code)]

/// Saved CPU register context — matches the interrupt/syscall stack frame.
///
/// Layout on the stack (low address → high address, RSP at bottom):
///   [RSP+0]   r15   (pushed last  by isr_common)
///   [RSP+8]   r14
///   [RSP+16]  r13
///   [RSP+24]  r12
///   [RSP+32]  r11
///   [RSP+40]  r10
///   [RSP+48]  r9
///   [RSP+56]  r8
///   [RSP+64]  rbp
///   [RSP+72]  rdi
///   [RSP+80]  rsi
///   [RSP+88]  rdx
///   [RSP+96]  rcx
///   [RSP+104] rbx
///   [RSP+112] rax   (pushed first by isr_common)
///   [RSP+120] vector
///   [RSP+128] error_code
///   [RSP+136] rip   (pushed by CPU / built by syscall_entry)
///   [RSP+144] cs
///   [RSP+152] rflags
///   [RSP+160] rsp
///   [RSP+168] ss
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SavedContext {
    // GPRs in stack order: r15 at RSP+0 (pushed last) through rax at RSP+112 (pushed first)
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
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
            r15: 0, r14: 0, r13: 0, r12: 0,
            r11: 0, r10: 0, r9: 0, r8: 0,
            rbp: 0, rdi: 0, rsi: 0, rdx: 0,
            rcx: 0, rbx: 0, rax: 0,
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
            r15: 0, r14: 0, r13: 0, r12: 0,
            r11: 0, r10: 0, r9: 0, r8: 0,
            rbp: 0, rdi: 0, rsi: 0, rdx: 0,
            rcx: 0, rbx: 0, rax: 0,
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
