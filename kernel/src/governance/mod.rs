//! Deterministic Governance Engine — Phase 5A of Fabric OS.
//!
//! Provides policy-gated message routing through a flat rules engine,
//! safety state machine, ACS lifecycle, and constitution integrity verification.
//!
//! Public API:
//!   governance::init()              — Load constitution, verify hash, boot state machines
//!   governance::evaluate_policy()   — Pre-route policy check for bus::send()
//!   governance::tick()              — Advance safety/ACS state machines
//!   governance::safety_state()      — Query current safety state
//!   governance::acs_state()         — Query current ACS state
//!   governance::heartbeat()         — ACS primary heartbeat
//!   governance::verify_constitution() — Verify constitution hash integrity

#![allow(dead_code)]

pub mod rules;
pub mod constitution;
pub mod safety;
pub mod acs;

use spin::Mutex;
use fabric_types::MessageHeader;
use fabric_types::governance::{SafetyState, AcsState, PolicyVerdict};
use crate::bus::BusError;
use crate::serial_println;

use rules::{RuleEngine, EvalContext};
use constitution::{genesis_rules, compute_constitution_hash, AmendmentTracker};
use safety::SafetyStateMachine;
use acs::AcsStateMachine;

/// Global governance engine instance.
pub static GOVERNANCE: Mutex<GovernanceEngine> = Mutex::new(GovernanceEngine::new());

/// The central governance engine.
pub struct GovernanceEngine {
    pub rules: RuleEngine,
    pub safety: SafetyStateMachine,
    pub acs: AcsStateMachine,
    pub amendments: AmendmentTracker,
    /// SHA3-256 hash of the constitution at boot.
    pub constitution_hash: [u8; 32],
    /// Current governance tick.
    current_tick: u64,
    /// Total policy evaluations.
    total_evaluations: u64,
    /// Total denials.
    total_denials: u64,
    /// Initialized flag.
    initialized: bool,
}

impl GovernanceEngine {
    pub const fn new() -> Self {
        Self {
            rules: RuleEngine::new(),
            safety: SafetyStateMachine::new(),
            acs: AcsStateMachine::new(),
            amendments: AmendmentTracker::new(),
            constitution_hash: [0u8; 32],
            current_tick: 0,
            total_evaluations: 0,
            total_denials: 0,
            initialized: false,
        }
    }

    /// Initialize: load genesis rules, compute constitution hash, boot state machines.
    pub fn init(&mut self) {
        let rules = genesis_rules();
        self.rules.load_rules(&rules);
        self.constitution_hash = compute_constitution_hash(&rules);
        self.initialized = true;
    }

    /// Advance governance tick — updates safety + ACS state machines.
    pub fn tick(&mut self) {
        self.current_tick += 1;
        self.safety.tick(self.current_tick);
        self.acs.tick(self.current_tick);

        // If ACS triggered emergency, escalate safety to Lockdown
        if self.acs.take_emergency_trigger() {
            self.safety.force_lockdown(self.current_tick);
        }
    }

    /// Advance governance ticks by N.
    pub fn advance_ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// Get current tick.
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// Get stats: (total_evaluations, total_denials).
    pub fn stats(&self) -> (u64, u64) {
        (self.total_evaluations, self.total_denials)
    }

    /// Verify constitution integrity — recompute hash and compare.
    pub fn verify_constitution(&self) -> bool {
        let rules = genesis_rules();
        let current_hash = compute_constitution_hash(&rules);
        current_hash == self.constitution_hash
    }

    /// Clear all state (for testing between OCRB tests).
    pub fn clear(&mut self) {
        self.safety.reset();
        self.acs.reset();
        self.amendments.reset();
        self.current_tick = 0;
        self.total_evaluations = 0;
        self.total_denials = 0;
        // Re-load genesis rules (don't clear them)
        let rules = genesis_rules();
        self.rules.load_rules(&rules);
        self.constitution_hash = compute_constitution_hash(&rules);
    }
}

