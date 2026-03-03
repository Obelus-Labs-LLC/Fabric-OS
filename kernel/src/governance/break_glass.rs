//! Break-glass mechanism — emergency governance bypass.
//!
//! When the system reaches a state where governance cannot function
//! (Safety=Lockdown + ACS=Failed), break-glass activates to allow
//! critical operations through. All operations during break-glass
//! are audit-logged. Auto-expires after 60 seconds.

#![allow(dead_code)]

use fabric_types::governance::{SafetyState, AcsState, BreakGlassReason};

/// Break-glass auto-expiry duration in ticks (60 seconds at 1kHz).
pub const BREAK_GLASS_EXPIRY_TICKS: u64 = 60_000;

/// Break-glass state machine.
pub struct BreakGlass {
    /// Whether break-glass is currently active.
    active: bool,
    /// Tick at which break-glass was activated.
    activated_tick: u64,
    /// Tick at which break-glass expires.
    expiry_tick: u64,
    /// Reason for activation.
    reason: BreakGlassReason,
    /// Number of operations that bypassed governance during this activation.
    operations_logged: u32,
    /// Total activations since boot (for audit).
    total_activations: u32,
}

impl BreakGlass {
    pub const fn new() -> Self {
        Self {
            active: false,
            activated_tick: 0,
            expiry_tick: 0,
            reason: BreakGlassReason::SafetyLockdown,
            operations_logged: 0,
            total_activations: 0,
        }
    }

    /// Check conditions and activate break-glass if needed.
    /// Returns true if break-glass was just activated.
    pub fn check_and_activate(
        &mut self,
        safety: SafetyState,
        acs: AcsState,
        current_tick: u64,
    ) -> bool {
        // Already active — check for expiry
        if self.active {
            if current_tick >= self.expiry_tick {
                self.deactivate();
            }
            return false;
        }

        // Activation conditions: Lockdown + Emergency ACS
        let should_activate = safety == SafetyState::Lockdown && acs == AcsState::Emergency;

        if should_activate {
            self.activate(BreakGlassReason::SafetyLockdown, current_tick);
            true
        } else {
            false
        }
    }

    /// Manually activate break-glass with a specific reason.
    pub fn activate(&mut self, reason: BreakGlassReason, current_tick: u64) {
        self.active = true;
        self.activated_tick = current_tick;
        self.expiry_tick = current_tick + BREAK_GLASS_EXPIRY_TICKS;
        self.reason = reason;
        self.operations_logged = 0;
        self.total_activations += 1;
    }

    /// Deactivate break-glass (either by expiry or safety recovery).
    pub fn deactivate(&mut self) {
        self.active = false;
    }

    /// Check if break-glass is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Record an operation that bypassed governance during break-glass.
    pub fn log_operation(&mut self) {
        self.operations_logged += 1;
    }

    /// Check expiry and deactivate if expired.
    pub fn check_expiry(&mut self, current_tick: u64) {
        if self.active && current_tick >= self.expiry_tick {
            self.deactivate();
        }
    }

    /// Check if safety recovered (Normal or Elevated) and deactivate.
    pub fn check_recovery(&mut self, safety: SafetyState) {
        if self.active && (safety == SafetyState::Normal || safety == SafetyState::Elevated) {
            self.deactivate();
        }
    }

    /// Get the reason for current activation.
    pub fn reason(&self) -> BreakGlassReason {
        self.reason
    }

    /// Get operations logged during current activation.
    pub fn operations_logged(&self) -> u32 {
        self.operations_logged
    }

    /// Get total activations since boot.
    pub fn total_activations(&self) -> u32 {
        self.total_activations
    }

    /// Get remaining ticks until expiry (0 if not active).
    pub fn remaining_ticks(&self, current_tick: u64) -> u64 {
        if self.active && current_tick < self.expiry_tick {
            self.expiry_tick - current_tick
        } else {
            0
        }
    }

    /// Reset state (for testing).
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}
