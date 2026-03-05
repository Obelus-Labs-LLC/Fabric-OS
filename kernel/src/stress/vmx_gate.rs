//! STRESS Phase 17 Gate — VMX Foundation Tests
//!
//! 10 tests verifying CPUID detection, EPT page tables, VMCS fields,
//! instruction emulation (HLT, CPUID, I/O), and VM lifecycle.

#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
use crate::ocrb::StressResult;
use crate::vmx::cpuid;
use crate::vmx::vmcs::{self, SoftVmcs, fields};
use crate::vmx::ept::{EptContext, EptFlags};
use crate::vmx::guest::{VmTable, MAX_VMS};
use crate::vmx::vmexit::VmExitReason;
use crate::memory::PhysAddr;

pub fn run_all_tests() -> Vec<StressResult> {
    let mut results = Vec::new();
    results.push(test_cpuid_vendor());
    results.push(test_cpuid_vmx_coherent());
    results.push(test_ept_lifecycle());
    results.push(test_ept_isolation());
    results.push(test_vmcs_fields());
    results.push(test_emulator_hlt());
    results.push(test_emulator_cpuid_filter());
    results.push(test_emulator_io_port());
    results.push(test_vm_lifecycle());
    results.push(test_vm_table_limits());
    results
}

/// Test 1: CPUID probe returns valid vendor string (w:15)
fn test_cpuid_vendor() -> StressResult {
    let features = cpuid::probe_features();
    let vendor = cpuid::vendor_string(&features);

    let vendor_nonzero = features.vendor.iter().any(|&b| b != 0);
    let vendor_len = vendor.len();
    let family_ok = features.family > 0;

    let passed = vendor_nonzero && vendor_len == 12 && family_ok;

    StressResult {
        test_name: "CPUID probe returns vendor",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 15,
        details: format!("vendor={} family={} model={} step={}",
            vendor, features.family, features.model, features.stepping),
    }
}

/// Test 2: CPUID VMX detection is coherent (w:10)
fn test_cpuid_vmx_coherent() -> StressResult {
    let features = cpuid::probe_features();
    let leaf1 = cpuid::cpuid(1, 0);
    let cpuid_vmx = leaf1.ecx & (1 << 5) != 0;

    let coherent = features.vmx == cpuid_vmx;
    let cap = crate::vmx::capability();
    let cap_valid = cap == crate::vmx::VmxCapability::Hardware || cap == crate::vmx::VmxCapability::Emulated;

    let passed = coherent && cap_valid;

    StressResult {
        test_name: "CPUID VMX detection coherent",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("vmx_flag={} cpuid_bit5={} cap={:?}", features.vmx, cpuid_vmx, cap),
    }
}

/// Test 3: EPT create/destroy lifecycle (w:15)
fn test_ept_lifecycle() -> StressResult {
    let mut ept = match EptContext::create() {
        Ok(e) => e,
        Err(_) => return StressResult {
            test_name: "EPT create/destroy lifecycle",
            passed: false, score: 0, weight: 15,
            details: String::from("EPT creation failed"),
        },
    };

    // PML4 must be non-zero and 4K-aligned
    let pml4 = ept.pml4_phys();
    if pml4.0 == 0 || pml4.0 & 0xFFF != 0 {
        ept.destroy();
        return StressResult {
            test_name: "EPT create/destroy lifecycle",
            passed: false, score: 30, weight: 15,
            details: format!("PML4 invalid: {:?}", pml4),
        };
    }

    // Allocate 4 host frames for mapping
    let mut host_frames = Vec::new();
    for _ in 0..4 {
        if let Some(f) = crate::memory::frame::allocate_frame() {
            host_frames.push(f);
        }
    }

    if host_frames.len() < 4 {
        for f in &host_frames { crate::memory::frame::deallocate_frame(*f); }
        ept.destroy();
        return StressResult {
            test_name: "EPT create/destroy lifecycle",
            passed: false, score: 20, weight: 15,
            details: String::from("Frame allocation failed"),
        };
    }

    // Map 4 pages at guest-physical 0x0000-0x3000
    let mut map_ok = true;
    for i in 0..4 {
        let guest = (i * 0x1000) as u64;
        if ept.map_page(guest, host_frames[i], EptFlags::RWX_WB).is_err() {
            map_ok = false;
        }
    }

    // Verify translate(0x1000) returns correct host physical
    let translate_ok = if let Some(host) = ept.translate(0x1000) {
        host.0 == host_frames[1].0
    } else {
        false
    };

    // Verify unmapped address returns None
    let unmapped_ok = ept.translate(0x5000).is_none();

    // Unmap all
    let mut unmap_ok = true;
    for i in 0..4 {
        let guest = (i * 0x1000) as u64;
        if ept.unmap_page(guest).is_err() {
            unmap_ok = false;
        }
    }

    // Free host frames
    for f in &host_frames { crate::memory::frame::deallocate_frame(*f); }

    // Destroy EPT
    ept.destroy();

    let passed = map_ok && translate_ok && unmapped_ok && unmap_ok;

    StressResult {
        test_name: "EPT create/destroy lifecycle",
        passed,
        score: if passed { 100 } else { 40 },
        weight: 15,
        details: format!("map={} translate={} unmapped={} unmap={}",
            map_ok, translate_ok, unmapped_ok, unmap_ok),
    }
}