// === Convenience free functions (lock ordering: GOVERNANCE < TABLE < STORE < BUS) ===

/// Initialize the governance subsystem. Must be called after heap init.
pub fn init() {
    GOVERNANCE.lock().init();
    let gov = GOVERNANCE.lock();
    let rule_count = gov.rules.rule_count();
    let hash = &gov.constitution_hash;
    serial_println!("[GOV] Governance engine initialized");
    serial_println!(
        "[GOV] Constitution: {} rules, hash: {:02x}{:02x}{:02x}{:02x}...",
        rule_count,
        hash[0], hash[1], hash[2], hash[3]
    );
}

/// Evaluate policy for a message header. Called BEFORE bus::send() locks BUS.
///
/// Lock ordering: acquires GOVERNANCE, then briefly TABLE, then briefly STORE.
/// All released before returning, so caller can safely lock BUS afterward.
pub fn evaluate_policy(header: &MessageHeader) -> Result<(), BusError> {
    let mut gov = GOVERNANCE.lock();

    if !gov.initialized {
        return Ok(()); // Governance not yet initialized — allow all
    }

    // Build eval context while holding GOVERNANCE lock.
    // Lock TABLE briefly to get sender's effective_priority.
    let sender_priority = {
        let table = crate::process::TABLE.lock();
        table.get(header.sender)
            .map(|pcb| pcb.effective_priority)
            .unwrap_or(0)
    }; // TABLE lock dropped

    // Lock STORE briefly to get resource kind from capability.
    let resource_kind = {
        let store = crate::capability::STORE.lock();
        store.get_token_info(header.capability_id)
            .map(|(_, res)| res.kind())
            .unwrap_or(0)
    }; // STORE lock dropped

    let ctx = EvalContext {
        sender_pid: header.sender.0,
        receiver_pid: header.receiver.0,
        msg_type: header.msg_type.0,
        capability_id: header.capability_id,
        resource_kind,
        sender_priority,
        safety_state: gov.safety.state(),
        acs_state: gov.acs.state(),
    };

    let (verdict, _rule_name, _should_log) = gov.rules.evaluate(&ctx);
    gov.total_evaluations += 1;

    match verdict {
        PolicyVerdict::Allow => Ok(()),
        PolicyVerdict::Deny => {
            gov.total_denials += 1;
            Err(BusError::PolicyDenied)
        }
        PolicyVerdict::Escalate(target_state) => {
            // Escalate safety state, then re-evaluate
            let tick = gov.current_tick;
            gov.safety.force_state(target_state, tick);
            let ctx2 = EvalContext {
                sender_pid: ctx.sender_pid,
                receiver_pid: ctx.receiver_pid,
                msg_type: ctx.msg_type,
                capability_id: ctx.capability_id,
                resource_kind: ctx.resource_kind,
                sender_priority: ctx.sender_priority,
                safety_state: gov.safety.state(),
                acs_state: ctx.acs_state,
            };
            let (verdict2, _, _) = gov.rules.evaluate(&ctx2);
            match verdict2 {
                PolicyVerdict::Allow => Ok(()),
                _ => {
                    gov.total_denials += 1;
                    Err(BusError::PolicyDenied)
                }
            }
        }
    }
}

/// Advance governance tick (call from main tick loop).
pub fn tick() {
    GOVERNANCE.lock().tick();
}

/// Query current safety state.
pub fn safety_state() -> SafetyState {
    GOVERNANCE.lock().safety.state()
}

/// Query current ACS state.
pub fn acs_state() -> AcsState {
    GOVERNANCE.lock().acs.state()
}

/// ACS primary heartbeat.
pub fn heartbeat() {
    let mut gov = GOVERNANCE.lock();
    let tick = gov.current_tick;
    gov.acs.heartbeat(tick);
}

/// Verify constitution integrity.
pub fn verify_constitution() -> bool {
    GOVERNANCE.lock().verify_constitution()
}
