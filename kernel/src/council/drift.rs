//! Drift detector — cosine similarity between current and golden weight snapshots.
//!
//! Monitors model weight drift after training updates. If similarity drops below
//! threshold, freezes learning and triggers rollback.

#![allow(dead_code)]

use super::model::WEIGHT_SIZE;

/// Cosine similarity threshold (scaled x1000). Below this = drift alert.
pub const DRIFT_THRESHOLD: u32 = 800;

/// Drift detector for a single model.
pub struct DriftDetector {
    /// Golden weight snapshot (reference point).
    golden_snapshot: [u8; WEIGHT_SIZE],
    /// Whether golden snapshot is valid.
    initialized: bool,
    /// Whether learning is frozen due to drift.
    pub frozen: bool,
    /// Last computed similarity (scaled x1000).
    pub last_similarity: u32,
}

impl DriftDetector {
    pub const fn new() -> Self {
        Self {
            golden_snapshot: [0u8; WEIGHT_SIZE],
            initialized: false,
            frozen: false,
            last_similarity: 1000, // Perfect similarity initially
        }
    }

    /// Set the golden snapshot (typically at init or after verified-good state).
    pub fn set_golden(&mut self, weights: &[u8; WEIGHT_SIZE]) {
        self.golden_snapshot = *weights;
        self.initialized = true;
        self.last_similarity = 1000;
    }

    /// Compute cosine similarity between current weights and golden snapshot.
    /// Returns value scaled x1000 (1000 = identical, 0 = orthogonal).
    ///
    /// Uses integer arithmetic: dot(a,b) / (|a| * |b|) * 1000
    pub fn cosine_similarity(&self, current: &[u8; WEIGHT_SIZE]) -> u32 {
        if !self.initialized {
            return 1000; // No reference = assume perfect
        }

        let mut dot: u64 = 0;
        let mut mag_a: u64 = 0;
        let mut mag_b: u64 = 0;

        for i in 0..WEIGHT_SIZE {
            let a = self.golden_snapshot[i] as u64;
            let b = current[i] as u64;
            dot += a * b;
            mag_a += a * a;
            mag_b += b * b;
        }

        if mag_a == 0 || mag_b == 0 {
            return 0;
        }

        // Integer square root approximation for magnitude
        let mag_product = isqrt(mag_a) * isqrt(mag_b);
        if mag_product == 0 {
            return 0;
        }

        // Scale by 1000 for precision
        let similarity = (dot * 1000) / mag_product;

        // Clamp to 1000 max
        if similarity > 1000 { 1000 } else { similarity as u32 }
    }

    /// Check drift of current weights against golden snapshot.
    /// Returns true if drift exceeds threshold (learning should freeze).
    pub fn check_drift(&mut self, current: &[u8; WEIGHT_SIZE]) -> bool {
        self.last_similarity = self.cosine_similarity(current);
        if self.last_similarity < DRIFT_THRESHOLD {
            self.frozen = true;
            true // Drift detected
        } else {
            false
        }
    }

    /// Unfreeze learning (after rollback).
    pub fn unfreeze(&mut self) {
        self.frozen = false;
    }

    /// Reset detector state.
    pub fn reset(&mut self) {
        self.initialized = false;
        self.frozen = false;
        self.last_similarity = 1000;
    }
}

/// Integer square root (Newton's method).
fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}
