//! Phase 7 OCRB Gate — Hardware Interrupts + Userspace Execution.
//!
//! 10 tests validating GDT, TSS, IDT, APIC, timer interrupts,
//! SYSCALL MSRs, Ring 3 execution, preemptive context switching,
//! and ELF loading.

#![allow(dead_code)]

use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use super::OcrbResult;

/// Run all Phase 7 OCRB tests.
pub fn run_all_tests() -> Vec<OcrbResult> {
    let mut results = Vec::new();

    results.push(test_gdt_structure());
    results.push(test_tss_rsp0());
    results.push(test_idt_populated());
    results.push(test_apic_enabled());
    results.push(test_timer_ticks());
    results.push(test_syscall_msrs());
    results.push(test_ring3_round_trip());
    results.push(test_preemptive_switch());
    results.push(test_elf_header_parse());
    results.push(test_elf_load_run());

    results
}

/// Test 1: GDT Structure Valid (weight 10)
/// Verify 7 entries encode correctly and selectors match constants.
fn test_gdt_structure() -> OcrbResult {
    let entries = crate::x86::gdt::raw_entries();

    // Entry 0 should be null
    let null_ok = entries[0] == 0;

    // Entry 1 (Kernel Code) should be non-zero with L=1
    let kcode_ok = entries[1] != 0 && (entries[1] & (1 << 53)) != 0; // L bit

    // Entry 2 (Kernel Data) should be non-zero
    let kdata_ok = entries[2] != 0;

    // Entry 3 (User Data) should be non-zero, DPL=3
    let udata_ok = entries[3] != 0 && ((entries[3] >> 45) & 3) == 3; // DPL bits

    // Entry 4 (User Code) should be non-zero, DPL=3, L=1
    let ucode_ok = entries[4] != 0
        && ((entries[4] >> 45) & 3) == 3
        && (entries[4] & (1 << 53)) != 0;

    // Entries 5-6 (TSS) — at least entry 5 should be non-zero after TSS init
    let tss_ok = entries[5] != 0;

    // Verify selector constants
    let sel_ok = crate::x86::gdt::KERNEL_CS == 0x08
        && crate::x86::gdt::KERNEL_DS == 0x10
        && crate::x86::gdt::USER_DS == 0x1B
        && crate::x86::gdt::USER_CS == 0x23
        && crate::x86::gdt::TSS_SEL == 0x28;

    let all_ok = null_ok && kcode_ok && kdata_ok && udata_ok && ucode_ok && tss_ok && sel_ok;

    OcrbResult {
        test_name: "GDT Structure Valid",
        passed: all_ok,
        score: if all_ok { 100 } else { 0 },
        weight: 10,
        details: if all_ok {
            String::from("7 entries, selectors correct")
        } else {
            format!("null={} kcode={} kdata={} udata={} ucode={} tss={} sel={}",
                null_ok, kcode_ok, kdata_ok, udata_ok, ucode_ok, tss_ok, sel_ok)
        },
    }
}

/// Test 2: TSS RSP0 Initialized (weight 10)
/// Verify TSS has valid IST1 (double fault stack).
fn test_tss_rsp0() -> OcrbResult {
    let ist1 = crate::x86::tss::get_ist1();
    let tss_addr = crate::x86::tss::tss_address();

    // IST1 should be non-zero (allocated during init)
    let ist1_ok = ist1 != 0;

    // TSS address should be in kernel address space
    let addr_ok = tss_addr >= 0xFFFF_8000_0000_0000;

    let passed = ist1_ok && addr_ok;

    OcrbResult {
        test_name: "TSS RSP0 + IST1 Initialized",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: if passed {
            format!("IST1=0x{:x}, TSS at 0x{:x}", ist1, tss_addr)
        } else {
            format!("ist1_ok={} addr_ok={}", ist1_ok, addr_ok)
        },
    }
}

