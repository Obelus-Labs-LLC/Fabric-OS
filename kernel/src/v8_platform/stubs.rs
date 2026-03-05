//! Stub implementations for FabricOS kernel modules
//! 
//! These stubs allow the V8 platform interface to compile
//! without the full FabricOS kernel source.
//! 
//! In a real build, these would be provided by:
//! - crate::dma::DmaManager
//! - crate::scheduler::Scheduler
//! - crate::syscall
//! - crate::rng::KernelRng

#![no_std]

use core::ptr::NonNull;

// ============================================================================
// DMA Manager Stub
// ============================================================================

pub struct DmaManager {
    base_addr: usize,
    size: usize,
    next_alloc: usize,
}

#[derive(Debug)]
pub struct DmaAllocation {
    ptr: NonNull<u8>,
    size: usize,
}

impl DmaAllocation {
    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }
}

impl DmaManager {
    pub fn new(base_addr: usize, size: usize) -> Self {
        Self {
            base_addr,
            size,
            next_alloc: base_addr,
        }
    }
    
    pub fn allocate(&self, size: usize) -> Option<DmaAllocation> {
        // Stub: Just return a fake pointer
        // Real implementation would manage physical memory
        let ptr = self.next_alloc as *mut u8;
        Some(DmaAllocation {
            ptr: NonNull::new(ptr)?,
            size,
        })
    }
    
    /// Allocate executable pages for JIT code
    pub fn allocate_executable(&self, size: usize) -> Option<DmaAllocation> {
        // Stub: Same as regular allocate
        self.allocate(size)
    }
    
    /// Allocate huge pages (2MB) for large objects
    pub fn allocate_huge(&self, size: usize) -> Option<DmaAllocation> {
        // Stub: Same as regular allocate
        self.allocate(size)
    }
    
    pub fn deallocate(&self, addr: usize, size: usize) {
        // Stub: No-op
        let _ = addr;
        let _ = size;
    }
    
    /// Make memory executable (RX permissions)
    pub fn protect_executable(&self, _addr: usize, _size: usize) -> Result<(), SysCallError> {
        // Stub: Success
        Ok(())
    }
    
    /// Make memory read-only (R permissions)
    pub fn protect_readonly(&self, _addr: usize, _size: usize) -> Result<(), SysCallError> {
        // Stub: Success
        Ok(())
    }
}

// ============================================================================
// Scheduler Stub
// ============================================================================

pub type ThreadId = u64;

#[derive(Debug, Clone, Copy)]
pub enum ThreadPriority {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

pub struct Scheduler;

impl Scheduler {
    pub fn create_thread(
        id: u64,
        entry: extern "C" fn(*mut core::ffi::c_void),
        arg: *mut core::ffi::c_void,
        priority: ThreadPriority,
    ) -> Result<ThreadId, SysCallError> {
        let _ = entry;
        let _ = arg;
        let _ = priority;
        Ok(id)
    }
    
    pub fn join_thread(id: ThreadId) -> Result<(), SysCallError> {
        let _ = id;
        Ok(())
    }
    
    pub fn yield_current() {
        // Stub: No-op
    }
    
    pub fn current_thread_id() -> ThreadId {
        // Stub: Return main thread ID
        1
    }
    
    pub fn should_reschedule() -> bool {
        false
    }
}

// ============================================================================
// Syscall Stub
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysCallError {
    InvalidArgument,
    NoMemory,
    NotImplemented,
    PermissionDenied,
}

pub mod syscall {
    use super::SysCallError;
    
    /// Get monotonic time in nanoseconds (syscall 23)
    pub fn get_time_ns() -> Result<u64, SysCallError> {
        // Stub: Return fake time based on instruction count approximation
        // Real: Would use kernel's timer subsystem
        Ok(unsafe { core::arch::x86_64::_rdtsc() })
    }
    
    /// Create new thread (syscall 30)
    pub fn thread_create(entry: usize, arg: usize, priority: u8) -> u32 {
        let _ = entry;
        let _ = arg;
        let _ = priority;
        // Stub: Return fake thread ID
        1
    }
    
    /// Yield current thread (syscall 31)
    pub fn thread_yield() {
        // Stub: No-op
    }
    
    /// Set thread name (syscall 32)
    pub fn set_thread_name(_name: *const u8) -> u32 {
        // Stub: Success
        0
    }
    
    /// Sleep for milliseconds (syscall 33)
    pub fn sleep_ms(_ms: u64) {
        // Stub: No-op
    }
    
    /// Sleep for microseconds (syscall 34)
    pub fn sleep_us(_us: u64) {
        // Stub: No-op
    }
    
    /// Set TLS value (syscall 35)
    pub fn set_tls(_key: usize, value: usize) {
        // Stub: Store in fake TLS
        let _ = value;
    }
    
    /// Get TLS value (syscall 36)
    pub fn get_tls(_key: usize) -> usize {
        // Stub: Return 0
        0
    }
    
    /// Write byte to serial port (syscall 40)
    pub fn serial_write(b: u8) {
        // Stub: No-op (would write to UART)
        let _ = b;
    }
    
    /// Open file (syscall 41)
    pub fn open(_path: *const u8, _flags: u32) -> i32 {
        // Stub: Return fake fd
        3
    }
    
    /// Read from file (syscall 42)
    pub fn read(_fd: i32, _buf: *mut u8, _len: usize) -> i32 {
        // Stub: Return 0 bytes
        0
    }
    
    /// Close file (syscall 43)
    pub fn close(_fd: i32) {
        // Stub: No-op
    }
}

// ============================================================================
// RNG Stub
// ============================================================================

pub struct KernelRng;

impl KernelRng {
    pub fn init() -> Result<(), super::V8PlatformError> {
        Ok(())
    }
    
    pub fn fill_bytes(buf: &mut [u8]) {
        // Stub: Fill with pseudo-random based on RDTSC
        // Real: Would use hardware RNG or entropy pool
        for (i, byte) in buf.iter_mut().enumerate() {
            let tsc = unsafe { core::arch::x86_64::_rdtsc() };
            *byte = (tsc.wrapping_add(i as u64)) as u8;
        }
    }
}

// ============================================================================
// IRQ Router Stub
// ============================================================================

pub struct IrqRouter;

impl IrqRouter {
    pub fn register_handler(_irq: u8, _handler: fn()) {
        // Stub
    }
}
