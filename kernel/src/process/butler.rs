//! Butler — the root supervisor process.
//!
//! Butler is ProcessId(1), supervised by KERNEL (pid 0). It uses OneForOne
//! strategy by default. All top-level processes are children of Butler.

#![allow(dead_code)]

use alloc::string::String;
use fabric_types::{
    Intent, IntentCategory, Priority, EnergyClass, ProcessId,
    SupervisionStrategy, Timestamp,
};

use super::pcb::ProcessControlBlock;
use super::scheduler::Scheduler;
use super::supervisor::RestartTracker;
use super::table::ProcessTable;

/// Butler restart intensity: very high since it's the root supervisor.
const BUTLER_MAX_RESTARTS: u32 = 1000;
const BUTLER_INTENSITY_WINDOW: u64 = 60;

/// Initialize Butler as the root supervisor.
/// Must be called during process::init().
pub fn init(table: &mut ProcessTable, scheduler: &mut Scheduler) {
    let now = scheduler.current_tick();

    let intent = Intent {
        category: IntentCategory::Compute,
        priority: Priority::Critical,
        energy_class: EnergyClass::Balanced,
        _pad: 0,
        _reserved: 0,
        deadline: Timestamp::ZERO,
    };

    // Butler is pid 1, always
    let pid = ProcessId::BUTLER;

    // Force table to allocate pid 1
    let _ = table.alloc_pid(); // consumes 1

    let mut pcb = ProcessControlBlock::new(
        pid,
        intent,
        String::from("Butler"),
        ProcessId::KERNEL,
        SupervisionStrategy::OneForOne,
        0,
        now,
    );

    // Butler gets a very high restart intensity — root supervisor must be resilient
    pcb.restart_tracker = RestartTracker::new(BUTLER_MAX_RESTARTS, BUTLER_INTENSITY_WINDOW);

    table.insert(pcb).expect("insert Butler");
    scheduler.enqueue(pid, Priority::Critical as u8);
}
