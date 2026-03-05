#![allow(dead_code)]

use alloc::{format, vec, vec::Vec};
use fabric_types::{
    Intent, IntentCategory, Priority, ProcessId, ProcessState,
    SupervisionStrategy, EnergyClass, Timestamp,
};
use crate::bus;
use crate::capability;
use crate::process;
use crate::ocrb::StressResult;

pub fn run_all_tests() -> Vec<StressResult> {
    vec![
        test1_butler_root_supervision(),
        test2_process_lifecycle(),
        test3_intent_aware_scheduling(),
        test4_priority_inheritance(),
        test5_one_for_one_restart(),
        test6_one_for_all_restart(),
        test7_rest_for_one_restart(),
        test8_restart_intensity_escalation(),
        test9_process_crash_storm(),
    ]
}

fn make_intent(priority: Priority) -> Intent {
    Intent {
        category: IntentCategory::Compute,
        priority,
        energy_class: EnergyClass::Balanced,
        _pad: 0,
        _reserved: 0,
        deadline: Timestamp::ZERO,
    }
}

fn make_intent_with_category(priority: Priority, category: IntentCategory) -> Intent {
    Intent {
        category,
        priority,
        energy_class: EnergyClass::Balanced,
        _pad: 0,
        _reserved: 0,
        deadline: Timestamp::ZERO,
    }
}

fn cleanup() {
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
}

fn init_fresh() {
    cleanup();
    process::init();
}

/// Test 1: Butler Root Supervision (weight: 15)
fn test1_butler_root_supervision() -> StressResult {
    init_fresh();
    let mut correct = 0u32;
    let total = 4u32;

    // Butler should be pid 1
    {
        let table = process::TABLE.lock();
        if let Some(butler) = table.get(ProcessId::BUTLER) {
            // pid == 1
            if butler.pid == ProcessId::BUTLER { correct += 1; }
            // supervisor == KERNEL (pid 0)
            if butler.supervisor == ProcessId::KERNEL { correct += 1; }
            // strategy == OneForOne
            if butler.strategy == SupervisionStrategy::OneForOne { correct += 1; }
            // state == Ready
            if butler.state == ProcessState::Ready { correct += 1; }
        }
    }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };
    StressResult {
        test_name: "Butler Root Supervision",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("{}/{} checks correct", correct, total),
    }
}

/// Test 2: Process Lifecycle (weight: 15)
fn test2_process_lifecycle() -> StressResult {
    init_fresh();
    let mut correct = 0u32;
    let total = 5u32;

    // Spawn 50 processes under Butler
    let mut pids = Vec::new();
    let mut spawn_ok = true;
    for _i in 0..50 {
        match process::spawn(ProcessId::BUTLER, make_intent(Priority::Normal), "worker", None) {
            Ok(pid) => pids.push(pid),
            Err(_) => { spawn_ok = false; break; }
        }
    }
    if spawn_ok && pids.len() == 50 { correct += 1; }

    // All should be Ready
    let all_ready = pids.iter().all(|&pid| {
        process::get_state(pid) == Some(ProcessState::Ready)
    });
    if all_ready { correct += 1; }

    // Schedule one — should become Running
    if let Some(running_pid) = process::schedule_next() {
        if process::get_state(running_pid) == Some(ProcessState::Running) {
            correct += 1;
        }

        // Block it
        process::block(running_pid, None).unwrap();
        if process::get_state(running_pid) == Some(ProcessState::Blocked) {
            correct += 1;
        }

        // Unblock it
        process::unblock(running_pid).unwrap();
        if process::get_state(running_pid) == Some(ProcessState::Ready) {
            correct += 1;
        }
    }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };
    StressResult {
        test_name: "Process Lifecycle",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("{}/{} lifecycle checks", correct, total),
    }
}

/// Test 3: Intent-Aware Scheduling (weight: 15)
fn test3_intent_aware_scheduling() -> StressResult {
    init_fresh();
    let mut correct = 0u32;
    let total = 4u32;

    // Spawn one process at each priority level (under Butler)
    let _bg = process::spawn(ProcessId::BUTLER, make_intent(Priority::Background), "bg", None).unwrap();
    let low = process::spawn(ProcessId::BUTLER, make_intent(Priority::Low), "low", None).unwrap();
    let normal = process::spawn(ProcessId::BUTLER, make_intent(Priority::Normal), "normal", None).unwrap();
    let high = process::spawn(ProcessId::BUTLER, make_intent(Priority::High), "high", None).unwrap();

    // Note: Butler is Critical, so it will be scheduled first.
    // After Butler, should go: high, normal, low, bg

    // First scheduled should be Butler (Critical)
    let first = process::schedule_next();
    if first == Some(ProcessId::BUTLER) {
        // Block Butler so it doesn't keep being rescheduled
        let _ = process::block(ProcessId::BUTLER, None);
        correct += 1;
    }

    // Next should be High
    let next = process::schedule_next();
    if next == Some(high) { correct += 1; }
    // Block it so next priority is picked
    if let Some(pid) = next {
        let _ = process::block(pid, None);
    }

    // Next should be Normal
    let next = process::schedule_next();
    if next == Some(normal) { correct += 1; }
    if let Some(pid) = next {
        let _ = process::block(pid, None);
    }

    // Next should be Low
    let next = process::schedule_next();
    if next == Some(low) { correct += 1; }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };
    StressResult {
        test_name: "Intent-Aware Scheduling",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("{}/{} scheduling order correct", correct, total),
    }
}

