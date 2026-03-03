//! Process Model + Scheduler — Phase 3 of Fabric OS.
//!
//! Provides process lifecycle management, intent-aware scheduling,
//! priority inheritance, and Erlang-style supervision trees.
//!
//! Public API:
//!   process::init()            — Initialize Butler, scheduler, process table
//!   process::spawn()           — Create a new process under a supervisor
//!   process::terminate()       — Terminate a process
//!   process::block()           — Block a process (waiting on resource)
//!   process::unblock()         — Unblock a process
//!   process::crash()           — Simulate a crash (triggers supervision)
//!   process::tick()            — Advance scheduler tick
//!   process::schedule_next()   — Get next process to run
//!   process::get_state()       — Query process state
//!   process::count()           — Live process count

#![allow(dead_code)]

pub mod pcb;
pub mod scheduler;
pub mod supervisor;
pub mod butler;
pub mod table;

use spin::Mutex;
use alloc::string::String;
use fabric_types::{
    Intent, ProcessId, ProcessState, SupervisionStrategy,
};
use crate::serial_println;
use crate::memory::{VirtAddr, PAGE_SIZE};
use crate::memory::frame;
use crate::memory::page_table::PageTableFlags;

pub use table::{ProcessTable, ProcessError};
pub use pcb::{ProcessControlBlock, ExitReason};

/// Global process table.
pub static TABLE: Mutex<ProcessTable> = Mutex::new(ProcessTable::new());

/// Global scheduler.
pub static SCHEDULER: Mutex<scheduler::Scheduler> = Mutex::new(scheduler::Scheduler::new());

/// Maximum priority inheritance chain depth.
const MAX_INHERITANCE_DEPTH: u32 = 4;

/// Initialize the process subsystem.
pub fn init() {
    let mut table = TABLE.lock();
    let mut sched = SCHEDULER.lock();
    butler::init(&mut table, &mut sched);
    drop(sched);
    drop(table);

    // Register Butler on the bus
    let _ = crate::bus::register_process(ProcessId::BUTLER);

    serial_println!("[PROC] Process subsystem initialized");
    serial_println!("[PROC] Butler (pid:1) is root supervisor");
}

/// Spawn a new process under a supervisor.
pub fn spawn(
    supervisor_pid: ProcessId,
    intent: Intent,
    description: &str,
    strategy: Option<SupervisionStrategy>,
) -> Result<ProcessId, ProcessError> {
    let mut table = TABLE.lock();
    let mut sched = SCHEDULER.lock();

    // Verify supervisor exists
    if !table.contains(supervisor_pid) {
        return Err(ProcessError::SupervisorNotFound);
    }

    let now = sched.current_tick();
    let pid_raw = table.alloc_pid();
    let pid = ProcessId::new(pid_raw);

    // Get spawn_order from supervisor's current child count
    let spawn_order = table
        .get(supervisor_pid)
        .map(|sup| sup.children.len() as u32)
        .unwrap_or(0);

    let strat = strategy.unwrap_or(SupervisionStrategy::OneForOne);
    let pcb = ProcessControlBlock::new(
        pid,
        intent,
        String::from(description),
        supervisor_pid,
        strat,
        spawn_order,
        now,
    );

    let eff_prio = pcb.effective_priority;
    table.insert(pcb)?;

    // Add child to supervisor's children list
    if let Some(sup) = table.get_mut(supervisor_pid) {
        sup.children.push(pid);
    }

    // Enqueue in scheduler
    sched.enqueue(pid, eff_prio);

    drop(sched);
    drop(table);

    // Register on the bus (ignore error if bus not ready)
    let _ = crate::bus::register_process(pid);

    Ok(pid)
}

/// Terminate a process.
pub fn terminate(pid: ProcessId) -> Result<(), ProcessError> {
    let mut table = TABLE.lock();
    let mut sched = SCHEDULER.lock();

    let pcb = table.get_mut(pid).ok_or(ProcessError::NotFound)?;
    pcb.state = ProcessState::Terminated;
    pcb.exit_reason = Some(ExitReason::Normal);
    sched.dequeue(pid);

    // Remove from supervisor's children list
    let supervisor_pid = pcb.supervisor;
    if let Some(sup) = table.get_mut(supervisor_pid) {
        sup.children.retain(|&c| c != pid);
    }

    drop(sched);
    drop(table);

    // Unregister from bus
    let _ = crate::bus::BUS.lock().unregister_process(pid);

    Ok(())
}

