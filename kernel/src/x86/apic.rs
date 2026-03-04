//! Local APIC — Advanced Programmable Interrupt Controller.
//!
//! Provides timer interrupts for preemptive scheduling. Memory-mapped I/O
//! accessed via HHDM. IO APIC is deferred to Phase 8 (keyboard/disk IRQs).

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::memory::{self, VirtAddr, PhysAddr};
use crate::memory::page_table::PageTableFlags;
use crate::serial_println;

// --- MSR addresses ---
const IA32_APIC_BASE_MSR: u32 = 0x1B;

// --- APIC register offsets (from APIC base) ---
const APIC_ID:              u32 = 0x020;
const APIC_VERSION:         u32 = 0x030;
const APIC_EOI:             u32 = 0x0B0;
const APIC_SPURIOUS:        u32 = 0x0F0;
const APIC_LVT_TIMER:       u32 = 0x320;
const APIC_TIMER_INIT_COUNT: u32 = 0x380;
const APIC_TIMER_CUR_COUNT:  u32 = 0x390;
const APIC_TIMER_DIV_CONFIG: u32 = 0x3E0;

// Timer LVT bits
const TIMER_PERIODIC: u32 = 1 << 17;
const TIMER_MASKED: u32   = 1 << 16;

// Spurious vector register bits
const SPURIOUS_ENABLE: u32 = 1 << 8;

/// Timer interrupt vector.
pub const TIMER_VECTOR: u8 = 32;
/// Spurious interrupt vector.
pub const SPURIOUS_VECTOR: u8 = 255;

/// APIC base virtual address (set during init).
static APIC_BASE_VIRT: AtomicU64 = AtomicU64::new(0);
/// Whether the APIC has been initialized.
static APIC_INITIALIZED: AtomicBool = AtomicBool::new(false);

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

/// Read a 32-bit APIC register.
fn apic_read(offset: u32) -> u32 {
    let base = APIC_BASE_VIRT.load(Ordering::Relaxed);
    assert!(base != 0, "APIC not initialized");
    unsafe {
        core::ptr::read_volatile((base + offset as u64) as *const u32)
    }
}

/// Write a 32-bit APIC register.
fn apic_write(offset: u32, value: u32) {
    let base = APIC_BASE_VIRT.load(Ordering::Relaxed);
    assert!(base != 0, "APIC not initialized");
    unsafe {
        core::ptr::write_volatile((base + offset as u64) as *mut u32, value);
    }
}

/// Initialize the Local APIC.
/// 1. Read APIC base from MSR, map via HHDM.
/// 2. Enable APIC + set spurious vector.
/// 3. Configure timer (initially masked — call `start_timer()` to begin ticking).
pub fn init() {
    // Read APIC base MSR
    let apic_base_msr = unsafe { rdmsr(IA32_APIC_BASE_MSR) };
    let apic_base_phys = apic_base_msr & 0xFFFF_FFFF_FFFF_F000;

    // Verify APIC is enabled in MSR (bit 11)
    if apic_base_msr & (1 << 11) == 0 {
        // Enable it
        unsafe { wrmsr(IA32_APIC_BASE_MSR, apic_base_msr | (1 << 11)); }
    }

    // Map via HHDM — the APIC MMIO region (typically 0xFEE00000) may not
    // be covered by Limine's HHDM since it only maps RAM, not MMIO.
    // Explicitly map the APIC page into the kernel page table.
    let apic_base_virt = apic_base_phys + memory::hhdm_offset();
    let virt = VirtAddr::new(apic_base_virt);
    let phys = PhysAddr(apic_base_phys);
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::WRITE_THROUGH
        | PageTableFlags::NO_EXECUTE;
    match crate::memory::mapper::map(virt, phys, flags) {
        Ok(()) => {},
        Err(crate::memory::mapper::MapError::AlreadyMapped) => {
            // Already mapped by bootloader HHDM — fine
        },
        Err(e) => {
            serial_println!("[APIC] WARNING: Failed to map APIC page: {:?}", e);
        }
    }
    APIC_BASE_VIRT.store(apic_base_virt, Ordering::Release);

    // Read APIC ID and version for diagnostic
    let apic_id = apic_read(APIC_ID) >> 24;
    let version = apic_read(APIC_VERSION);
    serial_println!("[APIC] Local APIC ID={}, version=0x{:x}, base=0x{:x}",
        apic_id, version & 0xFF, apic_base_phys);

    // Enable APIC via spurious interrupt vector register
    apic_write(APIC_SPURIOUS, SPURIOUS_VECTOR as u32 | SPURIOUS_ENABLE);

    // Configure timer (masked initially)
    apic_write(APIC_TIMER_DIV_CONFIG, 0x3); // Divide by 16
    apic_write(APIC_LVT_TIMER, TIMER_VECTOR as u32 | TIMER_PERIODIC | TIMER_MASKED);
    apic_write(APIC_TIMER_INIT_COUNT, 0);

    APIC_INITIALIZED.store(true, Ordering::Release);
    serial_println!("[APIC] Initialized (timer masked, spurious vector {})", SPURIOUS_VECTOR);
}

/// Start the APIC timer with a given initial count value.
/// In QEMU, the APIC timer typically runs at the bus frequency.
/// A value of ~0x20000 gives roughly 1ms ticks on most QEMU configs.
pub fn start_timer(initial_count: u32) {
    // Unmask timer and set to periodic mode
    apic_write(APIC_LVT_TIMER, TIMER_VECTOR as u32 | TIMER_PERIODIC);
    apic_write(APIC_TIMER_INIT_COUNT, initial_count);
    serial_println!("[APIC] Timer started (periodic, initial count=0x{:x})", initial_count);
}

/// Stop the APIC timer (mask it).
pub fn stop_timer() {
    apic_write(APIC_LVT_TIMER, TIMER_VECTOR as u32 | TIMER_PERIODIC | TIMER_MASKED);
    apic_write(APIC_TIMER_INIT_COUNT, 0);
}

/// Send End-Of-Interrupt to the Local APIC.
pub fn send_eoi() {
    if APIC_INITIALIZED.load(Ordering::Relaxed) {
        apic_write(APIC_EOI, 0);
    }
}

/// Check if the APIC is initialized.
pub fn is_initialized() -> bool {
    APIC_INITIALIZED.load(Ordering::Relaxed)
}

/// Get the APIC ID of this CPU.
pub fn apic_id() -> u32 {
    apic_read(APIC_ID) >> 24
}

/// Get the APIC base virtual address (for testing).
pub fn base_virt() -> u64 {
    APIC_BASE_VIRT.load(Ordering::Relaxed)
}

/// Read the current timer count.
pub fn timer_current_count() -> u32 {
    apic_read(APIC_TIMER_CUR_COUNT)
}

/// Read the IA32_APIC_BASE MSR (for testing).
pub fn read_apic_base_msr() -> u64 {
    unsafe { rdmsr(IA32_APIC_BASE_MSR) }
}
