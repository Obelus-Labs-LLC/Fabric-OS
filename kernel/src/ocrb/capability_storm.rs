#![allow(dead_code)]

use alloc::{format, vec, vec::Vec};
use crate::capability::{self, ResourceId, ProcessId, Perm, Budget, CapabilityError};
use crate::ocrb::OcrbResult;

pub fn run_all_tests() -> Vec<OcrbResult> {
    vec![
        test1_mass_creation(),
        test2_validation_throughput(),
        test3_hmac_tamper_detection(),
        test4_delegation_chain_depth(),
        test5_permission_escalation(),
        test6_budget_enforcement(),
        test7_nonce_replay(),
        test8_expiration(),
        test9_revocation_storm(),
    ]
}

/// Test 1: Mass Token Creation (weight: 15)
/// Create 10,000 tokens, verify all unique IDs and valid HMACs.
fn test1_mass_creation() -> OcrbResult {
    capability::STORE.lock().clear();
    let mut errors = 0u32;
    let mut ids: Vec<u64> = Vec::with_capacity(10000);

    for i in 0..10000u32 {
        match capability::create(
            ResourceId::new(i as u64 + 1),
            Perm::READ | Perm::WRITE,
            ProcessId::new(i % 100),
            None,
            None,
        ) {
            Ok(cap_id) => ids.push(cap_id.0),
            Err(_) => errors += 1,
        }
    }

    // Verify all IDs are unique (sequential, so just check count)
    ids.sort();
    ids.dedup();
    if ids.len() != 10000 {
        errors += (10000 - ids.len()) as u32;
    }

    // Verify HMACs are valid on a sample
    {
        let store = capability::STORE.lock();
        for &id in ids.iter().take(100) {
            if let Some(stored) = store.get(id) {
                if !crate::capability::hmac_engine::verify(
                    &stored.token.active_bytes(),
                    &stored.hmac,
                ) {
                    errors += 1;
                }
            } else {
                errors += 1;
            }
        }
    }

    // Clean up
    capability::STORE.lock().clear();

    let score = if errors == 0 { 100 } else { (100u32).saturating_sub(errors * 5) as u8 };

    OcrbResult {
        test_name: "Mass Token Creation",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("10000 tokens, {} unique IDs, {} errors", ids.len(), errors),
    }
}

/// Test 2: Validation Throughput (weight: 20)
/// Create 1,000 tokens, validate each 10 times (10K total validations).
fn test2_validation_throughput() -> OcrbResult {
    capability::STORE.lock().clear();
    let mut errors = 0u32;

    let mut ids: Vec<u64> = Vec::with_capacity(1000);
    for i in 0..1000u32 {
        match capability::create(
            ResourceId::new(i as u64 + 1),
            Perm::READ | Perm::WRITE,
            ProcessId::new(i),
            None,
            None,
        ) {
            Ok(cap_id) => ids.push(cap_id.0),
            Err(_) => errors += 1,
        }
    }

    // Validate each token 10 times with incrementing nonces
    let mut validations = 0u32;
    for &id in &ids {
        for nonce in 1..=10u32 {
            match capability::validate(id, Perm::READ, nonce) {
                Ok(()) => validations += 1,
                Err(_) => errors += 1,
            }
        }
    }

    capability::STORE.lock().clear();

    let score = if errors == 0 { 100 } else { (100u32).saturating_sub(errors) as u8 };

    OcrbResult {
        test_name: "Validation Throughput",
        passed: score >= 80,
        score,
        weight: 20,
        details: format!("{} validations, {} errors", validations, errors),
    }
}

/// Test 3: HMAC Tamper Detection (weight: 15)
/// Create tokens, tamper with fields, verify HMAC rejects them.
fn test3_hmac_tamper_detection() -> OcrbResult {
    capability::STORE.lock().clear();
    let mut errors = 0u32;
    let mut detected = 0u32;

    for i in 0..100u32 {
        let cap_id = capability::create(
            ResourceId::new(i as u64 + 1),
            Perm::READ | Perm::WRITE,
            ProcessId::new(1),
            None,
            None,
        ).expect("create for tamper test");

        // Tamper: modify the stored token's resource field
        {
            let store = capability::STORE.lock();
            if let Some(stored) = store.get(cap_id.0) {
                let mut tampered = stored.token;
                tampered.resource = ResourceId::new(0xDEAD);
                let tampered_bytes = tampered.active_bytes();

                // The stored HMAC should NOT match the tampered bytes
                if !crate::capability::hmac_engine::verify(&tampered_bytes, &stored.hmac) {
                    detected += 1;
                } else {
                    errors += 1; // HMAC should have failed
                }
            }
        }
    }

    capability::STORE.lock().clear();

    let score = if errors == 0 && detected == 100 { 100 } else { 0 };

    OcrbResult {
        test_name: "HMAC Tamper Detection",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("{}/100 tampers detected, {} missed", detected, errors),
    }
}

