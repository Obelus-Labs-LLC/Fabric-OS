//! V8 Threading for FabricOS
//!
//! Provides thread creation, joining, and thread-local storage for V8 isolates.
//! Uses FabricOS scheduler for lightweight process spawning.

use core::ffi::c_void;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use super::stubs::{Scheduler, ThreadId, ThreadPriority, SysCallError, syscall};
use super::{V8PlatformError, V8ThreadId};

/// Thread-local storage key for V8 isolate pointer
pub const V8_ISOLATE_TLS_KEY: usize = 0x5648_0001; // "VH" + 0001

/// Thread-local storage key for V8 thread ID
pub const V8_THREAD_ID_TLS_KEY: usize = 0x5648_0002;

/// Maximum number of V8 worker threads
pub const MAX_V8_THREADS: usize = 64;

/// Thread entry point wrapper
pub type V8ThreadFn = extern "C" fn(*mut c_void);

/// Thread state tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Created,
    Running,
    Finished,
    Joined,
}

/// V8 thread information
#[derive(Debug, Clone, Copy)]
pub struct V8ThreadInfo {
    pub id: V8ThreadId,
    pub state: ThreadState,
    pub priority: ThreadPriority,
    pub stack_size: usize,
    pub entry: Option<V8ThreadFn>,
    pub arg: *mut c_void,
}

// Safety: Send/Sync needed for static storage
unsafe impl Send for V8ThreadInfo {}
unsafe impl Sync for V8ThreadInfo {}

/// Thread ID counter (starts at 1000 to avoid conflicts)
static THREAD_COUNTER: AtomicU64 = AtomicU64::new(1000);

/// Active thread table (simplified - real impl would use hash map)
static mut THREAD_TABLE: [Option<V8ThreadInfo>; MAX_V8_THREADS] = [None; MAX_V8_THREADS];

/// Number of active threads
static ACTIVE_THREAD_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Initialize threading subsystem
/// 
/// # Safety
/// Must be called exactly once during kernel initialization
pub unsafe fn init_threading() {
    // Clear thread table
    for i in 0..MAX_V8_THREADS {
        THREAD_TABLE[i] = None;
    }
    ACTIVE_THREAD_COUNT.store(0, Ordering::SeqCst);
}

/// Generate new thread ID
fn next_thread_id() -> V8ThreadId {
    V8ThreadId(THREAD_COUNTER.fetch_add(1, Ordering::SeqCst))
}

/// Find free slot in thread table
fn find_free_thread_slot() -> Option<usize> {
    unsafe {
        for i in 0..MAX_V8_THREADS {
            if THREAD_TABLE[i].is_none() {
                return Some(i);
            }
        }
    }
    None
}

/// Store thread info in table
/// 
/// # Safety
/// Must hold thread table lock or be during init
unsafe fn store_thread_info(info: V8ThreadInfo) -> Result<usize, V8PlatformError> {
    let slot = find_free_thread_slot()
        .ok_or(V8PlatformError::ThreadCreationFailed)?;
    
    THREAD_TABLE[slot] = Some(info);
    ACTIVE_THREAD_COUNT.fetch_add(1, Ordering::SeqCst);
    
    Ok(slot)
}

/// Get thread info by ID
fn get_thread_info(id: V8ThreadId) -> Option<&'static mut V8ThreadInfo> {
    unsafe {
        for i in 0..MAX_V8_THREADS {
            if let Some(ref mut info) = THREAD_TABLE[i] {
                if info.id == id {
                    return Some(info);
                }
            }
        }
    }
    None
}

/// Thread entry trampoline
/// 
/// This wrapper is called by the FabricOS scheduler when the thread starts.
extern "C" fn thread_trampoline(slot: usize) {
    unsafe {
        if let Some(ref mut info) = THREAD_TABLE[slot] {
            // Update state
            info.state = ThreadState::Running;
            
            // Store thread ID in TLS
            set_thread_local(V8_THREAD_ID_TLS_KEY, info.id.0 as *mut c_void);
            
            // Call actual entry point
            if let Some(entry) = info.entry {
                entry(info.arg);
            }
            
            // Mark as finished
            info.state = ThreadState::Finished;
        }
    }
}

/// Create a new V8 thread
/// 
/// Spawns a new FabricOS process/thread that will execute the given function.
/// Returns the thread ID on success.
pub fn v8_create_thread(
    f: V8ThreadFn,
    arg: *mut c_void
) -> Result<V8ThreadId, V8PlatformError> {
    v8_create_thread_with_priority(f, arg, ThreadPriority::Normal)
}

/// Create thread with specific priority
pub fn v8_create_thread_with_priority(
    f: V8ThreadFn,
    arg: *mut c_void,
    priority: ThreadPriority
) -> Result<V8ThreadId, V8PlatformError> {
    let id = next_thread_id();
    
    let info = V8ThreadInfo {
        id,
        state: ThreadState::Created,
        priority,
        stack_size: 2 * 1024 * 1024, // 2MB stack
        entry: Some(f),
        arg,
    };
    
    unsafe {
        let slot = store_thread_info(info)?;
        
        // Create actual OS thread via syscall
        let result = syscall::thread_create(
            thread_trampoline as usize,
            slot,
            priority_as_u8(priority)
        );
        
        if result == 0 {
            Ok(id)
        } else {
            // Clean up thread table entry
            THREAD_TABLE[slot] = None;
            ACTIVE_THREAD_COUNT.fetch_sub(1, Ordering::SeqCst);
            Err(V8PlatformError::ThreadCreationFailed)
        }
    }
}

/// Convert priority to syscall value
fn priority_as_u8(p: ThreadPriority) -> u8 {
    match p {
        ThreadPriority::Idle => 0,
        ThreadPriority::BelowNormal => 25,
        ThreadPriority::Normal => 50,
        ThreadPriority::AboveNormal => 75,
        ThreadPriority::High => 90,
        ThreadPriority::Realtime => 99,
    }
}

