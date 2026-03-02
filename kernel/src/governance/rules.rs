//! Deterministic rule engine — flat pattern matching, no recursion.
//!
//! Each PolicyRule has up to 4 AND-joined Conditions. Rules are evaluated
//! in priority-descending order; first match wins (firewall model).

#![allow(dead_code)]

use fabric_types::governance::{
    RuleCondition, RuleAction, SafetyState, AcsState, PolicyVerdict,
};

/// Maximum conditions per rule (AND-joined).
pub const MAX_CONDITIONS: usize = 4;

/// Maximum rules in the engine.
pub const MAX_RULES: usize = 64;

/// A single condition within a rule.
#[derive(Clone, Copy, Debug)]
pub struct Condition {
    pub kind: RuleCondition,
    pub value: u64,
}

impl Condition {
    pub const fn new(kind: RuleCondition, value: u64) -> Self {
        Self { kind, value }
    }

    /// Evaluate this condition against the given context.
    pub fn evaluate(&self, ctx: &EvalContext) -> bool {
        match self.kind {
            RuleCondition::SenderEquals => ctx.sender_pid == self.value as u32,
            RuleCondition::ReceiverEquals => ctx.receiver_pid == self.value as u32,
            RuleCondition::MsgTypeEquals => ctx.msg_type == self.value as u16,
            RuleCondition::ResourceKindEquals => ctx.resource_kind == self.value as u16,
            RuleCondition::PriorityBelow => ctx.sender_priority < self.value as u8,
            RuleCondition::Always => true,
            RuleCondition::SafetyStateAtLeast => {
                (ctx.safety_state as u8) >= (self.value as u8)
            }
            RuleCondition::AcsStateEquals => {
                (ctx.acs_state as u8) == (self.value as u8)
            }
            RuleCondition::SenderNotButler => ctx.sender_pid != 1,
        }
    }
}

/// A single policy rule with up to MAX_CONDITIONS AND-joined conditions.
#[derive(Clone, Copy)]
pub struct PolicyRule {
    /// Rule name (for logging/debug).
    pub name: &'static str,
    /// Priority — higher values are evaluated first.
    pub priority: u16,
    /// Number of active conditions (0..=MAX_CONDITIONS).
    pub condition_count: u8,
    /// AND-joined conditions.
    pub conditions: [Condition; MAX_CONDITIONS],
    /// Action to take if all conditions match.
    pub action: RuleAction,
}

impl PolicyRule {
    pub const fn empty() -> Self {
        Self {
            name: "",
            priority: 0,
            condition_count: 0,
            conditions: [Condition { kind: RuleCondition::Always, value: 0 }; MAX_CONDITIONS],
            action: RuleAction::Allow,
        }
    }

    /// Check if all conditions match the given context.
    pub fn matches(&self, ctx: &EvalContext) -> bool {
        for i in 0..self.condition_count as usize {
            if !self.conditions[i].evaluate(ctx) {
                return false;
            }
        }
        true
    }

    /// Convert the rule's action to a PolicyVerdict.
    pub fn verdict(&self) -> PolicyVerdict {
        match self.action {
            RuleAction::Allow => PolicyVerdict::Allow,
            RuleAction::AllowIfCapValid => PolicyVerdict::Allow, // cap checked later in pipeline
            RuleAction::Deny => PolicyVerdict::Deny,
            RuleAction::DenyAndLog => PolicyVerdict::Deny,
            RuleAction::EscalateToChaos => PolicyVerdict::Escalate(SafetyState::Chaos),
        }
    }
}

impl core::fmt::Debug for PolicyRule {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PolicyRule")
            .field("name", &self.name)
            .field("priority", &self.priority)
            .field("action", &self.action)
            .finish()
    }
}

/// Context passed to the rule engine for evaluation.
pub struct EvalContext {
    pub sender_pid: u32,
    pub receiver_pid: u32,
    pub msg_type: u16,
    pub capability_id: u64,
    pub resource_kind: u16,
    pub sender_priority: u8,
    pub safety_state: SafetyState,
    pub acs_state: AcsState,
}

/// The rule engine — evaluates rules in priority order.
pub struct RuleEngine {
    rules: [PolicyRule; MAX_RULES],
    count: usize,
}

impl RuleEngine {
    pub const fn new() -> Self {
        Self {
            rules: [PolicyRule::empty(); MAX_RULES],
            count: 0,
        }
    }

    /// Load rules (sorted by priority descending internally).
    pub fn load_rules(&mut self, rules: &[PolicyRule]) {
        let n = rules.len().min(MAX_RULES);
        for i in 0..n {
            self.rules[i] = rules[i];
        }
        self.count = n;
        // Sort by priority descending (insertion sort — small N)
        for i in 1..self.count {
            let mut j = i;
            while j > 0 && self.rules[j].priority > self.rules[j - 1].priority {
                self.rules.swap(j, j - 1);
                j -= 1;
            }
        }
    }

    /// Evaluate all rules against the context. First match wins.
    /// Returns (verdict, matched_rule_name, should_audit_log).
    pub fn evaluate(&self, ctx: &EvalContext) -> (PolicyVerdict, &'static str, bool) {
        for i in 0..self.count {
            let rule = &self.rules[i];
            if rule.matches(ctx) {
                let should_log = rule.action == RuleAction::DenyAndLog;
                return (rule.verdict(), rule.name, should_log);
            }
        }
        // Default: allow (no rule matched)
        (PolicyVerdict::Allow, "default-allow", false)
    }

    /// Get the number of loaded rules.
    pub fn rule_count(&self) -> usize {
        self.count
    }

    /// Get a rule by index (for testing/introspection).
    pub fn get_rule(&self, index: usize) -> Option<&PolicyRule> {
        if index < self.count {
            Some(&self.rules[index])
        } else {
            None
        }
    }

    /// Replace a rule by name (for amendment testing). Returns true if found.
    pub fn replace_rule(&mut self, name: &str, new_rule: PolicyRule) -> bool {
        for i in 0..self.count {
            if self.rules[i].name == name {
                self.rules[i] = new_rule;
                // Re-sort after replacement
                for j in 1..self.count {
                    let mut k = j;
                    while k > 0 && self.rules[k].priority > self.rules[k - 1].priority {
                        self.rules.swap(k, k - 1);
                        k -= 1;
                    }
                }
                return true;
            }
        }
        false
    }
}
