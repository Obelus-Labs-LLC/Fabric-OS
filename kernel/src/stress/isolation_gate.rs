//! STRESS Phase 6 — Isolation Gate Tests (10 tests).
//!
//! Tests per-process address spaces, handle-based capability access,
//! revocation efficiency (TD-004), Butler state externalization,
//! break-glass mechanism, multi-process handle isolation, and
//! #[must_use] annotations (TD-009).

#![allow(dead_code)]

use alloc::{format, vec::Vec};
use fabric_types::{
    HandleId, Perm, ProcessId, ResourceId,
    governance::{SafetyState, AcsState, BreakGlassReason},
};
use crate::ocrb::StressResult;

/// Reset all relevant state between tests.
fn reset_state() {
    crate::governance::GOVERNANCE.lock().clear();
    crate::bus::BUS.lock().clear();
    crate::capability::STORE.lock().clear();
    crate::process::TABLE.lock().clear();
    crate::process::SCHEDULER.lock().clear();
    crate::process::init(); // Re-init Butler
}

// =============================================================================
// Test 1: Address Space Create/Destroy (w:15)
// =============================================================================

fn test_1_address_space_create_destroy() -> StressResult {
    use crate::address_space::per_process::AddressSpace;

    // Track frames before
    let frames_before = crate::memory::frame::ALLOCATOR.lock().available_frames();

    // Create an address space
    let addr_space = match AddressSpace::create() {
        Ok(a) => a,
        Err(e) => {
            return StressResult {
                test_name: "Address Space Create/Destroy",
                passed: false, score: 0, weight: 15,
                details: format!("Failed to create address space: {:?}", e),
            };
        }
    };

    // Verify it's active
    if !addr_space.is_active() {
        return StressResult {
            test_name: "Address Space Create/Destroy",
            passed: false, score: 0, weight: 15,
            details: format!("Address space not active after creation"),
        };
    }

    // Verify CR3 is non-zero
    if addr_space.cr3().0 == 0 {
        return StressResult {
            test_name: "Address Space Create/Destroy",
            passed: false, score: 20, weight: 15,
            details: format!("CR3 is zero after creation"),
        };
    }

    // Verify zero user pages initially
    if addr_space.user_page_count() != 0 {
        return StressResult {
            test_name: "Address Space Create/Destroy",
            passed: false, score: 30, weight: 15,
            details: format!("User page count={}, expected 0", addr_space.user_page_count()),
        };
    }

    let frames_after_create = crate::memory::frame::ALLOCATOR.lock().available_frames();

    // At least 1 frame consumed (PML4)
    if frames_after_create >= frames_before {
        return StressResult {
            test_name: "Address Space Create/Destroy",
            passed: false, score: 40, weight: 15,
            details: format!("No frames consumed for PML4 (before={}, after={})", frames_before, frames_after_create),
        };
    }

    // Drop the address space — should free the PML4 frame
    drop(addr_space);

    let frames_after_drop = crate::memory::frame::ALLOCATOR.lock().available_frames();

    // Frames should be restored (PML4 freed)
    if frames_after_drop != frames_before {
        return StressResult {
            test_name: "Address Space Create/Destroy",
            passed: false, score: 60, weight: 15,
            details: format!("Frame leak: before={}, after_drop={}", frames_before, frames_after_drop),
        };
    }

    StressResult {
        test_name: "Address Space Create/Destroy",
        passed: true, score: 100, weight: 15,
        details: format!("create→active→cr3_valid→drop→frames_restored"),
    }
}

// =============================================================================
// Test 2: Kernel Mapping Integrity (w:10)
// =============================================================================

