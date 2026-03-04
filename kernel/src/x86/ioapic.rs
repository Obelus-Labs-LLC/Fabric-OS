//! IO APIC — Route external IRQs to CPU interrupt vectors.
//!
//! Standard IO APIC at physical address 0xFEC00000, accessed via HHDM.
//! Routes IRQ1 (keyboard) to vector 33 and IRQ11 (virtio-net) to vector 43.

#![allow(dead_code)]

use crate::memory;
use crate::memory::{PhysAddr, VirtAddr, page_table::PageTableFlags};
use crate::serial_println;

/// IO APIC physical base address (standard x86).
const IOAPIC_PHYS_BASE: u64 = 0xFEC00000;

/// IO APIC register offsets (indirect access via IOREGSEL/IOWIN).
const IOAPIC_REG_ID: u32 = 0x00;
const IOAPIC_REG_VER: u32 = 0x01;
const IOAPIC_REG_REDTBL_BASE: u32 = 0x10;

/// Read an IO APIC register via MMIO (indirect addressing).
unsafe fn ioapic_read(base: *mut u32, reg: u32) -> u32 {
    // IOREGSEL at offset 0x00, IOWIN at offset 0x10
    base.write_volatile(reg);
    base.add(4).read_volatile() // offset 0x10 = 4 u32s
}

/// Write an IO APIC register via MMIO (indirect addressing).
unsafe fn ioapic_write(base: *mut u32, reg: u32, value: u32) {
    base.write_volatile(reg);
    base.add(4).write_volatile(value);
}

/// Read the IO APIC ID register (for diagnostics).
pub fn read_id() -> u32 {
    let base = ioapic_base();
    unsafe { (ioapic_read(base, IOAPIC_REG_ID) >> 24) & 0xF }
}

/// Read the IO APIC version and max redirection entries.
pub fn read_version() -> (u8, u8) {
    let base = ioapic_base();
    let ver = unsafe { ioapic_read(base, IOAPIC_REG_VER) };
    let version = (ver & 0xFF) as u8;
    let max_entries = ((ver >> 16) & 0xFF) as u8;
    (version, max_entries)
}

/// Route an IRQ to a specific interrupt vector.
///
/// Uses fixed delivery mode, physical destination to APIC ID 0 (BSP).
/// Active-low, level-triggered for PCI (edge-triggered for ISA like keyboard).
pub fn route_irq(irq: u8, vector: u8, level_triggered: bool) {
    let base = ioapic_base();
    let reg_low = IOAPIC_REG_REDTBL_BASE + (irq as u32) * 2;
    let reg_high = reg_low + 1;

    // High: destination APIC ID = 0 (BSP) in bits 24-27
    let high: u32 = 0 << 24;

    // Low: vector + delivery mode (000=Fixed) + polarity + trigger
    let mut low: u32 = vector as u32; // bits 0-7: vector
    // bits 8-10: delivery mode = 000 (Fixed)
    // bit 11: destination mode = 0 (physical)
    if level_triggered {
        low |= 1 << 13; // bit 13: polarity = active-low
        low |= 1 << 15; // bit 15: trigger mode = level
    }
    // bit 16: mask = 0 (unmasked)

    unsafe {
        ioapic_write(base, reg_high, high);
        ioapic_write(base, reg_low, low);
    }
}

/// Mask (disable) an IRQ in the IO APIC.
pub fn mask_irq(irq: u8) {
    let base = ioapic_base();
    let reg_low = IOAPIC_REG_REDTBL_BASE + (irq as u32) * 2;
    unsafe {
        let mut low = ioapic_read(base, reg_low);
        low |= 1 << 16; // set mask bit
        ioapic_write(base, reg_low, low);
    }
}

/// Get the IO APIC virtual base address via HHDM.
fn ioapic_base() -> *mut u32 {
    (IOAPIC_PHYS_BASE + memory::hhdm_offset()) as *mut u32
}

/// Initialize the IO APIC: route keyboard (IRQ1) and virtio-net (IRQ11).
pub fn init() {
    // Map IO APIC MMIO page — Limine HHDM only covers RAM, not device MMIO
    let virt = VirtAddr::new(IOAPIC_PHYS_BASE + memory::hhdm_offset());
    let phys = PhysAddr::new(IOAPIC_PHYS_BASE);
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_CACHE;
    if let Err(_) = memory::mapper::map(virt, phys, flags) {
        // Might already be mapped — continue
        serial_println!("[IO_APIC] MMIO page already mapped or map failed, continuing...");
    }

    let id = read_id();
    let (version, max_entries) = read_version();
    serial_println!("[IO_APIC] ID={}, version=0x{:02x}, max_entries={}",
        id, version, max_entries);

    // First mask all entries
    for irq in 0..=max_entries {
        mask_irq(irq);
    }

    // Route IRQ1 -> vector 33 (keyboard, ISA = edge-triggered)
    route_irq(1, 33, false);
    serial_println!("[IO_APIC] IRQ1 -> vector 33 (keyboard)");

    // Route IRQ11 -> vector 43 (virtio-net, PCI = level-triggered)
    route_irq(11, 43, true);
    serial_println!("[IO_APIC] IRQ11 -> vector 43 (virtio-net)");
}
