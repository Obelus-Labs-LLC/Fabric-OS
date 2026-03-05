//! Software Virtual Machine Control Structure (VMCS).
//!
//! Stores guest CPU state using the same field layout as Intel's hardware VMCS.
//! For Phase 17, all access is via the SoftVmcs struct (no VMREAD/VMWRITE).
//! Future phases will add a hardware VMCS path using VMREAD/VMWRITE instructions.

#![allow(dead_code)]

/// Intel VMCS field encodings (subset needed for Phase 17).
/// Reference: Intel SDM Vol 3, Appendix B.
pub mod fields {
    // 16-bit guest state
    pub const GUEST_CS_SEL: u32 = 0x0802;
    pub const GUEST_DS_SEL: u32 = 0x0806;
    pub const GUEST_ES_SEL: u32 = 0x0800;
    pub const GUEST_SS_SEL: u32 = 0x0804;

    // 32-bit guest state
    pub const GUEST_CS_LIMIT: u32 = 0x4802;
    pub const GUEST_DS_LIMIT: u32 = 0x4806;
    pub const GUEST_ES_LIMIT: u32 = 0x4800;
    pub const GUEST_SS_LIMIT: u32 = 0x4804;
    pub const GUEST_CS_ACCESS: u32 = 0x4816;
    pub const GUEST_DS_ACCESS: u32 = 0x481A;
    pub const GUEST_ES_ACCESS: u32 = 0x4814;
    pub const GUEST_SS_ACCESS: u32 = 0x4818;

    // 64-bit / natural-width guest state
    pub const GUEST_CR0: u32 = 0x6800;
    pub const GUEST_CR3: u32 = 0x6802;
    pub const GUEST_CR4: u32 = 0x6804;
    pub const GUEST_CS_BASE: u32 = 0x6808;
    pub const GUEST_DS_BASE: u32 = 0x680C;
    pub const GUEST_ES_BASE: u32 = 0x6806;
    pub const GUEST_SS_BASE: u32 = 0x680A;
    pub const GUEST_RSP: u32 = 0x681C;
    pub const GUEST_RIP: u32 = 0x681E;
    pub const GUEST_RFLAGS: u32 = 0x6820;

    // 32-bit control fields
    pub const PIN_BASED_CONTROLS: u32 = 0x4000;
    pub const PRIMARY_PROC_CONTROLS: u32 = 0x4002;
    pub const SECONDARY_PROC_CONTROLS: u32 = 0x401E;
    pub const VM_EXIT_CONTROLS: u32 = 0x400C;
    pub const VM_ENTRY_CONTROLS: u32 = 0x4012;

    // 64-bit control fields
    pub const EPT_POINTER: u32 = 0x201A;

    // 32-bit read-only data fields
    pub const VM_EXIT_REASON: u32 = 0x4402;
    pub const VM_EXIT_INSTR_LEN: u32 = 0x440C;
    pub const VM_EXIT_INSTR_INFO: u32 = 0x440E;

    // Natural-width read-only data fields
    pub const VM_EXIT_QUALIFICATION: u32 = 0x6400;

    // Host state
    pub const HOST_CR3: u32 = 0x6C02;
    pub const HOST_RSP: u32 = 0x6C14;
    pub const HOST_RIP: u32 = 0x6C16;
    pub const HOST_CS_SEL: u32 = 0x0C02;
    pub const HOST_SS_SEL: u32 = 0x0C04;
}

/// Segment register state.
#[derive(Clone, Copy, Debug, Default)]
pub struct SegmentReg {
    pub selector: u16,
    pub base: u64,
    pub limit: u32,
    pub access: u32,
}

/// Guest CPU register state.
#[derive(Clone, Debug)]
pub struct GuestRegisters {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rsi: u64, pub rdi: u64, pub rbp: u64, pub rsp: u64,
    pub r8: u64, pub r9: u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
    pub cr0: u64, pub cr3: u64, pub cr4: u64,
    pub cs: SegmentReg, pub ds: SegmentReg,
    pub es: SegmentReg, pub ss: SegmentReg,
}

impl GuestRegisters {
    /// Create default guest state (64-bit flat mode, RIP=0).
    pub fn new() -> Self {
        Self {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, rbp: 0, rsp: 0,
            r8: 0, r9: 0, r10: 0, r11: 0,
            r12: 0, r13: 0, r14: 0, r15: 0,
            rip: 0,
            rflags: 0x2, // bit 1 always set in x86
            cr0: 0x0000_0001, // PE (protected mode)
            cr3: 0, cr4: 0,
            cs: SegmentReg { selector: 0x08, base: 0, limit: 0xFFFF_FFFF, access: 0xA09B },
            ds: SegmentReg { selector: 0x10, base: 0, limit: 0xFFFF_FFFF, access: 0xC093 },
            es: SegmentReg { selector: 0x10, base: 0, limit: 0xFFFF_FFFF, access: 0xC093 },
            ss: SegmentReg { selector: 0x10, base: 0, limit: 0xFFFF_FFFF, access: 0xC093 },
        }
    }

    /// Read a GPR by index (ModRM encoding: 0=RAX,1=RCX,2=RDX,3=RBX,4=RSP,5=RBP,6=RSI,7=RDI,8-15=R8-R15).
    pub fn read_gpr(&self, index: u8) -> u64 {
        match index {
            0 => self.rax, 1 => self.rcx, 2 => self.rdx, 3 => self.rbx,
            4 => self.rsp, 5 => self.rbp, 6 => self.rsi, 7 => self.rdi,
            8 => self.r8, 9 => self.r9, 10 => self.r10, 11 => self.r11,
            12 => self.r12, 13 => self.r13, 14 => self.r14, 15 => self.r15,
            _ => 0,
        }
    }

