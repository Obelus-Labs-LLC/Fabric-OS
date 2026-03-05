//! FabricOS Kernel
//!
//! Microkernel with V8 JavaScript Platform support

#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod v8_platform;

// Re-export all V8 platform types for convenience
pub use v8_platform::{
    // Core types
    V8PlatformError, V8ThreadId, V8PlatformConfig,
    FabricOSV8Platform,
    init_platform, get_platform, init_v8_platform,
    
    // Memory
    v8_alloc, v8_free, v8_realloc,
    v8_alloc_executable, v8_alloc_large_pages,
    v8_protect_executable, v8_protect_readonly,
    init_memory_allocator, get_alloc_stats, AllocStats,
    V8MemoryType, V8Allocation, V8_PAGE_SIZE, V8_LARGE_PAGE_SIZE,
    
    // Threads
    v8_create_thread, v8_create_thread_with_priority,
    v8_join_thread, v8_yield, v8_current_thread,
    v8_set_thread_name, v8_sleep_ms, v8_sleep_us,
    set_v8_isolate, get_v8_isolate,
    V8ThreadFn, ThreadState, V8ThreadInfo, V8ThreadPool,
    V8_ISOLATE_TLS_KEY, MAX_V8_THREADS,
    init_threading, v8_active_thread_count,
    
    // Time
    v8_monotonic_time, v8_monotonic_time_ms, v8_monotonic_time_us,
    v8_sleep, v8_sleep_ns,
    v8_profile_timer, cycles_to_ns,
    V8Timer, V8Timeout,
    busy_wait_ns, busy_wait_us,
    init_time, NS_PER_SEC, NS_PER_MS, NS_PER_US,
    
    // I/O
    v8_log_message, v8_read_entropy,
    v8_random_u32, v8_random_u64, v8_random_f64,
    v8_hash_seed, V8LogLevel, V8LogBuffer, V8SerialWriter,
    v8_open_file, v8_read_file, v8_close_file, V8FileHandle,
    init_io, check_entropy_quality,
    
    // Sync
    V8Spinlock, V8Mutex,
    
    // FFI
    ffi,
};

// Re-export stubs for testing
#[cfg(not(feature = "fabricos-kernel"))]
pub use v8_platform::stubs;

/// Allocation error handler for #[no_std] environments
#[cfg(not(feature = "fabricos-kernel"))]
#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!("Kernel allocation failed: {:?}", layout);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_v8_platform_exports() {
        // Just verify types are exported
        let _ = v8_monotonic_time();
    }
}