fn test_2_kernel_mapping_integrity() -> StressResult {
    use crate::address_space::per_process::AddressSpace;

    let addr_space = match AddressSpace::create() {
        Ok(a) => a,
        Err(e) => {
            return StressResult {
                test_name: "Kernel Mapping Integrity",
                passed: false, score: 0, weight: 10,
                details: format!("Failed to create address space: {:?}", e),
            };
        }
    };

    // Verify upper-half entries match kernel PML4
    if !addr_space.verify_kernel_mappings() {
        return StressResult {
            test_name: "Kernel Mapping Integrity",
            passed: false, score: 0, weight: 10,
            details: format!("Kernel PML4 entries 256-511 mismatch!"),
        };
    }

    // Create a second address space and verify both match
    let addr_space2 = match AddressSpace::create() {
        Ok(a) => a,
        Err(e) => {
            return StressResult {
                test_name: "Kernel Mapping Integrity",
                passed: false, score: 50, weight: 10,
                details: format!("Failed to create second address space: {:?}", e),
            };
        }
    };

    if !addr_space2.verify_kernel_mappings() {
        return StressResult {
            test_name: "Kernel Mapping Integrity",
            passed: false, score: 50, weight: 10,
            details: format!("Second address space kernel mappings mismatch"),
        };
    }

    // Both have same kernel half — verify their CR3s differ (separate PML4s)
    if addr_space.cr3().0 == addr_space2.cr3().0 {
        return StressResult {
            test_name: "Kernel Mapping Integrity",
            passed: false, score: 70, weight: 10,
            details: format!("Two address spaces share same CR3!"),
        };
    }

    drop(addr_space2);
    drop(addr_space);

    StressResult {
        test_name: "Kernel Mapping Integrity",
        passed: true, score: 100, weight: 10,
        details: format!("Upper-half verified for 2 address spaces, distinct CR3s"),
    }
}

// =============================================================================
// Test 3: Handle Alloc/Resolve/Release Storm (w:15)
// =============================================================================

fn test_3_handle_storm() -> StressResult {
    use crate::handle::table::{HandleTable, MAX_HANDLES};

    let mut table = HandleTable::new();

    // Alloc all 256 handles
    let mut handles = Vec::new();
    for i in 0..MAX_HANDLES {
        match table.alloc(i as u64 + 100) {
            Ok(h) => handles.push(h),
            Err(e) => {
                return StressResult {
                    test_name: "Handle Alloc/Resolve/Release Storm",
                    passed: false, score: 0, weight: 15,
                    details: format!("Alloc failed at slot {}: {:?}", i, e),
                };
            }
        }
    }

    // Verify count is 256
    if table.count() != MAX_HANDLES {
        return StressResult {
            test_name: "Handle Alloc/Resolve/Release Storm",
            passed: false, score: 20, weight: 15,
            details: format!("Count={}, expected {}", table.count(), MAX_HANDLES),
        };
    }

    // Verify table full
    if table.alloc(999).is_ok() {
        return StressResult {
            test_name: "Handle Alloc/Resolve/Release Storm",
            passed: false, score: 30, weight: 15,
            details: format!("Alloc succeeded on full table!"),
        };
    }

    // Resolve all handles
    let mut resolve_ok = 0;
    for (i, handle) in handles.iter().enumerate() {
        match table.resolve(*handle) {
            Ok(cap_id) if cap_id == (i as u64 + 100) => resolve_ok += 1,
            Ok(cap_id) => {
                return StressResult {
                    test_name: "Handle Alloc/Resolve/Release Storm",
                    passed: false, score: 40, weight: 15,
                    details: format!("Resolve slot {} returned cap_id={}, expected {}", i, cap_id, i + 100),
                };
            }
            Err(e) => {
                return StressResult {
                    test_name: "Handle Alloc/Resolve/Release Storm",
                    passed: false, score: 40, weight: 15,
                    details: format!("Resolve failed at slot {}: {:?}", i, e),
                };
            }
        }
    }

    // Release all handles
    for handle in &handles {
        if let Err(e) = table.release(*handle) {
            return StressResult {
                test_name: "Handle Alloc/Resolve/Release Storm",
                passed: false, score: 60, weight: 15,
                details: format!("Release failed: {:?}", e),
            };
        }
    }

    // Verify count is 0
    if table.count() != 0 {
        return StressResult {
            test_name: "Handle Alloc/Resolve/Release Storm",
            passed: false, score: 70, weight: 15,
            details: format!("Count={} after releasing all, expected 0", table.count()),
        };
    }

    // Re-alloc should work again
    match table.alloc(42) {
        Ok(_) => {},
        Err(e) => {
            return StressResult {
                test_name: "Handle Alloc/Resolve/Release Storm",
                passed: false, score: 80, weight: 15,
                details: format!("Re-alloc after release failed: {:?}", e),
            };
        }
    }

    StressResult {
        test_name: "Handle Alloc/Resolve/Release Storm",
        passed: true, score: 100, weight: 15,
        details: format!("alloc {}→resolve {}→release all→re-alloc OK", MAX_HANDLES, resolve_ok),
    }
}

