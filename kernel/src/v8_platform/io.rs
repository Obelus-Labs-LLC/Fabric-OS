//! V8 I/O and Entropy for FabricOS
//!
//! Provides kernel RNG for V8 Math.random seeding and serial output for debugging.
//! All I/O goes through FabricOS syscalls for security isolation.

use core::fmt::{self, Write};

use super::stubs::{KernelRng, syscall, SysCallError};
use super::V8PlatformError;

/// Log level for V8 messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum V8LogLevel {
    /// Verbose debugging
    Verbose = 0,
    /// Debug messages
    Debug = 1,
    /// Informational
    Info = 2,
    /// Warnings
    Warning = 3,
    /// Errors
    Error = 4,
    /// Fatal errors
    Fatal = 5,
}

/// Maximum log message length
const MAX_LOG_LEN: usize = 256;

/// Serial output writer for V8
pub struct V8SerialWriter;

impl V8SerialWriter {
    /// Create new serial writer
    pub const fn new() -> Self {
        Self
    }
    
    /// Write bytes to serial port
    fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            syscall::serial_write(b);
        }
    }
}

impl Write for V8SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_bytes(s.as_bytes());
        Ok(())
    }
}

/// Global serial writer instance
static mut SERIAL_WRITER: V8SerialWriter = V8SerialWriter::new();

/// Log a message from V8
/// 
/// Sends message to serial output with level prefix.
/// 
/// # Arguments
/// * `level` - Log severity level
/// * `msg` - Message to log
/// 
/// # Example
/// ```
/// v8_log_message(V8LogLevel::Info, "V8 initialized");
/// ```
pub fn v8_log_message(level: V8LogLevel, msg: &str) {
    let prefix = match level {
        V8LogLevel::Verbose => "[V8:V] ",
        V8LogLevel::Debug => "[V8:D] ",
        V8LogLevel::Info => "[V8:I] ",
        V8LogLevel::Warning => "[V8:W] ",
        V8LogLevel::Error => "[V8:E] ",
        V8LogLevel::Fatal => "[V8:F] ",
    };
    
    unsafe {
        let _ = SERIAL_WRITER.write_str(prefix);
        
        // Truncate if too long
        let truncated = if msg.len() > MAX_LOG_LEN - 10 {
            &msg[..MAX_LOG_LEN - 10]
        } else {
            msg
        };
        
        let _ = SERIAL_WRITER.write_str(truncated);
        let _ = SERIAL_WRITER.write_str("\r\n");
    }
}

/// Log formatted message (like println!)
/// 
/// # Example
/// ```
/// v8_logf!(V8LogLevel::Debug, "Value: {}", 42);
/// ```
#[macro_export]
macro_rules! v8_logf {
    ($level:expr, $fmt:literal $(, $arg:expr)*) => {
        {
            use core::fmt::Write;
            let mut buf = $crate::v8_platform::io::V8LogBuffer::<256>::new();
            let _ = write!(&mut buf, $fmt $(, $arg)*);
            $crate::v8_platform::io::v8_log_message($level, buf.as_str());
        }
    };
}

/// Stack-allocated log buffer
pub struct V8LogBuffer<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> V8LogBuffer<N> {
    /// Create new empty buffer
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }
    
    /// Get contents as string slice
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("<invalid utf8>")
    }
}

impl<const N: usize> Write for V8LogBuffer<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let remaining = N.saturating_sub(self.len);
        let to_copy = bytes.len().min(remaining);
        
        self.buf[self.len..self.len + to_copy].copy_from_slice(&bytes[..to_copy]);
        self.len += to_copy;
        
        Ok(())
    }
}

/// Initialize I/O subsystem
pub fn init_io() {
    // Nothing special needed for stub
    v8_log_message(V8LogLevel::Info, "V8 I/O subsystem initialized");
}

/// Read entropy from kernel RNG
/// 
/// Fills buffer with cryptographically secure random bytes
/// from the kernel entropy pool. Used by V8 for:
/// - Math.random() seeding
/// - JIT code randomization (ASLR)
/// - Hash table seeding
/// 
/// # Arguments
/// * `buf` - Buffer to fill with random bytes
/// 
/// # Example
/// ```
/// let mut seed = [0u8; 32];
/// v8_read_entropy(&mut seed);
/// ```
pub fn v8_read_entropy(buf: &mut [u8]) {
    KernelRng::fill_bytes(buf);
}

/// Generate random u32
/// 
/// Convenience function for 32-bit random values
pub fn v8_random_u32() -> u32 {
    let mut buf = [0u8; 4];
    v8_read_entropy(&mut buf);
    u32::from_le_bytes(buf)
}

/// Generate random u64
/// 
/// Convenience function for 64-bit random values
pub fn v8_random_u64() -> u64 {
    let mut buf = [0u8; 8];
    v8_read_entropy(&mut buf);
    u64::from_le_bytes(buf)
}

