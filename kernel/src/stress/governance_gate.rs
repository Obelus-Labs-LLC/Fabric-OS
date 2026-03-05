//! STRESS Phase 5A — Governance Gate Tests
//!
//! 8 tests verifying the deterministic governance engine:
//! constitution integrity, policy enforcement, safety states,
//! ACS dead-man switch, amendments, and policy under load.

#![allow(dead_code)]

use alloc::{format, vec::Vec};
use fabric_types::{
    MessageHeader, ProcessId, TypeId, Timestamp, ResourceId, Perm,
};
use fabric_types::governance::{SafetyState, AcsState};
use crate::governance;
use crate::governance::constitution::{GENESIS_RULE_COUNT, compute_constitution_hash, genesis_rules};
use crate::governance::rules::EvalContext;
use crate::bus;
use crate::capability;
use crate::process;
use crate::ocrb::StressResult;

/// Run all Phase 5A tests.
pub fn run_all_tests() -> Vec<StressResult> {
    let mut results = Vec::new();

    results.push(test_1_constitution_load_integrity());
    reset_state();

    results.push(test_2_normal_state_policy());
    reset_state();

    results.push(test_3_safety_state_transitions());
    reset_state();

    results.push(test_4_lockdown_blocks_messages());
    reset_state();

    results.push(test_5_acs_dead_man_switch());
    reset_state();

    results.push(test_6_amendment_cooling());
    reset_state();

    results.push(test_7_constitution_tamper_detection());
    reset_state();

    results.push(test_8_policy_under_load());
    reset_state();

    results
}

/// Reset all subsystem state between tests.
fn reset_state() {
    governance::GOVERNANCE.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    // Re-init Butler for clean state
    process::init();
}

/// Set up a sender process with a capability for bus messaging.
/// Returns (pid, cap_id).
fn setup_sender(pid_raw: u32, _priority: u8) -> (ProcessId, u64) {
    let pid = ProcessId::new(pid_raw);

    // Register on bus (Butler is already registered by process::init())
    if pid_raw != 1 {
        // Spawn the process under Butler for process table entry
        let intent = fabric_types::Intent::default();
        let _ = process::spawn(
            ProcessId::BUTLER,
            intent,
            "gov-test",
            None,
        );
        // Override the PID in process table (the spawned PID may not match)
        // Actually, we can't control PID allocation. Let's register directly on bus
        // and create a PCB manually if needed.

        // For simplicity: just register on bus
        let _ = bus::register_process(pid);
    }

    // Create an IPC capability
    let cap = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | pid_raw as u64),
        Perm::READ | Perm::WRITE,
        pid,
        None,
        None,
    ).expect("create test cap");

    (pid, cap.0)
}

/// Build a test message header.
fn build_header(
    sender: ProcessId,
    receiver: ProcessId,
    cap_id: u64,
    seq: u64,
) -> MessageHeader {
    let mut header = MessageHeader::zeroed();
    header.version = MessageHeader::VERSION;
    header.msg_type = TypeId(1);
    header.sender = sender;
    header.receiver = receiver;
    header.capability_id = cap_id;
    header.sequence = seq;
    header.timestamp = Timestamp(0);
    header.payload_len = 0;
    header
}

// ============================================================================
// Test 1: Constitution Load + Integrity (weight: 15)
// ============================================================================
fn test_1_constitution_load_integrity() -> StressResult {
    let mut score: u8 = 0;

    let gov = governance::GOVERNANCE.lock();

    // Check rule count == 9
    if gov.rules.rule_count() == GENESIS_RULE_COUNT {
        score += 30;
    } else {
        return StressResult {
            test_name: "Constitution Load + Integrity",
            passed: false,
            score,
            weight: 15,
            details: format!("Expected {} rules, got {}", GENESIS_RULE_COUNT, gov.rules.rule_count()),
        };
    }

    // Verify SHA3-256 hash
    let rules = genesis_rules();
    let expected_hash = compute_constitution_hash(&rules);
    if gov.constitution_hash == expected_hash {
        score += 30;
    } else {
        return StressResult {
            test_name: "Constitution Load + Integrity",
            passed: false,
            score,
            weight: 15,
            details: format!("Constitution hash mismatch!"),
        };
    }

    // Verify hash is non-zero
    let all_zero = gov.constitution_hash.iter().all(|&b| b == 0);
    if !all_zero {
        score += 20;
    }

    // Verify first rule is butler-unrestricted (highest priority = 1000)
    if let Some(rule) = gov.rules.get_rule(0) {
        if rule.name == "butler-unrestricted" && rule.priority == 1000 {
            score += 20;
        }
    }

    drop(gov);

    StressResult {
        test_name: "Constitution Load + Integrity",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("9 rules loaded, hash verified"),
    }
}

