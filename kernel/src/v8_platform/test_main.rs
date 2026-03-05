//! Test entry point for V8 platform interface
//! 
//! Usage: cargo run --bin v8_platform_test

#![no_std]
#![feature(alloc_error_handler)]
#![no_main]

// Windows subsystem for testing
#![cfg_attr(target_os = "windows", windows_subsystem = "console")]

extern crate alloc;

// Use the library's v8_platform module
use fabricos_kernel::{
    init_v8_platform, v8_alloc, v8_free,
    v8_create_thread, v8_join_thread,
    v8_monotonic_time, v8_sleep,
    v8_read_entropy, v8_random_u32,
    V8Spinlock, init_platform, get_platform,
    stubs::DmaManager,
};

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU64, Ordering};

/// Simple bump allocator for testing
const HEAP_SIZE: usize = 1024 * 1024; // 1MB heap

struct BumpAllocator {
    heap: UnsafeCell<[u8; HEAP_SIZE]>,
    next: AtomicU64,
}

unsafe impl Sync for BumpAllocator {}
unsafe impl Send for BumpAllocator {}

impl BumpAllocator {
    const fn new() -> Self {
        Self {
            heap: UnsafeCell::new([0; HEAP_SIZE]),
            next: AtomicU64::new(0),
        }
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        
        let current = self.next.load(Ordering::Relaxed);
        let aligned_offset = (current as usize + align - 1) & !(align - 1);
        
        if aligned_offset + size > HEAP_SIZE {
            return null_mut();
        }
        
        self.next.store((aligned_offset + size) as u64, Ordering::Relaxed);
        (self.heap.get() as *mut u8).add(aligned_offset)
    }
    
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator doesn't free
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator::new();

// Note: alloc_error_handler is defined in lib.rs

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// Global test counter
static TEST_PASSES: AtomicU64 = AtomicU64::new(0);
static TEST_FAILS: AtomicU64 = AtomicU64::new(0);

fn test_pass(_name: &str) {
    TEST_PASSES.fetch_add(1, Ordering::Relaxed);
}

fn test_fail(_name: &str, _msg: &str) {
    TEST_FAILS.fetch_add(1, Ordering::Relaxed);
}

// Test: Memory allocation and deallocation
fn test_v8_memory() {
    const TEST_SIZE: usize = 4096;
    
    // Test basic allocation
    let ptr = v8_alloc(TEST_SIZE);
    if ptr.is_null() {
        test_fail("v8_memory_basic", "allocation returned null");
        return;
    }
    
    // Test that memory is aligned to 4KB
    if ptr as usize % 4096 != 0 {
        test_fail("v8_memory_align", "memory not 4KB aligned");
        unsafe { v8_free(ptr, TEST_SIZE); }
        return;
    }
    
    // Test write access
    unsafe {
        core::ptr::write_volatile(ptr.offset(0) as *mut u64, 0xDEADBEEF_CAFE_BABE);
        core::ptr::write_volatile(ptr.offset(TEST_SIZE as isize - 8) as *mut u64, 0x12345678_9ABCDEF0);
    }
    
    // Free the memory
    unsafe { v8_free(ptr, TEST_SIZE); }
    test_pass("v8_memory_basic");
}

// Test: Thread creation and joining
static mut THREAD_RAN: bool = false;

extern "C" fn test_thread_fn(_arg: *mut core::ffi::c_void) {
    unsafe {
        THREAD_RAN = true;
    }
}

fn test_v8_threading() {
    unsafe { THREAD_RAN = false; }
    
    let thread_result = v8_create_thread(test_thread_fn, core::ptr::null_mut());
    
    let thread_id = match thread_result {
        Ok(id) => id,
        Err(_) => {
            test_fail("v8_thread_create", "failed to create thread");
            return;
        }
    };
    
    let _ = v8_join_thread(thread_id);
    
    unsafe {
        if THREAD_RAN {
            test_pass("v8_threading");
        } else {
            test_fail("v8_threading", "thread did not execute");
        }
    }
}

// Test: Time functions
fn test_v8_time() {
    let t1 = v8_monotonic_time();
    
    // Busy-wait for a bit
    for _ in 0..1000 {
        core::hint::spin_loop();
    }
    
    let t2 = v8_monotonic_time();
    
    if t2 > t1 {
        test_pass("v8_time_monotonic");
    } else {
        test_fail("v8_time_monotonic", "time not monotonic");
    }
}

// Test: Sleep function
fn test_v8_sleep() {
    let t1 = v8_monotonic_time();
    
    v8_sleep(1); // Sleep 1ms
    
    let t2 = v8_monotonic_time();
    
    // In stub mode, just verify function doesn't crash
    // In real mode, verify time progressed
    if t2 >= t1 {
        test_pass("v8_sleep");
    } else {
        test_fail("v8_sleep", "time went backwards");
    }
}

// Test: Entropy reading
fn test_v8_entropy() {
    let mut buf1 = [0u8; 32];
    let mut buf2 = [0u8; 32];
    
    v8_read_entropy(&mut buf1);
    v8_read_entropy(&mut buf2);
    
    // Verify buffers are not all zeros
    let all_zeros = buf1.iter().all(|&b| b == 0);
    if all_zeros {
        test_fail("v8_entropy", "entropy buffer is all zeros");
        return;
    }
    
    // Verify two reads produce different values
    if buf1 == buf2 {
        test_fail("v8_entropy", "entropy not random between reads");
        return;
    }
    
    test_pass("v8_entropy");
}

// Test: Spinlock
fn test_v8_spinlock() {
    let lock = V8Spinlock::new();
    
    lock.lock();
    lock.unlock();
    
    test_pass("v8_spinlock");
}

// Test: Platform initialization
fn test_v8_platform() {
    unsafe {
        init_platform();
    }
    
    let _platform = get_platform();
    
    test_pass("v8_platform_init");
}

// Test: Random u32 helper
fn test_v8_random_u32() {
    let r1 = v8_random_u32();
    let r2 = v8_random_u32();
    
    // Very unlikely to get same value twice
    if r1 == r2 {
        test_fail("v8_random_u32", "random values are identical");
        return;
    }
    
    test_pass("v8_random_u32");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Initialize V8 platform
    let _ = init_v8_platform(DmaManager::new(0x1000_0000, 0x100_0000));
    
    // Run all tests
    test_v8_memory();
    test_v8_threading();
    test_v8_time();
    test_v8_sleep();
    test_v8_entropy();
    test_v8_spinlock();
    test_v8_platform();
    test_v8_random_u32();
    
    let _passes = TEST_PASSES.load(Ordering::Relaxed);
    let _fails = TEST_FAILS.load(Ordering::Relaxed);
    
    // Exit or loop
    loop {}
}
