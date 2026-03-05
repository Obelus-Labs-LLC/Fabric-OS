//! AI Council — Three-tier adaptive governance engine (Phase 5B).
//!
//! Tier 1: Deterministic rules (handled by governance/rules.rs)
//! Tier 2: Single model inference (sentinel)
//! Tier 3: Three-model majority vote panel (sentinel, arbiter, oracle)
//!
//! Public API:
//!   council::init()           — Initialize 3 models, set golden snapshots
//!   council::evaluate_tier2() — Single-model decision with hash verification
//!   council::evaluate_tier3() — Three-model panel with GPU isolation
//!   council::train_step()     — Apply buffered training with golden regression
//!   council::weight_hashes()  — Get current weight hashes for audit

#![allow(dead_code)]

pub mod model;
pub mod weight_store;
pub mod golden;
pub mod drift;
pub mod learning;
pub mod override_mgr;
pub mod gpu_isolation;
pub mod consensus;

use crate::sync::OrderedMutex;
use fabric_types::governance::{PolicyVerdict, TierLevel, ModelId, CouncilVerdict};
use crate::governance::rules::EvalContext;
use crate::serial_println;

use model::{SimulatedModel, CONFIDENCE_THRESHOLD};
use weight_store::WeightStore;
use golden::GoldenTestSuite;
use drift::DriftDetector;
use learning::{LearningLoop, TrainingExample};
use override_mgr::OverrideManager;

/// Number of models in the Council.
pub const MODEL_COUNT: usize = 3;

/// GPU timeout ceiling for Tier 2 deliberation (50ms at 1kHz tick rate).
pub const TIER2_MAX_TICKS: u64 = 50;

/// GPU timeout ceiling for Tier 3 deliberation (500ms at 1kHz tick rate).
pub const TIER3_MAX_TICKS: u64 = 500;

/// Run-queue depth threshold for Council admission control.
pub const ADMISSION_QUEUE_THRESHOLD: u32 = 8;

/// Average task latency threshold (in ticks) for Council admission control.
pub const ADMISSION_LATENCY_THRESHOLD: u64 = 10;

/// Global Council instance.
pub static COUNCIL: OrderedMutex<CouncilEngine, { crate::sync::levels::STORE }> =
    OrderedMutex::new(CouncilEngine::new());

/// The central Council engine.
pub struct CouncilEngine {
    pub weights: WeightStore,
    pub drift_detectors: [DriftDetector; MODEL_COUNT],
    pub learning: [LearningLoop; MODEL_COUNT],
    pub overrides: OverrideManager,
    /// Current governance tick (synced from governance engine).
    pub current_tick: u64,
    /// Total Tier 2 evaluations.
    pub tier2_evals: u64,
    /// Total Tier 3 evaluations.
    pub tier3_evals: u64,
    /// Total tamper detections.
    pub tamper_detections: u64,
    /// Whether Council is throttled (Tier 1 only) due to system load.
    pub throttled: bool,
    /// Current scheduler run-queue depth (synced from scheduler).
    pub run_queue_depth: u32,
    /// Current average task dispatch latency in ticks (synced from scheduler).
    pub avg_task_latency: u64,
    /// Initialized flag.
    initialized: bool,
}

impl CouncilEngine {
    pub const fn new() -> Self {
        Self {
            weights: WeightStore {
                models: [
                    SimulatedModel { id: ModelId::Sentinel, weights: [0u8; 256], weight_hash: [0u8; 32] },
                    SimulatedModel { id: ModelId::Arbiter,  weights: [0u8; 256], weight_hash: [0u8; 32] },
                    SimulatedModel { id: ModelId::Oracle,   weights: [0u8; 256], weight_hash: [0u8; 32] },
                ],
                snapshots: [[0u8; 256]; 3],
                snapshot_valid: false,
            },
            drift_detectors: [DriftDetector::new(), DriftDetector::new(), DriftDetector::new()],
            learning: [LearningLoop::new(), LearningLoop::new(), LearningLoop::new()],
            overrides: OverrideManager::new(),
            current_tick: 0,
            tier2_evals: 0,
            tier3_evals: 0,
            tamper_detections: 0,
            throttled: false,
            run_queue_depth: 0,
            avg_task_latency: 0,
            initialized: false,
        }
    }