// ============================================================================
// Test 2: Normal State Policy (weight: 10)
// ============================================================================
fn test_2_normal_state_policy() -> StressResult {
    let mut score: u8 = 0;

    // Test rule evaluation directly via EvalContext
    let gov = governance::GOVERNANCE.lock();

    // Butler (PID 1) should always be allowed
    let butler_ctx = EvalContext {
        sender_pid: 1,
        receiver_pid: 2,
        msg_type: 1,
        capability_id: 100,
        resource_kind: 0,
        sender_priority: 5,
        safety_state: SafetyState::Normal,
        acs_state: AcsState::Active,
        tier_escalated: false,
    };
    let (verdict, name, _) = gov.rules.evaluate(&butler_ctx);
    if verdict == fabric_types::PolicyVerdict::Allow && name == "butler-unrestricted" {
        score += 30;
    }

    // Normal PID 2 should be allowed in Normal state (hits default-allow)
    let normal_ctx = EvalContext {
        sender_pid: 2,
        receiver_pid: 3,
        msg_type: 1,
        capability_id: 100,
        resource_kind: 0,
        sender_priority: 5,
        safety_state: SafetyState::Normal,
        acs_state: AcsState::Active,
        tier_escalated: false,
    };
    let (verdict, name, _) = gov.rules.evaluate(&normal_ctx);
    if verdict == fabric_types::PolicyVerdict::Allow && name == "normal-default-allow" {
        score += 30;
    }

    // PID 0 (kernel spoof) should be denied
    let kernel_ctx = EvalContext {
        sender_pid: 0,
        receiver_pid: 2,
        msg_type: 1,
        capability_id: 100,
        resource_kind: 0,
        sender_priority: 5,
        safety_state: SafetyState::Normal,
        acs_state: AcsState::Active,
        tier_escalated: false,
    };
    let (verdict, name, should_log) = gov.rules.evaluate(&kernel_ctx);
    if verdict == fabric_types::PolicyVerdict::Deny && name == "deny-kernel-spoof" && should_log {
        score += 40;
    }

    drop(gov);

    StressResult {
        test_name: "Normal State Policy",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("Butler allowed, normal pass, PID 0 blocked"),
    }
}

// ============================================================================
// Test 3: Safety State Transitions (weight: 15)
// ============================================================================
fn test_3_safety_state_transitions() -> StressResult {
    let mut score: u8 = 0;
    let mut gov = governance::GOVERNANCE.lock();

    // Start at Normal
    if gov.safety.state() == SafetyState::Normal {
        score += 10;
    }

    // Report 3 anomalies → should escalate to Elevated
    gov.safety.report_anomaly(100);
    gov.safety.report_anomaly(101);
    gov.safety.report_anomaly(102);
    if gov.safety.state() == SafetyState::Elevated {
        score += 15;
    }

    // Report 3 alarms → should escalate to Chaos
    gov.safety.report_alarm(200);
    gov.safety.report_alarm(201);
    gov.safety.report_alarm(202);
    if gov.safety.state() == SafetyState::Chaos {
        score += 15;
    }

    // Force lockdown
    gov.safety.force_lockdown(300);
    if gov.safety.state() == SafetyState::Lockdown {
        score += 15;
    }

    // Human confirm → Safe
    let confirmed = gov.safety.human_confirm(400);
    if confirmed && gov.safety.state() == SafetyState::Safe {
        score += 15;
    }

    // Human confirm while in Safe (should fail — only works from Lockdown)
    let second_confirm = gov.safety.human_confirm(500);
    if !second_confirm {
        score += 10;
    }

    // Tick enough to burn down Safe → Normal (1_800_000 ticks)
    gov.safety.tick(400 + 1_800_001);
    if gov.safety.state() == SafetyState::Normal {
        score += 10;
    }

    // Verify total transitions: Normal→Elevated→Chaos→Lockdown→Safe→Normal = 5
    if gov.safety.total_transitions() == 5 {
        score += 10;
    }

    drop(gov);

    StressResult {
        test_name: "Safety State Transitions",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("All 5 states traversed, invalid transitions rejected"),
    }
}

