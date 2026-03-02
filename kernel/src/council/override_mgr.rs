//! Override manager — Tier 2/3 overrides of Tier 1 decisions with TTL decay.
//!
//! When Council overrides a Tier 1 Deny → Allow, the override is stored
//! with a TTL. Expired overrides are automatically removed.

#![allow(dead_code)]

use fabric_types::governance::{PolicyVerdict, TierLevel};

/// Maximum concurrent overrides.
pub const MAX_OVERRIDES: usize = 16;

/// Default TTL for overrides in ticks (5 minutes at 1kHz).
pub const DEFAULT_OVERRIDE_TTL: u64 = 300_000;

/// A single override entry.
#[derive(Clone, Copy)]
pub struct OverrideEntry {
    /// Which Tier 1 rule was overridden.
    pub rule_name: &'static str,
    /// The override verdict (typically Allow).
    pub verdict: PolicyVerdict,
    /// Tick at which this override expires.
    pub expiry_tick: u64,
    /// Which tier produced this override.
    pub tier_source: TierLevel,
    /// Whether this slot is active.
    pub active: bool,
    /// Context key: sender_pid for matching.
    pub sender_pid: u32,
    /// Context key: msg_type for matching.
    pub msg_type: u16,
}

impl OverrideEntry {
    pub const fn empty() -> Self {
        Self {
            rule_name: "",
            verdict: PolicyVerdict::Allow,
            expiry_tick: 0,
            tier_source: TierLevel::Tier2,
            active: false,
            sender_pid: 0,
            msg_type: 0,
        }
    }
}

/// Override manager.
pub struct OverrideManager {
    entries: [OverrideEntry; MAX_OVERRIDES],
    count: usize,
}

impl OverrideManager {
    pub const fn new() -> Self {
        Self {
            entries: [OverrideEntry::empty(); MAX_OVERRIDES],
            count: 0,
        }
    }

    /// Add an override. Returns false if table full.
    pub fn add(
        &mut self,
        rule_name: &'static str,
        verdict: PolicyVerdict,
        tier_source: TierLevel,
        current_tick: u64,
        sender_pid: u32,
        msg_type: u16,
    ) -> bool {
        // Find a free slot (or reuse expired)
        for entry in &mut self.entries {
            if !entry.active || current_tick >= entry.expiry_tick {
                *entry = OverrideEntry {
                    rule_name,
                    verdict,
                    expiry_tick: current_tick + DEFAULT_OVERRIDE_TTL,
                    tier_source,
                    active: true,
                    sender_pid,
                    msg_type,
                };
                if !entry.active {
                    self.count += 1;
                }
                return true;
            }
        }
        false // Table full
    }

    /// Check if an override exists for the given context. Returns the verdict if found.
    /// Also prunes expired entries.
    pub fn check(&mut self, sender_pid: u32, msg_type: u16, current_tick: u64) -> Option<PolicyVerdict> {
        for entry in &mut self.entries {
            if !entry.active {
                continue;
            }
            // Prune expired
            if current_tick >= entry.expiry_tick {
                entry.active = false;
                self.count = self.count.saturating_sub(1);
                continue;
            }
            // Match on sender + msg_type
            if entry.sender_pid == sender_pid && entry.msg_type == msg_type {
                return Some(entry.verdict);
            }
        }
        None
    }

    /// Remove all overrides from a specific tier.
    pub fn clear_tier(&mut self, tier: TierLevel) {
        for entry in &mut self.entries {
            if entry.active && entry.tier_source == tier {
                entry.active = false;
                self.count = self.count.saturating_sub(1);
            }
        }
    }

    /// Count active (non-expired) overrides.
    pub fn active_count(&self, current_tick: u64) -> usize {
        self.entries.iter()
            .filter(|e| e.active && current_tick < e.expiry_tick)
            .count()
    }

    /// Reset all overrides.
    pub fn reset(&mut self) {
        for entry in &mut self.entries {
            *entry = OverrideEntry::empty();
        }
        self.count = 0;
    }
}
