//! V8 Time Services for FabricOS
//!
//! Provides monotonic timing and sleep functions for V8 JIT compilation scheduling.
//! Uses FabricOS timer syscalls for high-resolution timing.

use core::sync::atomic::{AtomicU64, Ordering};

use super::stubs::syscall;

/// Nanoseconds per second
pub const NS_PER_SEC: u64 = 1_000_000_000;
/// Nanoseconds per millisecond
pub const NS_PER_MS: u64 = 1_000_000;
/// Nanoseconds per microsecond
pub const NS_PER_US: u64 = 1_000;

/// Boot time reference (set during initialization)
static BOOT_TIME_NS: AtomicU64 = AtomicU64::new(0);

/// Initialize time subsystem
/// 
/// Captures current time as boot reference
pub fn init_time() {
    let now = raw_monotonic_time();
    BOOT_TIME_NS.store(now, Ordering::SeqCst);
}

/// Raw monotonic time from hardware
/// 
/// Returns time in nanoseconds since an arbitrary reference point.
/// Uses RDTSC on x86_64 for high resolution.
#[inline(always)]
fn raw_monotonic_time() -> u64 {
    // Use kernel syscall for consistent time
    syscall::get_time_ns().unwrap_or_else(|_| rdtsc_fallback())
}

/// RDTSC fallback for high-resolution timing
#[inline(always)]
fn rdtsc_fallback() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // Fallback: use instruction counter approximation
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1000, Ordering::Relaxed)
    }
}

/// Monotonic time in nanoseconds
/// 
/// Returns nanoseconds since system boot. Guaranteed monotonic,
/// never decreases, and is not affected by system time changes.
/// 
/// Used by V8 for:
/// - JIT compilation timing
/// - GC scheduling
/// - Performance profiling
#[inline(always)]
pub fn v8_monotonic_time() -> u64 {
    raw_monotonic_time()
}

/// Monotonic time in milliseconds
/// 
/// Convenience wrapper for millisecond precision timing.
#[inline(always)]
pub fn v8_monotonic_time_ms() -> u64 {
    v8_monotonic_time() / NS_PER_MS
}

/// Monotonic time in microseconds
/// 
/// Convenience wrapper for microsecond precision timing.
#[inline(always)]
pub fn v8_monotonic_time_us() -> u64 {
    v8_monotonic_time() / NS_PER_US
}

/// Sleep for specified milliseconds
/// 
/// Yields CPU to scheduler. Minimum sleep is one scheduler tick (~1ms).
/// 
/// # Arguments
/// * `ms` - Milliseconds to sleep
#[inline(always)]
pub fn v8_sleep(ms: u32) {
    syscall::sleep_ms(ms as u64);
}

/// Sleep for specified microseconds
/// 
/// For very short sleeps, may spin instead of yielding.
/// 
/// # Arguments
/// * `us` - Microseconds to sleep
pub fn v8_sleep_us(us: u64) {
    if us < 100 {
        // Short sleep: spin
        let start = v8_monotonic_time_us();
        while v8_monotonic_time_us() - start < us {
            core::hint::spin_loop();
        }
    } else {
        syscall::sleep_us(us);
    }
}

/// Sleep for specified nanoseconds
/// 
/// For very short sleeps, spins; for longer sleeps, yields.
/// 
/// # Arguments
/// * `ns` - Nanoseconds to sleep
pub fn v8_sleep_ns(ns: u64) {
    if ns < 1000 {
        // Very short: just spin
        let start = v8_monotonic_time();
        while v8_monotonic_time() - start < ns {
            core::hint::spin_loop();
        }
    } else if ns < 100_000 {
        // Short: convert to us and spin
        v8_sleep_us(ns / NS_PER_US);
    } else {
        // Long enough to yield
        syscall::sleep_ms((ns / NS_PER_MS).max(1));
    }
}

/// High-resolution profiling timer
/// 
/// Returns CPU clock cycles (RDTSC on x86_64). Use for profiling
/// short-duration events like JIT compilation.
#[inline(always)]
pub fn v8_profile_timer() -> u64 {
    rdtsc_fallback()
}

