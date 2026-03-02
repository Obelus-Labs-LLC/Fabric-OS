#![allow(dead_code)]

use spin::Mutex;
use crate::memory::{PhysAddr, PAGE_SIZE};
use crate::serial_println;

/// Maximum order for buddy allocator (order 10 = 4 MiB blocks)
const MAX_ORDER: usize = 10;

/// Maximum physical address space we support (1 GiB)
const MAX_PHYS_ADDR: u64 = 0x4000_0000;

/// Maximum number of physical frames
const MAX_FRAMES: usize = (MAX_PHYS_ADDR as usize) / PAGE_SIZE;

/// Bitmap size in bytes (1 bit per frame)
const BITMAP_SIZE: usize = MAX_FRAMES / 8;

/// Global frame allocator
pub static ALLOCATOR: Mutex<BuddyAllocator> = Mutex::new(BuddyAllocator::new());

pub struct BuddyAllocator {
    /// Head of free list per order (physical address, 0 = empty)
    free_lists: [u64; MAX_ORDER + 1],
    /// 1 bit per frame: 1 = allocated, 0 = free
    bitmap: [u8; BITMAP_SIZE],
    /// Total usable frames
    total_frames: usize,
    /// Currently free frames
    free_frames: usize,
    /// Highest order block found
    max_order: usize,
    initialized: bool,
}

impl BuddyAllocator {
    pub const fn new() -> Self {
        Self {
            free_lists: [0; MAX_ORDER + 1],
            bitmap: [0xFF; BITMAP_SIZE],
            total_frames: 0,
            free_frames: 0,
            max_order: 0,
            initialized: false,
        }
    }

    /// Initialize from usable memory regions (base, length) pairs
    pub fn init_from_regions(&mut self, regions: &[(u64, u64)]) {
        // Mark usable frames as free in bitmap
        for &(base, length) in regions {
            let start_frame = ceil_div(base as usize, PAGE_SIZE);
            let end_frame = (base as usize + length as usize) / PAGE_SIZE;

            for frame in start_frame..end_frame {
                if frame < MAX_FRAMES {
                    self.clear_bit(frame);
                    self.total_frames += 1;
                    self.free_frames += 1;
                }
            }
        }

        // Build free lists from bitmap
        self.build_free_lists();
        self.initialized = true;

        serial_println!(
            "[MEMORY] Buddy allocator initialized (max order: {})",
            self.max_order
        );
        serial_println!(
            "[MEMORY] Free frames: {} ({} KiB)",
            self.free_frames,
            self.free_frames * 4
        );
    }

    fn build_free_lists(&mut self) {
        let mut frame = 0;
        while frame < MAX_FRAMES {
            if !self.is_free(frame) {
                frame += 1;
                continue;
            }

            let order = self.max_block_order(frame);
            if order > self.max_order {
                self.max_order = order;
            }

            self.push_free_list(frame, order);
            frame += 1 << order;
        }
    }

    /// Find the largest buddy-aligned block starting at `frame`
    fn max_block_order(&self, frame: usize) -> usize {
        let mut order = 0;
        while order < MAX_ORDER {
            let next_size = 1usize << (order + 1);
            if frame % next_size != 0 {
                break;
            }
            // Check that all frames in the upper half are free
            let mut all_free = true;
            for i in (1 << order)..next_size {
                if frame + i >= MAX_FRAMES || !self.is_free(frame + i) {
                    all_free = false;
                    break;
                }
            }
            if !all_free {
                break;
            }
            order += 1;
        }
        order
    }

    // --- Bitmap operations ---

    fn set_bit(&mut self, frame: usize) {
        self.bitmap[frame / 8] |= 1 << (frame % 8);
    }

    fn clear_bit(&mut self, frame: usize) {
        self.bitmap[frame / 8] &= !(1 << (frame % 8));
    }

    fn is_free(&self, frame: usize) -> bool {
        frame < MAX_FRAMES && (self.bitmap[frame / 8] & (1 << (frame % 8))) == 0
    }

    // --- Free list operations ---
    // Each free block stores a next-pointer (phys addr) at its first 8 bytes via HHDM.

    fn push_free_list(&mut self, frame: usize, order: usize) {
        let addr = frame_to_addr(frame);
        let virt = PhysAddr::new(addr).to_virt();

        unsafe {
            let ptr = virt.as_u64() as *mut u64;
            *ptr = self.free_lists[order];
        }
        self.free_lists[order] = addr;
    }

