//! FPU Save/Restore for V8 JavaScript Engine
//!
//! The FabricOS kernel uses +soft-float (no SSE/FPU) to avoid saving FPU state
//! on every context switch. V8 requires hardware floating-point (SSE2) for
//! JavaScript number operations. This module provides FPU context management
//! at the FFI boundary: enable SSE before entering V8, restore kernel state
//! on return.
//!
//! Analogous to Linux's `kernel_fpu_begin()`/`kernel_fpu_end()`.

/// FXSAVE/FXRSTOR state area: 512 bytes, 16-byte aligned.
///
/// Contains x87 FPU, MMX, and SSE register state.
/// See Intel SDM Vol. 1, Section 10.5 "FXSAVE/FXRSTOR Instructions".
#[repr(C, align(16))]
pub struct FpuState {
    data: [u8; 512],
}

impl FpuState {
    /// Create a zeroed FPU state.
    pub const fn new() -> Self {
        Self { data: [0u8; 512] }
    }
}

/// Save the current FPU/SSE state into `state` via FXSAVE.
///
/// # Safety
/// - `state` must be valid and 16-byte aligned (guaranteed by repr(align(16)))
/// - Caller must ensure no concurrent access to FPU registers during save
#[inline]
pub unsafe fn fpu_save(state: &mut FpuState) {
    core::arch::asm!(
        "fxsave [{}]",
        in(reg) state.data.as_mut_ptr(),
        options(nostack, preserves_flags)
    );
}

/// Restore FPU/SSE state from `state` via FXRSTOR.
///
/// # Safety
/// - `state` must contain a valid FXSAVE image
/// - Caller must ensure no concurrent access to FPU registers during restore
#[inline]
pub unsafe fn fpu_restore(state: &FpuState) {
    core::arch::asm!(
        "fxrstor [{}]",
        in(reg) state.data.as_ptr(),
        options(nostack, preserves_flags)
    );
}

/// Enable SSE/SSE2 instructions by configuring CR0 and CR4.
///
/// - Clears CR0.EM (bit 2) — disable x87 emulation
/// - Clears CR0.TS (bit 3) — allow FPU access without #NM
/// - Sets CR4.OSFXSR (bit 9) — enable FXSAVE/FXRSTOR
/// - Sets CR4.OSXMMEXCPT (bit 10) — enable SSE exceptions
///
/// # Safety
/// Must run in ring 0. Modifies control registers.
#[inline]
pub unsafe fn fpu_enable_sse() {
    let mut cr0: u64;
    core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
    cr0 &= !(1 << 2); // Clear EM
    cr0 &= !(1 << 3); // Clear TS
    core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack));

    let mut cr4: u64;
    core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
    cr4 |= 1 << 9;  // OSFXSR
    cr4 |= 1 << 10; // OSXMMEXCPT
    core::arch::asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));
}

/// Disable FPU access by setting CR0.TS (bit 3).
///
/// Any subsequent FPU/SSE instruction will raise #NM (Device Not Available),
/// catching accidental float use in kernel soft-float code.
///
/// # Safety
/// Must run in ring 0. Modifies CR0.
#[inline]
pub unsafe fn fpu_disable_sse() {
    let mut cr0: u64;
    core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
    cr0 |= 1 << 3; // Set TS
    core::arch::asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack));
}