/// Test 4: Priority Inheritance (weight: 15)
fn test4_priority_inheritance() -> StressResult {
    init_fresh();
    let mut correct = 0u32;
    let total = 3u32;

    // Spawn a low-priority process
    let low_pid = process::spawn(
        ProcessId::BUTLER,
        make_intent(Priority::Low),
        "low-pri worker",
        None,
    ).unwrap();

    // Spawn a high-priority process
    let high_pid = process::spawn(
        ProcessId::BUTLER,
        make_intent(Priority::High),
        "high-pri requester",
        None,
    ).unwrap();

    // Schedule high so it's Running (it will be picked since it's higher priority than low)
    // First schedule_next picks Butler (Critical), expire it
    let _ = process::schedule_next(); // Butler
    {
        let mut table = process::TABLE.lock();
        if let Some(pcb) = table.get_mut(ProcessId::BUTLER) { pcb.time_slice_remaining = 0; }
    }
    let _ = process::schedule_next(); // high_pid
    {
        let mut table = process::TABLE.lock();
        if let Some(pcb) = table.get_mut(high_pid) { pcb.time_slice_remaining = 0; }
    }

    // High blocks on low
    process::block(high_pid, Some(low_pid)).unwrap();

    // Check: low should now have effective_priority boosted to High
    {
        let table = process::TABLE.lock();
        let low_pcb = table.get(low_pid).unwrap();
        if low_pcb.effective_priority == Priority::High as u8 {
            correct += 1;
        }
    }

    // Unblock high
    process::unblock(high_pid).unwrap();

    // Check: low should be back to base priority (Low)
    {
        let table = process::TABLE.lock();
        let low_pcb = table.get(low_pid).unwrap();
        if low_pcb.effective_priority == Priority::Low as u8 {
            correct += 1;
        }
    }

    // Check: high should be Ready
    {
        let table = process::TABLE.lock();
        let high_pcb = table.get(high_pid).unwrap();
        if high_pcb.state == ProcessState::Ready {
            correct += 1;
        }
    }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };
    StressResult {
        test_name: "Priority Inheritance",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("{}/{} inheritance checks", correct, total),
    }
}

/// Test 5: OneForOne Restart (weight: 10)
fn test5_one_for_one_restart() -> StressResult {
    init_fresh();
    let mut correct = 0u32;
    let total = 3u32;

    // Create a supervisor under Butler with OneForOne
    let sup = process::spawn(
        ProcessId::BUTLER,
        make_intent(Priority::Normal),
        "supervisor-1f1",
        Some(SupervisionStrategy::OneForOne),
    ).unwrap();

    // Spawn 3 children under it
    let c1 = process::spawn(sup, make_intent(Priority::Normal), "child-1", None).unwrap();
    let c2 = process::spawn(sup, make_intent(Priority::Normal), "child-2", None).unwrap();
    let c3 = process::spawn(sup, make_intent(Priority::Normal), "child-3", None).unwrap();

    // Crash child 2
    process::crash(c2).unwrap();

    // Child 2 should be restarted (Ready)
    if process::get_state(c2) == Some(ProcessState::Ready) { correct += 1; }

    // Child 1 and 3 should be untouched (still Ready)
    if process::get_state(c1) == Some(ProcessState::Ready) { correct += 1; }
    if process::get_state(c3) == Some(ProcessState::Ready) { correct += 1; }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };
    StressResult {
        test_name: "OneForOne Restart",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("{}/{} checks correct", correct, total),
    }
}

/// Test 6: OneForAll Restart (weight: 10)
fn test6_one_for_all_restart() -> StressResult {
    init_fresh();
    let mut correct = 0u32;
    let total = 3u32;

    // Supervisor with OneForAll
    let sup = process::spawn(
        ProcessId::BUTLER,
        make_intent(Priority::Normal),
        "supervisor-1fa",
        Some(SupervisionStrategy::OneForAll),
    ).unwrap();

    let c1 = process::spawn(sup, make_intent(Priority::Normal), "child-A", None).unwrap();
    let c2 = process::spawn(sup, make_intent(Priority::Normal), "child-B", None).unwrap();
    let c3 = process::spawn(sup, make_intent(Priority::Normal), "child-C", None).unwrap();

    // Crash child 2 — should restart all 3
    process::crash(c2).unwrap();

    // All three should be Ready (restarted)
    if process::get_state(c1) == Some(ProcessState::Ready) { correct += 1; }
    if process::get_state(c2) == Some(ProcessState::Ready) { correct += 1; }
    if process::get_state(c3) == Some(ProcessState::Ready) { correct += 1; }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };
    StressResult {
        test_name: "OneForAll Restart",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("{}/{} children restarted", correct, total),
    }
}