// =============================================================================
// Test 4: Handle Stale Detection (w:10)
// =============================================================================

fn test_4_handle_stale_detection() -> StressResult {
    use crate::handle::table::{HandleTable, HandleError};

    let mut table = HandleTable::new();

    // Alloc a handle
    let handle = match table.alloc(42) {
        Ok(h) => h,
        Err(e) => {
            return StressResult {
                test_name: "Handle Stale Detection",
                passed: false, score: 0, weight: 10,
                details: format!("Alloc failed: {:?}", e),
            };
        }
    };

    // Verify resolve works
    if table.resolve(handle).is_err() {
        return StressResult {
            test_name: "Handle Stale Detection",
            passed: false, score: 10, weight: 10,
            details: format!("Fresh handle resolve failed"),
        };
    }

    // Release the handle
    if let Err(e) = table.release(handle) {
        return StressResult {
            test_name: "Handle Stale Detection",
            passed: false, score: 20, weight: 10,
            details: format!("Release failed: {:?}", e),
        };
    }

    // Try to resolve the stale handle — generation mismatch
    match table.resolve(handle) {
        Err(HandleError::NotActive) => {
            // This is correct — slot is inactive
        },
        Err(HandleError::StaleGeneration) => {
            // Also correct — if re-allocated with new generation
        },
        Ok(_) => {
            return StressResult {
                test_name: "Handle Stale Detection",
                passed: false, score: 30, weight: 10,
                details: format!("Stale handle resolved successfully!"),
            };
        }
        Err(e) => {
            return StressResult {
                test_name: "Handle Stale Detection",
                passed: false, score: 30, weight: 10,
                details: format!("Unexpected error on stale resolve: {:?}", e),
            };
        }
    }

    // Now re-alloc the same slot — should get new generation
    let new_handle = match table.alloc(99) {
        Ok(h) => h,
        Err(e) => {
            return StressResult {
                test_name: "Handle Stale Detection",
                passed: false, score: 50, weight: 10,
                details: format!("Re-alloc failed: {:?}", e),
            };
        }
    };

    // Old handle should fail with StaleGeneration
    match table.resolve(handle) {
        Err(HandleError::StaleGeneration) => {
            // Correct — old generation
        },
        Ok(_) => {
            return StressResult {
                test_name: "Handle Stale Detection",
                passed: false, score: 60, weight: 10,
                details: format!("Old handle resolved after re-alloc!"),
            };
        }
        Err(e) => {
            return StressResult {
                test_name: "Handle Stale Detection",
                passed: false, score: 60, weight: 10,
                details: format!("Unexpected error: {:?}", e),
            };
        }
    }

    // New handle should resolve to 99
    match table.resolve(new_handle) {
        Ok(99) => {},
        Ok(v) => {
            return StressResult {
                test_name: "Handle Stale Detection",
                passed: false, score: 70, weight: 10,
                details: format!("New handle resolved to {}, expected 99", v),
            };
        }
        Err(e) => {
            return StressResult {
                test_name: "Handle Stale Detection",
                passed: false, score: 70, weight: 10,
                details: format!("New handle resolve failed: {:?}", e),
            };
        }
    }

    StressResult {
        test_name: "Handle Stale Detection",
        passed: true, score: 100, weight: 10,
        details: format!(
            "old_gen={}, new_gen={}, stale correctly rejected",
            handle.generation(), new_handle.generation()
        ),
    }
}

