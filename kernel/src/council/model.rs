//! Simulated AI model — deterministic state machine with weight hash verification.
//!
//! Each model has a 256-byte weight vector, SHA3-256 hashed for integrity.
//! forward() produces deterministic verdicts from hash(weights ++ context_bytes).
//! Protocol-faithful: real ONNX models drop in later with same interface.

#![allow(dead_code)]

use sha3::{Sha3_256, Digest};
use fabric_types::governance::{PolicyVerdict, ModelId};
use crate::governance::rules::EvalContext;

/// Size of simulated weight vector.
pub const WEIGHT_SIZE: usize = 256;

/// Confidence threshold for Tier 2 → Tier 3 escalation.
pub const CONFIDENCE_THRESHOLD: u8 = 40;

/// A simulated AI model with deterministic inference.
pub struct SimulatedModel {
    /// Model identity.
    pub id: ModelId,
    /// Simulated weight vector.
    pub weights: [u8; WEIGHT_SIZE],
    /// SHA3-256 hash of current weights (computed at init and after updates).
    pub weight_hash: [u8; 32],
}

impl SimulatedModel {
    /// Create a new model with deterministic seed based on ModelId.
    pub fn new(id: ModelId) -> Self {
        let mut weights = [0u8; WEIGHT_SIZE];
        // Deterministic seed: each model gets a unique but reproducible weight vector
        let seed = match id {
            ModelId::Sentinel => 0x5E,  // Security-focused
            ModelId::Arbiter  => 0xA7,  // Fairness-focused
            ModelId::Oracle   => 0x0C,  // Resource optimization
        };
        for i in 0..WEIGHT_SIZE {
            // Simple deterministic initialization: seed XOR position with mixing
            weights[i] = seed ^ (i as u8).wrapping_mul(0x9D).wrapping_add(0x37);
        }
        let weight_hash = Self::compute_hash(&weights);
        Self { id, weights, weight_hash }
    }

    /// Compute SHA3-256 hash of a weight vector.
    pub fn compute_hash(weights: &[u8; WEIGHT_SIZE]) -> [u8; 32] {
        let mut hasher = Sha3_256::new();
        hasher.update(weights);
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Verify weight integrity. Returns true if hash matches.
    pub fn verify_integrity(&self) -> bool {
        let current = Self::compute_hash(&self.weights);
        current == self.weight_hash
    }

    /// Deterministic forward pass: hash(weights ++ context_bytes) → (verdict, confidence).
    ///
    /// Bits 0-6 of first output byte → confidence (0-100, clamped)
    /// Bit 7 of first output byte → Allow(1) / Deny(0)
    pub fn forward(&self, ctx: &EvalContext) -> (PolicyVerdict, u8) {
        let mut hasher = Sha3_256::new();
        hasher.update(&self.weights);
        // Serialize context into deterministic byte stream
        hasher.update(&ctx.sender_pid.to_le_bytes());
        hasher.update(&ctx.receiver_pid.to_le_bytes());
        hasher.update(&ctx.msg_type.to_le_bytes());
        hasher.update(&ctx.capability_id.to_le_bytes());
        hasher.update(&ctx.resource_kind.to_le_bytes());
        hasher.update(&[ctx.sender_priority]);
        hasher.update(&[ctx.safety_state as u8]);
        hasher.update(&[ctx.acs_state as u8]);

        let result = hasher.finalize();
        let raw = result[0];

        // Confidence: bits 0-6, scaled to 0-100
        let confidence = (raw & 0x7F) % 101;
        // Verdict: bit 7
        let verdict = if raw & 0x80 != 0 {
            PolicyVerdict::Allow
        } else {
            PolicyVerdict::Deny
        };

        (verdict, confidence)
    }

    /// Simulated gradient update: XOR-based weight perturbation.
    /// Updates weight_hash after modification.
    pub fn update_weights(&mut self, gradient: &[u8; WEIGHT_SIZE]) {
        for i in 0..WEIGHT_SIZE {
            self.weights[i] ^= gradient[i];
        }
        self.weight_hash = Self::compute_hash(&self.weights);
    }

    /// Snapshot current weights (for rollback).
    pub fn snapshot_weights(&self) -> [u8; WEIGHT_SIZE] {
        self.weights
    }

    /// Restore weights from snapshot.
    pub fn restore_weights(&mut self, snapshot: &[u8; WEIGHT_SIZE]) {
        self.weights = *snapshot;
        self.weight_hash = Self::compute_hash(&self.weights);
    }

    /// Reset to initial state (re-seed from ModelId).
    pub fn reset(&mut self) {
        let fresh = Self::new(self.id);
        self.weights = fresh.weights;
        self.weight_hash = fresh.weight_hash;
    }
}