    fn pop_free_list(&mut self, order: usize) -> Option<usize> {
        let addr = self.free_lists[order];
        if addr == 0 {
            return None;
        }

        let virt = PhysAddr::new(addr).to_virt();
        unsafe {
            self.free_lists[order] = *(virt.as_u64() as *const u64);
        }

        Some(addr as usize / PAGE_SIZE)
    }

    fn remove_from_free_list(&mut self, frame: usize, order: usize) -> bool {
        let target = frame_to_addr(frame);

        if self.free_lists[order] == 0 {
            return false;
        }

        // Check head
        if self.free_lists[order] == target {
            let virt = PhysAddr::new(target).to_virt();
            unsafe {
                self.free_lists[order] = *(virt.as_u64() as *const u64);
            }
            return true;
        }

        // Walk the list
        let mut current = self.free_lists[order];
        while current != 0 {
            let virt = PhysAddr::new(current).to_virt();
            let next = unsafe { *(virt.as_u64() as *const u64) };

            if next == target {
                let target_virt = PhysAddr::new(target).to_virt();
                let target_next = unsafe { *(target_virt.as_u64() as *const u64) };
                unsafe {
                    *(virt.as_u64() as *mut u64) = target_next;
                }
                return true;
            }
            current = next;
        }

        false
    }

    // --- Public API ---

    /// Allocate a block of 2^order frames
    pub fn allocate(&mut self, order: usize) -> Option<PhysAddr> {
        if !self.initialized || order > MAX_ORDER {
            return None;
        }

        // Find smallest available order >= requested
        let mut avail_order = order;
        while avail_order <= MAX_ORDER {
            if self.free_lists[avail_order] != 0 {
                break;
            }
            avail_order += 1;
        }

        if avail_order > MAX_ORDER {
            return None;
        }

        // Pop block from free list
        let frame = self.pop_free_list(avail_order)?;

        // Split down to requested order
        while avail_order > order {
            avail_order -= 1;
            let buddy_frame = frame + (1 << avail_order);
            self.push_free_list(buddy_frame, avail_order);
        }

        // Mark allocated in bitmap
        let count = 1usize << order;
        for i in 0..count {
            self.set_bit(frame + i);
        }
        self.free_frames -= count;

        Some(PhysAddr::new(frame_to_addr(frame)))
    }

    /// Deallocate a block of 2^order frames
    pub fn deallocate(&mut self, addr: PhysAddr, order: usize) {
        let frame = addr.as_u64() as usize / PAGE_SIZE;
        let count = 1usize << order;

        // Mark free in bitmap
        for i in 0..count {
            self.clear_bit(frame + i);
        }
        self.free_frames += count;

        // Try to merge with buddy up the order chain
        let mut current_order = order;
        let mut current_frame = frame;

        while current_order < MAX_ORDER {
            let buddy_frame = current_frame ^ (1 << current_order);

            // Check buddy block is entirely free
            let buddy_size = 1usize << current_order;
            let mut buddy_free = true;
            for i in 0..buddy_size {
                if buddy_frame + i >= MAX_FRAMES || !self.is_free(buddy_frame + i) {
                    buddy_free = false;
                    break;
                }
            }

            if !buddy_free {
                break;
            }

            // Remove buddy from its free list
            if !self.remove_from_free_list(buddy_frame, current_order) {
                break;
            }

            // Merge: use the lower address
            current_frame = current_frame.min(buddy_frame);
            current_order += 1;
        }

        // Add merged block to free list
        self.push_free_list(current_frame, current_order);
    }

    pub fn available_frames(&self) -> usize {
        self.free_frames
    }

    pub fn used_frames(&self) -> usize {
        self.total_frames - self.free_frames
    }

    pub fn total_frames(&self) -> usize {
        self.total_frames
    }
}

// --- Module-level helpers ---

fn frame_to_addr(frame: usize) -> u64 {
    (frame * PAGE_SIZE) as u64
}

fn ceil_div(a: usize, b: usize) -> usize {
    (a + b - 1) / b
}

/// Initialize frame allocator from usable memory regions
pub fn init(regions: &[(u64, u64)]) {
    ALLOCATOR.lock().init_from_regions(regions);
}

/// Allocate a single frame (order 0, 4 KiB)
pub fn allocate_frame() -> Option<PhysAddr> {
    ALLOCATOR.lock().allocate(0)
}

/// Deallocate a single frame
pub fn deallocate_frame(addr: PhysAddr) {
    ALLOCATOR.lock().deallocate(addr, 0);
}