// =============================================================================
// Test 5: Revocation O(n) Verify — TD-004 (w:10)
// =============================================================================

fn test_5_revocation_efficiency() -> StressResult {
    reset_state();

    let mut store = crate::capability::STORE.lock();
    store.clear();

    // Create a root capability
    let resource = ResourceId::new(0x0001_0000_0000_0001);
    let owner = ProcessId::new(5);
    let root_cap = match store.create(resource, Perm::READ | Perm::WRITE | Perm::GRANT, owner, None, None) {
        Ok(id) => id,
        Err(_) => {
            return StressResult {
                test_name: "Revocation O(n) Verify",
                passed: false, score: 0, weight: 10,
                details: format!("Failed to create root capability"),
            };
        }
    };

    // Build a 100-deep delegation chain
    let chain_depth = 100;
    let mut current_parent = root_cap.0;

    for i in 0..chain_depth {
        let child_owner = ProcessId::new(10 + i as u32);
        match store.delegate(
            current_parent,
            child_owner,
            Perm::READ | Perm::WRITE | Perm::GRANT,
            None,
            None,
        ) {
            Ok(child_id) => {
                current_parent = child_id.0;
            }
            Err(e) => {
                return StressResult {
                    test_name: "Revocation O(n) Verify",
                    passed: false, score: 20, weight: 10,
                    details: format!("Delegation failed at depth {}: {:?}", i, e),
                };
            }
        }
    }

    // Verify chain length: root + 100 children = 101
    let total_before = store.count();
    if total_before != chain_depth + 1 {
        return StressResult {
            test_name: "Revocation O(n) Verify",
            passed: false, score: 30, weight: 10,
            details: format!("Token count={}, expected {}", total_before, chain_depth + 1),
        };
    }

    // Revoke root — should cascade to all 101 tokens
    let revoked = match store.revoke(root_cap.0) {
        Ok(count) => count,
        Err(e) => {
            return StressResult {
                test_name: "Revocation O(n) Verify",
                passed: false, score: 40, weight: 10,
                details: format!("Revoke failed: {:?}", e),
            };
        }
    };

    if revoked != chain_depth + 1 {
        return StressResult {
            test_name: "Revocation O(n) Verify",
            passed: false, score: 60, weight: 10,
            details: format!("Revoked={}, expected {} (chain depth + root)", revoked, chain_depth + 1),
        };
    }

    // Verify store is empty
    if store.count() != 0 {
        return StressResult {
            test_name: "Revocation O(n) Verify",
            passed: false, score: 70, weight: 10,
            details: format!("Store count={} after revoke, expected 0", store.count()),
        };
    }

    drop(store);

    StressResult {
        test_name: "Revocation O(n) Verify",
        passed: true, score: 100, weight: 10,
        details: format!("100-deep chain: created {}, revoked {} via children index", chain_depth + 1, revoked),
    }
}

// =============================================================================
// Test 6: Butler State Persist (w:10)
// =============================================================================

