//! V8 Platform Interface for FabricOS
//!
//! Scenario D: Direct V8 port to FabricOS syscalls
//! Provides V8 JavaScript engine with OS services without std library
//!
//! Architecture:
//! - D2: memory.rs - DMA-based memory allocation with 4KB alignment
//! - D3: threads.rs - Thread creation/joining via scheduler
//! - D4: time.rs - Monotonic timing via timer syscalls
//! - D5: io.rs - Entropy and logging via kernel RNG/serial
//! - D6: mod.rs (this file) - Platform integration and FFI

#![no_std]

extern crate alloc;

// Conditional imports for real kernel vs stub mode
#[cfg(feature = "fabricos-kernel")]
use crate::dma::{DmaManager, DmaAllocation};
#[cfg(feature = "fabricos-kernel")]
use crate::irq::IrqRouter;
#[cfg(feature = "fabricos-kernel")]
use crate::scheduler::{Scheduler, ThreadId, ThreadPriority};
#[cfg(feature = "fabricos-kernel")]
use crate::syscall::{self, SysCallError};
#[cfg(feature = "fabricos-kernel")]
use crate::rng::KernelRng;

#[cfg(not(feature = "fabricos-kernel"))]
use stubs::{DmaManager, Scheduler, ThreadId, ThreadPriority, SysCallError, syscall, KernelRng};

#[cfg(not(feature = "fabricos-kernel"))]
pub mod stubs;

// Sub-modules (D2-D5)
pub mod memory;
pub mod threads;
pub mod time;
pub mod io;

// Re-export all public APIs
pub use memory::{
    v8_alloc, v8_free, v8_realloc,
    v8_alloc_executable, v8_alloc_large_pages,
    v8_protect_executable, v8_protect_readonly,
    init_memory_allocator, get_alloc_stats, AllocStats,
    V8MemoryType, V8Allocation, V8_PAGE_SIZE, V8_LARGE_PAGE_SIZE,
};

pub use threads::{
    v8_create_thread, v8_create_thread_with_priority,
    v8_join_thread, v8_yield, v8_current_thread,
    v8_set_thread_name, v8_sleep_ms, v8_sleep_us,
    set_v8_isolate, get_v8_isolate,
    V8ThreadFn, ThreadState, V8ThreadInfo, V8ThreadPool,
    V8_ISOLATE_TLS_KEY, MAX_V8_THREADS,
    init_threading, v8_active_thread_count,
};

pub use time::{
    v8_monotonic_time, v8_monotonic_time_ms, v8_monotonic_time_us,
    v8_sleep, v8_sleep_ns,
    v8_profile_timer, cycles_to_ns,
    V8Timer, V8Timeout,
    busy_wait_ns, busy_wait_us,
    init_time, NS_PER_SEC, NS_PER_MS, NS_PER_US,
};

pub use io::{
    v8_log_message, v8_read_entropy,
    v8_random_u32, v8_random_u64, v8_random_f64,
    v8_hash_seed, V8LogLevel, V8LogBuffer, V8SerialWriter,
    v8_open_file, v8_read_file, v8_close_file, V8FileHandle,
    init_io, check_entropy_quality,
};

/// V8 Platform error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V8PlatformError {
    MemoryAllocationFailed,
    ThreadCreationFailed,
    InvalidPointer,
    SyscallFailed(SysCallError),
}

/// Thread handle for V8
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V8ThreadId(pub ThreadId);

// ============================================================================
// Platform Singleton
// ============================================================================

/// Global platform instance
static mut V8_PLATFORM: Option<FabricOSV8Platform> = None;

/// V8 Platform configuration
#[derive(Debug, Clone)]
pub struct V8PlatformConfig {
    /// Number of worker threads for background tasks
    pub worker_threads: usize,
    /// Heap size limit in bytes
    pub heap_size_limit: usize,
    /// Enable huge pages for large objects
    pub use_huge_pages: bool,
    /// Log level for V8 messages
    pub log_level: V8LogLevel,
}

impl Default for V8PlatformConfig {
    fn default() -> Self {
        Self {
            worker_threads: 4,
            heap_size_limit: 512 * 1024 * 1024, // 512MB
            use_huge_pages: true,
            log_level: V8LogLevel::Info,
        }
    }
}

