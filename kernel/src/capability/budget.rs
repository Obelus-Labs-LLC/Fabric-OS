//! Budget enforcement for rate-limited capabilities.
//!
//! Tracks usage counts per capability per time interval. When a capability's
//! budget is exhausted for the current interval, further uses are rejected
//! until the interval resets.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use fabric_types::Budget;

/// Per-capability usage tracking: (uses_this_interval, interval_start_tick).
pub struct BudgetTracker {
    usage: BTreeMap<u64, (u32, u64)>,
}

impl BudgetTracker {
    pub const fn new() -> Self {
        Self {
            usage: BTreeMap::new(),
        }
    }

    /// Check if a capability's budget allows another use.
    /// If allowed, increments the usage counter and returns true.
    /// If exhausted, returns false.
    pub fn check_and_consume(&mut self, cap_id: u64, budget: &Budget, current_tick: u64) -> bool {
        let entry = self.usage.entry(cap_id).or_insert((0, current_tick));

        // Reset if interval has elapsed
        if current_tick.saturating_sub(entry.1) >= budget.interval_ticks {
            entry.0 = 0;
            entry.1 = current_tick;
        }

        if entry.0 < budget.max_uses {
            entry.0 += 1;
            true
        } else {
            false
        }
    }

    /// Get remaining uses for a capability in the current interval.
    pub fn remaining(&self, cap_id: u64, budget: &Budget, current_tick: u64) -> u32 {
        match self.usage.get(&cap_id) {
            Some(&(used, start)) => {
                if current_tick.saturating_sub(start) >= budget.interval_ticks {
                    budget.max_uses
                } else {
                    budget.max_uses.saturating_sub(used)
                }
            }
            None => budget.max_uses,
        }
    }

    /// Remove tracking for a revoked capability.
    pub fn remove(&mut self, cap_id: u64) {
        self.usage.remove(&cap_id);
    }

    /// Clear all tracking state.
    pub fn clear(&mut self) {
        self.usage.clear();
    }
}
