//! Virtio — shared data structures for virtqueues.
//!
//! Implements the split virtqueue layout (legacy mode):
//!   Descriptor table → Available ring → Used ring
//! All allocated from physically contiguous DMA-safe pages.

#![allow(dead_code)]

pub mod net;

use crate::memory::frame;
use crate::memory::PAGE_SIZE;
use crate::io::{inw, outw, outl};

/// Virtqueue descriptor (16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtqDesc {
    pub addr: u64,   // Physical address of buffer
    pub len: u32,    // Length of buffer
    pub flags: u16,  // VIRTQ_DESC_F_NEXT, VIRTQ_DESC_F_WRITE
    pub next: u16,   // Next descriptor index (if NEXT flag set)
}

/// Descriptor flags.
pub const VIRTQ_DESC_F_NEXT: u16 = 1;
pub const VIRTQ_DESC_F_WRITE: u16 = 2;

/// Virtqueue available ring header.
#[repr(C)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    // Followed by ring[queue_size] entries (u16 each)
}

/// Virtqueue used ring header.
#[repr(C)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    // Followed by ring[queue_size] VirtqUsedElem entries
}

/// Used ring element (8 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtqUsedElem {
    pub id: u32,  // Descriptor chain head index
    pub len: u32, // Total bytes written by device
}

/// A single virtqueue with descriptor table, available ring, and used ring.
pub struct Virtqueue {
    pub desc: *mut VirtqDesc,
    pub avail: *mut VirtqAvail,
    pub used: *mut VirtqUsed,
    pub size: u16,
    pub free_head: u16,
    pub last_used_idx: u16,
    pub num_free: u16,
    /// Physical address of the descriptor table (for device programming).
    pub phys_addr: u64,
}

// Safety: Virtqueue raw pointers reference DMA memory that is only accessed
// with the NIC mutex held. No concurrent aliasing.
unsafe impl Send for Virtqueue {}

impl Virtqueue {
    /// Allocate and initialize a virtqueue with the given number of descriptors.
    ///
    /// Uses the buddy allocator for physically contiguous pages.
    /// Legacy virtio requires the descriptor table + available ring in one
    /// contiguous region, and the used ring page-aligned after that.
    pub fn new(size: u16, io_base: u16, queue_idx: u16) -> Option<Self> {
        // Calculate sizes
        let desc_size = (size as usize) * core::mem::size_of::<VirtqDesc>();
        let avail_size = 6 + (size as usize) * 2; // flags(2) + idx(2) + ring(2*N) + used_event(2)
        let used_size = 6 + (size as usize) * core::mem::size_of::<VirtqUsedElem>(); // flags(2) + idx(2) + ring(8*N) + avail_event(2)

        // Descriptor table + avail ring in first region (page-aligned)
        let first_region = desc_size + avail_size;
        let first_pages = (first_region + PAGE_SIZE - 1) / PAGE_SIZE;

        // Used ring in second region (page-aligned)
        let used_pages = (used_size + PAGE_SIZE - 1) / PAGE_SIZE;

        let total_pages = first_pages + used_pages;
        let order = pages_to_order(total_pages);

        // Allocate contiguous pages
        let phys = {
            let mut alloc = frame::ALLOCATOR.lock();
            alloc.allocate(order as usize)?
        };

        let virt = phys.to_virt().as_u64() as *mut u8;
        let phys_addr = phys.as_u64();

        // Zero all memory
        unsafe {
            core::ptr::write_bytes(virt, 0, total_pages * PAGE_SIZE);
        }

        // Layout pointers
        let desc = virt as *mut VirtqDesc;
        let avail = unsafe { virt.add(desc_size) } as *mut VirtqAvail;
        let used = unsafe { virt.add(first_pages * PAGE_SIZE) } as *mut VirtqUsed;

        // Initialize free list (chain all descriptors)
        for i in 0..size {
            unsafe {
                let d = &mut *desc.add(i as usize);
                d.next = if i + 1 < size { i + 1 } else { 0 };
                d.flags = 0;
            }
        }

        // Tell device about this queue
        unsafe {
            // Select queue
            outw(io_base + 14, queue_idx); // VIRTIO_PCI_QUEUE_SEL
            // Set queue size
            outw(io_base + 12, size); // VIRTIO_PCI_QUEUE_SIZE
            // Set queue address (in 4096-byte page units)
            outl(io_base + 8, (phys_addr / PAGE_SIZE as u64) as u32); // VIRTIO_PCI_QUEUE_PFN
        }

        Some(Self {
            desc,
            avail,
            used,
            size,
            free_head: 0,
            last_used_idx: 0,
            num_free: size,
            phys_addr,
        })
    }

    /// Allocate a descriptor from the free list.
    pub fn alloc_desc(&mut self) -> Option<u16> {
        if self.num_free == 0 {
            return None;
        }
        let idx = self.free_head;
        let next = unsafe { (*self.desc.add(idx as usize)).next };
        self.free_head = next;
        self.num_free -= 1;
        Some(idx)
    }

    /// Free a descriptor back to the free list.
    pub fn free_desc(&mut self, idx: u16) {
        unsafe {
            let d = &mut *self.desc.add(idx as usize);
            d.flags = 0;
            d.next = self.free_head;
        }
        self.free_head = idx;
        self.num_free += 1;
    }

    /// Submit a descriptor chain to the available ring.
    pub fn submit(&mut self, head: u16) {
        let avail_idx = unsafe { (*self.avail).idx };
        let ring_entry = avail_idx % self.size;
        unsafe {
            let ring_ptr = (self.avail as *mut u16).add(2 + ring_entry as usize);
            ring_ptr.write_volatile(head);
            // Memory barrier
            core::sync::atomic::fence(core::sync::atomic::Ordering::Release);
            (*self.avail).idx = avail_idx.wrapping_add(1);
        }
    }

    /// Check if device has consumed entries. Returns (desc_idx, bytes_written) if so.
    pub fn poll_used(&mut self) -> Option<(u16, u32)> {
        let used_idx = unsafe { (*self.used).idx };
        if self.last_used_idx == used_idx {
            return None;
        }
        let ring_entry = self.last_used_idx % self.size;
        let elem = unsafe {
            let ring_ptr = (self.used as *mut u8).add(4) as *mut VirtqUsedElem;
            *ring_ptr.add(ring_entry as usize)
        };
        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        Some((elem.id as u16, elem.len))
    }
}

/// Convert a page count to a buddy allocator order (ceil(log2(pages))).
pub fn pages_to_order(pages: usize) -> u32 {
    if pages <= 1 { return 0; }
    let mut order = 0u32;
    let mut size = 1usize;
    while size < pages {
        size <<= 1;
        order += 1;
    }
    order
}
