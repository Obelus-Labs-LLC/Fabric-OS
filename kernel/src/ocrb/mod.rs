#![allow(dead_code)]

pub mod memory_stress;
pub mod capability_storm;
pub mod bus_byzantine;
pub mod process_storm;
pub mod driver_isolation;
pub mod governance_gate;
pub mod council_gate;
pub mod isolation_gate;
pub mod hardware_gate;
pub mod vfs_gate;

use alloc::string::String;
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

/// Run all Phase 1 OCRB tests and print results
pub fn run_phase1_gate() {
    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB]   Phase 1 — Capability Storm Gate");
    serial_println!("[OCRB] ============================================");

    let results = capability_storm::run_all_tests();

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
        serial_println!("[OCRB] GATE: PASS — Phase 1 capability engine verified");
    } else {
        serial_println!("[OCRB] GATE: FAIL — ORI below 80 threshold");
    }

    serial_println!("[OCRB] ============================================");
}

/// Run all Phase 2 OCRB tests and print results
pub fn run_phase2_gate() {
    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB]   Phase 2 — Bus Byzantine + Flood Gate");
    serial_println!("[OCRB] ============================================");

    let results = bus_byzantine::run_all_tests();

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
        serial_println!("[OCRB] GATE: PASS — Phase 2 message bus verified");
    } else {
        serial_println!("[OCRB] GATE: FAIL — ORI below 80 threshold");
    }

    serial_println!("[OCRB] ============================================");
}

/// Run all Phase 3 OCRB tests and print results
pub fn run_phase3_gate() {
    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB]   Phase 3 — Process + Scheduler Storm Gate");
    serial_println!("[OCRB] ============================================");

    let results = process_storm::run_all_tests();

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
        serial_println!("[OCRB] GATE: PASS — Phase 3 process model verified");
    } else {
        serial_println!("[OCRB] GATE: FAIL — ORI below 80 threshold");
    }

    serial_println!("[OCRB] ============================================");
}

/// Run all Phase 4 OCRB tests and print results
pub fn run_phase4_gate() {
    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB]   Phase 4 — Driver Isolation Gate");
    serial_println!("[OCRB] ============================================");

    let results = driver_isolation::run_all_tests();

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
        serial_println!("[OCRB] GATE: PASS — Phase 4 driver isolation verified");
    } else {
        serial_println!("[OCRB] GATE: FAIL — ORI below 80 threshold");
    }

    serial_println!("[OCRB] ============================================");
}

/// Run all Phase 5A OCRB tests and print results
pub fn run_phase5a_gate() {
    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB]   Phase 5A — Governance Gate");
    serial_println!("[OCRB] ============================================");

    let results = governance_gate::run_all_tests();

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
        serial_println!("[OCRB] GATE: PASS — Phase 5A governance verified");
    } else {
        serial_println!("[OCRB] GATE: FAIL — ORI below 80 threshold");
    }

    serial_println!("[OCRB] ============================================");
}

/// Run all Phase 5B OCRB tests and print results
pub fn run_phase5b_gate() {
    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB]   Phase 5B — Council Gate");
    serial_println!("[OCRB] ============================================");

    let results = council_gate::run_all_tests();

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
        serial_println!("[OCRB] GATE: PASS — Phase 5B council verified");
    } else {
        serial_println!("[OCRB] GATE: FAIL — ORI below 80 threshold");
    }

    serial_println!("[OCRB] ============================================");
}

/// Run all Phase 6 OCRB tests and print results
pub fn run_phase6_gate() {
    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB]   Phase 6 — Isolation Gate");
    serial_println!("[OCRB] ============================================");

    let results = isolation_gate::run_all_tests();

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
        serial_println!("[OCRB] GATE: PASS — Phase 6 isolation verified");
    } else {
        serial_println!("[OCRB] GATE: FAIL — ORI below 80 threshold");
    }

    serial_println!("[OCRB] ============================================");
}

/// Run all Phase 7 OCRB tests and print results
pub fn run_phase7_gate() {
    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB]   Phase 7 — Hardware + Userspace Gate");
    serial_println!("[OCRB] ============================================");

    let results = hardware_gate::run_all_tests();

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
        serial_println!("[OCRB] GATE: PASS — Phase 7 hardware + userspace verified");
    } else {
        serial_println!("[OCRB] GATE: FAIL — ORI below 80 threshold");
    }

    serial_println!("[OCRB] ============================================");
}

/// Run all Phase 8 OCRB tests and print results
pub fn run_phase8_gate() {
    serial_println!("[OCRB] ============================================");
    serial_println!("[OCRB]   Phase 8 — VFS + Filesystem Gate");
    serial_println!("[OCRB] ============================================");

    let results = vfs_gate::run_all_tests();

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
        serial_println!("[OCRB] GATE: PASS — Phase 8 VFS + filesystem verified");
    } else {
        serial_println!("[OCRB] GATE: FAIL — ORI below 80 threshold");
    }

    serial_println!("[OCRB] ============================================");
}
