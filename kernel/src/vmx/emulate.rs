//! Software x86-64 instruction emulator for the VM execution engine.
//!
//! Minimal decoder handling: HLT, CPUID, NOP, CLI, STI, IN/OUT imm8,
//! MOV reg,imm (with REX.W), JMP rel8, RET. Enough to run small guest
//! code blobs for OCRB testing.

#![allow(dead_code)]

use super::vmcs::SoftVmcs;
use super::ept::EptContext;
use super::vmexit::{VmExitReason, VmExitInfo, VmExitAction, handle_vmexit};
use crate::memory::hhdm_offset;

/// Decoded x86 instruction.
#[derive(Debug, PartialEq, Eq)]
pub struct DecodedInsn {
    pub kind: InsnKind,
    pub length: u8,
}

/// Instruction types we can decode.
#[derive(Debug, PartialEq, Eq)]
pub enum InsnKind {
    Hlt,
    Cpuid,
    Nop,
    Cli,
    Sti,
    InAl(u16),       // IN AL, imm8 -- port number
    OutAl(u16),      // OUT imm8, AL -- port number
    MovImmReg { reg: u8, imm: u64 },  // MOV reg, imm64 (REX.W + 0xB8+rd)
    MovImm32Reg { reg: u8, imm: u32 }, // MOV reg, imm32 (0xB8+rd, no REX.W)
    MovImm8Reg { reg: u8, imm: u8 },  // MOV r8, imm8 (0xB0+rb)
    JmpRel8(i8),
    Ret,
    Unknown(u8),
}

/// Read a byte from guest memory via EPT translation.
fn read_guest_byte(ept: &EptContext, guest_addr: u64) -> Option<u8> {
    let host_phys = ept.translate(guest_addr)?;
    let host_virt = host_phys.0 + hhdm_offset();
    Some(unsafe { *(host_virt as *const u8) })
}

/// Read multiple bytes from guest memory.
fn read_guest_bytes(ept: &EptContext, guest_addr: u64, count: usize) -> Option<alloc::vec::Vec<u8>> {
    let mut bytes = alloc::vec::Vec::with_capacity(count);
    for i in 0..count {
        bytes.push(read_guest_byte(ept, guest_addr + i as u64)?);
    }
    Some(bytes)
}

/// Write a byte to guest memory via EPT translation.
pub fn write_guest_byte(ept: &EptContext, guest_addr: u64, value: u8) -> bool {
    if let Some(host_phys) = ept.translate(guest_addr) {
        let host_virt = host_phys.0 + hhdm_offset();
        unsafe { *(host_virt as *mut u8) = value; }
        true
    } else {
        false
    }
}

