//! GPU temporal isolation — deny GPU resource access during Tier 3 deliberation.
//!
//! During Tier 3 panel voting, a temporary deny rule is injected into the
//! governance rule engine that blocks KIND_GPU access for non-Butler processes.
//! The rule is removed when deliberation completes.

#![allow(dead_code)]

use fabric_types::governance::{RuleCondition, RuleAction};
use crate::governance::rules::{PolicyRule, Condition};

/// Name of the temporary GPU isolation rule.
pub const GPU_ISOLATION_RULE_NAME: &str = "tier3-gpu-isolation";

/// Priority of the GPU isolation rule (higher than most, below Butler).
pub const GPU_ISOLATION_PRIORITY: u16 = 920;

/// Build the temporary GPU isolation deny rule.
pub fn gpu_isolation_rule() -> PolicyRule {
    let empty_cond = Condition::new(RuleCondition::Always, 0);
    PolicyRule {
        name: GPU_ISOLATION_RULE_NAME,
        priority: GPU_ISOLATION_PRIORITY,
        condition_count: 2,
        conditions: [
            Condition::new(RuleCondition::ResourceKindEquals, 8), // KIND_GPU = 8
            Condition::new(RuleCondition::SenderNotButler, 0),
            empty_cond, empty_cond,
        ],
        action: RuleAction::DenyAndLog,
    }
}
