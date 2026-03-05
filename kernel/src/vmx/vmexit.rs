//! VM Exit handling -- reason codes, dispatch logic, and exit handlers.
//!
//! Handles CPUID, I/O port access, HLT, and control register access.
//! Used by both the software emulator and future hardware VMX path.

#![allow(dead_code)]

use super::vmcs::SoftVmcs;
use super::cpuid;

/// VM exit reason codes (Intel SDM Vol 3, Appendix C).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmExitReason {
    ExceptionOrNmi,     // 0
    ExternalInterrupt,  // 1
    Cpuid,              // 10
    Hlt,                // 12
    CrAccess,           // 28
    IoInstruction,      // 30
    EptViolation,       // 48
    EptMisconfig,       // 49
    Unknown(u32),
}

impl VmExitReason {
    pub fn from_raw(raw: u32) -> Self {
        match raw & 0xFFFF {
            0 => Self::ExceptionOrNmi,
            1 => Self::ExternalInterrupt,
            10 => Self::Cpuid,
            12 => Self::Hlt,
            28 => Self::CrAccess,
            30 => Self::IoInstruction,
            48 => Self::EptViolation,
            49 => Self::EptMisconfig,
            other => Self::Unknown(other),
        }
    }

    pub fn to_raw(self) -> u32 {
        match self {
            Self::ExceptionOrNmi => 0,
            Self::ExternalInterrupt => 1,
            Self::Cpuid => 10,
            Self::Hlt => 12,
            Self::CrAccess => 28,
            Self::IoInstruction => 30,
            Self::EptViolation => 48,
            Self::EptMisconfig => 49,
            Self::Unknown(v) => v,
        }
    }
}

/// Decoded VM exit information.
#[derive(Clone, Debug)]
pub struct VmExitInfo {
    pub reason: VmExitReason,
    pub qualification: u64,
    pub guest_rip: u64,
    pub instruction_length: u32,
}

/// Result of handling a VM exit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VmExitAction {
    /// Continue guest execution (advance RIP by instruction_length).
    Continue,
    /// Guest halted -- stop execution.
    Halt,
    /// I/O operation intercepted.
    IoExit {
        port: u16,
        is_out: bool,
        data: u8,
    },
    /// Fatal error -- terminate the VM.
    Fatal(&'static str),
}

/// Dispatch a VM exit to the appropriate handler.
pub fn handle_vmexit(vmcs: &mut SoftVmcs, exit_info: &VmExitInfo) -> VmExitAction {
    match exit_info.reason {
        VmExitReason::Cpuid => handle_cpuid(vmcs),
        VmExitReason::Hlt => VmExitAction::Halt,
        VmExitReason::IoInstruction => handle_io(vmcs, exit_info.qualification),
        VmExitReason::CrAccess => handle_cr_access(vmcs, exit_info.qualification),
        _ => VmExitAction::Fatal("unhandled VM exit reason"),
    }
}

/// Handle CPUID exit: emulate CPUID with filtered results.
/// Clears VMX bit (ECX bit 5) so guest does not see nested VMX.
fn handle_cpuid(vmcs: &mut SoftVmcs) -> VmExitAction {
    let leaf = vmcs.guest.rax as u32;
    let subleaf = vmcs.guest.rcx as u32;

    let result = cpuid::cpuid(leaf, subleaf);

    vmcs.guest.rax = result.eax as u64;
    vmcs.guest.rbx = result.ebx as u64;
    vmcs.guest.rcx = result.ecx as u64;
    vmcs.guest.rdx = result.edx as u64;

    // Filter: hide VMX from guest (leaf 1, ECX bit 5)
    if leaf == 1 {
        vmcs.guest.rcx &= !(1 << 5);
    }

    VmExitAction::Continue
}

/// Handle I/O instruction exit.
/// Exit qualification bits:
///   [15:0]  = port number (when bit 16 set) or from DX
///   [2:0]   = size (0=1byte, 1=2byte, 3=4byte)
///   [3]     = direction (0=OUT, 1=IN)
///   Actually for software emulation, we pass port and direction directly.
fn handle_io(vmcs: &mut SoftVmcs, qualification: u64) -> VmExitAction {
    let port = (qualification >> 16) as u16;
    let is_in = qualification & (1 << 3) != 0;

    if is_in {
        // IN: read from port into AL
        vmcs.guest.rax = (vmcs.guest.rax & !0xFF) | 0xFF; // return 0xFF for unhandled ports
        VmExitAction::Continue
    } else {
        // OUT: write AL to port
        let data = vmcs.guest.rax as u8;
        VmExitAction::IoExit {
            port,
            is_out: true,
            data,
        }
    }
}

/// Handle CR access exit.
/// Qualification bits [3:0] = CR number, [5:4] = access type (0=MOV to CR, 1=MOV from CR),
/// [11:8] = source/dest register.
fn handle_cr_access(vmcs: &mut SoftVmcs, qualification: u64) -> VmExitAction {
    let cr_num = (qualification & 0xF) as u8;
    let access_type = ((qualification >> 4) & 3) as u8;
    let reg = ((qualification >> 8) & 0xF) as u8;

    match (access_type, cr_num) {
        (0, 0) => { // MOV to CR0
            vmcs.guest.cr0 = vmcs.guest.read_gpr(reg);
        }
        (0, 3) => { // MOV to CR3
            vmcs.guest.cr3 = vmcs.guest.read_gpr(reg);
        }
        (0, 4) => { // MOV to CR4
            vmcs.guest.cr4 = vmcs.guest.read_gpr(reg);
        }
        (1, 0) => { // MOV from CR0
            let val = vmcs.guest.cr0;
            vmcs.guest.write_gpr(reg, val);
        }
        (1, 3) => { // MOV from CR3
            let val = vmcs.guest.cr3;
            vmcs.guest.write_gpr(reg, val);
        }
        (1, 4) => { // MOV from CR4
            let val = vmcs.guest.cr4;
            vmcs.guest.write_gpr(reg, val);
        }
        _ => return VmExitAction::Fatal("unsupported CR access"),
    }

    VmExitAction::Continue
}