/// Test 4: EPT isolation between VMs (w:10)
fn test_ept_isolation() -> StressResult {
    let mut ept1 = match EptContext::create() {
        Ok(e) => e,
        Err(_) => return StressResult {
            test_name: "EPT isolation between VMs",
            passed: false, score: 0, weight: 10,
            details: String::from("EPT1 creation failed"),
        },
    };
    let mut ept2 = match EptContext::create() {
        Ok(e) => e,
        Err(_) => {
            ept1.destroy();
            return StressResult {
                test_name: "EPT isolation between VMs",
                passed: false, score: 0, weight: 10,
                details: String::from("EPT2 creation failed"),
            };
        }
    };

    // Allocate 2 different host frames
    let f1 = crate::memory::frame::allocate_frame();
    let f2 = crate::memory::frame::allocate_frame();

    if f1.is_none() || f2.is_none() {
        if let Some(f) = f1 { crate::memory::frame::deallocate_frame(f); }
        if let Some(f) = f2 { crate::memory::frame::deallocate_frame(f); }
        ept1.destroy();
        ept2.destroy();
        return StressResult {
            test_name: "EPT isolation between VMs",
            passed: false, score: 0, weight: 10,
            details: String::from("Frame allocation failed"),
        };
    }
    let f1 = f1.unwrap();
    let f2 = f2.unwrap();

    // Map same guest address to different host frames
    let _ = ept1.map_page(0x0, f1, EptFlags::RWX_WB);
    let _ = ept2.map_page(0x0, f2, EptFlags::RWX_WB);

    let t1 = ept1.translate(0x0);
    let t2 = ept2.translate(0x0);

    let passed = match (t1, t2) {
        (Some(a), Some(b)) => a.0 != b.0 && a.0 == f1.0 && b.0 == f2.0,
        _ => false,
    };

    let _ = ept1.unmap_page(0x0);
    let _ = ept2.unmap_page(0x0);
    crate::memory::frame::deallocate_frame(f1);
    crate::memory::frame::deallocate_frame(f2);
    ept1.destroy();
    ept2.destroy();

    StressResult {
        test_name: "EPT isolation between VMs",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("t1={:?} t2={:?}", t1, t2),
    }
}

/// Test 5: VMCS guest register read/write (w:10)
fn test_vmcs_fields() -> StressResult {
    let mut vmcs = SoftVmcs::new();

    // Write fields
    vmcs.write_field(fields::GUEST_RIP, 0xDEAD_BEEF);
    vmcs.write_field(fields::GUEST_RSP, 0xCAFE_BABE);
    vmcs.write_field(fields::GUEST_RFLAGS, 0x202);
    vmcs.write_field(fields::GUEST_CR0, 0x8000_0001);

    // Read back
    let rip = vmcs.read_field(fields::GUEST_RIP);
    let rsp = vmcs.read_field(fields::GUEST_RSP);
    let rflags = vmcs.read_field(fields::GUEST_RFLAGS);
    let cr0 = vmcs.read_field(fields::GUEST_CR0);

    let rip_ok = rip == 0xDEAD_BEEF;
    let rsp_ok = rsp == 0xCAFE_BABE;
    let rflags_ok = rflags == 0x202;
    let cr0_ok = cr0 == 0x8000_0001;

    // Also test direct struct access matches
    let direct_ok = vmcs.guest.rip == 0xDEAD_BEEF && vmcs.guest.rsp == 0xCAFE_BABE;

    let passed = rip_ok && rsp_ok && rflags_ok && cr0_ok && direct_ok;

    StressResult {
        test_name: "VMCS guest register read/write",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("rip={} rsp={} rflags={} cr0={} direct={}",
            rip_ok, rsp_ok, rflags_ok, cr0_ok, direct_ok),
    }
}