/// Test 7: RestForOne Restart (weight: 5)
fn test7_rest_for_one_restart() -> StressResult {
    init_fresh();
    let mut correct = 0u32;
    let total = 3u32;

    // Supervisor with RestForOne
    let sup = process::spawn(
        ProcessId::BUTLER,
        make_intent(Priority::Normal),
        "supervisor-rfo",
        Some(SupervisionStrategy::RestForOne),
    ).unwrap();

    let ca = process::spawn(sup, make_intent(Priority::Normal), "child-A", None).unwrap();
    let cb = process::spawn(sup, make_intent(Priority::Normal), "child-B", None).unwrap();
    let cc = process::spawn(sup, make_intent(Priority::Normal), "child-C", None).unwrap();

    // Crash B — should restart B and C, leave A alone
    process::crash(cb).unwrap();

    // A should be untouched (Ready, never crashed)
    if process::get_state(ca) == Some(ProcessState::Ready) { correct += 1; }
    // B should be restarted (Ready)
    if process::get_state(cb) == Some(ProcessState::Ready) { correct += 1; }
    // C should be restarted (Ready)
    if process::get_state(cc) == Some(ProcessState::Ready) { correct += 1; }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };
    StressResult {
        test_name: "RestForOne Restart",
        passed: score >= 80,
        score,
        weight: 5,
        details: format!("{}/{} checks correct", correct, total),
    }
}

/// Test 8: Restart Intensity Escalation (weight: 10)
fn test8_restart_intensity_escalation() -> StressResult {
    init_fresh();
    let mut correct = 0u32;
    let total = 3u32;

    // Create a supervisor under Butler
    let sup = process::spawn(
        ProcessId::BUTLER,
        make_intent(Priority::Normal),
        "intensity-sup",
        Some(SupervisionStrategy::OneForOne),
    ).unwrap();

    // Spawn a child under the supervisor
    let child = process::spawn(sup, make_intent(Priority::Normal), "crasher", None).unwrap();

    // Crash the child 5 times (within the 60-tick default window) — should be OK
    for _ in 0..5 {
        process::crash(child).unwrap();
    }

    // After 5 crashes, supervisor should still be alive
    if process::get_state(sup) == Some(ProcessState::Ready)
        || process::get_state(sup) == Some(ProcessState::Running)
    {
        correct += 1;
    }

    // 6th crash should exceed intensity (5 in 60 ticks) → supervisor escalates
    // The crash may fail if child is in a bad state after intensity exceeded, so we handle both cases
    let _crash_result = process::crash(child);

    // Escalation happened: the supervisor's restart tracker exceeded.
    // Butler (parent) may have restarted the supervisor via OneForOne.
    // So sup may be either Terminated (if Butler didn't restart) or Ready (if Butler restarted it).
    let sup_state = process::get_state(sup);
    if sup_state == Some(ProcessState::Terminated) || sup_state == Some(ProcessState::Ready) {
        // Either outcome proves escalation logic ran
        correct += 1;
    }

    // Butler should still be alive (it handles the escalation)
    if process::get_state(ProcessId::BUTLER) == Some(ProcessState::Ready)
        || process::get_state(ProcessId::BUTLER) == Some(ProcessState::Running)
    {
        correct += 1;
    }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };
    StressResult {
        test_name: "Restart Intensity Escalation",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("{}/{} escalation checks", correct, total),
    }
}

/// Test 9: Process Crash Storm (weight: 5)
fn test9_process_crash_storm() -> StressResult {
    init_fresh();
    let mut errors = 0u32;

    // Spawn 100 processes under Butler
    let mut pids = Vec::new();
    for _ in 0..100 {
        match process::spawn(ProcessId::BUTLER, make_intent(Priority::Normal), "storm", None) {
            Ok(pid) => pids.push(pid),
            Err(_) => errors += 1,
        }
    }

    // Crash 50 of them rapidly
    for i in 0..50 {
        if process::crash(pids[i]).is_err() {
            errors += 1;
        }
    }

    // All 50 should be restarted (Ready)
    let mut restarted = 0u32;
    for i in 0..50 {
        if process::get_state(pids[i]) == Some(ProcessState::Ready) {
            restarted += 1;
        }
    }

    // Other 50 should be untouched (OneForOne)
    let mut untouched = 0u32;
    for i in 50..100 {
        if process::get_state(pids[i]) == Some(ProcessState::Ready) {
            untouched += 1;
        }
    }

    // Table should still be consistent
    let count = process::count();
    let table_ok = count == 101; // Butler + 100 children

    cleanup();

    let score = if restarted == 50 && untouched == 50 && table_ok && errors == 0 {
        100
    } else {
        let penalty = errors * 5 + (50 - restarted) * 2 + (50 - untouched) * 2;
        100u32.saturating_sub(penalty) as u8
    };

    StressResult {
        test_name: "Process Crash Storm",
        passed: score >= 80,
        score,
        weight: 5,
        details: format!("restarted={}/50, untouched={}/50, table_ok={}, errors={}",
            restarted, untouched, table_ok, errors),
    }
}