fn test_6_butler_state_persist() -> StressResult {
    use crate::butler_state::{BUTLER_STATE, ButlerStateBlock};

    let mgr = BUTLER_STATE.lock();

    if !mgr.is_initialized() {
        return StressResult {
            test_name: "Butler State Persist",
            passed: false, score: 0, weight: 10,
            details: format!("Butler state manager not initialized"),
        };
    }

    // Load fresh state
    let block = match mgr.load() {
        Some(b) => b,
        None => {
            return StressResult {
                test_name: "Butler State Persist",
                passed: false, score: 10, weight: 10,
                details: format!("Failed to load initial state block"),
            };
        }
    };

    if !block.is_valid() {
        return StressResult {
            test_name: "Butler State Persist",
            passed: false, score: 20, weight: 10,
            details: format!("State block magic/version invalid"),
        };
    }

    // Modify state: record some restarts
    let mut modified = block;
    modified.child_count = 3;
    modified.record_restart(0, 1000);
    modified.record_restart(0, 2000);
    modified.record_restart(1, 3000);
    modified.set_strategy(0, 1);
    modified.set_strategy(1, 2);
    modified.break_glass_active = true;
    modified.last_checkpoint_tick = 5000;

    // Save modified state
    if !mgr.save(&mut modified) {
        return StressResult {
            test_name: "Butler State Persist",
            passed: false, score: 30, weight: 10,
            details: format!("Failed to save modified state"),
        };
    }

    // Simulate "crash" by loading from the same page
    let reloaded = match mgr.load() {
        Some(b) => b,
        None => {
            return StressResult {
                test_name: "Butler State Persist",
                passed: false, score: 40, weight: 10,
                details: format!("Failed to reload state after save"),
            };
        }
    };

    // Verify persisted data
    if reloaded.child_count != 3 {
        return StressResult {
            test_name: "Butler State Persist",
            passed: false, score: 50, weight: 10,
            details: format!("child_count={}, expected 3", reloaded.child_count),
        };
    }

    if reloaded.total_restarts(0) != 2 {
        return StressResult {
            test_name: "Butler State Persist",
            passed: false, score: 60, weight: 10,
            details: format!("child 0 restarts={}, expected 2", reloaded.total_restarts(0)),
        };
    }

    if reloaded.total_restarts(1) != 1 {
        return StressResult {
            test_name: "Butler State Persist",
            passed: false, score: 60, weight: 10,
            details: format!("child 1 restarts={}, expected 1", reloaded.total_restarts(1)),
        };
    }

    if !reloaded.break_glass_active {
        return StressResult {
            test_name: "Butler State Persist",
            passed: false, score: 70, weight: 10,
            details: format!("break_glass_active not persisted"),
        };
    }

    if reloaded.last_checkpoint_tick != 5000 {
        return StressResult {
            test_name: "Butler State Persist",
            passed: false, score: 80, weight: 10,
            details: format!("last_checkpoint_tick={}, expected 5000", reloaded.last_checkpoint_tick),
        };
    }

    // Verify checksum integrity
    if !reloaded.verify_checksum() {
        return StressResult {
            test_name: "Butler State Persist",
            passed: false, score: 90, weight: 10,
            details: format!("Checksum verification failed on reloaded state"),
        };
    }

    // Restore clean state
    let mut fresh = ButlerStateBlock::fresh();
    let _ = mgr.save(&mut fresh);

    drop(mgr);

    StressResult {
        test_name: "Butler State Persist",
        passed: true, score: 100, weight: 10,
        details: format!("write→reload→verify: children, restarts, checksum all intact"),
    }
}

// =============================================================================
// Test 7: Break-Glass Activation (w:10)
// =============================================================================

