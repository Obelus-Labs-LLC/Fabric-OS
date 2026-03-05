//! Driver SDK — safe abstractions for hardware access.
//!
//! Provides MmioRegion (memory-mapped I/O), PioRegion (port I/O),
//! DmaBuffer (DMA-capable memory), and DriverResources (resource bundle).
//! All read/write operations use volatile semantics for correctness.

#![allow(dead_code)]

use core::ptr;

/// Memory-Mapped I/O region.
///
/// Wraps a virtual address range and provides volatile read/write access.
/// The caller must ensure `base` points to valid MMIO space mapped with
/// NO_CACHE + WRITE_THROUGH flags.
#[derive(Clone, Copy, Debug)]
pub struct MmioRegion {
    pub base: usize,
    pub size: usize,
}

impl MmioRegion {
    pub const fn new(base: usize, size: usize) -> Self {
        Self { base, size }
    }

    /// Check if offset is within bounds for a given access width.
    pub fn check_bounds(&self, offset: usize, width: usize) -> bool {
        offset + width <= self.size
    }

    pub fn read8(&self, offset: usize) -> Option<u8> {
        if !self.check_bounds(offset, 1) { return None; }
        unsafe { Some(ptr::read_volatile((self.base + offset) as *const u8)) }
    }

    pub fn read16(&self, offset: usize) -> Option<u16> {
        if !self.check_bounds(offset, 2) { return None; }
        unsafe { Some(ptr::read_volatile((self.base + offset) as *const u16)) }
    }

    pub fn read32(&self, offset: usize) -> Option<u32> {
        if !self.check_bounds(offset, 4) { return None; }
        unsafe { Some(ptr::read_volatile((self.base + offset) as *const u32)) }
    }

    pub fn write8(&self, offset: usize, value: u8) -> bool {
        if !self.check_bounds(offset, 1) { return false; }
        unsafe { ptr::write_volatile((self.base + offset) as *mut u8, value); }
        true
    }

    pub fn write16(&self, offset: usize, value: u16) -> bool {
        if !self.check_bounds(offset, 2) { return false; }
        unsafe { ptr::write_volatile((self.base + offset) as *mut u16, value); }
        true
    }

    pub fn write32(&self, offset: usize, value: u32) -> bool {
        if !self.check_bounds(offset, 4) { return false; }
        unsafe { ptr::write_volatile((self.base + offset) as *mut u32, value); }
        true
    }
}

/// Port I/O region.
///
/// Wraps an I/O port range and provides safe access via x86 in/out instructions.
/// Delegates to `crate::io::inb/outb` etc.
#[derive(Clone, Copy, Debug)]
pub struct PioRegion {
    pub base: u16,
    pub size: u16,
}

impl PioRegion {
    pub const fn new(base: u16, size: u16) -> Self {
        Self { base, size }
    }

    pub fn check_bounds(&self, offset: u16, width: u16) -> bool {
        offset + width <= self.size
    }

    pub fn read8(&self, offset: u16) -> Option<u8> {
        if !self.check_bounds(offset, 1) { return None; }
        unsafe { Some(crate::io::inb(self.base + offset)) }
    }

    pub fn read16(&self, offset: u16) -> Option<u16> {
        if !self.check_bounds(offset, 2) { return None; }
        unsafe { Some(crate::io::inw(self.base + offset)) }
    }

    pub fn read32(&self, offset: u16) -> Option<u32> {
        if !self.check_bounds(offset, 4) { return None; }
        unsafe { Some(crate::io::inl(self.base + offset)) }
    }

    pub fn write8(&self, offset: u16, value: u8) -> bool {
        if !self.check_bounds(offset, 1) { return false; }
        unsafe { crate::io::outb(self.base + offset, value); }
        true
    }

    pub fn write16(&self, offset: u16, value: u16) -> bool {
        if !self.check_bounds(offset, 2) { return false; }
        unsafe { crate::io::outw(self.base + offset, value); }
        true
    }

    pub fn write32(&self, offset: u16, value: u32) -> bool {
        if !self.check_bounds(offset, 4) { return false; }
        unsafe { crate::io::outl(self.base + offset, value); }
        true
    }
}

/// DMA-capable memory buffer.
///
/// Represents a contiguous physical memory region allocated via the buddy
/// allocator. Both virtual and physical addresses are stored for hardware
/// programming (devices need physical addresses, CPU uses virtual).
#[derive(Clone, Copy, Debug)]
pub struct DmaBuffer {
    pub virt: usize,
    pub phys: usize,
    pub size: usize,
    pub order: u8,
}

/// Resource bundle for a driver.
///
/// Collects all hardware resources assigned to a single driver instance:
/// MMIO regions (up to 6, matching PCI BAR count), PIO regions, DMA buffers,
/// and an optional IRQ vector.
pub struct DriverResources {
    pub mmio: [Option<MmioRegion>; 6],
    pub pio: [Option<PioRegion>; 4],
    pub dma: [Option<DmaBuffer>; 4],
    pub irq_vector: Option<u8>,
}

impl DriverResources {
    pub const fn new() -> Self {
        Self {
            mmio: [None; 6],
            pio: [None; 4],
            dma: [None; 4],
            irq_vector: None,
        }
    }

    pub fn add_mmio(&mut self, region: MmioRegion) -> bool {
        for slot in &mut self.mmio {
            if slot.is_none() {
                *slot = Some(region);
                return true;
            }
        }
        false
    }

    pub fn add_pio(&mut self, region: PioRegion) -> bool {
        for slot in &mut self.pio {
            if slot.is_none() {
                *slot = Some(region);
                return true;
            }
        }
        false
    }

    pub fn add_dma(&mut self, buffer: DmaBuffer) -> bool {
        for slot in &mut self.dma {
            if slot.is_none() {
                *slot = Some(buffer);
                return true;
            }
        }
        false
    }

    pub fn mmio_count(&self) -> usize {
        self.mmio.iter().filter(|s| s.is_some()).count()
    }

    pub fn pio_count(&self) -> usize {
        self.pio.iter().filter(|s| s.is_some()).count()
    }

    pub fn dma_count(&self) -> usize {
        self.dma.iter().filter(|s| s.is_some()).count()
    }
}