/// Decode the next instruction at the given guest RIP.
pub fn decode_at(ept: &EptContext, guest_rip: u64) -> Option<DecodedInsn> {
    // Fetch up to 15 bytes (max x86 instruction length)
    let bytes = read_guest_bytes(ept, guest_rip, 15)?;
    if bytes.is_empty() {
        return None;
    }

    let mut pos: usize = 0;
    let mut rex_w = false;
    let mut rex_b = false;

    // Check for REX prefix (0x40-0x4F)
    if bytes[pos] >= 0x40 && bytes[pos] <= 0x4F {
        rex_w = bytes[pos] & 0x08 != 0; // REX.W
        rex_b = bytes[pos] & 0x01 != 0; // REX.B (extends reg field)
        pos += 1;
        if pos >= bytes.len() {
            return Some(DecodedInsn { kind: InsnKind::Unknown(bytes[0]), length: 1 });
        }
    }

    let opcode = bytes[pos];
    pos += 1;

    let insn = match opcode {
        0x90 => DecodedInsn { kind: InsnKind::Nop, length: pos as u8 },

        0xF4 => DecodedInsn { kind: InsnKind::Hlt, length: pos as u8 },

        0xFA => DecodedInsn { kind: InsnKind::Cli, length: pos as u8 },

        0xFB => DecodedInsn { kind: InsnKind::Sti, length: pos as u8 },

        0xC3 => DecodedInsn { kind: InsnKind::Ret, length: pos as u8 },

        // IN AL, imm8
        0xE4 => {
            if pos >= bytes.len() { return None; }
            let port = bytes[pos] as u16;
            DecodedInsn { kind: InsnKind::InAl(port), length: (pos + 1) as u8 }
        }

        // OUT imm8, AL
        0xE6 => {
            if pos >= bytes.len() { return None; }
            let port = bytes[pos] as u16;
            DecodedInsn { kind: InsnKind::OutAl(port), length: (pos + 1) as u8 }
        }

        // JMP rel8
        0xEB => {
            if pos >= bytes.len() { return None; }
            let offset = bytes[pos] as i8;
            DecodedInsn { kind: InsnKind::JmpRel8(offset), length: (pos + 1) as u8 }
        }

        // MOV r8, imm8 (0xB0+rb)
        b @ 0xB0..=0xB7 => {
            if pos >= bytes.len() { return None; }
            let reg = (b - 0xB0) | if rex_b { 8 } else { 0 };
            let imm = bytes[pos];
            DecodedInsn { kind: InsnKind::MovImm8Reg { reg, imm }, length: (pos + 1) as u8 }
        }

        // MOV reg, imm (0xB8+rd)
        b @ 0xB8..=0xBF => {
            let reg_base = (b - 0xB8) | if rex_b { 8 } else { 0 };
            if rex_w {
                // 64-bit immediate
                if pos + 8 > bytes.len() { return None; }
                let imm = u64::from_le_bytes([
                    bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3],
                    bytes[pos+4], bytes[pos+5], bytes[pos+6], bytes[pos+7],
                ]);
                DecodedInsn { kind: InsnKind::MovImmReg { reg: reg_base, imm }, length: (pos + 8) as u8 }
            } else {
                // 32-bit immediate
                if pos + 4 > bytes.len() { return None; }
                let imm = u32::from_le_bytes([
                    bytes[pos], bytes[pos+1], bytes[pos+2], bytes[pos+3],
                ]);
                DecodedInsn { kind: InsnKind::MovImm32Reg { reg: reg_base, imm }, length: (pos + 4) as u8 }
            }
        }

        // Two-byte opcodes (0x0F prefix)
        0x0F => {
            if pos >= bytes.len() { return None; }
            let op2 = bytes[pos];
            pos += 1;
            match op2 {
                0xA2 => DecodedInsn { kind: InsnKind::Cpuid, length: pos as u8 },
                _ => DecodedInsn { kind: InsnKind::Unknown(op2), length: pos as u8 },
            }
        }

        _ => DecodedInsn { kind: InsnKind::Unknown(opcode), length: pos as u8 },
    };

    Some(insn)
}

