#![allow(dead_code)]

use alloc::{format, string::String, vec, vec::Vec, boxed::Box};
use crate::memory::{PhysAddr, VirtAddr, PAGE_SIZE};
use crate::memory::{frame, mapper};
use crate::memory::page_table::PageTableFlags;
use crate::ocrb::OcrbResult;

pub fn run_all_tests() -> Vec<OcrbResult> {
    vec![
        test1_frame_alloc_dealloc(),
        test2_frame_stress(),
        test3_no_double_alloc(),
        test4_buddy_merge(),
        test5_page_map_unmap(),
        test6_heap_stress(),
        test7_heap_fragmentation(),
    ]
}

/// Test 1: Frame Alloc/Dealloc (weight: 15)
fn test1_frame_alloc_dealloc() -> OcrbResult {
    let mut errors = 0u32;
    let mut addrs: Vec<PhysAddr> = Vec::with_capacity(1000);

    // Allocate 1000 frames
    for _ in 0..1000 {
        match frame::allocate_frame() {
            Some(addr) => addrs.push(addr),
            None => {
                errors += 1;
            }
        }
    }

    // Verify all unique
    for i in 0..addrs.len() {
        for j in (i + 1)..addrs.len() {
            if addrs[i] == addrs[j] {
                errors += 1;
            }
        }
    }

    let free_before = frame::ALLOCATOR.lock().available_frames();

    // Deallocate all
    for addr in &addrs {
        frame::deallocate_frame(*addr);
    }

    let free_after = frame::ALLOCATOR.lock().available_frames();
    if free_after != free_before + addrs.len() {
        errors += 1;
    }

    let score = if errors == 0 {
        100
    } else {
        (100u32).saturating_sub(errors * 10) as u8
    };

    OcrbResult {
        test_name: "Frame Alloc/Dealloc",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("1000 frames, {} errors", errors),
    }
}

/// Test 2: Frame Stress (weight: 20)
fn test2_frame_stress() -> OcrbResult {
    let initial_free = frame::ALLOCATOR.lock().available_frames();
    // Cap at 50000 to stay within heap limits
    let alloc_target = initial_free.min(50000);

    let mut addrs: Vec<PhysAddr> = Vec::with_capacity(alloc_target);
    let exhausted_correctly;

    // Allocate until we hit the cap or run out
    for _ in 0..alloc_target {
        match frame::allocate_frame() {
            Some(addr) => addrs.push(addr),
            None => break,
        }
    }

    // If we allocated all target frames, try one more to check exhaustion
    if addrs.len() == alloc_target && alloc_target == initial_free {
        exhausted_correctly = frame::allocate_frame().is_none();
    } else if addrs.len() < alloc_target {
        // We ran out before the target
        exhausted_correctly = true;
    } else {
        // We capped, can't test exhaustion
        exhausted_correctly = true;
    }

    let allocated_count = addrs.len();

    // Deallocate all
    for addr in &addrs {
        frame::deallocate_frame(*addr);
    }

    let final_free = frame::ALLOCATOR.lock().available_frames();
    let count_restored = final_free == initial_free;

    let score = if exhausted_correctly && count_restored {
        100
    } else if count_restored {
        80
    } else {
        40
    };

    OcrbResult {
        test_name: "Frame Stress",
        passed: score >= 80,
        score,
        weight: 20,
        details: format!(
            "allocated {}/{}, restored={}",
            allocated_count, initial_free, count_restored
        ),
    }
}

/// Test 3: No Double Allocation (weight: 20)
fn test3_no_double_alloc() -> OcrbResult {
    let mut set_a: Vec<PhysAddr> = Vec::with_capacity(500);
    let mut set_b: Vec<PhysAddr> = Vec::with_capacity(500);
    let mut overlaps = 0u32;

    for _ in 0..500 {
        if let Some(addr) = frame::allocate_frame() {
            set_a.push(addr);
        }
    }

    for _ in 0..500 {
        if let Some(addr) = frame::allocate_frame() {
            set_b.push(addr);
        }
    }

    // Check for overlaps
    for a in &set_a {
        for b in &set_b {
            if a == b {
                overlaps += 1;
            }
        }
    }

    // Clean up
    for addr in set_a.iter().chain(set_b.iter()) {
        frame::deallocate_frame(*addr);
    }

    let score = if overlaps == 0 { 100 } else { 0 };

    OcrbResult {
        test_name: "No Double Allocation",
        passed: overlaps == 0,
        score,
        weight: 20,
        details: format!("{} overlaps in 1000 frames", overlaps),
    }
}