/// Wait for thread to complete
/// 
/// Blocks until the specified thread exits.
pub fn v8_join_thread(id: V8ThreadId) -> Result<(), V8PlatformError> {
    unsafe {
        // In stub mode, just poll
        loop {
            if let Some(info) = get_thread_info(id) {
                if info.state == ThreadState::Finished || info.state == ThreadState::Joined {
                    info.state = ThreadState::Joined;
                    
                    // Find and remove from table
                    for i in 0..MAX_V8_THREADS {
                        if let Some(ref entry) = THREAD_TABLE[i] {
                            if entry.id == id {
                                THREAD_TABLE[i] = None;
                                ACTIVE_THREAD_COUNT.fetch_sub(1, Ordering::SeqCst);
                                break;
                            }
                        }
                    }
                    
                    return Ok(());
                }
            } else {
                return Err(V8PlatformError::ThreadCreationFailed);
            }
            
            // Yield to let other threads run
            v8_yield();
        }
    }
}

/// Yield CPU to other threads
#[inline(always)]
pub fn v8_yield() {
    syscall::thread_yield();
}

/// Get current thread ID
pub fn v8_current_thread() -> V8ThreadId {
    unsafe {
        let raw = get_thread_local(V8_THREAD_ID_TLS_KEY) as u64;
        if raw == 0 {
            // Main thread or uninitialized
            V8ThreadId(0)
        } else {
            V8ThreadId(raw)
        }
    }
}

/// Set current thread name (for debugging)
pub fn v8_set_thread_name(name: &str) -> Result<(), V8PlatformError> {
    let c_name = match c_str_from_str(name) {
        Some(s) => s,
        None => return Err(V8PlatformError::ThreadCreationFailed),
    };
    
    let result = unsafe { syscall::set_thread_name(c_name.as_ptr()) };
    
    if result == 0 {
        Ok(())
    } else {
        Err(V8PlatformError::SyscallFailed(SysCallError::InvalidArgument))
    }
}

/// Convert Rust string to C string (stack allocated)
fn c_str_from_str(s: &str) -> Option<[u8; 32]> {
    if s.len() > 31 {
        return None;
    }
    
    let mut buf = [0u8; 32];
    buf[..s.len()].copy_from_slice(s.as_bytes());
    Some(buf)
}

/// Thread-local storage: Set value
/// 
/// # Safety
/// Key must be valid TLS slot
pub unsafe fn set_thread_local(key: usize, value: *mut c_void) {
    syscall::set_tls(key, value as usize);
}

/// Thread-local storage: Get value
/// 
/// # Safety
/// Key must be valid TLS slot
pub unsafe fn get_thread_local(key: usize) -> *mut c_void {
    syscall::get_tls(key) as *mut c_void
}

/// Set V8 isolate pointer in TLS
/// 
/// # Safety
/// isolate must be valid V8 Isolate pointer or null
pub unsafe fn set_v8_isolate(isolate: *mut c_void) {
    set_thread_local(V8_ISOLATE_TLS_KEY, isolate);
}

/// Get V8 isolate pointer from TLS
pub fn get_v8_isolate() -> *mut c_void {
    unsafe { get_thread_local(V8_ISOLATE_TLS_KEY) }
}

/// Thread pool for V8 background tasks
pub struct V8ThreadPool {
    workers: [Option<V8ThreadId>; 8],
    count: usize,
}

impl V8ThreadPool {
    /// Create new thread pool
    pub const fn new() -> Self {
        Self {
            workers: [None; 8],
            count: 0,
        }
    }
    
    /// Spawn worker threads
    pub fn spawn_workers(&mut self, count: usize) -> Result<(), V8PlatformError> {
        extern "C" fn worker_loop(_: *mut c_void) {
            loop {
                // Check for tasks
                // In real impl: dequeue from work queue
                v8_yield();
            }
        }
        
        for i in 0..count.min(8) {
            let id = v8_create_thread_with_priority(
                worker_loop,
                null_mut(),
                ThreadPriority::BelowNormal
            )?;
            self.workers[i] = Some(id);
        }
        
        self.count = count.min(8);
        Ok(())
    }
    
    /// Shutdown thread pool
    pub fn shutdown(&mut self) {
        // In real impl: signal workers to exit and join
        for i in 0..self.count {
            if let Some(id) = self.workers[i] {
                let _ = v8_join_thread(id);
                self.workers[i] = None;
            }
        }
        self.count = 0;
    }
}

/// Get count of active V8 threads
pub fn v8_active_thread_count() -> usize {
    ACTIVE_THREAD_COUNT.load(Ordering::Relaxed)
}

/// Sleep/yield for milliseconds
pub fn v8_sleep_ms(ms: u64) {
    syscall::sleep_ms(ms);
}

/// Sleep for microseconds
pub fn v8_sleep_us(us: u64) {
    syscall::sleep_us(us);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_thread_id_generation() {
        let id1 = next_thread_id();
        let id2 = next_thread_id();
        assert!(id2.0 > id1.0);
    }
    
    #[test]
    fn test_priority_conversion() {
        assert_eq!(priority_as_u8(ThreadPriority::Idle), 0);
        assert_eq!(priority_as_u8(ThreadPriority::Normal), 50);
        assert_eq!(priority_as_u8(ThreadPriority::Realtime), 99);
    }
    
    #[test]
    fn test_c_str_conversion() {
        let s = "test_thread";
        let c = c_str_from_str(s).unwrap();
        assert_eq!(&c[..s.len()], s.as_bytes());
        assert_eq!(c[s.len()], 0);
    }
}