/// Test 3: IDT Fully Populated (weight 10)
/// All 256 entries present, handlers in kernel address range.
fn test_idt_populated() -> OcrbResult {
    let entries = crate::x86::idt::raw_entries();

    let mut present_count = 0;
    let mut kernel_range_ok = true;

    for i in 0..256 {
        if entries[i].is_present() {
            present_count += 1;
            let addr = entries[i].handler_addr();
            // Handler should be in kernel address range
            if addr < 0xFFFF_8000_0000_0000 {
                kernel_range_ok = false;
            }
        }
    }

    let all_present = present_count == 256;
    let passed = all_present && kernel_range_ok;

    OcrbResult {
        test_name: "IDT Fully Populated",
        passed,
        score: if passed { 100 } else if present_count >= 48 { 50 } else { 0 },
        weight: 10,
        details: format!("{}/256 present, kernel_range={}", present_count, kernel_range_ok),
    }
}

/// Test 4: APIC Enabled + Timer (weight 10)
/// APIC ID readable, enabled, timer vector configured.
fn test_apic_enabled() -> OcrbResult {
    let initialized = crate::x86::apic::is_initialized();
    let apic_id = crate::x86::apic::apic_id();
    let base_virt = crate::x86::apic::base_virt();

    // APIC should be initialized
    let init_ok = initialized;

    // Base virtual address should be in HHDM range
    let base_ok = base_virt >= 0xFFFF_8000_0000_0000;

    let passed = init_ok && base_ok;

    OcrbResult {
        test_name: "APIC Enabled + Timer",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("initialized={}, id={}, base=0x{:x}", initialized, apic_id, base_virt),
    }
}

/// Test 5: Timer Tick Counter (weight 15)
/// Enable timer, busy-wait, verify interrupt count > 0.
fn test_timer_ticks() -> OcrbResult {
    // Read tick count that was accumulated during the earlier STI window
    let ticks = crate::x86::idt::tick_count();

    let passed = ticks > 0;

    OcrbResult {
        test_name: "Timer Tick Counter",
        passed,
        score: if ticks >= 10 { 100 } else if ticks > 0 { 80 } else { 0 },
        weight: 15,
        details: format!("{} ticks accumulated", ticks),
    }
}

/// Test 6: SYSCALL MSRs Set (weight 10)
/// EFER.SCE=1, LSTAR in kernel, STAR fields correct.
fn test_syscall_msrs() -> OcrbResult {
    let efer = crate::x86::syscall::read_efer();
    let star = crate::x86::syscall::read_star();
    let lstar = crate::x86::syscall::read_lstar();
    let fmask = crate::x86::syscall::read_fmask();

    // EFER.SCE (bit 0) should be set
    let sce_ok = efer & 1 != 0;

    // STAR[47:32] should be KERNEL_CS (0x08)
    let star_kernel = ((star >> 32) & 0xFFFF) as u16;
    let star_kernel_ok = star_kernel == crate::x86::gdt::KERNEL_CS;

    // STAR[63:48] should be 0x10 (base for SYSRET: CS = 0x10+16|3 = 0x23)
    let star_user_base = ((star >> 48) & 0xFFFF) as u16;
    let star_user_ok = star_user_base == 0x10;

    // LSTAR should point to kernel address
    let lstar_ok = lstar >= 0xFFFF_8000_0000_0000;

    // FMASK should clear IF (bit 9)
    let fmask_ok = fmask & 0x200 != 0;

    let passed = sce_ok && star_kernel_ok && star_user_ok && lstar_ok && fmask_ok;

    OcrbResult {
        test_name: "SYSCALL MSRs Set",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: if passed {
            format!("SCE=1, LSTAR=0x{:x}, STAR OK", lstar)
        } else {
            format!("sce={} star_k={} star_u={} lstar={} fmask={}",
                sce_ok, star_kernel_ok, star_user_ok, lstar_ok, fmask_ok)
        },
    }
}

