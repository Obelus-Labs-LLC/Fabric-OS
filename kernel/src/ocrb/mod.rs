#![allow(dead_code)]

pub mod memory_stress;

use alloc::{string::String, vec::Vec};
use crate::serial_println;

pub struct OcrbResult {
    pub test_name: &'static str,
    pub passed: bool,
    pub score: u8,
    pub weight: u8,
    pub details: String,
}

/// Run all Phase 0 OCRB tests and print results
pub fn run_phase0_gate() {
    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB]   Phase 0 — Memory Stress Gate");
    serial_println!("[OCRB] ============================================");

    let results = memory_stress::run_all_tests();

    let mut weighted_sum: u32 = 0;
    let mut total_weight: u32 = 0;

    for result in &results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        serial_println!(
            "[OCRB] {} [{:>3}/100] (w:{:>2}) — {}",
            status,
            result.score,
            result.weight,
            result.test_name
        );
        if !result.details.is_empty() {
            serial_println!("[OCRB]   {}", result.details);
        }
        weighted_sum += result.score as u32 * result.weight as u32;
        total_weight += result.weight as u32;
    }

    let ori = if total_weight > 0 {
        weighted_sum / total_weight
    } else {
        0
    };

    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB] ORI Score: {}/100", ori);

    if ori >= 80 {
        serial_println!("[OCRB] GATE: PASS — Phase 0 memory subsystem verified");
    } else {
        serial_println!("[OCRB] GATE: FAIL — ORI below 80 threshold");
    }

    serial_println!("[OCRB] ============================================");
}