/// Test 4: Delegation Chain Depth (weight: 10)
/// Create a root, delegate 100 levels deep, revoke root, verify all gone.
fn test4_delegation_chain_depth() -> OcrbResult {
    capability::STORE.lock().clear();

    let root_id = capability::create(
        ResourceId::new(1),
        Perm::READ | Perm::WRITE | Perm::GRANT,
        ProcessId::new(1),
        None,
        None,
    ).expect("create root for chain test");

    let mut parent_id = root_id.0;
    let mut chain_depth = 0u32;

    for i in 0..100u32 {
        match capability::delegate(
            parent_id,
            ProcessId::new(i + 2),
            Perm::READ | Perm::GRANT, // keep GRANT to continue delegating
            None,
            None,
        ) {
            Ok(child) => {
                parent_id = child.0;
                chain_depth += 1;
            }
            Err(_) => break,
        }
    }

    // Validate the deepest token
    let deepest_valid = capability::validate(parent_id, Perm::READ, 1).is_ok();

    // Revoke root — should cascade to all 101 tokens
    let revoked = capability::revoke(root_id.0).unwrap_or(0);
    let expected = chain_depth as usize + 1; // root + children
    let count_after = capability::count();

    capability::STORE.lock().clear();

    let score = if deepest_valid && revoked == expected && count_after == 0 {
        100
    } else {
        0
    };

    OcrbResult {
        test_name: "Delegation Chain Depth",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("depth={}, revoked={}/{}, empty={}", chain_depth, revoked, expected, count_after == 0),
    }
}

/// Test 5: Permission Escalation Prevention (weight: 15)
fn test5_permission_escalation() -> OcrbResult {
    capability::STORE.lock().clear();
    let mut correct = 0u32;
    let total = 3u32;

    // Case 1: Parent has no GRANT — delegation should fail
    let parent_no_grant = capability::create(
        ResourceId::new(1),
        Perm::READ,
        ProcessId::new(1),
        None,
        None,
    ).expect("create parent_no_grant");

    match capability::delegate(parent_no_grant.0, ProcessId::new(2), Perm::READ, None, None) {
        Err(CapabilityError::DelegationDenied) => correct += 1,
        _ => {}
    }

    // Case 2: Parent has READ|GRANT, child wants READ|WRITE — escalation
    let parent_rg = capability::create(
        ResourceId::new(2),
        Perm::READ | Perm::GRANT,
        ProcessId::new(1),
        None,
        None,
    ).expect("create parent_rg");

    match capability::delegate(parent_rg.0, ProcessId::new(2), Perm::READ | Perm::WRITE, None, None) {
        Err(CapabilityError::PermissionEscalation) => correct += 1,
        _ => {}
    }

    // Case 3: Parent has READ|GRANT, child wants READ — should succeed
    match capability::delegate(parent_rg.0, ProcessId::new(3), Perm::READ, None, None) {
        Ok(_) => correct += 1,
        _ => {}
    }

    capability::STORE.lock().clear();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };

    OcrbResult {
        test_name: "Permission Escalation Prevention",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("{}/{} cases correct", correct, total),
    }
}

/// Test 6: Budget Enforcement (weight: 10)
fn test6_budget_enforcement() -> OcrbResult {
    capability::STORE.lock().clear();
    let mut correct = 0u32;
    let total = 3u32;

    let budget = Budget { max_uses: 5, interval_ticks: 100 };
    let cap_id = capability::create(
        ResourceId::new(1),
        Perm::READ,
        ProcessId::new(1),
        None,
        Some(budget),
    ).expect("create budgeted cap");

    // Use 5 times (should all pass)
    let mut uses_ok = true;
    for nonce in 1..=5u32 {
        if capability::validate(cap_id.0, Perm::READ, nonce).is_err() {
            uses_ok = false;
        }
    }
    if uses_ok { correct += 1; }

    // 6th use should fail (budget exhausted)
    match capability::validate(cap_id.0, Perm::READ, 6) {
        Err(CapabilityError::BudgetExhausted) => correct += 1,
        _ => {}
    }

    // Advance ticks past interval, try again (should pass — new interval)
    capability::advance_ticks(100);
    match capability::validate(cap_id.0, Perm::READ, 7) {
        Ok(()) => correct += 1,
        _ => {}
    }

    capability::STORE.lock().clear();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };

    OcrbResult {
        test_name: "Budget Enforcement",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("{}/{} cases correct", correct, total),
    }
}

