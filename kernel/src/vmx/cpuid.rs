//! CPUID instruction wrapper and CPU feature probing.
//!
//! First use of CPUID in the kernel. Detects VMX capability,
//! CPU vendor, family/model/stepping, and optional VMX MSRs.

#![allow(dead_code)]

/// CPUID result (EAX, EBX, ECX, EDX).
#[derive(Clone, Copy, Debug)]
pub struct CpuidResult {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

/// CPU feature flags from CPUID leaf 1.
#[derive(Clone, Debug)]
pub struct CpuFeatures {
    pub vendor: [u8; 12],
    pub family: u8,
    pub model: u8,
    pub stepping: u8,
    pub vmx: bool,
    pub sse2: bool,
    pub xsave: bool,
}

/// VMX basic information from IA32_VMX_BASIC MSR (0x480).
#[derive(Clone, Copy, Debug)]
pub struct VmxBasicInfo {
    pub revision_id: u32,
    pub vmcs_size: u16,
    pub true_ctls: bool,
}

/// VMX secondary controls from IA32_VMX_PROCBASED_CTLS2 MSR (0x48B).
#[derive(Clone, Copy, Debug)]
pub struct VmxSecondaryCtls {
    pub ept: bool,
    pub vpid: bool,
    pub unrestricted_guest: bool,
}

/// Execute the CPUID instruction.
/// Note: rbx is reserved by LLVM, so we save/restore it manually.
pub fn cpuid(leaf: u32, subleaf: u32) -> CpuidResult {
    let (eax, ebx, ecx, edx): (u32, u32, u32, u32);
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            inout("eax") leaf => eax,
            ebx_out = out(reg) ebx,
            inout("ecx") subleaf => ecx,
            out("edx") edx,
        );
    }
    CpuidResult { eax, ebx, ecx, edx }
}

/// Probe CPU features from CPUID leaves 0 and 1.
pub fn probe_features() -> CpuFeatures {
    // Leaf 0: vendor string (EBX-EDX-ECX order)
    let leaf0 = cpuid(0, 0);
    let mut vendor = [0u8; 12];
    vendor[0..4].copy_from_slice(&leaf0.ebx.to_le_bytes());
    vendor[4..8].copy_from_slice(&leaf0.edx.to_le_bytes());
    vendor[8..12].copy_from_slice(&leaf0.ecx.to_le_bytes());

    // Leaf 1: feature flags + version info
    let leaf1 = cpuid(1, 0);

    let stepping = (leaf1.eax & 0xF) as u8;
    let mut model = ((leaf1.eax >> 4) & 0xF) as u8;
    let mut family = ((leaf1.eax >> 8) & 0xF) as u8;

    // Extended model/family for families >= 6 or == 15
    if family == 6 || family == 15 {
        model += (((leaf1.eax >> 16) & 0xF) as u8) << 4;
    }
    if family == 15 {
        family += ((leaf1.eax >> 20) & 0xFF) as u8;
    }

    let vmx = leaf1.ecx & (1 << 5) != 0;
    let sse2 = leaf1.edx & (1 << 26) != 0;
    let xsave = leaf1.ecx & (1 << 26) != 0;

    CpuFeatures {
        vendor,
        family,
        model,
        stepping,
        vmx,
        sse2,
        xsave,
    }
}

/// Read IA32_VMX_BASIC MSR. Only valid if CPUID reports VMX support.
pub fn vmx_basic() -> Option<VmxBasicInfo> {
    const IA32_VMX_BASIC: u32 = 0x480;
    let val = unsafe { rdmsr(IA32_VMX_BASIC) };
    Some(VmxBasicInfo {
        revision_id: (val & 0x7FFF_FFFF) as u32,
        vmcs_size: ((val >> 32) & 0x1FFF) as u16,
        true_ctls: val & (1 << 55) != 0,
    })
}

/// Read IA32_VMX_PROCBASED_CTLS2 MSR. Only valid if VMX supported.
pub fn vmx_secondary_ctls() -> Option<VmxSecondaryCtls> {
    const IA32_VMX_PROCBASED_CTLS2: u32 = 0x48B;
    let val = unsafe { rdmsr(IA32_VMX_PROCBASED_CTLS2) };
    let allowed = (val >> 32) as u32;
    Some(VmxSecondaryCtls {
        ept: allowed & (1 << 1) != 0,
        vpid: allowed & (1 << 5) != 0,
        unrestricted_guest: allowed & (1 << 7) != 0,
    })
}

/// Get the CPU vendor as a string slice.
pub fn vendor_string(features: &CpuFeatures) -> &str {
    core::str::from_utf8(&features.vendor).unwrap_or("Unknown")
}

/// Read a model-specific register.
unsafe fn rdmsr(msr: u32) -> u64 {
    let (low, high): (u32, u32);
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
    );
    ((high as u64) << 32) | (low as u64)
}