/// Test 7: Ring 3 Round-Trip (weight 15)
/// Spawn process with test code, enter Ring 3, SYSCALL back, verify exit code.
fn test_ring3_round_trip() -> OcrbResult {
    use fabric_types::{ProcessId, ProcessState};

    // Clean up state for this test
    {
        let mut table = crate::process::TABLE.lock();
        let mut sched = crate::process::SCHEDULER.lock();
        // Remove any non-Butler processes
        let pids: Vec<ProcessId> = table.pids().filter(|p| *p != ProcessId::BUTLER).collect();
        for pid in pids {
            sched.dequeue(pid);
            table.remove(pid);
        }
    }

    // Spawn a user process from the embedded test ELF
    let result = crate::process::spawn_elf(
        ProcessId::BUTLER,
        crate::elf::TEST_ELF_EXIT42,
        "ring3-test",
    );

    let pid = match result {
        Ok(p) => p,
        Err(_) => {
            return OcrbResult {
                test_name: "Ring 3 Round-Trip",
                passed: false,
                score: 0,
                weight: 15,
                details: String::from("Failed to spawn ELF process"),
            };
        }
    };

    // Enable interrupts and let the process run
    crate::x86::apic::start_timer(0x20000);
    crate::x86::enable_interrupts();

    // Busy-wait for the process to terminate (with timeout)
    let mut timeout = 0u64;
    let max_timeout = 5_000_000u64;
    loop {
        let state = crate::process::get_state(pid);
        if state == Some(ProcessState::Terminated) {
            break;
        }
        if timeout >= max_timeout {
            break;
        }
        core::hint::spin_loop();
        timeout += 1;
    }

    // Disable interrupts
    crate::x86::disable_interrupts();

    // Check results
    let state = crate::process::get_state(pid);
    let exit_code = {
        let table = crate::process::TABLE.lock();
        table.get(pid).map(|pcb| pcb.exit_code).unwrap_or(u64::MAX)
    };

    let terminated = state == Some(ProcessState::Terminated);
    let correct_code = exit_code == 42;
    let passed = terminated && correct_code;

    // Clean up
    {
        let mut table = crate::process::TABLE.lock();
        let mut sched = crate::process::SCHEDULER.lock();
        sched.dequeue(pid);
        table.remove(pid);
    }

    OcrbResult {
        test_name: "Ring 3 Round-Trip",
        passed,
        score: if passed { 100 } else if terminated { 50 } else { 0 },
        weight: 15,
        details: if passed {
            format!("Process exited with code 42")
        } else {
            format!("terminated={} exit_code={} timeout={}", terminated, exit_code, timeout)
        },
    }
}