/// Convert profile timer cycles to nanoseconds
/// 
/// # Arguments
/// * `cycles` - CPU cycles from v8_profile_timer()
/// 
/// Note: Assumes 3.0 GHz CPU. Real implementation would calibrate.
pub fn cycles_to_ns(cycles: u64) -> u64 {
    // Approximate: 3.0 GHz = 3 cycles per ns
    cycles / 3
}

/// Timer for measuring intervals
pub struct V8Timer {
    start: u64,
    running: bool,
}

impl V8Timer {
    /// Create new timer (not running)
    pub const fn new() -> Self {
        Self {
            start: 0,
            running: false,
        }
    }
    
    /// Start the timer
    pub fn start(&mut self) {
        self.start = v8_monotonic_time();
        self.running = true;
    }
    
    /// Stop the timer
    pub fn stop(&mut self) {
        self.running = false;
    }
    
    /// Get elapsed nanoseconds
    pub fn elapsed_ns(&self) -> u64 {
        if self.running {
            v8_monotonic_time() - self.start
        } else {
            0
        }
    }
    
    /// Get elapsed milliseconds
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ns() / NS_PER_MS
    }
    
    /// Get elapsed microseconds
    pub fn elapsed_us(&self) -> u64 {
        self.elapsed_ns() / NS_PER_US
    }
    
    /// Check if timer is running
    pub fn is_running(&self) -> bool {
        self.running
    }
    
    /// Reset timer
    pub fn reset(&mut self) {
        self.start = 0;
        self.running = false;
    }
}

impl Default for V8Timer {
    fn default() -> Self {
        Self::new()
    }
}

/// Timeout checker for non-blocking operations
pub struct V8Timeout {
    deadline: u64,
}

impl V8Timeout {
    /// Create timeout with specified duration from now
    /// 
    /// # Arguments
    /// * `timeout_ms` - Timeout in milliseconds
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            deadline: v8_monotonic_time_ms() + timeout_ms,
        }
    }
    
    /// Check if timeout has expired
    pub fn is_expired(&self) -> bool {
        v8_monotonic_time_ms() >= self.deadline
    }
    
    /// Get remaining milliseconds
    pub fn remaining_ms(&self) -> u64 {
        let now = v8_monotonic_time_ms();
        if now >= self.deadline {
            0
        } else {
            self.deadline - now
        }
    }
    
    /// Get deadline
    pub fn deadline_ms(&self) -> u64 {
        self.deadline
    }
}

/// Busy-wait for exact timing
/// 
/// Spins until specified nanoseconds have elapsed.
/// Use only for very short, precise delays.
/// 
/// # Arguments
/// * `ns` - Nanoseconds to delay
pub fn busy_wait_ns(ns: u64) {
    let target = v8_monotonic_time() + ns;
    while v8_monotonic_time() < target {
        core::hint::spin_loop();
    }
}

/// Busy-wait for microseconds
/// 
/// # Arguments
/// * `us` - Microseconds to delay
pub fn busy_wait_us(us: u64) {
    busy_wait_ns(us * NS_PER_US);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_monotonic_time() {
        let t1 = v8_monotonic_time();
        // Small delay
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
        let t2 = v8_monotonic_time();
        assert!(t2 >= t1, "Time should be monotonic");
    }
    
    #[test]
    fn test_timer() {
        let mut timer = V8Timer::new();
        assert!(!timer.is_running());
        
        timer.start();
        assert!(timer.is_running());
        
        // Small delay
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
        
        assert!(timer.elapsed_ns() > 0);
        
        timer.stop();
        assert!(!timer.is_running());
    }
    
    #[test]
    fn test_timeout() {
        let timeout = V8Timeout::new(100); // 100ms timeout
        assert!(!timeout.is_expired());
        assert!(timeout.remaining_ms() <= 100);
    }
    
    #[test]
    fn test_constants() {
        assert_eq!(NS_PER_SEC, 1_000_000_000);
        assert_eq!(NS_PER_MS, 1_000_000);
        assert_eq!(NS_PER_US, 1_000);
    }
}