/// Test 7: Nonce Replay Prevention (weight: 10)
fn test7_nonce_replay() -> OcrbResult {
    capability::STORE.lock().clear();
    let mut correct = 0u32;
    let total = 4u32;

    let cap_id = capability::create(
        ResourceId::new(1),
        Perm::READ,
        ProcessId::new(1),
        None,
        None,
    ).expect("create for nonce test");

    // nonce=1 should pass
    match capability::validate(cap_id.0, Perm::READ, 1) {
        Ok(()) => correct += 1,
        _ => {}
    }

    // nonce=1 again should fail (replay)
    match capability::validate(cap_id.0, Perm::READ, 1) {
        Err(CapabilityError::NonceReplay) => correct += 1,
        _ => {}
    }

    // nonce=0 should fail (regression)
    match capability::validate(cap_id.0, Perm::READ, 0) {
        Err(CapabilityError::NonceReplay) => correct += 1,
        _ => {}
    }

    // nonce=2 should pass
    match capability::validate(cap_id.0, Perm::READ, 2) {
        Ok(()) => correct += 1,
        _ => {}
    }

    capability::STORE.lock().clear();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };

    OcrbResult {
        test_name: "Nonce Replay Prevention",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("{}/{} cases correct", correct, total),
    }
}

/// Test 8: Expiration Enforcement (weight: 5)
fn test8_expiration() -> OcrbResult {
    capability::STORE.lock().clear();
    let mut correct = 0u32;
    let total = 2u32;

    let cap_id = capability::create(
        ResourceId::new(1),
        Perm::READ,
        ProcessId::new(1),
        Some(50), // expires in 50 ticks
        None,
    ).expect("create expiring cap");

    // At tick 0, should be valid
    match capability::validate(cap_id.0, Perm::READ, 1) {
        Ok(()) => correct += 1,
        _ => {}
    }

    // Advance to tick 50 (created at tick 0, deadline = 0 + 50 = 50)
    capability::advance_ticks(50);

    // Should now be expired
    match capability::validate(cap_id.0, Perm::READ, 2) {
        Err(CapabilityError::Expired) => correct += 1,
        _ => {}
    }

    capability::STORE.lock().clear();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };

    OcrbResult {
        test_name: "Expiration Enforcement",
        passed: score >= 80,
        score,
        weight: 5,
        details: format!("{}/{} cases correct", correct, total),
    }
}

/// Test 9: Revocation Storm (weight: 10)
/// Create 1,000 roots with 9 children each (10K total), revoke all roots,
/// verify store is empty.
fn test9_revocation_storm() -> OcrbResult {
    capability::STORE.lock().clear();
    let mut errors = 0u32;
    let mut root_ids: Vec<u64> = Vec::with_capacity(1000);

    // Create 1,000 root tokens
    for i in 0..1000u32 {
        match capability::create(
            ResourceId::new(i as u64 + 1),
            Perm::READ | Perm::WRITE | Perm::GRANT,
            ProcessId::new(1),
            None,
            None,
        ) {
            Ok(cap_id) => root_ids.push(cap_id.0),
            Err(_) => errors += 1,
        }
    }

    // Delegate 9 children from each root
    let mut total_delegated = 0u32;
    for &root in &root_ids {
        for j in 0..9u32 {
            match capability::delegate(
                root,
                ProcessId::new(j + 100),
                Perm::READ,
                None,
                None,
            ) {
                Ok(_) => total_delegated += 1,
                Err(_) => errors += 1,
            }
        }
    }

    let count_before = capability::count();

    // Revoke all roots (each cascades to its 9 children)
    let mut total_revoked = 0usize;
    for &root in &root_ids {
        match capability::revoke(root) {
            Ok(n) => total_revoked += n,
            Err(_) => errors += 1,
        }
    }

    let count_after = capability::count();
    capability::STORE.lock().clear();

    let expected = 1000 + total_delegated as usize;
    let all_revoked = count_after == 0 && total_revoked == expected;

    let score = if all_revoked && errors == 0 { 100 } else { 0 };

    OcrbResult {
        test_name: "Revocation Storm",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!(
            "created={}, revoked={}/{}, empty={}, errors={}",
            count_before, total_revoked, expected, count_after == 0, errors
        ),
    }
}