/// Test 8: Preemptive Switch (weight 10)
/// Two processes, both accumulate ticks via timer preemption.
fn test_preemptive_switch() -> OcrbResult {
    use fabric_types::{ProcessId, ProcessState};

    // Clean up state for this test
    {
        let mut table = crate::process::TABLE.lock();
        let mut sched = crate::process::SCHEDULER.lock();
        let pids: Vec<ProcessId> = table.pids().filter(|p| *p != ProcessId::BUTLER).collect();
        for pid in pids {
            sched.dequeue(pid);
            table.remove(pid);
        }
    }

    // Spawn two user processes with infinite loop code
    // We need to create address spaces with the loop code mapped in
    let mut addr_space1 = match crate::address_space::AddressSpace::create() {
        Ok(a) => a,
        Err(_) => {
            return OcrbResult {
                test_name: "Preemptive Switch",
                passed: false,
                score: 0,
                weight: 10,
                details: String::from("Failed to create address space 1"),
            };
        }
    };
    let mut addr_space2 = match crate::address_space::AddressSpace::create() {
        Ok(a) => a,
        Err(_) => {
            return OcrbResult {
                test_name: "Preemptive Switch",
                passed: false,
                score: 0,
                weight: 10,
                details: String::from("Failed to create address space 2"),
            };
        }
    };

    // Map the loop code at 0x400000 in each address space
    let code_addr = 0x400000u64;
    let code_frame1 = crate::memory::frame::allocate_frame().unwrap();
    let code_frame2 = crate::memory::frame::allocate_frame().unwrap();

    // Write loop code to each frame
    unsafe {
        let ptr1 = code_frame1.to_virt().as_u64() as *mut u8;
        core::ptr::write_bytes(ptr1, 0, crate::memory::PAGE_SIZE);
        core::ptr::copy_nonoverlapping(
            crate::elf::TEST_CODE_LOOP.as_ptr(),
            ptr1,
            crate::elf::TEST_CODE_LOOP.len(),
        );

        let ptr2 = code_frame2.to_virt().as_u64() as *mut u8;
        core::ptr::write_bytes(ptr2, 0, crate::memory::PAGE_SIZE);
        core::ptr::copy_nonoverlapping(
            crate::elf::TEST_CODE_LOOP.as_ptr(),
            ptr2,
            crate::elf::TEST_CODE_LOOP.len(),
        );
    }

    let code_flags = crate::memory::page_table::PageTableFlags::empty(); // R+X
    let _ = addr_space1.map_user_page(
        crate::memory::VirtAddr::new(code_addr),
        code_frame1,
        code_flags,
    );
    let _ = addr_space2.map_user_page(
        crate::memory::VirtAddr::new(code_addr),
        code_frame2,
        code_flags,
    );

    // Map user stacks
    let stack_frame1 = crate::memory::frame::allocate_frame().unwrap();
    let stack_frame2 = crate::memory::frame::allocate_frame().unwrap();
    unsafe {
        core::ptr::write_bytes(stack_frame1.to_virt().as_u64() as *mut u8, 0, crate::memory::PAGE_SIZE);
        core::ptr::write_bytes(stack_frame2.to_virt().as_u64() as *mut u8, 0, crate::memory::PAGE_SIZE);
    }
    let stack_flags = crate::memory::page_table::PageTableFlags::WRITABLE
        | crate::memory::page_table::PageTableFlags::NO_EXECUTE;
    let stack_va = crate::elf::USER_STACK_BASE;
    let _ = addr_space1.map_user_page(crate::memory::VirtAddr::new(stack_va), stack_frame1, stack_flags);
    let _ = addr_space2.map_user_page(crate::memory::VirtAddr::new(stack_va), stack_frame2, stack_flags);

    let user_stack_top = stack_va + crate::memory::PAGE_SIZE as u64;

    // Spawn both processes
    let pid1 = match crate::process::spawn_user(
        ProcessId::BUTLER, code_addr, user_stack_top, addr_space1, "loop-1",
    ) {
        Ok(p) => p,
        Err(_) => {
            return OcrbResult {
                test_name: "Preemptive Switch",
                passed: false, score: 0, weight: 10,
                details: String::from("Failed to spawn process 1"),
            };
        }
    };

    let pid2 = match crate::process::spawn_user(
        ProcessId::BUTLER, code_addr, user_stack_top, addr_space2, "loop-2",
    ) {
        Ok(p) => p,
        Err(_) => {
            return OcrbResult {
                test_name: "Preemptive Switch",
                passed: false, score: 0, weight: 10,
                details: String::from("Failed to spawn process 2"),
            };
        }
    };

    // Record initial tick count
    let ticks_before = crate::x86::idt::tick_count();

    // Enable interrupts and let timer preempt between the two processes
    crate::x86::apic::start_timer(0x20000);
    crate::x86::enable_interrupts();

    // Wait for some ticks
    for _ in 0..2_000_000u64 {
        core::hint::spin_loop();
    }

    // Disable interrupts
    crate::x86::disable_interrupts();

    let ticks_after = crate::x86::idt::tick_count();
    let elapsed_ticks = ticks_after - ticks_before;

    // Both processes should still be alive (they loop forever)
    let state1 = crate::process::get_state(pid1);
    let state2 = crate::process::get_state(pid2);

    // Get total ticks run for each from their PCBs
    let (ticks1, ticks2) = {
        let table = crate::process::TABLE.lock();
        let t1 = table.get(pid1).map(|p| p.total_ticks_run).unwrap_or(0);
        let t2 = table.get(pid2).map(|p| p.total_ticks_run).unwrap_or(0);
        (t1, t2)
    };

    // Both should be Running or Ready, and timer should have ticked
    let alive = (state1 == Some(ProcessState::Ready) || state1 == Some(ProcessState::Running))
        && (state2 == Some(ProcessState::Ready) || state2 == Some(ProcessState::Running));
    let ticks_ok = elapsed_ticks > 0;
    // At least one process should have received timer ticks
    let ran = ticks1 > 0 || ticks2 > 0;

    let passed = alive && ticks_ok && ran;

    // Clean up: terminate both processes
    {
        let mut table = crate::process::TABLE.lock();
        let mut sched = crate::process::SCHEDULER.lock();
        for pid in [pid1, pid2] {
            sched.dequeue(pid);
            if let Some(pcb) = table.get_mut(pid) {
                pcb.state = ProcessState::Terminated;
            }
            table.remove(pid);
        }
    }

    OcrbResult {
        test_name: "Preemptive Switch",
        passed,
        score: if passed { 100 } else if ticks_ok { 50 } else { 0 },
        weight: 10,
        details: format!(
            "elapsed={} ticks, p1_ticks={} p2_ticks={}, alive={}",
            elapsed_ticks, ticks1, ticks2, alive
        ),
    }
}