/// Generate random f64 in [0, 1)
/// 
/// Used for Math.random() implementation
pub fn v8_random_f64() -> f64 {
    // Generate 53 bits of precision (IEEE 754 double mantissa)
    let bits = v8_random_u64();
    let mantissa = bits & ((1u64 << 53) - 1);
    mantissa as f64 / (1u64 << 53) as f64
}

/// Random seed for hash tables
/// 
/// Returns a random seed that changes on each boot
/// to prevent hash collision attacks.
pub fn v8_hash_seed() -> u64 {
    static mut SEED: Option<u64> = None;
    
    unsafe {
        if let Some(seed) = SEED {
            seed
        } else {
            let seed = v8_random_u64();
            SEED = Some(seed);
            seed
        }
    }
}

/// File handle for V8 snapshot loading
#[derive(Debug, Clone, Copy)]
pub struct V8FileHandle(u32);

/// Open file for reading
/// 
/// # Arguments
/// * `path` - File path (null-terminated)
/// 
/// Returns file handle on success
pub fn v8_open_file(path: &str) -> Result<V8FileHandle, V8PlatformError> {
    let c_path = match c_str_from_str(path) {
        Some(s) => s,
        None => return Err(V8PlatformError::InvalidPointer),
    };
    
    let fd = syscall::open(c_path.as_ptr(), 0);
    
    if fd < 0 {
        Err(V8PlatformError::SyscallFailed(SysCallError::InvalidArgument))
    } else {
        Ok(V8FileHandle(fd as u32))
    }
}

/// Read from file
/// 
/// # Arguments
/// * `handle` - File handle from v8_open_file
/// * `buf` - Buffer to read into
/// 
/// Returns bytes read
pub fn v8_read_file(handle: V8FileHandle, buf: &mut [u8]) -> Result<usize, V8PlatformError> {
    let n = syscall::read(handle.0 as i32, buf.as_mut_ptr(), buf.len());
    
    if n < 0 {
        Err(V8PlatformError::SyscallFailed(SysCallError::InvalidArgument))
    } else {
        Ok(n as usize)
    }
}

/// Close file
pub fn v8_close_file(handle: V8FileHandle) {
    syscall::close(handle.0 as i32);
}

/// Convert Rust string to C string
fn c_str_from_str(s: &str) -> Option<[u8; 128]> {
    if s.len() > 127 {
        return None;
    }
    
    let mut buf = [0u8; 128];
    buf[..s.len()].copy_from_slice(s.as_bytes());
    Some(buf)
}

/// Entropy quality checker
/// 
/// Verifies RNG is producing sufficiently random output
pub fn check_entropy_quality() -> bool {
    let mut buf = [0u8; 256];
    v8_read_entropy(&mut buf);
    
    // Basic check: ensure not all zeros or all same value
    let first = buf[0];
    let all_same = buf.iter().all(|&b| b == first);
    let all_zeros = buf.iter().all(|&b| b == 0);
    
    !all_same && !all_zeros
}

/// Dump entropy sample for debugging
#[cfg(debug_assertions)]
pub fn dump_entropy_sample() {
    let mut buf = [0u8; 32];
    v8_read_entropy(&mut buf);
    
    v8_log_message(V8LogLevel::Debug, "Entropy sample:");
    
    for (i, chunk) in buf.chunks(8).enumerate() {
        let mut hex_buf = V8LogBuffer::<64>::new();
        let _ = write!(&mut hex_buf, "  [{:02x}] ", i * 8);
        for b in chunk {
            let _ = write!(&mut hex_buf, "{:02x} ", b);
        }
        v8_log_message(V8LogLevel::Debug, hex_buf.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_entropy_not_zero() {
        let mut buf = [0u8; 32];
        v8_read_entropy(&mut buf);
        
        let all_zeros = buf.iter().all(|&b| b == 0);
        assert!(!all_zeros, "Entropy should not be all zeros");
    }
    
    #[test]
    fn test_entropy_varies() {
        let mut buf1 = [0u8; 32];
        let mut buf2 = [0u8; 32];
        
        v8_read_entropy(&mut buf1);
        v8_read_entropy(&mut buf2);
        
        assert_ne!(buf1, buf2, "Entropy should vary between reads");
    }
    
    #[test]
    fn test_random_u32_range() {
        let r = v8_random_u32();
        // Just verify it produces a value (not panic)
        let _ = r;
    }
    
    #[test]
    fn test_random_f64_range() {
        let r = v8_random_f64();
        assert!(r >= 0.0 && r < 1.0, "f64 random should be in [0, 1)");
    }
    
    #[test]
    fn test_log_buffer() {
        let mut buf = V8LogBuffer::<32>::new();
        use core::fmt::Write;
        let _ = write!(&mut buf, "test {} {}", 1, 2);
        assert_eq!(buf.as_str(), "test 1 2");
    }
    
    #[test]
    fn test_entropy_quality() {
        assert!(check_entropy_quality(), "Entropy quality check failed");
    }
}