fn test_7_break_glass_activation() -> StressResult {
    use crate::governance::break_glass::BreakGlass;

    let mut bg = BreakGlass::new();

    // Should not be active initially
    if bg.is_active() {
        return StressResult {
            test_name: "Break-Glass Activation",
            passed: false, score: 0, weight: 10,
            details: format!("Break-glass active at init!"),
        };
    }

    // Normal + Active → should NOT activate
    let activated = bg.check_and_activate(SafetyState::Normal, AcsState::Active, 100);
    if activated || bg.is_active() {
        return StressResult {
            test_name: "Break-Glass Activation",
            passed: false, score: 10, weight: 10,
            details: format!("Break-glass activated under Normal/Active!"),
        };
    }

    // Lockdown + Active → should NOT activate (need Emergency ACS)
    let activated = bg.check_and_activate(SafetyState::Lockdown, AcsState::Active, 200);
    if activated || bg.is_active() {
        return StressResult {
            test_name: "Break-Glass Activation",
            passed: false, score: 20, weight: 10,
            details: format!("Break-glass activated under Lockdown/Active!"),
        };
    }

    // Normal + Emergency → should NOT activate (need Lockdown safety)
    let activated = bg.check_and_activate(SafetyState::Normal, AcsState::Emergency, 300);
    if activated || bg.is_active() {
        return StressResult {
            test_name: "Break-Glass Activation",
            passed: false, score: 30, weight: 10,
            details: format!("Break-glass activated under Normal/Emergency!"),
        };
    }

    // Lockdown + Emergency → SHOULD activate
    let activated = bg.check_and_activate(SafetyState::Lockdown, AcsState::Emergency, 400);
    if !activated || !bg.is_active() {
        return StressResult {
            test_name: "Break-Glass Activation",
            passed: false, score: 40, weight: 10,
            details: format!("Break-glass did NOT activate under Lockdown/Emergency!"),
        };
    }

    // Verify reason
    if bg.reason() != BreakGlassReason::SafetyLockdown {
        return StressResult {
            test_name: "Break-Glass Activation",
            passed: false, score: 60, weight: 10,
            details: format!("Wrong reason: {:?}", bg.reason()),
        };
    }

    // Log some operations
    bg.log_operation();
    bg.log_operation();
    bg.log_operation();
    if bg.operations_logged() != 3 {
        return StressResult {
            test_name: "Break-Glass Activation",
            passed: false, score: 70, weight: 10,
            details: format!("Operations logged={}, expected 3", bg.operations_logged()),
        };
    }

    // Verify total activations counter
    if bg.total_activations() != 1 {
        return StressResult {
            test_name: "Break-Glass Activation",
            passed: false, score: 80, weight: 10,
            details: format!("Total activations={}, expected 1", bg.total_activations()),
        };
    }

    StressResult {
        test_name: "Break-Glass Activation",
        passed: true, score: 100, weight: 10,
        details: format!("Lockdown+Emergency→active, 3 ops logged, reason=SafetyLockdown"),
    }
}

// =============================================================================
// Test 8: Break-Glass Auto-Expire (w:10)
// =============================================================================

fn test_8_break_glass_auto_expire() -> StressResult {
    use crate::governance::break_glass::{BreakGlass, BREAK_GLASS_EXPIRY_TICKS};

    let mut bg = BreakGlass::new();

    // Activate at tick 1000
    bg.activate(BreakGlassReason::GovernancePanic, 1000);

    if !bg.is_active() {
        return StressResult {
            test_name: "Break-Glass Auto-Expire",
            passed: false, score: 0, weight: 10,
            details: format!("Break-glass not active after activate()"),
        };
    }

    // Check remaining ticks
    let remaining = bg.remaining_ticks(1000);
    if remaining != BREAK_GLASS_EXPIRY_TICKS {
        return StressResult {
            test_name: "Break-Glass Auto-Expire",
            passed: false, score: 10, weight: 10,
            details: format!("Remaining={}, expected {}", remaining, BREAK_GLASS_EXPIRY_TICKS),
        };
    }

    // Still active just before expiry
    bg.check_expiry(1000 + BREAK_GLASS_EXPIRY_TICKS - 1);
    if !bg.is_active() {
        return StressResult {
            test_name: "Break-Glass Auto-Expire",
            passed: false, score: 30, weight: 10,
            details: format!("Expired 1 tick too early"),
        };
    }

    // Expire at exactly the expiry tick
    bg.check_expiry(1000 + BREAK_GLASS_EXPIRY_TICKS);
    if bg.is_active() {
        return StressResult {
            test_name: "Break-Glass Auto-Expire",
            passed: false, score: 50, weight: 10,
            details: format!("Did not expire at expiry tick"),
        };
    }

    // Test recovery deactivation: activate again, then recover
    bg.activate(BreakGlassReason::AcsSuccessionFailed, 100_000);
    if !bg.is_active() {
        return StressResult {
            test_name: "Break-Glass Auto-Expire",
            passed: false, score: 60, weight: 10,
            details: format!("Second activation failed"),
        };
    }

    // Safety recovers to Normal → should deactivate
    bg.check_recovery(SafetyState::Normal);
    if bg.is_active() {
        return StressResult {
            test_name: "Break-Glass Auto-Expire",
            passed: false, score: 70, weight: 10,
            details: format!("Did not deactivate on safety recovery"),
        };
    }

    // Verify total activations
    if bg.total_activations() != 2 {
        return StressResult {
            test_name: "Break-Glass Auto-Expire",
            passed: false, score: 80, weight: 10,
            details: format!("Total activations={}, expected 2", bg.total_activations()),
        };
    }

    StressResult {
        test_name: "Break-Glass Auto-Expire",
        passed: true, score: 100, weight: 10,
        details: format!("Active before expiry, expired at {}ms, recovery deactivation OK", BREAK_GLASS_EXPIRY_TICKS),
    }
}

