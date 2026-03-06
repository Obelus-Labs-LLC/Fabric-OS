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

// --- TD-010: Typed free-list pointer ---
//
// `FreeBlockPtr` encapsulates ALL unsafe raw-pointer access for the intrusive
// free list. Free blocks store a "next" physical address in their first 8 bytes,
// accessed via HHDM virtual mapping. In debug builds, a canary value is written
// at byte offset 8 to detect corruption early.

/// Canary value written after the next pointer in debug builds.
#[cfg(debug_assertions)]
const FREE_BLOCK_CANARY: u64 = 0xFAB1_CF8E_EB10_C000;

/// Typed pointer to a free block on a buddy allocator free list.
///
/// Wraps a physical address. Distinct from `PhysAddr` to prevent accidental
/// use as a regular address. All raw pointer operations are confined to
/// `read_next()` and `write_next()`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FreeBlockPtr(u64);

impl FreeBlockPtr {
    const NULL: Self = Self(0);

    /// Create a FreeBlockPtr from a frame index.
    #[inline]
    fn from_frame(frame: usize) -> Self {
        debug_assert!(frame < MAX_FRAMES, "[BUDDY] frame index out of bounds: {}", frame);
        Self((frame * PAGE_SIZE) as u64)
    }

    #[inline]
    fn is_null(self) -> bool {
        self.0 == 0
    }

    /// Convert back to a frame index.
    #[inline]
    fn to_frame(self) -> usize {
        let frame = self.0 as usize / PAGE_SIZE;
        debug_assert!(frame < MAX_FRAMES, "[BUDDY] frame from ptr out of bounds: {:#x}", self.0);
        frame
    }

    /// Read the next pointer from this free block's first 8 bytes via HHDM.
    ///
    /// # Safety
    /// `self` must be a valid, page-aligned physical address with a
    /// corresponding HHDM virtual mapping. The block must currently be
    /// on a free list (not allocated to a caller).
    #[inline]
    unsafe fn read_next(self) -> Self {
        debug_assert!(!self.is_null(), "[BUDDY] read_next on NULL pointer");
        let virt = PhysAddr::new(self.0).to_virt().as_u64();
        debug_assert!(virt % 8 == 0, "[BUDDY] free list pointer not 8-byte aligned: {:#x}", virt);

        #[cfg(debug_assertions)]
        {
            let canary = unsafe { *((virt + 8) as *const u64) };
            assert!(
                canary == FREE_BLOCK_CANARY,
                "[BUDDY] Canary corruption at {:#x}: expected {:#x}, found {:#x}",
                self.0, FREE_BLOCK_CANARY, canary,
            );
        }

        Self(unsafe { *(virt as *const u64) })
    }

    /// Write the next pointer into this free block's first 8 bytes via HHDM.
    ///
    /// # Safety
    /// `self` must be a valid, page-aligned physical address with a
    /// corresponding HHDM virtual mapping. The block must currently be
    /// on a free list (not allocated to a caller).
    #[inline]
    unsafe fn write_next(self, next: FreeBlockPtr) {
        debug_assert!(!self.is_null(), "[BUDDY] write_next on NULL pointer");
        let virt = PhysAddr::new(self.0).to_virt().as_u64();
        debug_assert!(virt % 8 == 0, "[BUDDY] free list pointer not 8-byte aligned: {:#x}", virt);

        unsafe { *(virt as *mut u64) = next.0; }

        #[cfg(debug_assertions)]
        unsafe { *((virt + 8) as *mut u64) = FREE_BLOCK_CANARY; }
    }
}

// --- Buddy Allocator ---

pub struct BuddyAllocator {
    /// Head of free list per order (typed pointer, NULL = empty)
    free_lists: [FreeBlockPtr; MAX_ORDER + 1],
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
            free_lists: [FreeBlockPtr::NULL; MAX_ORDER + 1],
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

    // --- Free list operations (TD-010: typed FreeBlockPtr) ---

