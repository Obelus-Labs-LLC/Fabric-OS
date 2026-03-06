//! RAII FPU Guard for V8 FFI Boundary
//!
//! `FpuGuard` enables SSE2 on creation and restores the kernel's soft-float
//! state when dropped. All V8 entry points must be wrapped:
//!
//! ```rust,ignore
//! pub fn run_javascript(script: &str) -> i32 {
//!     let _guard = FpuGuard::new();
//!     unsafe { v8_run_script(script.as_ptr(), script.len()) }
//! }
//! ```

use super::fpu::{FpuState, fpu_save, fpu_restore, fpu_enable_sse, fpu_disable_sse};

/// RAII guard that enables SSE2 for the duration of its lifetime.
///
/// On creation: saves current FPU state, enables SSE2.
/// On drop: disables SSE (sets CR0.TS), restores saved FPU state.
///
/// This ensures V8's floating-point operations work at full hardware speed
/// while the kernel remains in +soft-float mode outside the guard scope.
pub struct FpuGuard {
    state: FpuState,
}

impl FpuGuard {
    /// Create a new FPU guard, enabling SSE2 for V8 code.
    ///
    /// # Safety Contract
    /// - Must be called from ring 0
    /// - Must not be nested (only one FpuGuard active per thread)
    /// - The guard must be dropped before returning to kernel soft-float code
    #[inline]
    pub fn new() -> Self {
        let mut state = FpuState::new();
        unsafe {
            fpu_enable_sse();
            fpu_save(&mut state);
        }
        Self { state }
    }
}

impl Drop for FpuGuard {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            fpu_restore(&self.state);
            fpu_disable_sse();
        }
    }
}
