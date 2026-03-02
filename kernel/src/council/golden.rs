//! Golden test suite — 10 curated test cases for Council regression testing.
//!
//! Run after every weight update. Any regression triggers rollback.
//! Cases cover Tier 1 passthrough, Tier 2/3 decisions, and integrity checks.

#![allow(dead_code)]

use fabric_types::governance::{PolicyVerdict, SafetyState, AcsState};
use crate::governance::rules::EvalContext;

/// Number of golden test cases.
pub const GOLDEN_CASE_COUNT: usize = 10;

/// A golden test case: context + expected verdict.
pub struct GoldenCase {
    pub name: &'static str,
    pub ctx: EvalContext,
    /// Expected verdict from Tier 1 evaluation.
    pub expected_tier1: PolicyVerdict,
}

/// Build the 10 golden test cases.
pub fn golden_cases() -> [GoldenCase; GOLDEN_CASE_COUNT] {
    [
        // 1. Butler send in Normal — always allowed
        GoldenCase {
            name: "butler-normal-allow",
            ctx: EvalContext {
                sender_pid: 1, receiver_pid: 2, msg_type: 1,
                capability_id: 100, resource_kind: 0, sender_priority: 5,
                safety_state: SafetyState::Normal, acs_state: AcsState::Active,
                tier_escalated: false,
            },
            expected_tier1: PolicyVerdict::Allow,
        },
        // 2. PID 0 kernel spoof — always denied
        GoldenCase {
            name: "kernel-spoof-deny",
            ctx: EvalContext {
                sender_pid: 0, receiver_pid: 2, msg_type: 1,
                capability_id: 100, resource_kind: 0, sender_priority: 5,
                safety_state: SafetyState::Normal, acs_state: AcsState::Active,
                tier_escalated: false,
            },
            expected_tier1: PolicyVerdict::Deny,
        },
        // 3. Low-priority in Elevated — denied
        GoldenCase {
            name: "elevated-low-pri-deny",
            ctx: EvalContext {
                sender_pid: 5, receiver_pid: 2, msg_type: 1,
                capability_id: 100, resource_kind: 0, sender_priority: 1,
                safety_state: SafetyState::Elevated, acs_state: AcsState::Active,
                tier_escalated: false,
            },
            expected_tier1: PolicyVerdict::Deny,
        },
        // 4. Normal user, device access — AllowIfCapValid (maps to Allow)
        GoldenCase {
            name: "device-access-cap-gated",
            ctx: EvalContext {
                sender_pid: 5, receiver_pid: 2, msg_type: 1,
                capability_id: 100, resource_kind: 3, sender_priority: 5,
                safety_state: SafetyState::Normal, acs_state: AcsState::Active,
                tier_escalated: false,
            },
            expected_tier1: PolicyVerdict::Allow, // AllowIfCapValid → Allow in verdict()
        },
        // 5. ACS Emergency — escalate to Chaos
        GoldenCase {
            name: "acs-emergency-escalate",
            ctx: EvalContext {
                sender_pid: 5, receiver_pid: 2, msg_type: 1,
                capability_id: 100, resource_kind: 0, sender_priority: 5,
                safety_state: SafetyState::Normal, acs_state: AcsState::Emergency,
                tier_escalated: false,
            },
            expected_tier1: PolicyVerdict::Escalate(SafetyState::Chaos),
        },
        // 6. Normal process in Normal state — allowed
        GoldenCase {
            name: "normal-process-allow",
            ctx: EvalContext {
                sender_pid: 10, receiver_pid: 3, msg_type: 2,
                capability_id: 200, resource_kind: 0, sender_priority: 3,
                safety_state: SafetyState::Normal, acs_state: AcsState::Active,
                tier_escalated: false,
            },
            expected_tier1: PolicyVerdict::Allow,
        },
        // 7. Lockdown blocks non-Butler
        GoldenCase {
            name: "lockdown-deny-non-butler",
            ctx: EvalContext {
                sender_pid: 5, receiver_pid: 2, msg_type: 1,
                capability_id: 100, resource_kind: 0, sender_priority: 5,
                safety_state: SafetyState::Lockdown, acs_state: AcsState::Active,
                tier_escalated: false,
            },
            expected_tier1: PolicyVerdict::Deny,
        },
        // 8. Chaos blocks low-priority
        GoldenCase {
            name: "chaos-low-pri-deny",
            ctx: EvalContext {
                sender_pid: 5, receiver_pid: 2, msg_type: 1,
                capability_id: 100, resource_kind: 0, sender_priority: 2,
                safety_state: SafetyState::Chaos, acs_state: AcsState::Active,
                tier_escalated: false,
            },
            expected_tier1: PolicyVerdict::Deny,
        },
        // 9. AI resource access gated — non-Butler
        GoldenCase {
            name: "ai-resource-cap-gated",
            ctx: EvalContext {
                sender_pid: 5, receiver_pid: 2, msg_type: 1,
                capability_id: 100, resource_kind: 6, sender_priority: 5,
                safety_state: SafetyState::Normal, acs_state: AcsState::Active,
                tier_escalated: false,
            },
            expected_tier1: PolicyVerdict::Allow, // AllowIfCapValid → Allow
        },
        // 10. Butler in Lockdown — still allowed
        GoldenCase {
            name: "butler-lockdown-allow",
            ctx: EvalContext {
                sender_pid: 1, receiver_pid: 2, msg_type: 1,
                capability_id: 100, resource_kind: 0, sender_priority: 5,
                safety_state: SafetyState::Lockdown, acs_state: AcsState::Active,
                tier_escalated: false,
            },
            expected_tier1: PolicyVerdict::Allow,
        },
    ]
}

/// Golden test suite runner.
pub struct GoldenTestSuite;

impl GoldenTestSuite {
    /// Run all golden tests against the current rule engine.
    /// Returns (passed_count, total, first_failure_name).
    pub fn run_all(rules: &crate::governance::rules::RuleEngine) -> (usize, usize, Option<&'static str>) {
        let cases = golden_cases();
        let mut passed = 0;
        let mut first_failure: Option<&'static str> = None;

        for case in &cases {
            let (verdict, _, _) = rules.evaluate(&case.ctx);
            if verdict == case.expected_tier1 {
                passed += 1;
            } else if first_failure.is_none() {
                first_failure = Some(case.name);
            }
        }

        (passed, GOLDEN_CASE_COUNT, first_failure)
    }
}
