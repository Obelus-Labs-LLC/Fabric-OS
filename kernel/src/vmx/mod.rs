//! VMX (Virtual Machine Extensions) Foundation.
//!
//! Phase 17: Software-emulated hypervisor with CPUID detection, VMCS data
//! structures, EPT page tables, and minimal x86 instruction emulator.
//! All STRESS tests pass via software emulation (no hardware VMX required).
//! Future phases add VMXON/VMLAUNCH for hardware-accelerated guests.

#![allow(dead_code)]

pub mod cpuid;
pub mod vmcs;
pub mod ept;
pub mod vmexit;
pub mod emulate;
pub mod guest;

use core::sync::atomic::{AtomicU8, Ordering};
use crate::serial_println;

/// VMX availability mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmxCapability {
    /// Hardware VMX supported and VMXON succeeded.
    Hardware,
    /// VMX not available; using software emulation.
    Emulated,
}

/// 0=uninit, 1=hardware, 2=emulated
static VMX_MODE: AtomicU8 = AtomicU8::new(0);

/// Get the current VMX capability mode.
pub fn capability() -> VmxCapability {
    match VMX_MODE.load(Ordering::Acquire) {
        1 => VmxCapability::Hardware,
        _ => VmxCapability::Emulated,
    }
}

/// Initialize the VMX subsystem.
/// Probes CPUID for VMX support and sets the execution mode.
/// Phase 17: always uses Emulated mode (VMXON not attempted on TCG).
pub fn init() {
    let features = cpuid::probe_features();

    serial_println!("[VMX] CPU vendor: {}", cpuid::vendor_string(&features));
    serial_println!("[VMX] Family {} Model {} Stepping {}",
        features.family, features.model, features.stepping);
    serial_println!("[VMX] VMX reported by CPUID: {}", features.vmx);
    serial_println!("[VMX] SSE2: {}, XSAVE: {}", features.sse2, features.xsave);

    // Phase 17: do not attempt VMXON (TCG will #UD).
    // Set mode to Emulated regardless of CPUID VMX bit.
    VMX_MODE.store(2, Ordering::Release);
    serial_println!("[VMX] Mode: Software Emulation (Phase 17)");

    if features.vmx {
        // Log VMX MSR info for diagnostics (only if CPUID says VMX is available)
        // Note: on TCG, CPUID may still report VMX=false, which is fine.
        serial_println!("[VMX] Hardware VMX detected but not activated (deferred to future phase)");
    }
}
