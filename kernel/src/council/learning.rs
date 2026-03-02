//! Learning loop — buffered training with golden regression and drift safety.
//!
//! Collects Council decisions as training examples, applies simulated gradient
//! updates, and rolls back if golden tests regress or drift threshold exceeded.

#![allow(dead_code)]

use fabric_types::governance::PolicyVerdict;
use super::model::WEIGHT_SIZE;

/// Maximum buffered training examples.
pub const MAX_TRAINING_BUFFER: usize = 32;

/// Per-model training cap per period (86.4M ticks ~ 24h).
pub const MAX_UPDATES_PER_PERIOD: u32 = 10;

/// Training period in ticks (matches amendment cooling).
pub const TRAINING_PERIOD_TICKS: u64 = 86_400_000;

/// A single training example.
#[derive(Clone, Copy)]
pub struct TrainingExample {
    /// Serialized context hash (for gradient computation).
    pub ctx_hash: [u8; 32],
    /// The verdict that was rendered.
    pub verdict: PolicyVerdict,
    /// Confidence of the decision.
    pub confidence: u8,
}

impl TrainingExample {
    pub const fn empty() -> Self {
        Self {
            ctx_hash: [0u8; 32],
            verdict: PolicyVerdict::Allow,
            confidence: 0,
        }
    }
}

/// Learning loop state for a single model.
pub struct LearningLoop {
    /// Buffered training examples.
    buffer: [TrainingExample; MAX_TRAINING_BUFFER],
    /// Number of examples in buffer.
    buffer_count: usize,
    /// Total updates applied in current period.
    updates_this_period: u32,
    /// Tick at which current period started.
    period_start_tick: u64,
    /// Whether learning is active.
    pub active: bool,
}

impl LearningLoop {
    pub const fn new() -> Self {
        Self {
            buffer: [TrainingExample::empty(); MAX_TRAINING_BUFFER],
            buffer_count: 0,
            updates_this_period: 0,
            period_start_tick: 0,
            active: true,
        }
    }

    /// Record a training example. Returns false if buffer full.
    pub fn record(&mut self, example: TrainingExample) -> bool {
        if !self.active || self.buffer_count >= MAX_TRAINING_BUFFER {
            return false;
        }
        self.buffer[self.buffer_count] = example;
        self.buffer_count += 1;
        true
    }

    /// Check if training cap allows another update.
    pub fn can_update(&self, current_tick: u64) -> bool {
        if !self.active {
            return false;
        }
        // Check if we're in a new period
        if current_tick.saturating_sub(self.period_start_tick) >= TRAINING_PERIOD_TICKS {
            return true; // New period, counter resets
        }
        self.updates_this_period < MAX_UPDATES_PER_PERIOD
    }

    /// Compute a simulated gradient from buffered examples.
    /// Returns a 256-byte gradient vector (XOR-based perturbation).
    pub fn compute_gradient(&self) -> [u8; WEIGHT_SIZE] {
        let mut gradient = [0u8; WEIGHT_SIZE];
        if self.buffer_count == 0 {
            return gradient;
        }

        // Simulate gradient: XOR all context hashes, scaled by confidence
        for i in 0..self.buffer_count {
            let ex = &self.buffer[i];
            // Spread the 32-byte context hash across the 256-byte gradient
            for j in 0..WEIGHT_SIZE {
                let hash_byte = ex.ctx_hash[j % 32];
                // Low-magnitude perturbation: only flip low bits based on confidence
                let mask = if ex.confidence > 50 { 0x03 } else { 0x01 };
                gradient[j] ^= hash_byte & mask;
            }
        }

        gradient
    }

    /// Mark an update as applied and clear the buffer.
    pub fn mark_update(&mut self, current_tick: u64) {
        // Reset period counter if needed
        if current_tick.saturating_sub(self.period_start_tick) >= TRAINING_PERIOD_TICKS {
            self.updates_this_period = 0;
            self.period_start_tick = current_tick;
        }
        self.updates_this_period += 1;
        self.buffer_count = 0;
    }

    /// Freeze learning (drift or regression detected).
    pub fn freeze(&mut self) {
        self.active = false;
    }

    /// Unfreeze learning (after rollback + verification).
    pub fn unfreeze(&mut self) {
        self.active = true;
    }

    /// Get number of buffered examples.
    pub fn buffer_count(&self) -> usize {
        self.buffer_count
    }

    /// Get updates count this period.
    pub fn updates_this_period(&self) -> u32 {
        self.updates_this_period
    }

    /// Reset all state.
    pub fn reset(&mut self) {
        self.buffer_count = 0;
        self.updates_this_period = 0;
        self.period_start_tick = 0;
        self.active = true;
    }
}