    fn push_free_list(&mut self, frame: usize, order: usize) {
        debug_assert!(self.is_free(frame), "[BUDDY] push_free_list: frame {} not free in bitmap", frame);
        let ptr = FreeBlockPtr::from_frame(frame);
        // SAFETY: ptr is page-aligned (from frame index), mapped via HHDM,
        // and bitmap confirms block is free.
        unsafe { ptr.write_next(self.free_lists[order]); }
        self.free_lists[order] = ptr;
    }

    fn pop_free_list(&mut self, order: usize) -> Option<usize> {
        let head = self.free_lists[order];
        if head.is_null() { return None; }
        let frame = head.to_frame();
        debug_assert!(self.is_free(frame), "[BUDDY] pop_free_list: frame {} not free in bitmap", frame);
        // SAFETY: head was stored by push_free_list from a valid frame.
        self.free_lists[order] = unsafe { head.read_next() };
        Some(frame)
    }

    fn remove_from_free_list(&mut self, frame: usize, order: usize) -> bool {
        let target = FreeBlockPtr::from_frame(frame);
        debug_assert!(self.is_free(frame), "[BUDDY] remove_from_free_list: frame {} not free in bitmap", frame);

        if self.free_lists[order].is_null() { return false; }

        // Head removal
        if self.free_lists[order] == target {
            // SAFETY: target is page-aligned from frame index.
            self.free_lists[order] = unsafe { target.read_next() };
            return true;
        }

        // Walk to find and unlink target (bounded to prevent infinite loop)
        let mut current = self.free_lists[order];
        let mut iterations = 0usize;
        while !current.is_null() {
            iterations += 1;
            if iterations > MAX_FRAMES {
                panic!("[BUDDY] free list cycle detected at order {} after {} iterations", order, iterations);
            }
            // SAFETY: current was stored by push_free_list from a valid frame.
            let next = unsafe { current.read_next() };
            if next == target {
                // SAFETY: target/current are valid page-aligned addresses.
                let target_next = unsafe { target.read_next() };
                unsafe { current.write_next(target_next); }
                return true;
            }
            current = next;
        }
        false
    }

    // --- Debug integrity checker (TD-010) ---

    /// Verify all free lists for consistency (debug builds only).
    ///
    /// Checks: page alignment, bounds, canary integrity, bitmap agreement,
    /// and cycle detection. Expensive — called from allocate/deallocate in
    /// debug builds.
    #[cfg(debug_assertions)]
    fn verify_free_lists(&self) {
        for order in 0..=MAX_ORDER {
            let mut current = self.free_lists[order];
            let mut count = 0usize;
            while !current.is_null() {
                count += 1;
                if count > MAX_FRAMES {
                    panic!("[BUDDY] verify: cycle in order {} after {} nodes", order, count);
                }

                // Bounds check
                let phys = current.0;
                assert!(
                    phys < MAX_PHYS_ADDR && phys % PAGE_SIZE as u64 == 0,
                    "[BUDDY] verify: bad address {:#x} in order {} list", phys, order
                );

                // Bitmap check — block should be free
                let frame = current.to_frame();
                assert!(
                    self.is_free(frame),
                    "[BUDDY] verify: frame {} in order {} list but bitmap says allocated", frame, order
                );

                // Advance (read_next also checks canary in debug)
                current = unsafe { current.read_next() };
            }
        }
    }

    // --- Public API ---

    /// Allocate a block of 2^order frames
    pub fn allocate(&mut self, order: usize) -> Option<PhysAddr> {
        if !self.initialized || order > MAX_ORDER {
            return None;
        }

        #[cfg(debug_assertions)]
        self.verify_free_lists();

        // Find smallest available order >= requested
        let mut avail_order = order;
        while avail_order <= MAX_ORDER {
            if !self.free_lists[avail_order].is_null() {
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

        Some(PhysAddr::new(FreeBlockPtr::from_frame(frame).0))
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

        #[cfg(debug_assertions)]
        self.verify_free_lists();
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