/// FabricOS V8 Platform singleton
/// 
/// This is the main interface that V8 uses to interact with the OS.
/// Implements the v8::Platform interface via FFI.
pub struct FabricOSV8Platform {
    config: V8PlatformConfig,
    initialized: bool,
}

impl FabricOSV8Platform {
    /// Create new platform instance with default config
    pub const fn new() -> Self {
        Self {
            config: V8PlatformConfig {
                worker_threads: 4,
                heap_size_limit: 512 * 1024 * 1024,
                use_huge_pages: true,
                log_level: V8LogLevel::Info,
            },
            initialized: false,
        }
    }
    
    /// Create with custom configuration
    pub fn with_config(config: V8PlatformConfig) -> Self {
        Self {
            config,
            initialized: false,
        }
    }
    
    /// Initialize the platform
    /// 
    /// Must be called before any V8 operations
    pub fn initialize(&mut self, dma: DmaManager) -> Result<(), V8PlatformError> {
        if self.initialized {
            return Ok(());
        }
        
        // Initialize subsystems
        unsafe {
            init_memory_allocator(dma);
            init_threading();
        }
        init_time();
        init_io();
        
        self.initialized = true;
        v8_log_message(V8LogLevel::Info, "FabricOS V8 Platform initialized");
        
        Ok(())
    }
    
    /// Shutdown the platform
    pub fn shutdown(&mut self) {
        if !self.initialized {
            return;
        }
        
        v8_log_message(V8LogLevel::Info, "FabricOS V8 Platform shutting down");
        self.initialized = false;
    }
    
    /// Check if platform is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
    
    /// Get configuration
    pub fn config(&self) -> &V8PlatformConfig {
        &self.config
    }
}

/// Initialize global platform
/// 
/// # Safety
/// Must be called exactly once before using get_platform()
pub unsafe fn init_platform() {
    V8_PLATFORM = Some(FabricOSV8Platform::new());
}

/// Get global platform instance
/// 
/// Returns None if init_platform() hasn't been called
pub fn get_platform() -> Option<&'static mut FabricOSV8Platform> {
    unsafe { V8_PLATFORM.as_mut() }
}

/// Convenience: Initialize V8 platform with DMA manager
/// 
/// This is the main entry point for V8 initialization
pub fn init_v8_platform(dma: DmaManager) -> Result<(), V8PlatformError> {
    unsafe {
        init_platform();
        if let Some(platform) = get_platform() {
            platform.initialize(dma)?;
        }
        Ok(())
    }
}

// ============================================================================
// Synchronization Primitives
// ============================================================================

use core::sync::atomic::{AtomicBool, Ordering};

/// Spinlock for V8 internal synchronization
/// 
/// Used when mutex would be too heavy or in no-alloc contexts
pub struct V8Spinlock {
    flag: AtomicBool,
}

impl V8Spinlock {
    /// Create new unlocked spinlock
    pub const fn new() -> Self {
        Self {
            flag: AtomicBool::new(false),
        }
    }
    
    /// Acquire lock (spins until available)
    pub fn lock(&self) {
        while self.flag.compare_exchange_weak(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_err() {
            core::hint::spin_loop();
        }
    }
    
    /// Release lock
    pub fn unlock(&self) {
        self.flag.store(false, Ordering::Release);
    }
    
    /// Try to acquire lock (non-blocking)
    pub fn try_lock(&self) -> bool {
        self.flag.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_ok()
    }
}

// Safety: V8Spinlock is Send + Sync
unsafe impl Send for V8Spinlock {}
unsafe impl Sync for V8Spinlock {}

/// Mutex using FabricOS futex/atomic operations
pub struct V8Mutex {
    locked: AtomicBool,
}

impl V8Mutex {
    /// Create new unlocked mutex
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }
    
    /// Acquire lock
    pub fn lock(&self) {
        while self.locked.compare_exchange_weak(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_err() {
            // Yield instead of pure spinning for mutex
            v8_yield();
        }
    }
    
    /// Release lock
    pub fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
    
    /// Try to acquire lock
    pub fn try_lock(&self) -> bool {
        self.locked.compare_exchange(
            false,
            true,
            Ordering::Acquire,
            Ordering::Relaxed
        ).is_ok()
    }
}

// Safety: V8Mutex is Send + Sync
unsafe impl Send for V8Mutex {}
unsafe impl Sync for V8Mutex {}

// ============================================================================
// FFI Interface for V8 C++ Integration
// ============================================================================

/// C-compatible platform functions for V8 FFI
/// 
/// These functions have C calling convention and can be called from V8 C++ code
pub mod ffi {
    use super::*;
    use core::ffi::{c_void, c_char, c_int, c_uint, c_ulonglong};
    