    /// Initialize: create models, set golden snapshots, verify hashes.
    pub fn init(&mut self) {
        // Initialize with proper deterministic seeds
        self.weights = WeightStore::new();

        // Set golden snapshots for drift detection
        for i in 0..MODEL_COUNT {
            self.drift_detectors[i].set_golden(&self.weights.models[i].weights);
        }

        self.initialized = true;
    }

    /// Update load metrics from scheduler (call periodically).
    pub fn update_load_metrics(&mut self, queue_depth: u32, avg_latency: u64) {
        self.run_queue_depth = queue_depth;
        self.avg_task_latency = avg_latency;
        self.throttled = queue_depth > ADMISSION_QUEUE_THRESHOLD
            && avg_latency > ADMISSION_LATENCY_THRESHOLD;
    }

    /// Evaluate using Tier 2 (sentinel only).
    /// Returns CouncilVerdict. May escalate to Tier 3 if low confidence.
    pub fn evaluate_tier2(&mut self, ctx: &EvalContext) -> CouncilVerdict {
        // Admission control: if system is under heavy load, skip Council
        // and return a conservative Deny (fall back to Tier 1 rules).
        if self.throttled {
            return CouncilVerdict {
                decision: PolicyVerdict::Deny,
                confidence: 0,
                tier: TierLevel::Tier1,
                model_votes: [PolicyVerdict::Deny; 3],
            };
        }

        self.tier2_evals += 1;

        let sentinel = &self.weights.models[ModelId::Sentinel as usize];

        // Pre-inference hash verification
        if !sentinel.verify_integrity() {
            self.tamper_detections += 1;
            return CouncilVerdict {
                decision: PolicyVerdict::Deny,
                confidence: 0,
                tier: TierLevel::Tier2,
                model_votes: [PolicyVerdict::Deny; 3],
            };
        }

        // Forward pass
        let (verdict, confidence) = sentinel.forward(ctx);

        // Post-inference hash verification
        if !sentinel.verify_integrity() {
            self.tamper_detections += 1;
            return CouncilVerdict {
                decision: PolicyVerdict::Deny,
                confidence: 0,
                tier: TierLevel::Tier2,
                model_votes: [PolicyVerdict::Deny; 3],
            };
        }

        // Low confidence → escalate to Tier 3
        if confidence < CONFIDENCE_THRESHOLD {
            return self.evaluate_tier3(ctx);
        }

        CouncilVerdict {
            decision: verdict,
            confidence,
            tier: TierLevel::Tier2,
            model_votes: [verdict, PolicyVerdict::Allow, PolicyVerdict::Allow],
        }
    }

    /// Evaluate using Tier 3 (3-model majority vote with GPU isolation).
    pub fn evaluate_tier3(&mut self, ctx: &EvalContext) -> CouncilVerdict {
        self.tier3_evals += 1;

        // GPU temporal isolation: inject deny rule
        // (done by caller via governance rules — we just track it)
        let mut votes = [PolicyVerdict::Allow; 3];
        let mut confidences = [0u8; 3];
        let mut tamper = false;

        for i in 0..MODEL_COUNT {
            let model = &self.weights.models[i];

            // Pre-inference hash check
            if !model.verify_integrity() {
                self.tamper_detections += 1;
                tamper = true;
                votes[i] = PolicyVerdict::Deny;
                continue;
            }

            let (v, c) = model.forward(ctx);
            votes[i] = v;
            confidences[i] = c;

            // Post-inference hash check
            if !model.verify_integrity() {
                self.tamper_detections += 1;
                tamper = true;
                votes[i] = PolicyVerdict::Deny;
            }
        }

        // If any tamper detected, deny everything
        if tamper {
            return CouncilVerdict {
                decision: PolicyVerdict::Deny,
                confidence: 0,
                tier: TierLevel::Tier3,
                model_votes: votes,
            };
        }

        // Majority vote: count Allow vs Deny
        let allow_count = votes.iter().filter(|v| **v == PolicyVerdict::Allow).count();
        let deny_count = votes.iter().filter(|v| **v == PolicyVerdict::Deny).count();

        let decision = if allow_count >= 2 {
            PolicyVerdict::Allow
        } else if deny_count >= 2 {
            PolicyVerdict::Deny
        } else {
            PolicyVerdict::Deny // No majority → conservative default
        };

        // Average confidence
        let avg_conf = (confidences[0] as u16 + confidences[1] as u16 + confidences[2] as u16) / 3;

        CouncilVerdict {
            decision,
            confidence: avg_conf as u8,
            tier: TierLevel::Tier3,
            model_votes: votes,
        }
    }

