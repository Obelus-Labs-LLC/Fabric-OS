//! Weight store — manages 3 Council models with snapshot/rollback.
//!
//! Provides weight hash verification before and after inference,
//! plus snapshot/rollback for safe training updates.

#![allow(dead_code)]

use fabric_types::governance::ModelId;
use super::model::{SimulatedModel, WEIGHT_SIZE};

/// Number of Council models.
pub const MODEL_COUNT: usize = 3;

/// Weight store holding all Council models.
pub struct WeightStore {
    pub models: [SimulatedModel; MODEL_COUNT],
    /// Snapshots for rollback (one per model).
    pub(crate) snapshots: [[u8; WEIGHT_SIZE]; MODEL_COUNT],
    /// Whether snapshots are valid.
    pub(crate) snapshot_valid: bool,
}

impl WeightStore {
    /// Create with all 3 models initialized from deterministic seeds.
    pub fn new() -> Self {
        Self {
            models: [
                SimulatedModel::new(ModelId::Sentinel),
                SimulatedModel::new(ModelId::Arbiter),
                SimulatedModel::new(ModelId::Oracle),
            ],
            snapshots: [[0u8; WEIGHT_SIZE]; MODEL_COUNT],
            snapshot_valid: false,
        }
    }

    /// Get a model by ModelId.
    pub fn get(&self, id: ModelId) -> &SimulatedModel {
        &self.models[id as usize]
    }

    /// Get a mutable model by ModelId.
    pub fn get_mut(&mut self, id: ModelId) -> &mut SimulatedModel {
        &mut self.models[id as usize]
    }

    /// Verify integrity of all models. Returns false if any hash mismatch.
    pub fn verify_all(&self) -> bool {
        self.models.iter().all(|m| m.verify_integrity())
    }

    /// Verify integrity of a specific model.
    pub fn verify(&self, id: ModelId) -> bool {
        self.models[id as usize].verify_integrity()
    }

    /// Snapshot all model weights (for pre-training rollback).
    pub fn snapshot_all(&mut self) {
        for i in 0..MODEL_COUNT {
            self.snapshots[i] = self.models[i].snapshot_weights();
        }
        self.snapshot_valid = true;
    }

    /// Rollback all models to last snapshot.
    pub fn rollback_all(&mut self) -> bool {
        if !self.snapshot_valid {
            return false;
        }
        for i in 0..MODEL_COUNT {
            self.models[i].restore_weights(&self.snapshots[i]);
        }
        true
    }

    /// Get weight hashes for all models (for audit/verification).
    pub fn weight_hashes(&self) -> [[u8; 32]; MODEL_COUNT] {
        [
            self.models[0].weight_hash,
            self.models[1].weight_hash,
            self.models[2].weight_hash,
        ]
    }

    /// Reset all models to initial state.
    pub fn reset(&mut self) {
        for model in &mut self.models {
            model.reset();
        }
        self.snapshot_valid = false;
    }
}