/// Block a process, optionally declaring what it's waiting on.
/// Triggers priority inheritance if waiting_on is specified.
pub fn block(pid: ProcessId, waiting_on: Option<ProcessId>) -> Result<(), ProcessError> {
    let mut table = TABLE.lock();
    let mut sched = SCHEDULER.lock();

    let pcb = table.get_mut(pid).ok_or(ProcessError::NotFound)?;
    if pcb.state != ProcessState::Running && pcb.state != ProcessState::Ready {
        return Err(ProcessError::InvalidState);
    }

    pcb.state = ProcessState::Blocked;
    pcb.blocked_on = waiting_on;
    sched.dequeue(pid);

    // Priority inheritance
    if let Some(target_pid) = waiting_on {
        let blocker_priority = pcb.effective_priority;
        let _ = pcb; // release mutable borrow

        // Walk the chain, boosting priorities
        apply_priority_inheritance(target_pid, blocker_priority, &mut table, &mut sched, 0);
    }

    Ok(())
}

/// Apply priority inheritance recursively (up to MAX_INHERITANCE_DEPTH).
fn apply_priority_inheritance(
    target_pid: ProcessId,
    boost_priority: u8,
    table: &mut ProcessTable,
    scheduler: &mut scheduler::Scheduler,
    depth: u32,
) {
    if depth >= MAX_INHERITANCE_DEPTH {
        return;
    }

    if let Some(target) = table.get_mut(target_pid) {
        if target.effective_priority < boost_priority {
            let old_priority = target.effective_priority;
            target.effective_priority = boost_priority;

            // Move in scheduler queues
            scheduler.dequeue(target_pid);
            if target.state == ProcessState::Ready || target.state == ProcessState::Running {
                scheduler.enqueue(target_pid, boost_priority);
            }

            // If target is also blocked on someone, continue the chain
            if let Some(next_target) = target.blocked_on {
                apply_priority_inheritance(
                    next_target,
                    boost_priority,
                    table,
                    scheduler,
                    depth + 1,
                );
            }

            let _ = old_priority; // suppress warning
        }
    }
}

/// Unblock a process — set to Ready and revert priority inheritance.
pub fn unblock(pid: ProcessId) -> Result<(), ProcessError> {
    let mut table = TABLE.lock();
    let mut sched = SCHEDULER.lock();

    let pcb = table.get_mut(pid).ok_or(ProcessError::NotFound)?;
    if pcb.state != ProcessState::Blocked {
        return Err(ProcessError::InvalidState);
    }

    // Revert the target we were blocking on
    let was_blocking = pcb.blocked_on.take();

    pcb.state = ProcessState::Ready;
    pcb.wake_tick = sched.current_tick();

    // Apply intent boost
    sched.apply_intent_boost(pcb);

    let eff = pcb.effective_priority;
    sched.enqueue(pid, eff);

    // Revert priority on the process we were waiting on
    if let Some(target_pid) = was_blocking {
        revert_priority_inheritance(target_pid, &mut table, &mut sched);
    }

    Ok(())
}

/// Revert priority inheritance — reset target to base priority.
fn revert_priority_inheritance(
    target_pid: ProcessId,
    table: &mut ProcessTable,
    scheduler: &mut scheduler::Scheduler,
) {
    if let Some(target) = table.get_mut(target_pid) {
        if target.effective_priority != target.base_priority {
            target.effective_priority = target.base_priority;
            scheduler.dequeue(target_pid);
            if target.state == ProcessState::Ready || target.state == ProcessState::Running {
                scheduler.enqueue(target_pid, target.base_priority);
            }
        }
    }
}

/// Crash a process — triggers supervision tree handling.
pub fn crash(pid: ProcessId) -> Result<(), ProcessError> {
    let mut table = TABLE.lock();
    let mut sched = SCHEDULER.lock();

    let supervisor_pid = {
        let pcb = table.get_mut(pid).ok_or(ProcessError::NotFound)?;
        pcb.state = ProcessState::Terminated;
        pcb.exit_reason = Some(ExitReason::Crash);
        sched.dequeue(pid);
        pcb.supervisor
    };

    let now = sched.current_tick();

    // Trigger supervision
    let _ = supervisor::handle_child_crash(supervisor_pid, pid, now, &mut table, &mut sched);

    Ok(())
}

/// Advance the global tick — syncs scheduler, capability, and bus ticks.
pub fn tick() {
    let mut sched = SCHEDULER.lock();
    sched.tick();
    drop(sched);

    crate::capability::tick();
    crate::bus::BUS.lock().tick();
}

/// Select the next process to run.
pub fn schedule_next() -> Option<ProcessId> {
    let mut table = TABLE.lock();
    let mut sched = SCHEDULER.lock();
    sched.schedule_next(&mut table)
}