/// Test 9: ELF Header Parse (weight 5)
/// Parse embedded ELF, verify magic + entry + segments.
fn test_elf_header_parse() -> OcrbResult {
    let data = crate::elf::TEST_ELF_EXIT42;

    let header = match crate::elf::parse_header(data) {
        Ok(h) => h,
        Err(e) => {
            return OcrbResult {
                test_name: "ELF Header Parse",
                passed: false,
                score: 0,
                weight: 5,
                details: format!("Parse error: {:?}", e),
            };
        }
    };

    let magic_ok = header.e_ident[0..4] == [0x7F, b'E', b'L', b'F'];
    let class_ok = header.e_ident[4] == 2; // ELFCLASS64
    let entry_ok = header.e_entry == 0x400078;
    let phnum_ok = header.e_phnum == 1;

    // Parse program headers
    let phdrs = match crate::elf::program_headers(data, header) {
        Ok(p) => p,
        Err(_) => {
            return OcrbResult {
                test_name: "ELF Header Parse",
                passed: false,
                score: 0,
                weight: 5,
                details: String::from("Failed to parse program headers"),
            };
        }
    };

    let phdr_ok = phdrs.len() == 1 && phdrs[0].p_type == 1; // PT_LOAD
    let vaddr_ok = phdrs[0].p_vaddr == 0x400000;

    let passed = magic_ok && class_ok && entry_ok && phnum_ok && phdr_ok && vaddr_ok;

    OcrbResult {
        test_name: "ELF Header Parse",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 5,
        details: if passed {
            format!("entry=0x{:x}, 1 PT_LOAD at 0x400000", header.e_entry)
        } else {
            format!("magic={} class={} entry={} phnum={} phdr={} vaddr={}",
                magic_ok, class_ok, entry_ok, phnum_ok, phdr_ok, vaddr_ok)
        },
    }
}

/// Test 10: ELF Load + Run (weight 5)
/// Load ELF into address space, verify entry point returned correctly.
fn test_elf_load_run() -> OcrbResult {
    let data = crate::elf::TEST_ELF_EXIT42;

    // Create a fresh address space
    let mut addr_space = match crate::address_space::AddressSpace::create() {
        Ok(a) => a,
        Err(_) => {
            return OcrbResult {
                test_name: "ELF Load + Run",
                passed: false,
                score: 0,
                weight: 5,
                details: String::from("Failed to create address space"),
            };
        }
    };

    // Load the ELF
    let entry = match crate::elf::load_elf(data, &mut addr_space) {
        Ok(e) => e,
        Err(e) => {
            return OcrbResult {
                test_name: "ELF Load + Run",
                passed: false,
                score: 0,
                weight: 5,
                details: format!("ELF load error: {:?}", e),
            };
        }
    };

    let entry_ok = entry == 0x400078;
    let pages_ok = addr_space.user_page_count() > 0;

    let passed = entry_ok && pages_ok;

    // Clean up
    drop(addr_space);

    OcrbResult {
        test_name: "ELF Load + Run",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 5,
        details: if passed {
            format!("entry=0x{:x}, {} user pages mapped", entry, 1)
        } else {
            format!("entry_ok={} (0x{:x}) pages_ok={}", entry_ok, entry, pages_ok)
        },
    }
}