/// Execute one instruction step in the software VM.
pub fn step(vmcs: &mut SoftVmcs, ept: &EptContext) -> VmExitAction {
    let rip = vmcs.guest.rip;

    let insn = match decode_at(ept, rip) {
        Some(i) => i,
        None => return VmExitAction::Fatal("failed to decode instruction"),
    };

    match insn.kind {
        InsnKind::Nop => {
            vmcs.guest.rip += insn.length as u64;
            VmExitAction::Continue
        }

        InsnKind::Hlt => {
            vmcs.exit_reason = VmExitReason::Hlt.to_raw();
            vmcs.exit_qualification = 0;
            vmcs.exit_instr_len = insn.length as u32;
            vmcs.guest.rip += insn.length as u64;
            VmExitAction::Halt
        }

        InsnKind::Cli => {
            vmcs.guest.rflags &= !(1 << 9); // clear IF
            vmcs.guest.rip += insn.length as u64;
            VmExitAction::Continue
        }

        InsnKind::Sti => {
            vmcs.guest.rflags |= 1 << 9; // set IF
            vmcs.guest.rip += insn.length as u64;
            VmExitAction::Continue
        }

        InsnKind::Cpuid => {
            let exit_info = VmExitInfo {
                reason: VmExitReason::Cpuid,
                qualification: 0,
                guest_rip: rip,
                instruction_length: insn.length as u32,
            };
            let action = handle_vmexit(vmcs, &exit_info);
            vmcs.guest.rip += insn.length as u64;
            action
        }

        InsnKind::InAl(port) => {
            let exit_info = VmExitInfo {
                reason: VmExitReason::IoInstruction,
                qualification: ((port as u64) << 16) | (1 << 3), // IN direction
                guest_rip: rip,
                instruction_length: insn.length as u32,
            };
            let action = handle_vmexit(vmcs, &exit_info);
            vmcs.guest.rip += insn.length as u64;
            action
        }

        InsnKind::OutAl(port) => {
            // Capture data before handle_vmexit modifies state
            let data = vmcs.guest.rax as u8;
            vmcs.exit_reason = VmExitReason::IoInstruction.to_raw();
            vmcs.exit_qualification = ((port as u64) << 16);
            vmcs.exit_instr_len = insn.length as u32;
            vmcs.guest.rip += insn.length as u64;
            VmExitAction::IoExit { port, is_out: true, data }
        }

        InsnKind::MovImmReg { reg, imm } => {
            vmcs.guest.write_gpr(reg, imm);
            vmcs.guest.rip += insn.length as u64;
            VmExitAction::Continue
        }

        InsnKind::MovImm32Reg { reg, imm } => {
            vmcs.guest.write_gpr(reg, imm as u64);
            vmcs.guest.rip += insn.length as u64;
            VmExitAction::Continue
        }

        InsnKind::MovImm8Reg { reg, imm } => {
            // Write to low 8 bits of the register (AL=0, CL=1, DL=2, BL=3, etc.)
            let cur = vmcs.guest.read_gpr(reg);
            vmcs.guest.write_gpr(reg, (cur & !0xFF) | imm as u64);
            vmcs.guest.rip += insn.length as u64;
            VmExitAction::Continue
        }

        InsnKind::JmpRel8(offset) => {
            let target = vmcs.guest.rip.wrapping_add(insn.length as u64)
                .wrapping_add(offset as i64 as u64);
            vmcs.guest.rip = target;
            VmExitAction::Continue
        }

        InsnKind::Ret => {
            // Pop return address from stack (simplified -- no stack access in guest memory for now)
            vmcs.guest.rip += insn.length as u64;
            VmExitAction::Halt // Treat RET as halt in simple guest code
        }

        InsnKind::Unknown(op) => {
            vmcs.exit_reason = 0xFFFF;
            VmExitAction::Fatal("unknown instruction")
        }
    }
}

/// Run the software VM until a VM exit (HLT, I/O, or step limit).
pub fn run_guest(vmcs: &mut SoftVmcs, ept: &EptContext, max_steps: u64) -> VmExitInfo {
    let mut steps: u64 = 0;

    loop {
        if steps >= max_steps {
            return VmExitInfo {
                reason: VmExitReason::Unknown(0xFFFE),
                qualification: 0,
                guest_rip: vmcs.guest.rip,
                instruction_length: 0,
            };
        }

        let action = step(vmcs, ept);
        steps += 1;

        match action {
            VmExitAction::Continue => continue,
            VmExitAction::Halt => {
                return VmExitInfo {
                    reason: VmExitReason::Hlt,
                    qualification: 0,
                    guest_rip: vmcs.guest.rip,
                    instruction_length: vmcs.exit_instr_len,
                };
            }
            VmExitAction::IoExit { port, is_out, data } => {
                return VmExitInfo {
                    reason: VmExitReason::IoInstruction,
                    qualification: ((port as u64) << 16) | if is_out { 0 } else { 1 << 3 },
                    guest_rip: vmcs.guest.rip,
                    instruction_length: vmcs.exit_instr_len,
                };
            }
            VmExitAction::Fatal(msg) => {
                return VmExitInfo {
                    reason: VmExitReason::Unknown(0xFFFF),
                    qualification: 0,
                    guest_rip: vmcs.guest.rip,
                    instruction_length: 0,
                };
            }
        }
    }
}