// ============================================================================
// Test 4: Lockdown Blocks Messages (weight: 15)
// ============================================================================
fn test_4_lockdown_blocks_messages() -> StressResult {
    let mut score: u8 = 0;

    // Set up sender/receiver infrastructure
    let (_butler, butler_cap) = setup_sender(1, 5);
    let _ = bus::register_process(ProcessId::new(2));

    let cap_for_pid2 = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 2),
        Perm::READ | Perm::WRITE,
        ProcessId::new(2),
        None,
        None,
    ).expect("create cap for pid2");
    let _ = bus::register_process(ProcessId::new(3));

    // In Normal state, PID 2 should be able to send
    let header_p2 = build_header(ProcessId::new(2), ProcessId::new(3), cap_for_pid2.0, 1);
    match bus::send(&header_p2, None, 1) {
        Ok(()) => score += 20,
        Err(_) => {}
    }

    // Force Lockdown
    {
        let mut gov = governance::GOVERNANCE.lock();
        gov.safety.force_lockdown(1000);
    }

    // PID 2 send should now be denied (lockdown-deny-all)
    let header_p2_lock = build_header(ProcessId::new(2), ProcessId::new(3), cap_for_pid2.0, 2);
    match bus::send(&header_p2_lock, None, 2) {
        Err(bus::BusError::PolicyDenied) => score += 30,
        _ => {}
    }

    // Butler (PID 1) should still be allowed in Lockdown
    let header_butler = build_header(ProcessId::new(1), ProcessId::new(2), butler_cap, 1);
    match bus::send(&header_butler, None, 3) {
        Ok(()) => score += 30,
        Err(_) => {}
    }

    // Verify audit logged the denial
    let (audit_count, chain_valid) = bus::verify_audit_chain();
    if audit_count > 0 && chain_valid {
        score += 20;
    }

    StressResult {
        test_name: "Lockdown Blocks Messages",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("Non-Butler denied, Butler passes in Lockdown"),
    }
}

// ============================================================================
// Test 5: ACS Dead-Man Switch (weight: 15)
// ============================================================================
fn test_5_acs_dead_man_switch() -> StressResult {
    let mut score: u8 = 0;
    let mut gov = governance::GOVERNANCE.lock();

    // Start Active
    if gov.acs.state() == AcsState::Active {
        score += 10;
    }

    // Send initial heartbeat at tick 0
    gov.acs.heartbeat(0);

    // Advance past Degraded threshold (3_600_000 ticks = 1h)
    gov.acs.tick(3_600_001);
    if gov.acs.state() == AcsState::Degraded {
        score += 20;
    }

    // Set alternate exists and advance to Contingency (7_200_000 ticks = 2h)
    gov.acs.set_alternate_exists(true);
    gov.acs.tick(7_200_001);
    if gov.acs.state() == AcsState::Contingency {
        score += 15;
    }

    // Advance to Emergency (14_400_000 ticks = 4h)
    gov.acs.tick(14_400_001);
    if gov.acs.state() == AcsState::Emergency {
        score += 15;
    }

    // Emergency should trigger safety escalation flag
    if gov.acs.take_emergency_trigger() {
        score += 10;
    }

    // Heartbeat restores Active from Emergency
    gov.acs.heartbeat(15_000_000);
    if gov.acs.state() == AcsState::Active {
        score += 20;
    }

    // Verify total transitions: Active→Degraded→Contingency→Emergency→Active = 4
    if gov.acs.total_transitions() == 4 {
        score += 10;
    }

    drop(gov);

    StressResult {
        test_name: "ACS Dead-Man Switch",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("Heartbeat/timeout/Degraded/Contingency/Emergency verified"),
    }
}

// ============================================================================
// Test 6: Amendment Cooling (weight: 10)
// ============================================================================
fn test_6_amendment_cooling() -> StressResult {
    let mut score: u8 = 0;
    let mut gov = governance::GOVERNANCE.lock();

    // Proposal in Normal state should succeed
    let proposed = gov.amendments.propose(SafetyState::Normal, 1000);
    if proposed {
        score += 25;
    }

    // Proposal while cooling should fail
    let proposed_again = gov.amendments.propose(SafetyState::Normal, 2000);
    if !proposed_again {
        score += 25;
    }

    // Apply before cooling period expires should fail
    let applied = gov.amendments.apply(50_000);
    if !applied {
        score += 15;
    }

    // Apply after cooling period (86_400_000 ticks = ~24h)
    let applied_after = gov.amendments.apply(1000 + 86_400_001);
    if applied_after {
        score += 20;
    }

    // Proposal in non-Normal state should fail
    gov.safety.force_state(SafetyState::Elevated, 100_000_000);
    let proposed_elevated = gov.amendments.propose(SafetyState::Elevated, 100_000_001);
    if !proposed_elevated {
        score += 15;
    }

    drop(gov);

    StressResult {
        test_name: "Amendment Cooling",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("Cooling enforced, non-Normal blocked"),
    }
}

