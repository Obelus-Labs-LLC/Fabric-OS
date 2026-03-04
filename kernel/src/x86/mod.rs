//! x86_64 hardware abstraction — GDT, TSS, IDT, APIC, SYSCALL, context switch.
//!
//! Phase 7 brings the hardware to life: interrupt handling, preemptive scheduling,
//! and Ring 3 userspace execution.

#![allow(dead_code)]

pub mod gdt;
pub mod tss;
pub mod idt;
pub mod apic;
pub mod syscall;
pub mod context;

use crate::serial_println;

/// Initialize Phase 7A: GDT → TSS → IDT → APIC (timer initially stopped).
/// Interrupts remain disabled (CLI) until the caller is ready.
pub fn init() {
    serial_println!("[X86] Initializing x86_64 hardware...");

    // Order matters: GDT must be loaded before TSS (needs TSS selector),
    // IDT before APIC (timer fires interrupts).
    gdt::init();
    tss::init();
    idt::init();
    apic::init();

    serial_println!("[X86] x86_64 hardware initialized (GDT/TSS/IDT/APIC)");
}

/// Initialize Phase 7B: SYSCALL/SYSRET MSR configuration.
pub fn init_syscall() {
    syscall::init();
    serial_println!("[X86] SYSCALL/SYSRET configured");
}

/// Enable interrupts (STI). Call only after all handlers are installed.
pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nostack, nomem));
    }
}

/// Disable interrupts (CLI).
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nostack, nomem));
    }
}

/// Check if interrupts are enabled (read RFLAGS.IF).
pub fn interrupts_enabled() -> bool {
    let rflags: u64;
    unsafe {
        core::arch::asm!("pushfq; pop {}", out(reg) rflags, options(nomem));
    }
    rflags & (1 << 9) != 0
}