/// Query process state.
pub fn get_state(pid: ProcessId) -> Option<ProcessState> {
    TABLE.lock().get(pid).map(|pcb| pcb.state)
}

/// Live process count.
pub fn count() -> usize {
    TABLE.lock().count()
}

/// Spawn a userspace process with its own address space and kernel stack.
///
/// 1. Creates a basic PCB via spawn()
/// 2. Allocates a 2-page kernel stack (8KB, order 1)
/// 3. Builds a fake SavedContext for first-time Ring 3 entry
/// 4. Stores saved_rsp, kernel stack info, and address space in the PCB
pub fn spawn_user(
    supervisor_pid: ProcessId,
    entry_point: u64,
    user_stack_top: u64,
    address_space: crate::address_space::AddressSpace,
    description: &str,
) -> Result<ProcessId, ProcessError> {
    // Step 1: Spawn a basic PCB under the supervisor
    let pid = spawn(
        supervisor_pid,
        Intent::default(),
        description,
        None,
    )?;

    // Step 2: Allocate a 2-page kernel stack (order 1 = 2^1 = 2 pages = 8KB)
    let kernel_stack_phys = {
        let mut alloc = frame::ALLOCATOR.lock();
        alloc.allocate(1) // order 1 = 2 contiguous pages
    };
    let kernel_stack_phys = match kernel_stack_phys {
        Some(p) => p,
        None => {
            // Clean up: terminate the process we just spawned
            let _ = terminate(pid);
            return Err(ProcessError::TableFull); // re-use error; no OutOfMemory variant
        }
    };

    let kernel_stack_base = kernel_stack_phys.to_virt();
    let kernel_stack_top = kernel_stack_base.as_u64() + (2 * PAGE_SIZE as u64);

    // Zero the kernel stack
    unsafe {
        core::ptr::write_bytes(kernel_stack_base.as_u64() as *mut u8, 0, 2 * PAGE_SIZE);
    }

    // Step 3: Build the initial SavedContext for Ring 3 entry
    let initial_ctx = crate::x86::context::SavedContext::for_user_entry(
        entry_point,
        user_stack_top,
        crate::x86::gdt::USER_CS,
        crate::x86::gdt::USER_DS,
    );

    // Step 4: Place the context on the kernel stack and get saved_rsp
    let saved_rsp = crate::x86::context::place_initial_context(
        kernel_stack_top,
        &initial_ctx,
    );

    // Step 5: Update the PCB with kernel stack and address space info
    {
        let mut table = TABLE.lock();
        if let Some(pcb) = table.get_mut(pid) {
            pcb.saved_rsp = saved_rsp;
            pcb.kernel_stack_base = Some(kernel_stack_base);
            pcb.kernel_stack_top = kernel_stack_top;
            pcb.has_run = false;
            pcb.is_user = true;
            pcb.address_space = Some(address_space);
        }
    }

    serial_println!(
        "[PROC] Spawned user process pid:{} entry=0x{:x} kstack=0x{:x}",
        pid.0, entry_point, kernel_stack_top
    );

    Ok(pid)
}

/// Spawn a userspace process from ELF binary data.
///
/// Combines ELF loading with spawn_user: creates address space, loads ELF,
/// maps user stack, then spawns the process.
pub fn spawn_elf(
    supervisor_pid: ProcessId,
    elf_data: &[u8],
    description: &str,
) -> Result<ProcessId, ProcessError> {
    use crate::elf;

    // Create a per-process address space
    let mut addr_space = crate::address_space::AddressSpace::create()
        .map_err(|_| ProcessError::TableFull)?;

    // Load ELF binary into the address space
    let entry_point = elf::load_elf(elf_data, &mut addr_space)
        .map_err(|_| ProcessError::TableFull)?;

    // Map user stack pages
    let stack_base = elf::USER_STACK_BASE;
    let stack_pages = elf::USER_STACK_PAGES;

    for i in 0..stack_pages {
        let page_va = stack_base + i * PAGE_SIZE as u64;
        let stack_frame = frame::allocate_frame()
            .ok_or(ProcessError::TableFull)?;

        // Zero the stack frame
        unsafe {
            core::ptr::write_bytes(
                stack_frame.to_virt().as_u64() as *mut u8,
                0,
                PAGE_SIZE,
            );
        }

        let stack_flags = PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
        addr_space.map_user_page(
            VirtAddr::new(page_va),
            stack_frame,
            stack_flags,
        ).map_err(|_| ProcessError::TableFull)?;
    }

    let user_stack_top = elf::USER_STACK_TOP;

    // Spawn the user process
    spawn_user(supervisor_pid, entry_point, user_stack_top, addr_space, description)
}