// ============================================================================
// Test 7: Constitution Tamper Detection (weight: 10)
// ============================================================================
fn test_7_constitution_tamper_detection() -> StressResult {
    let mut score: u8 = 0;
    let mut gov = governance::GOVERNANCE.lock();

    // Original hash should verify
    if gov.verify_constitution() {
        score += 40;
    }

    // Tamper with the stored hash
    let original_hash = gov.constitution_hash;
    gov.constitution_hash[0] ^= 0xFF;

    // Verification should now fail
    if !gov.verify_constitution() {
        score += 40;
    }

    // Restore and verify again
    gov.constitution_hash = original_hash;
    if gov.verify_constitution() {
        score += 20;
    }

    drop(gov);

    StressResult {
        test_name: "Constitution Tamper Detection",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("Hash mismatch detected after tamper"),
    }
}

// ============================================================================
// Test 8: Policy Under Load (weight: 10)
// ============================================================================
fn test_8_policy_under_load() -> StressResult {
    let mut score: u8 = 0;

    let gov = governance::GOVERNANCE.lock();

    // Evaluate 1000 policy checks in Normal state — all should Allow
    let mut normal_allows = 0u32;
    for i in 0..1000 {
        let ctx = EvalContext {
            sender_pid: 2,
            receiver_pid: 3,
            msg_type: (i % 100) as u16,
            capability_id: 100,
            resource_kind: 0,
            sender_priority: 5,
            safety_state: SafetyState::Normal,
            acs_state: AcsState::Active,
            tier_escalated: false,
        };
        let (verdict, _, _) = gov.rules.evaluate(&ctx);
        if verdict == fabric_types::PolicyVerdict::Allow {
            normal_allows += 1;
        }
    }
    if normal_allows == 1000 {
        score += 30;
    }

    // Evaluate 1000 in Lockdown — all non-Butler should Deny
    let mut lockdown_denials = 0u32;
    for i in 0..1000 {
        let ctx = EvalContext {
            sender_pid: 2 + (i % 50) as u32,
            receiver_pid: 3,
            msg_type: (i % 100) as u16,
            capability_id: 100,
            resource_kind: 0,
            sender_priority: 5,
            safety_state: SafetyState::Lockdown,
            acs_state: AcsState::Active,
            tier_escalated: false,
        };
        let (verdict, _, _) = gov.rules.evaluate(&ctx);
        if verdict == fabric_types::PolicyVerdict::Deny {
            lockdown_denials += 1;
        }
    }
    if lockdown_denials == 1000 {
        score += 30;
    }

    // Butler should always be allowed, even in Lockdown
    let mut butler_allows = 0u32;
    for _ in 0..100 {
        let ctx = EvalContext {
            sender_pid: 1,
            receiver_pid: 3,
            msg_type: 1,
            capability_id: 100,
            resource_kind: 0,
            sender_priority: 5,
            safety_state: SafetyState::Lockdown,
            acs_state: AcsState::Active,
            tier_escalated: false,
        };
        let (verdict, _, _) = gov.rules.evaluate(&ctx);
        if verdict == fabric_types::PolicyVerdict::Allow {
            butler_allows += 1;
        }
    }
    if butler_allows == 100 {
        score += 20;
    }

    // Chaos state: high priority allowed, low priority denied
    let mut chaos_correct = 0u32;
    for i in 0..100 {
        let prio = if i % 2 == 0 { 5u8 } else { 1u8 }; // high vs low
        let ctx = EvalContext {
            sender_pid: 2,
            receiver_pid: 3,
            msg_type: 1,
            capability_id: 100,
            resource_kind: 0,
            sender_priority: prio,
            safety_state: SafetyState::Chaos,
            acs_state: AcsState::Active,
            tier_escalated: false,
        };
        let (verdict, _, _) = gov.rules.evaluate(&ctx);
        if prio >= 4 && verdict == fabric_types::PolicyVerdict::Allow {
            chaos_correct += 1;
        } else if prio < 4 && verdict == fabric_types::PolicyVerdict::Deny {
            chaos_correct += 1;
        }
    }
    if chaos_correct == 100 {
        score += 20;
    }

    drop(gov);

    StressResult {
        test_name: "Policy Under Load",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!(
            "Normal:{}/1000 allow, Lockdown:{}/1000 deny, Butler:{}/100 allow, Chaos:{}/100 correct",
            normal_allows, lockdown_denials, butler_allows, chaos_correct
        ),
    }
}