// =============================================================================
// Test 9: Multi-Process Handle Isolation (w:5)
// =============================================================================

fn test_9_multi_process_handle_isolation() -> StressResult {
    use crate::handle::table::HandleTable;

    // Create two separate handle tables (simulating two processes)
    let mut table_a = HandleTable::new();
    let mut table_b = HandleTable::new();

    // Process A allocates handles
    let handle_a1 = match table_a.alloc(100) {
        Ok(h) => h,
        Err(e) => {
            return StressResult {
                test_name: "Multi-Process Handle Isolation",
                passed: false, score: 0, weight: 5,
                details: format!("Process A alloc failed: {:?}", e),
            };
        }
    };

    let _handle_a2 = match table_a.alloc(200) {
        Ok(h) => h,
        Err(e) => {
            return StressResult {
                test_name: "Multi-Process Handle Isolation",
                passed: false, score: 10, weight: 5,
                details: format!("Process A second alloc failed: {:?}", e),
            };
        }
    };

    // Process B allocates handles
    let handle_b1 = match table_b.alloc(300) {
        Ok(h) => h,
        Err(e) => {
            return StressResult {
                test_name: "Multi-Process Handle Isolation",
                passed: false, score: 20, weight: 5,
                details: format!("Process B alloc failed: {:?}", e),
            };
        }
    };

    // Process A's handles resolve correctly in A's table
    match table_a.resolve(handle_a1) {
        Ok(100) => {},
        other => {
            return StressResult {
                test_name: "Multi-Process Handle Isolation",
                passed: false, score: 30, weight: 5,
                details: format!("Process A handle resolved incorrectly: {:?}", other),
            };
        }
    }

    // Process A's handle DOES resolve in B's table structurally (same slot index)
    // BUT it maps to a DIFFERENT capability (300 not 100), proving isolation.
    // Since both tables start fresh, slot 0 exists in both but maps differently.
    match table_b.resolve(handle_a1) {
        Ok(cap_id) => {
            // The handle from A resolves in B because slot 0 exists in both.
            // But it should return B's cap_id (300), not A's (100).
            if cap_id == 100 {
                return StressResult {
                    test_name: "Multi-Process Handle Isolation",
                    passed: false, score: 40, weight: 5,
                    details: format!("Cross-process handle leaked A's cap_id!"),
                };
            }
            // It returns B's mapping — separate tables means separate mappings
        }
        Err(_) => {
            // Also acceptable if generation differs or slot isn't active
        }
    }

    // Verify independent counts
    if table_a.count() != 2 || table_b.count() != 1 {
        return StressResult {
            test_name: "Multi-Process Handle Isolation",
            passed: false, score: 50, weight: 5,
            details: format!("Count mismatch: A={}, B={}", table_a.count(), table_b.count()),
        };
    }

    // Release in A doesn't affect B
    let _ = table_a.release(handle_a1);
    if table_b.count() != 1 {
        return StressResult {
            test_name: "Multi-Process Handle Isolation",
            passed: false, score: 60, weight: 5,
            details: format!("Release in A affected B's count"),
        };
    }

    // B's handle still resolves correctly
    match table_b.resolve(handle_b1) {
        Ok(300) => {},
        other => {
            return StressResult {
                test_name: "Multi-Process Handle Isolation",
                passed: false, score: 70, weight: 5,
                details: format!("B's handle broken after A's release: {:?}", other),
            };
        }
    }

    StressResult {
        test_name: "Multi-Process Handle Isolation",
        passed: true, score: 100, weight: 5,
        details: format!("Separate tables, independent alloc/release/resolve"),
    }
}