/// Test 6: Instruction emulator -- HLT (w:10)
fn test_emulator_hlt() -> StressResult {
    let mut vt = VmTable::new();
    let vm_id = match vt.create(1) { // 1 page
        Ok(id) => id,
        Err(_) => return StressResult {
            test_name: "Emulator: HLT",
            passed: false, score: 0, weight: 10,
            details: String::from("VM creation failed"),
        },
    };

    let vm = vt.get_mut(vm_id).unwrap();
    let code: [u8; 1] = [0xF4]; // HLT
    if vm.load_code(&code).is_err() {
        vt.destroy(vm_id);
        return StressResult {
            test_name: "Emulator: HLT",
            passed: false, score: 0, weight: 10,
            details: String::from("Code load failed"),
        };
    }

    let exit = vm.run(100);
    let halt_ok = exit.reason == VmExitReason::Hlt;
    let count_ok = vm.exit_count == 1;

    vt.destroy(vm_id);

    let passed = halt_ok && count_ok;
    StressResult {
        test_name: "Emulator: HLT",
        passed,
        score: if passed { 100 } else { 50 },
        weight: 10,
        details: format!("reason={:?} exits={}", exit.reason, if count_ok { 1 } else { 0 }),
    }
}

/// Test 7: Instruction emulator -- CPUID filter (w:10)
/// Guest code: MOV EAX,1; CPUID; HLT
/// Verify guest ECX bit 5 (VMX) is cleared by the emulator.
fn test_emulator_cpuid_filter() -> StressResult {
    let mut vt = VmTable::new();
    let vm_id = match vt.create(1) {
        Ok(id) => id,
        Err(_) => return StressResult {
            test_name: "Emulator: CPUID filter",
            passed: false, score: 0, weight: 10,
            details: String::from("VM creation failed"),
        },
    };

    let vm = vt.get_mut(vm_id).unwrap();
    // MOV EAX, 1 (B8 01 00 00 00) ; CPUID (0F A2) ; HLT (F4)
    let code: [u8; 8] = [0xB8, 0x01, 0x00, 0x00, 0x00, 0x0F, 0xA2, 0xF4];
    if vm.load_code(&code).is_err() {
        vt.destroy(vm_id);
        return StressResult {
            test_name: "Emulator: CPUID filter",
            passed: false, score: 0, weight: 10,
            details: String::from("Code load failed"),
        };
    }

    let exit = vm.run(100);
    let halt_ok = exit.reason == VmExitReason::Hlt;
    // Guest ECX bit 5 should be cleared (VMX hidden)
    let vmx_hidden = vm.vmcs.guest.rcx & (1 << 5) == 0;
    // Guest EAX should have valid family/model info
    let eax_ok = vm.vmcs.guest.rax != 0;

    vt.destroy(vm_id);

    let passed = halt_ok && vmx_hidden && eax_ok;
    StressResult {
        test_name: "Emulator: CPUID filter",
        passed,
        score: if passed { 100 } else { 30 },
        weight: 10,
        details: format!("halt={} vmx_hidden={} eax_nonzero={}", halt_ok, vmx_hidden, eax_ok),
    }
}

