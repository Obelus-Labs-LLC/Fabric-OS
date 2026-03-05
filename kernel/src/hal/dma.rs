//! DMA Buffer Manager — allocate and track DMA-capable memory.
//!
//! Provides contiguous physical memory allocation for device DMA,
//! with per-process tracking for cleanup on process exit.
//! Uses the buddy allocator for contiguous page allocation and
//! HHDM for physical-to-virtual address translation.

#![allow(dead_code)]

use spin::Mutex;
use crate::memory::{PhysAddr, PAGE_SIZE, hhdm_offset};
use crate::memory::frame;
use super::driver_sdk::DmaBuffer;

/// Maximum number of DMA buffers system-wide.
pub const MAX_DMA_BUFFERS: usize = 32;

/// A tracked DMA allocation.
#[derive(Clone, Copy, Debug)]
struct DmaEntry {
    buffer: DmaBuffer,
    owner_pid: u32,
    active: bool,
}

impl DmaEntry {
    const fn empty() -> Self {
        Self {
            buffer: DmaBuffer { virt: 0, phys: 0, size: 0, order: 0 },
            owner_pid: 0,
            active: false,
        }
    }
}

/// Manages DMA buffer allocations with per-process tracking.
pub struct DmaManager {
    entries: [DmaEntry; MAX_DMA_BUFFERS],
    count: usize,
}

impl DmaManager {
    pub const fn new() -> Self {
        Self {
            entries: [DmaEntry::empty(); MAX_DMA_BUFFERS],
            count: 0,
        }
    }

    /// Compute the buddy allocator order needed for `size` bytes.
    /// Returns the smallest order where (PAGE_SIZE << order) >= size.
    fn size_to_order(size: usize) -> u8 {
        if size == 0 { return 0; }
        let pages_needed = (size + PAGE_SIZE - 1) / PAGE_SIZE;
        let mut order = 0u8;
        while (1usize << order) < pages_needed {
            order += 1;
        }
        order
    }

    /// Allocate a DMA buffer of at least `size` bytes.
    ///
    /// Returns a DmaBuffer with both physical and virtual addresses.
    /// The physical address is page-aligned and suitable for hardware DMA.
    pub fn alloc(&mut self, size: usize, owner_pid: u32) -> Option<DmaBuffer> {
        if self.count >= MAX_DMA_BUFFERS || size == 0 {
            return None;
        }

        let order = Self::size_to_order(size);
        let actual_size = PAGE_SIZE << (order as usize);

        // Allocate contiguous pages from buddy allocator
        let phys_addr = frame::ALLOCATOR.lock().allocate(order as usize)?;
        let phys = phys_addr.as_u64() as usize;
        let virt = phys + hhdm_offset() as usize;

        let buffer = DmaBuffer { virt, phys, size: actual_size, order };

        // Find a free slot
        for entry in &mut self.entries {
            if !entry.active {
                entry.buffer = buffer;
                entry.owner_pid = owner_pid;
                entry.active = true;
                self.count += 1;
                return Some(buffer);
            }
        }

        // Shouldn't reach here since we checked count, but free the pages
        frame::ALLOCATOR.lock().deallocate(phys_addr, order as usize);
        None
    }

    /// Free a DMA buffer by its physical address.
    pub fn free(&mut self, phys: usize) -> bool {
        for entry in &mut self.entries {
            if entry.active && entry.buffer.phys == phys {
                let phys_addr = PhysAddr::new(phys as u64);
                frame::ALLOCATOR.lock().deallocate(phys_addr, entry.buffer.order as usize);
                entry.active = false;
                self.count -= 1;
                return true;
            }
        }
        false
    }

    /// Free all DMA buffers owned by a specific process.
    /// Called during process teardown to prevent DMA memory leaks.
    pub fn free_all_for_process(&mut self, pid: u32) -> usize {
        let mut freed = 0;
        for entry in &mut self.entries {
            if entry.active && entry.owner_pid == pid {
                let phys_addr = PhysAddr::new(entry.buffer.phys as u64);
                frame::ALLOCATOR.lock().deallocate(phys_addr, entry.buffer.order as usize);
                entry.active = false;
                self.count -= 1;
                freed += 1;
            }
        }
        freed
    }

    /// Get the number of active DMA allocations.
    pub fn active_count(&self) -> usize {
        self.count
    }

    /// Get the total allocated DMA memory in bytes.
    pub fn total_allocated_bytes(&self) -> usize {
        self.entries.iter()
            .filter(|e| e.active)
            .map(|e| e.buffer.size)
            .sum()
    }
}

/// Global DMA manager.
pub static DMA_MANAGER: Mutex<DmaManager> = Mutex::new(DmaManager::new());
