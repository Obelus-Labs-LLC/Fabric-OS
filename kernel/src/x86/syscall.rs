//! SYSCALL/SYSRET — fast user→kernel transition via MSRs.
//!
//! Configures IA32_EFER, IA32_STAR, IA32_LSTAR, and IA32_FMASK for the
//! SYSCALL instruction. The entry stub builds an interrupt-compatible
//! SavedContext frame so context switch and IRETQ work uniformly.

#![allow(dead_code)]
#![allow(static_mut_refs)]

use crate::serial_println;
use super::gdt;
use super::context::SavedContext;

// MSR addresses
const IA32_EFER:  u32 = 0xC000_0080;
const IA32_STAR:  u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_CSTAR: u32 = 0xC000_0083; // Unused (compat mode)
const IA32_FMASK: u32 = 0xC000_0084;

// EFER bits
const EFER_SCE: u64 = 1 << 0; // System Call Enable

// RFLAGS mask: clear IF (bit 9) on SYSCALL entry to disable interrupts
const FMASK_VALUE: u64 = 0x200;

/// Per-CPU scratch area for syscall entry.
/// [0] = user RSP save slot, [1] = kernel RSP for current process.
#[no_mangle]
static mut SYSCALL_SCRATCH: [u64; 2] = [0; 2];

/// Read a model-specific register.
unsafe fn rdmsr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nomem, nostack)
    );
    (high as u64) << 32 | low as u64
}

/// Write a model-specific register.
unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
        options(nomem, nostack)
    );
}

/// Initialize SYSCALL/SYSRET MSRs.
pub fn init() {
    unsafe {
        // Enable SYSCALL/SYSRET in EFER
        let efer = rdmsr(IA32_EFER);
        wrmsr(IA32_EFER, efer | EFER_SCE);

        // STAR: bits 47:32 = kernel CS (for SYSCALL), bits 63:48 = base for SYSRET
        // SYSCALL: CS = STAR[47:32], SS = STAR[47:32]+8
        // SYSRET:  CS = STAR[63:48]+16 | RPL3, SS = STAR[63:48]+8 | RPL3
        let star = ((gdt::KERNEL_CS as u64) << 32) | ((0x10u64) << 48);
        wrmsr(IA32_STAR, star);

        // LSTAR: syscall entry point address
        extern "C" { fn syscall_entry(); }
        wrmsr(IA32_LSTAR, syscall_entry as *const () as u64);

        // FMASK: clear IF on SYSCALL entry
        wrmsr(IA32_FMASK, FMASK_VALUE);

        // CSTAR: unused (32-bit compat mode)
        wrmsr(IA32_CSTAR, 0);
    }

    serial_println!("[SYSCALL] MSRs configured (EFER.SCE=1, LSTAR set, FMASK=0x{:x})", FMASK_VALUE);
}

/// Update the kernel stack pointer for syscall entry (called on context switch).
pub fn set_kernel_rsp(kernel_stack_top: u64) {
    unsafe {
        SYSCALL_SCRATCH[1] = kernel_stack_top;
    }
}

/// Read EFER MSR (for OCRB testing).
pub fn read_efer() -> u64 {
    unsafe { rdmsr(IA32_EFER) }
}

/// Read STAR MSR (for OCRB testing).
pub fn read_star() -> u64 {
    unsafe { rdmsr(IA32_STAR) }
}

/// Read LSTAR MSR (for OCRB testing).
pub fn read_lstar() -> u64 {
    unsafe { rdmsr(IA32_LSTAR) }
}

/// Read FMASK MSR (for OCRB testing).
pub fn read_fmask() -> u64 {
    unsafe { rdmsr(IA32_FMASK) }
}

/// Dead loop for terminated processes — HLT with interrupts enabled.
/// Timer will preempt and switch to the next process.
#[no_mangle]
extern "C" fn syscall_dead_loop() {
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)); }
    }
}