    /// Write a GPR by index.
    pub fn write_gpr(&mut self, index: u8, value: u64) {
        match index {
            0 => self.rax = value, 1 => self.rcx = value,
            2 => self.rdx = value, 3 => self.rbx = value,
            4 => self.rsp = value, 5 => self.rbp = value,
            6 => self.rsi = value, 7 => self.rdi = value,
            8 => self.r8 = value, 9 => self.r9 = value,
            10 => self.r10 = value, 11 => self.r11 = value,
            12 => self.r12 = value, 13 => self.r13 = value,
            14 => self.r14 = value, 15 => self.r15 = value,
            _ => {}
        }
    }
}

/// VMCS execution controls.
#[derive(Clone, Debug, Default)]
pub struct VmcsControls {
    pub pin_based: u32,
    pub proc_based: u32,
    pub secondary_proc: u32,
    pub exit_ctls: u32,
    pub entry_ctls: u32,
}

/// Host state saved in VMCS.
#[derive(Clone, Debug, Default)]
pub struct HostState {
    pub cr3: u64,
    pub rsp: u64,
    pub rip: u64,
    pub cs_sel: u16,
    pub ss_sel: u16,
}

/// Software VMCS -- stores all guest/host/control state in a Rust struct.
#[derive(Clone, Debug)]
pub struct SoftVmcs {
    pub guest: GuestRegisters,
    pub controls: VmcsControls,
    pub host: HostState,
    pub exit_reason: u32,
    pub exit_qualification: u64,
    pub exit_instr_len: u32,
    pub exit_instr_info: u32,
    pub eptp: u64,
}

impl SoftVmcs {
    pub fn new() -> Self {
        Self {
            guest: GuestRegisters::new(),
            controls: VmcsControls::default(),
            host: HostState::default(),
            exit_reason: 0,
            exit_qualification: 0,
            exit_instr_len: 0,
            exit_instr_info: 0,
            eptp: 0,
        }
    }

    /// Read a VMCS field by its Intel encoding.
    pub fn read_field(&self, field: u32) -> u64 {
        match field {
            fields::GUEST_RIP => self.guest.rip,
            fields::GUEST_RSP => self.guest.rsp,
            fields::GUEST_RFLAGS => self.guest.rflags,
            fields::GUEST_CR0 => self.guest.cr0,
            fields::GUEST_CR3 => self.guest.cr3,
            fields::GUEST_CR4 => self.guest.cr4,
            fields::GUEST_CS_SEL => self.guest.cs.selector as u64,
            fields::GUEST_CS_BASE => self.guest.cs.base,
            fields::GUEST_CS_LIMIT => self.guest.cs.limit as u64,
            fields::GUEST_CS_ACCESS => self.guest.cs.access as u64,
            fields::GUEST_DS_SEL => self.guest.ds.selector as u64,
            fields::GUEST_DS_BASE => self.guest.ds.base,
            fields::GUEST_ES_SEL => self.guest.es.selector as u64,
            fields::GUEST_SS_SEL => self.guest.ss.selector as u64,
            fields::VM_EXIT_REASON => self.exit_reason as u64,
            fields::VM_EXIT_QUALIFICATION => self.exit_qualification,
            fields::VM_EXIT_INSTR_LEN => self.exit_instr_len as u64,
            fields::EPT_POINTER => self.eptp,
            fields::PIN_BASED_CONTROLS => self.controls.pin_based as u64,
            fields::PRIMARY_PROC_CONTROLS => self.controls.proc_based as u64,
            fields::SECONDARY_PROC_CONTROLS => self.controls.secondary_proc as u64,
            fields::HOST_CR3 => self.host.cr3,
            fields::HOST_RSP => self.host.rsp,
            fields::HOST_RIP => self.host.rip,
            _ => 0,
        }
    }

    /// Write a VMCS field by its Intel encoding.
    pub fn write_field(&mut self, field: u32, value: u64) {
        match field {
            fields::GUEST_RIP => self.guest.rip = value,
            fields::GUEST_RSP => self.guest.rsp = value,
            fields::GUEST_RFLAGS => self.guest.rflags = value,
            fields::GUEST_CR0 => self.guest.cr0 = value,
            fields::GUEST_CR3 => self.guest.cr3 = value,
            fields::GUEST_CR4 => self.guest.cr4 = value,
            fields::GUEST_CS_SEL => self.guest.cs.selector = value as u16,
            fields::GUEST_CS_BASE => self.guest.cs.base = value,
            fields::GUEST_CS_LIMIT => self.guest.cs.limit = value as u32,
            fields::GUEST_CS_ACCESS => self.guest.cs.access = value as u32,
            fields::GUEST_DS_SEL => self.guest.ds.selector = value as u16,
            fields::GUEST_DS_BASE => self.guest.ds.base = value,
            fields::GUEST_ES_SEL => self.guest.es.selector = value as u16,
            fields::GUEST_SS_SEL => self.guest.ss.selector = value as u16,
            fields::EPT_POINTER => self.eptp = value,
            fields::PIN_BASED_CONTROLS => self.controls.pin_based = value as u32,
            fields::PRIMARY_PROC_CONTROLS => self.controls.proc_based = value as u32,
            fields::SECONDARY_PROC_CONTROLS => self.controls.secondary_proc = value as u32,
            fields::HOST_CR3 => self.host.cr3 = value,
            fields::HOST_RSP => self.host.rsp = value,
            fields::HOST_RIP => self.host.rip = value,
            _ => {}
        }
    }
}