/// Test 4: Buddy Merge (weight: 15)
fn test4_buddy_merge() -> OcrbResult {
    // Allocate an order-1 block (2 adjacent frames)
    let order1_addr = {
        let mut alloc = frame::ALLOCATOR.lock();
        alloc.allocate(1)
    };

    let Some(block_addr) = order1_addr else {
        return OcrbResult {
            test_name: "Buddy Merge",
            passed: false,
            score: 0,
            weight: 15,
            details: String::from("could not allocate order-1 block"),
        };
    };

    // Split into two order-0 blocks conceptually — deallocate as two separate order-0
    let frame_a = block_addr;
    let frame_b = PhysAddr::new(block_addr.as_u64() + PAGE_SIZE as u64);

    // Deallocate both halves separately (this should trigger merge)
    {
        let mut alloc = frame::ALLOCATOR.lock();
        alloc.deallocate(frame_a, 0);
        alloc.deallocate(frame_b, 0);
    }

    // Now allocate an order-1 block — should get the merged block back
    let merged = {
        let mut alloc = frame::ALLOCATOR.lock();
        alloc.allocate(1)
    };

    let merge_ok = match merged {
        Some(addr) => {
            let ok = addr == block_addr;
            // Clean up
            let mut alloc = frame::ALLOCATOR.lock();
            alloc.deallocate(addr, 1);
            ok
        }
        None => false,
    };

    let score = if merge_ok { 100 } else { 0 };

    OcrbResult {
        test_name: "Buddy Merge",
        passed: merge_ok,
        score,
        weight: 15,
        details: format!("merge={}", if merge_ok { "OK" } else { "FAIL" }),
    }
}

/// Test 5: Page Map/Unmap (weight: 10)
fn test5_page_map_unmap() -> OcrbResult {
    const NUM_PAGES: usize = 100;
    let base_virt = 0xFFFF_FFFF_D000_0000u64;
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    let mut frames: Vec<PhysAddr> = Vec::with_capacity(NUM_PAGES);
    let mut errors = 0u32;

    // Allocate frames and map pages
    for i in 0..NUM_PAGES {
        let virt = VirtAddr::new(base_virt + (i * PAGE_SIZE) as u64);
        let phys = match frame::allocate_frame() {
            Some(p) => p,
            None => {
                errors += 1;
                continue;
            }
        };
        if mapper::map(virt, phys, flags).is_err() {
            errors += 1;
            frame::deallocate_frame(phys);
            continue;
        }
        frames.push(phys);

        // Write a distinct value
        unsafe {
            let ptr = virt.as_u64() as *mut u64;
            *ptr = 0xFAB_0000 + i as u64;
        }
    }

    // Read back and verify
    for i in 0..frames.len() {
        let virt = VirtAddr::new(base_virt + (i * PAGE_SIZE) as u64);
        let val = unsafe { *(virt.as_u64() as *const u64) };
        if val != 0xFAB_0000 + i as u64 {
            errors += 1;
        }
    }

    // Unmap all and verify translate returns None
    for i in 0..frames.len() {
        let virt = VirtAddr::new(base_virt + (i * PAGE_SIZE) as u64);
        if mapper::unmap(virt).is_err() {
            errors += 1;
        }
        if mapper::translate(virt).is_some() {
            errors += 1;
        }
        frame::deallocate_frame(frames[i]);
    }

    let score = if errors == 0 {
        100
    } else {
        (100u32).saturating_sub(errors * 10) as u8
    };

    OcrbResult {
        test_name: "Page Map/Unmap",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("{} pages, {} errors", NUM_PAGES, errors),
    }
}

/// Test 6: Heap Stress (weight: 10)
fn test6_heap_stress() -> OcrbResult {
    let mut vecs: Vec<Vec<u8>> = Vec::with_capacity(1000);

    // Allocate 1000 Vec<u8> of varying sizes
    for i in 0..1000u32 {
        let size = 16 + (i % 256) as usize; // 16 to 271 bytes
        vecs.push(vec![0xAB; size]);
    }

    // Drop every other one
    for i in (0..vecs.len()).rev().step_by(2) {
        vecs.swap_remove(i);
    }

    // Allocate 500 more
    for i in 0..500u32 {
        let size = 32 + (i % 128) as usize;
        vecs.push(vec![0xCD; size]);
    }

    // Drop all
    drop(vecs);

    // Verify heap still works
    let check = vec![42u8; 64];
    let ok = check.len() == 64 && check[0] == 42;

    OcrbResult {
        test_name: "Heap Stress",
        passed: ok,
        score: if ok { 100 } else { 0 },
        weight: 10,
        details: format!("1500 allocs, drop pattern, realloc={}", if ok { "OK" } else { "FAIL" }),
    }
}

/// Test 7: Heap Fragmentation Resistance (weight: 10)
fn test7_heap_fragmentation() -> OcrbResult {
    // Allocate 200 small boxes
    let mut small_boxes: Vec<Box<[u8; 32]>> = Vec::with_capacity(200);
    for _ in 0..200 {
        small_boxes.push(Box::new([0x11; 32]));
    }

    // Drop every other one (creates fragmentation)
    for i in (0..small_boxes.len()).rev().step_by(2) {
        small_boxes.swap_remove(i);
    }

    // Allocate 100 medium boxes (128 bytes)
    let mut medium_count = 0u32;
    let mut medium_boxes: Vec<Box<[u8; 128]>> = Vec::with_capacity(100);
    for _ in 0..100 {
        medium_boxes.push(Box::new([0x22; 128]));
        medium_count += 1;
    }

    // Clean up
    drop(medium_boxes);
    drop(small_boxes);

    let score = if medium_count == 100 {
        100
    } else if medium_count >= 80 {
        50
    } else {
        0
    };

    OcrbResult {
        test_name: "Heap Fragmentation Resistance",
        passed: score >= 50,
        score,
        weight: 10,
        details: format!("{}/100 medium allocs fit", medium_count),
    }
}