/// Main syscall dispatch — called from assembly with RDI = pointer to SavedContext.
/// RAX = syscall number. Args in RDI (frame.rdi), RSI, RDX, R10, R8, R9.
#[no_mangle]
extern "C" fn syscall_dispatch(frame: *mut SavedContext) {
    let frame = unsafe { &mut *frame };
    let syscall_num = frame.rax;

    match syscall_num {
        // SYS_EXIT: rdi = exit code
        0 => {
            let exit_code = frame.rdi;
            serial_println!("[SYSCALL] sys_exit({})", exit_code);

            // Terminate the current process
            if let Some(mut sched) = crate::process::SCHEDULER.try_lock() {
                if let Some(pid) = sched.current() {
                    sched.dequeue(pid);
                    if let Some(mut table) = crate::process::TABLE.try_lock() {
                        if let Some(pcb) = table.get_mut(pid) {
                            pcb.state = fabric_types::ProcessState::Terminated;
                            pcb.exit_reason = Some(crate::process::ExitReason::Normal);
                            pcb.exit_code = exit_code;
                        }
                    }
                }
            }

            // Modify saved context to return to kernel-mode dead loop.
            // Timer will preempt and switch to next context (idle or another process).
            frame.rip = syscall_dead_loop as *const () as u64;
            frame.cs = gdt::KERNEL_CS as u64;
            frame.ss = gdt::KERNEL_DS as u64;
            frame.rflags = 0x202; // IF=1 so timer can preempt
            frame.rsp = unsafe { SYSCALL_SCRATCH[1] }; // kernel stack top
        },

        // SYS_YIELD: voluntarily yield time slice
        1 => {
            // Zero the time slice so next timer tick triggers switch
            if let Some(sched) = crate::process::SCHEDULER.try_lock() {
                if let Some(pid) = sched.current() {
                    if let Some(mut table) = crate::process::TABLE.try_lock() {
                        if let Some(pcb) = table.get_mut(pid) {
                            pcb.time_slice_remaining = 0;
                        }
                    }
                }
            }
            frame.rax = 0;
        },

        // SYS_WRITE: rdi = handle, rsi = buf_ptr, rdx = len
        2 => {
            let _handle = frame.rdi;
            let buf_ptr = frame.rsi;
            let len = frame.rdx;

            // For Phase 7 testing, write directly to serial (handle 1 = stdout)
            // Safety: we validate the pointer is in userspace range
            if buf_ptr < 0x0000_8000_0000_0000 && len < 4096 {
                let slice = unsafe {
                    core::slice::from_raw_parts(buf_ptr as *const u8, len as usize)
                };
                for &byte in slice {
                    crate::serial::write_byte(byte);
                }
                frame.rax = len; // Return bytes written
            } else {
                frame.rax = u64::MAX; // Error: invalid pointer
            }
        },

        // SYS_GETPID: return current process ID
        3 => {
            if let Some(sched) = crate::process::SCHEDULER.try_lock() {
                if let Some(pid) = sched.current() {
                    frame.rax = pid.0 as u64;
                } else {
                    frame.rax = 0;
                }
            } else {
                frame.rax = 0;
            }
        },

        _ => {
            serial_println!("[SYSCALL] Unknown syscall {}", syscall_num);
            frame.rax = u64::MAX; // Error
        },
    }
}

// ============================================================================
// SYSCALL entry stub — saves user state, switches to kernel stack, calls dispatch
// ============================================================================
core::arch::global_asm!(
    ".global syscall_entry",
    "syscall_entry:",
    // At this point:
    //   RCX = user RIP (saved by CPU)
    //   R11 = user RFLAGS (saved by CPU)
    //   RSP = user RSP (NOT switched — CPU does NOT switch RSP on SYSCALL)
    //   CS/SS = kernel segments (set by STAR MSR)

    // Save user RSP and load kernel RSP from scratch area
    "mov [rip + SYSCALL_SCRATCH], rsp",      // Save user RSP
    "mov rsp, [rip + SYSCALL_SCRATCH + 8]",  // Load kernel RSP

    // Build interrupt-compatible frame (SavedContext layout)
    // CPU-pushed part (we do it manually since SYSCALL doesn't push)
    "push 0x1B",                              // User SS (USER_DS = 0x1B)
    "push [rip + SYSCALL_SCRATCH]",           // User RSP
    "push r11",                               // User RFLAGS
    "push 0x23",                              // User CS (USER_CS = 0x23)
    "push rcx",                               // User RIP

    // Stub-pushed part
    "push 0",                                 // Error code = 0
    "push 256",                               // Vector = 256 (syscall marker)

    // Save all GPRs (same order as isr_common)
    "push rax",
    "push rbx",
    "push rcx",
    "push rdx",
    "push rsi",
    "push rdi",
    "push rbp",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",

    // Enable interrupts in kernel (FMASK cleared IF)
    "sti",

    // Call Rust dispatch: RDI = pointer to SavedContext
    "mov rdi, rsp",
    "call syscall_dispatch",

    // Disable interrupts for return path
    "cli",

    // Restore all GPRs
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rbp",
    "pop rdi",
    "pop rsi",
    "pop rdx",
    "pop rcx",
    "pop rbx",
    "pop rax",

    // Skip vector + error code
    "add rsp, 16",

    // Return to userspace via IRETQ (simpler than SYSRET, same SavedContext format)
    "iretq",
);