    /// Allocate memory (C API)
    #[no_mangle]
    pub extern "C" fn v8_fabricos_alloc(size: usize) -> *mut u8 {
        v8_alloc(size)
    }
    
    /// Free memory (C API)
    #[no_mangle]
    pub unsafe extern "C" fn v8_fabricos_free(ptr: *mut u8, size: usize) {
        v8_free(ptr, size);
    }
    
    /// Get monotonic time in nanoseconds (C API)
    #[no_mangle]
    pub extern "C" fn v8_fabricos_monotonic_time() -> c_ulonglong {
        v8_monotonic_time()
    }
    
    /// Sleep in milliseconds (C API)
    #[no_mangle]
    pub extern "C" fn v8_fabricos_sleep(ms: c_uint) {
        v8_sleep(ms);
    }
    
    /// Read entropy (C API)
    #[no_mangle]
    pub unsafe extern "C" fn v8_fabricos_read_entropy(buf: *mut u8, len: usize) {
        if !buf.is_null() {
            let slice = core::slice::from_raw_parts_mut(buf, len);
            v8_read_entropy(slice);
        }
    }
    
    /// Log message (C API)
    #[no_mangle]
    pub unsafe extern "C" fn v8_fabricos_log(level: c_int, msg: *const c_char) {
        if msg.is_null() {
            return;
        }
        
        let level = match level {
            0 => V8LogLevel::Verbose,
            1 => V8LogLevel::Debug,
            2 => V8LogLevel::Info,
            3 => V8LogLevel::Warning,
            4 => V8LogLevel::Error,
            5 => V8LogLevel::Fatal,
            _ => V8LogLevel::Info,
        };
        
        // Convert C string to Rust string (limited length)
        let mut len = 0;
        while *msg.add(len) != 0 && len < 256 {
            len += 1;
        }
        
        let bytes = core::slice::from_raw_parts(msg as *const u8, len);
        if let Ok(s) = core::str::from_utf8(bytes) {
            v8_log_message(level, s);
        }
    }
    
    /// Create thread (C API)
    #[no_mangle]
    pub unsafe extern "C" fn v8_fabricos_create_thread(
        entry: extern "C" fn(*mut c_void),
        arg: *mut c_void,
    ) -> c_ulonglong {
        match v8_create_thread(entry, arg) {
            Ok(id) => id.0,
            Err(_) => 0,
        }
    }
    
    /// Join thread (C API)
    #[no_mangle]
    pub extern "C" fn v8_fabricos_join_thread(id: c_ulonglong) -> c_int {
        match v8_join_thread(V8ThreadId(id)) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
    
    /// Yield thread (C API)
    #[no_mangle]
    pub extern "C" fn v8_fabricos_yield() {
        v8_yield();
    }
    
    /// Get random u64 (C API)
    #[no_mangle]
    pub extern "C" fn v8_fabricos_random_u64() -> c_ulonglong {
        v8_random_u64()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_spinlock() {
        let lock = V8Spinlock::new();
        assert!(lock.try_lock());
        lock.unlock();
        lock.lock();
        lock.unlock();
    }
    
    #[test]
    fn test_mutex() {
        let mutex = V8Mutex::new();
        assert!(mutex.try_lock());
        mutex.unlock();
        mutex.lock();
        mutex.unlock();
    }
    
    #[test]
    fn test_platform_config_default() {
        let config = V8PlatformConfig::default();
        assert_eq!(config.worker_threads, 4);
        assert_eq!(config.heap_size_limit, 512 * 1024 * 1024);
        assert!(config.use_huge_pages);
    }
    
    #[test]
    fn test_ffi_functions_exist() {
        // Just verify FFI functions are callable
        let _ = ffi::v8_fabricos_monotonic_time;
        let _ = ffi::v8_fabricos_yield;
        let _ = ffi::v8_fabricos_random_u64;
    }
}
