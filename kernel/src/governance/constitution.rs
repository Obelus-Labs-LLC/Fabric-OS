//! Constitution — 9 genesis rules embedded as const, SHA3-256 integrity hash.
//!
//! The constitution is the immutable foundation of the governance system.
//! Rules are compiled into the kernel binary (no filesystem dependency).
//! A SHA3-256 hash is computed at boot and verified on each policy check.
//!
//! Amendments require: Normal safety state + 86.4M tick cooling period (~24h @ 1kHz).

#![allow(dead_code)]

use sha3::{Sha3_256, Digest};
use fabric_types::governance::{RuleCondition, RuleAction, SafetyState};
use super::rules::{Condition, PolicyRule};

/// Cooling period for amendments: ~24 hours at 1kHz tick rate.
pub const AMENDMENT_COOLING_TICKS: u64 = 86_400_000;

/// Number of genesis rules.
pub const GENESIS_RULE_COUNT: usize = 9;

/// Build the 9 genesis rules as a const-compatible array.
pub fn genesis_rules() -> [PolicyRule; GENESIS_RULE_COUNT] {
    let empty_cond = Condition::new(RuleCondition::Always, 0);

    [
        // 1. butler-unrestricted: Butler (PID 1) always allowed
        PolicyRule {
            name: "butler-unrestricted",
            priority: 1000,
            condition_count: 1,
            conditions: [
                Condition::new(RuleCondition::SenderEquals, 1),
                empty_cond, empty_cond, empty_cond,
            ],
            action: RuleAction::Allow,
        },
        // 2. lockdown-deny-all: In Lockdown, deny all non-Butler messages
        PolicyRule {
            name: "lockdown-deny-all",
            priority: 900,
            condition_count: 2,
            conditions: [
                Condition::new(RuleCondition::SafetyStateAtLeast, SafetyState::Lockdown as u64),
                Condition::new(RuleCondition::SenderNotButler, 0),
                empty_cond, empty_cond,
            ],
            action: RuleAction::DenyAndLog,
        },
        // 3. chaos-critical-only: In Chaos, deny low-priority non-Butler messages
        PolicyRule {
            name: "chaos-critical-only",
            priority: 800,
            condition_count: 3,
            conditions: [
                Condition::new(RuleCondition::SafetyStateAtLeast, SafetyState::Chaos as u64),
                Condition::new(RuleCondition::SenderNotButler, 0),
                Condition::new(RuleCondition::PriorityBelow, 4),
                empty_cond,
            ],
            action: RuleAction::DenyAndLog,
        },
        // 4. elevated-throttle-low: In Elevated+, deny very low priority non-Butler
        PolicyRule {
            name: "elevated-throttle-low",
            priority: 700,
            condition_count: 2,
            conditions: [
                Condition::new(RuleCondition::SafetyStateAtLeast, SafetyState::Elevated as u64),
                Condition::new(RuleCondition::PriorityBelow, 2),
                empty_cond, empty_cond,
            ],
            action: RuleAction::Deny,
        },
        // 5. deny-kernel-spoof: PID 0 (kernel) cannot send messages
        PolicyRule {
            name: "deny-kernel-spoof",
            priority: 950,
            condition_count: 1,
            conditions: [
                Condition::new(RuleCondition::SenderEquals, 0),
                empty_cond, empty_cond, empty_cond,
            ],
            action: RuleAction::DenyAndLog,
        },
        // 6. acs-emergency-escalate: ACS Emergency triggers Chaos escalation
        PolicyRule {
            name: "acs-emergency-escalate",
            priority: 850,
            condition_count: 1,
            conditions: [
                Condition::new(RuleCondition::AcsStateEquals, 3), // Emergency = 3
                empty_cond, empty_cond, empty_cond,
            ],
            action: RuleAction::EscalateToChaos,
        },
        // 7. device-access-gated: Device resource access requires valid cap (non-Butler)
        PolicyRule {
            name: "device-access-gated",
            priority: 600,
            condition_count: 2,
            conditions: [
                Condition::new(RuleCondition::ResourceKindEquals, 3), // KIND_DEVICE upper 16 bits = 3
                Condition::new(RuleCondition::SenderNotButler, 0),
                empty_cond, empty_cond,
            ],
            action: RuleAction::AllowIfCapValid,
        },
        // 8. normal-default-allow: Default allow in Normal state
        PolicyRule {
            name: "normal-default-allow",
            priority: 100,
            condition_count: 1,
            conditions: [
                Condition::new(RuleCondition::Always, 0),
                empty_cond, empty_cond, empty_cond,
            ],
            action: RuleAction::Allow,
        },
        // 9. safe-state-allow: In Safe state, allow non-Butler with cap validation
        PolicyRule {
            name: "safe-state-allow",
            priority: 650,
            condition_count: 2,
            conditions: [
                Condition::new(RuleCondition::SafetyStateAtLeast, SafetyState::Safe as u64),
                Condition::new(RuleCondition::SenderNotButler, 0),
                empty_cond, empty_cond,
            ],
            action: RuleAction::AllowIfCapValid,
        },
    ]
}