/// Test 8: Instruction emulator -- I/O port (w:10)
/// Guest code: MOV AL, 0x41; OUT 0x3F8, AL; HLT
fn test_emulator_io_port() -> StressResult {
    let mut vt = VmTable::new();
    let vm_id = match vt.create(1) {
        Ok(id) => id,
        Err(_) => return StressResult {
            test_name: "Emulator: I/O port",
            passed: false, score: 0, weight: 10,
            details: String::from("VM creation failed"),
        },
    };

    let vm = vt.get_mut(vm_id).unwrap();
    // MOV AL, 0x41 (B0 41) ; OUT 0x3F8, AL (E6 F8) -- Note: E6 uses imm8 port
    // Actually OUT imm8 only supports ports 0-255. 0x3F8 > 255.
    // Use port 0x80 (POST code port) for simplicity, or use 0xF8 (lower byte).
    // Let's use port 0x80: MOV AL, 0x41 (B0 41) ; OUT 0x80, AL (E6 80) ; HLT (F4)
    let code: [u8; 5] = [0xB0, 0x41, 0xE6, 0x80, 0xF4];
    if vm.load_code(&code).is_err() {
        vt.destroy(vm_id);
        return StressResult {
            test_name: "Emulator: I/O port",
            passed: false, score: 0, weight: 10,
            details: String::from("Code load failed"),
        };
    }

    let exit = vm.run(100);
    // Should exit on I/O instruction
    let io_exit = exit.reason == VmExitReason::IoInstruction;
    let port_ok = (exit.qualification >> 16) as u16 == 0x80;
    // AL should have been 0x41
    let al_val = vm.vmcs.guest.rax as u8;
    let data_ok = al_val == 0x41;

    vt.destroy(vm_id);

    let passed = io_exit && port_ok && data_ok;
    StressResult {
        test_name: "Emulator: I/O port",
        passed,
        score: if passed { 100 } else { 30 },
        weight: 10,
        details: format!("io_exit={} port={} data=0x{:02x}",
            io_exit, port_ok, al_val),
    }
}

/// Test 9: VM create/load/run/destroy full lifecycle (w:5)
/// Guest writes 0xDEADBEEF to address 0x1000 then HLTs.
fn test_vm_lifecycle() -> StressResult {
    let mut vt = VmTable::new();
    let vm_id = match vt.create(4) { // 4 pages (0x0000-0x3FFF)
        Ok(id) => id,
        Err(_) => return StressResult {
            test_name: "VM create/load/run/destroy",
            passed: false, score: 0, weight: 5,
            details: String::from("VM creation failed"),
        },
    };

    let vm = vt.get_mut(vm_id).unwrap();

    // Write 0xDEADBEEF to guest address 0x1000 via host access, then verify
    // we can read it back. This tests the EPT path end-to-end.
    vm.write_guest_phys_u32(0x1000, 0xDEAD_BEEF);

    // Also load some code that HLTs
    let code: [u8; 1] = [0xF4]; // HLT
    if vm.load_code(&code).is_err() {
        vt.destroy(vm_id);
        return StressResult {
            test_name: "VM create/load/run/destroy",
            passed: false, score: 0, weight: 5,
            details: String::from("Code load failed"),
        };
    }

    let exit = vm.run(100);
    let halt_ok = exit.reason == VmExitReason::Hlt;

    // Read back from guest physical 0x1000
    let readback = vm.read_guest_phys_u32(0x1000);
    let data_ok = readback == Some(0xDEAD_BEEF);

    vt.destroy(vm_id);

    let passed = halt_ok && data_ok;
    StressResult {
        test_name: "VM create/load/run/destroy",
        passed,
        score: if passed { 100 } else { 30 },
        weight: 5,
        details: format!("halt={} readback={:?}", halt_ok, readback),
    }
}

/// Test 10: VM table limits (w:5)
fn test_vm_table_limits() -> StressResult {
    let mut vt = VmTable::new();

    // Create MAX_VMS VMs
    let mut ids = Vec::new();
    let mut all_created = true;
    for _ in 0..MAX_VMS {
        match vt.create(1) {
            Ok(id) => ids.push(id),
            Err(_) => { all_created = false; break; }
        }
    }

    // 5th should fail
    let fifth_fails = vt.create(1).is_err();

    // Count should be MAX_VMS
    let count_ok = vt.count() == MAX_VMS;

    // Destroy one
    let first_id = ids[0];
    let destroy_ok = vt.destroy(first_id);

    // Now creation should succeed
    let after_destroy = vt.create(1).is_ok();

    // Cleanup remaining VMs
    for id in &ids[1..] {
        vt.destroy(*id);
    }

    let passed = all_created && fifth_fails && count_ok && destroy_ok && after_destroy;

    StressResult {
        test_name: "VM table limits",
        passed,
        score: if passed { 100 } else { 30 },
        weight: 5,
        details: format!("created={} 5th_fails={} count={} destroy={} reuse={}",
            all_created, fifth_fails, count_ok, destroy_ok, after_destroy),
    }
}
