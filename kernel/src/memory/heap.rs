#![allow(dead_code)]

use linked_list_allocator::LockedHeap;
use crate::memory::{VirtAddr, PAGE_SIZE};
use crate::memory::page_table::PageTableFlags;
use crate::memory::{frame, mapper};
use crate::serial_println;

/// Heap starts after the kernel's mapped region
pub const HEAP_START: u64 = 0xFFFF_FFFF_8040_0000;
/// 4 MiB heap (Phase 1: 10K tokens × ~120B + 3 BTreeMaps + OCRB overhead)
pub const HEAP_SIZE: usize = 4 * 1024 * 1024;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Initialize the kernel heap: map pages and init the allocator
pub fn init() {
    let pages = HEAP_SIZE / PAGE_SIZE;
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    for i in 0..pages {
        let virt = VirtAddr::new(HEAP_START + (i * PAGE_SIZE) as u64);
        let phys = frame::allocate_frame().expect("heap: out of frames");
        mapper::map(virt, phys, flags).expect("heap: map failed");
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
    }

    serial_println!(
        "[HEAP] Kernel heap initialized: 0x{:x} - 0x{:x} ({} KiB)",
        HEAP_START,
        HEAP_START + HEAP_SIZE as u64,
        HEAP_SIZE / 1024
    );
}