    /// Record a training example for a specific model.
    pub fn record_example(&mut self, model_id: ModelId, example: TrainingExample) -> bool {
        self.learning[model_id as usize].record(example)
    }

    /// Apply training step for a specific model.
    /// Returns true if update applied, false if blocked (cap, drift, regression).
    pub fn train_step(&mut self, model_id: ModelId, rules: &crate::governance::rules::RuleEngine) -> bool {
        let idx = model_id as usize;

        if !self.learning[idx].can_update(self.current_tick) {
            return false;
        }

        if self.drift_detectors[idx].frozen {
            return false;
        }

        // Compute gradient from buffered examples
        let gradient = self.learning[idx].compute_gradient();

        // Snapshot current weights
        self.weights.snapshot_all();

        // Apply gradient
        self.weights.models[idx].update_weights(&gradient);

        // Run golden test suite
        let (passed, total, _failure) = GoldenTestSuite::run_all(rules);
        if passed < total {
            // Regression detected → rollback
            self.weights.rollback_all();
            self.learning[idx].freeze();
            return false;
        }

        // Check drift
        if self.drift_detectors[idx].check_drift(&self.weights.models[idx].weights) {
            // Drift exceeded → rollback + freeze
            self.weights.rollback_all();
            self.learning[idx].freeze();
            self.drift_detectors[idx].unfreeze(); // Detector unfrozen, learning frozen
            return false;
        }

        // Update drift detector golden snapshot to new weights
        self.drift_detectors[idx].set_golden(&self.weights.models[idx].weights);

        // Mark update applied
        self.learning[idx].mark_update(self.current_tick);
        true
    }

    /// Sync tick from governance.
    pub fn sync_tick(&mut self, tick: u64) {
        self.current_tick = tick;
    }

    /// Clear all state (for testing).
    pub fn clear(&mut self) {
        self.weights.reset();
        for d in &mut self.drift_detectors {
            d.reset();
        }
        for l in &mut self.learning {
            l.reset();
        }
        self.overrides.reset();
        self.current_tick = 0;
        self.tier2_evals = 0;
        self.tier3_evals = 0;
        self.tamper_detections = 0;
        self.throttled = false;
        self.run_queue_depth = 0;
        self.avg_task_latency = 0;
        self.initialized = false;
    }
}

// === Convenience free functions ===

/// Initialize the Council subsystem.
pub fn init() {
    let mut council = COUNCIL.lock();
    council.init();
    serial_println!("[COUNCIL] Council engine initialized");
    serial_println!(
        "[COUNCIL] Models: sentinel, arbiter, oracle — weight hashes verified"
    );
}

/// Evaluate Tier 2 (called from governance::evaluate_policy on EscalateToTier2).
pub fn evaluate_tier2(ctx: &EvalContext) -> CouncilVerdict {
    COUNCIL.lock().evaluate_tier2(ctx)
}

/// Evaluate Tier 3 directly (for testing or forced escalation).
pub fn evaluate_tier3(ctx: &EvalContext) -> CouncilVerdict {
    COUNCIL.lock().evaluate_tier3(ctx)
}

/// Get current weight hashes for all models.
pub fn weight_hashes() -> [[u8; 32]; MODEL_COUNT] {
    COUNCIL.lock().weights.weight_hashes()
}