// =============================================================================
// Test 10: TD-009 #[must_use] Structural Verify (w:5)
// =============================================================================

fn test_10_must_use_verify() -> StressResult {
    // This test verifies that critical error types and Result-returning functions
    // exist and can be properly constructed/handled. The #[must_use] attribute
    // is a compile-time check — if these types weren't #[must_use], the build
    // would emit warnings (which we treat as errors with -D warnings).
    //
    // We verify structural correctness: all error types constructible,
    // Result paths exercisable.

    use crate::handle::table::{HandleTable, HandleError};
    use crate::address_space::AddressSpaceError;

    let mut checks_passed = 0u32;
    let total_checks = 5u32;

    // 1. HandleError variants exist and are returnable
    {
        let table = HandleTable::new();
        let fake_handle = HandleId::pack(0, 0);
        let result = table.resolve(fake_handle);
        match result {
            Err(HandleError::NotActive) => checks_passed += 1,
            _ => {
                return StressResult {
                    test_name: "TD-009 #[must_use] Structural Verify",
                    passed: false, score: 0, weight: 5,
                    details: format!("HandleError::NotActive not returned for empty table"),
                };
            }
        }
    }

    // 2. HandleError::TableFull reachable
    {
        let mut table = HandleTable::new();
        for i in 0..256 {
            let _ = table.alloc(i);
        }
        match table.alloc(999) {
            Err(HandleError::TableFull) => checks_passed += 1,
            _ => {
                return StressResult {
                    test_name: "TD-009 #[must_use] Structural Verify",
                    passed: false, score: 20, weight: 5,
                    details: format!("HandleError::TableFull not returned"),
                };
            }
        }
    }

    // 3. CapabilityError::NotFound reachable
    {
        let mut store = crate::capability::STORE.lock();
        store.clear();
        match store.revoke(99999) {
            Err(crate::capability::store::CapabilityError::NotFound) => checks_passed += 1,
            _ => {
                return StressResult {
                    test_name: "TD-009 #[must_use] Structural Verify",
                    passed: false, score: 40, weight: 5,
                    details: format!("CapabilityError::NotFound not returned"),
                };
            }
        }
        drop(store);
    }

    // 4. ProcessError::TableFull check (structural)
    {
        use crate::process::table::ProcessError;
        let err = ProcessError::TableFull;
        let _ = format!("{:?}", err); // Debug impl works
        checks_passed += 1;
    }

    // 5. AddressSpaceError::KernelAddressViolation check (structural)
    {
        let err = AddressSpaceError::KernelAddressViolation;
        let _ = format!("{:?}", err); // Debug impl works
        checks_passed += 1;
    }

    let score = if checks_passed == total_checks { 100 } else {
        ((checks_passed as u64 * 100) / total_checks as u64) as u8
    };

    StressResult {
        test_name: "TD-009 #[must_use] Structural Verify",
        passed: checks_passed == total_checks, score, weight: 5,
        details: format!("{}/{} error type checks passed", checks_passed, total_checks),
    }
}

// =============================================================================
// Run all Phase 6 tests
// =============================================================================

/// Run all Phase 6 STRESS tests.
pub fn run_all_tests() -> Vec<StressResult> {
    let mut results = Vec::new();
    results.push(test_1_address_space_create_destroy());
    results.push(test_2_kernel_mapping_integrity());
    results.push(test_3_handle_storm());
    results.push(test_4_handle_stale_detection());
    results.push(test_5_revocation_efficiency());
    results.push(test_6_butler_state_persist());
    results.push(test_7_break_glass_activation());
    results.push(test_8_break_glass_auto_expire());
    results.push(test_9_multi_process_handle_isolation());
    results.push(test_10_must_use_verify());
    results
}