/// Compute SHA3-256 hash of the genesis rules for integrity verification.
///
/// Hash input: for each rule, serialize (name_bytes, priority, condition_count,
/// conditions[kind,value], action) into a deterministic byte stream.
pub fn compute_constitution_hash(rules: &[PolicyRule]) -> [u8; 32] {
    let mut hasher = Sha3_256::new();

    for rule in rules {
        // Hash name as bytes
        hasher.update(rule.name.as_bytes());
        // Hash priority
        hasher.update(&rule.priority.to_le_bytes());
        // Hash condition count
        hasher.update(&[rule.condition_count]);
        // Hash each active condition
        for i in 0..rule.condition_count as usize {
            hasher.update(&[rule.conditions[i].kind as u8]);
            hasher.update(&rule.conditions[i].value.to_le_bytes());
        }
        // Hash action
        hasher.update(&[rule.action as u8]);
    }

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Amendment tracker — enforces cooling period and state requirements.
pub struct AmendmentTracker {
    /// Tick at which the last amendment was proposed.
    last_proposal_tick: u64,
    /// Number of amendments applied.
    total_amendments: u32,
    /// Whether an amendment is currently in cooling.
    cooling: bool,
}

impl AmendmentTracker {
    pub const fn new() -> Self {
        Self {
            last_proposal_tick: 0,
            total_amendments: 0,
            cooling: false,
        }
    }

    /// Propose an amendment. Returns true if the proposal is accepted (cooling starts).
    /// Rejected if: not in Normal safety state, or cooling period active.
    pub fn propose(&mut self, safety_state: SafetyState, current_tick: u64) -> bool {
        if safety_state != SafetyState::Normal {
            return false;
        }
        if self.cooling {
            let elapsed = current_tick.saturating_sub(self.last_proposal_tick);
            if elapsed < AMENDMENT_COOLING_TICKS {
                return false;
            }
        }
        self.last_proposal_tick = current_tick;
        self.cooling = true;
        true
    }

    /// Apply the amendment after cooling period. Returns true if ready.
    pub fn apply(&mut self, current_tick: u64) -> bool {
        if !self.cooling {
            return false;
        }
        let elapsed = current_tick.saturating_sub(self.last_proposal_tick);
        if elapsed < AMENDMENT_COOLING_TICKS {
            return false;
        }
        self.total_amendments += 1;
        self.cooling = false;
        true
    }

    /// Is an amendment currently cooling?
    pub fn is_cooling(&self) -> bool {
        self.cooling
    }

    /// Total amendments applied.
    pub fn total_amendments(&self) -> u32 {
        self.total_amendments
    }

    /// Reset (for testing).
    pub fn reset(&mut self) {
        self.last_proposal_tick = 0;
        self.total_amendments = 0;
        self.cooling = false;
    }
}
