//! Governance types — shared between kernel and userspace.
//!
//! Defines safety states, ACS lifecycle, rule conditions/actions,
//! policy verdicts, and Council types for the governance engine.

#![allow(dead_code)]

/// Safety state machine states (severity-ordered).
///
/// Numeric values encode severity: Normal(0) < Elevated(1) < Safe(2) < Chaos(3) < Lockdown(4).
/// `SafetyStateAtLeast` comparisons use `>=` on the discriminant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum SafetyState {
    Normal   = 0,
    Elevated = 1,
    Safe     = 2,
    Chaos    = 3,
    Lockdown = 4,
}

/// Authority Continuity System (ACS) lifecycle states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AcsState {
    Active      = 0,
    Degraded    = 1,
    Contingency = 2,
    Emergency   = 3,
}

/// Condition types for policy rule matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RuleCondition {
    /// Match if sender PID == value
    SenderEquals       = 0,
    /// Match if receiver PID == value
    ReceiverEquals     = 1,
    /// Match if message TypeId == value (u16 in low bits)
    MsgTypeEquals      = 2,
    /// Match if resource kind (upper 16 bits of ResourceId) == value (u16 in low bits)
    ResourceKindEquals = 3,
    /// Match if sender's effective_priority < value
    PriorityBelow      = 4,
    /// Always matches (value ignored)
    Always             = 5,
    /// Match if current SafetyState >= value (as u8 discriminant)
    SafetyStateAtLeast = 6,
    /// Match if current AcsState == value (as u8 discriminant)
    AcsStateEquals     = 7,
    /// Match if sender PID != 1 (Butler). Value ignored.
    SenderNotButler    = 8,
    /// Match if message was escalated from Tier 2/3 Council.
    TierEscalated      = 9,
}

/// Actions taken when a rule matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RuleAction {
    /// Allow the message unconditionally.
    Allow           = 0,
    /// Deny silently (no audit).
    Deny            = 1,
    /// Deny and log a PolicyViolation audit entry.
    DenyAndLog      = 2,
    /// Allow only if the capability is valid (checked later in pipeline).
    AllowIfCapValid = 3,
    /// Escalate safety state to Chaos, then re-evaluate.
    EscalateToChaos = 4,
    /// Escalate to Council Tier 2 for AI-assisted decision.
    EscalateToTier2 = 5,
}

/// Result of policy evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyVerdict {
    /// Message is allowed to proceed through the bus pipeline.
    Allow,
    /// Message is denied.
    Deny,
    /// Safety state should be escalated, then re-evaluate.
    Escalate(SafetyState),
}

// === Council (Phase 5B) types ===

/// Council decision tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TierLevel {
    Tier1 = 1,
    Tier2 = 2,
    Tier3 = 3,
}

/// Model identity within the Council.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ModelId {
    /// Security-focused model.
    Sentinel = 0,
    /// Fairness-focused model.
    Arbiter  = 1,
    /// Resource optimization model.
    Oracle   = 2,
}

/// Council verdict with confidence and voting details.
#[derive(Clone, Copy, Debug)]
pub struct CouncilVerdict {
    /// Final decision.
    pub decision: PolicyVerdict,
    /// Confidence level (0-100).
    pub confidence: u8,
    /// Which tier produced this verdict.
    pub tier: TierLevel,
    /// Per-model votes (Tier 3 only; default Allow for unused slots).
    pub model_votes: [PolicyVerdict; 3],
}
