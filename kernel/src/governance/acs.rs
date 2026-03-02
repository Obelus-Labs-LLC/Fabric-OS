//! Authority Continuity System (ACS) — heartbeat-based lifecycle with dead-man switch.
//!
//! States: Active(0) → Degraded(1) → Contingency(2) → Emergency(3)
//!
//! Transitions (tick-based):
//!   Active → Degraded: missed heartbeat (1h = 3_600_000 ticks)
//!   Degraded → Contingency: 2h total without heartbeat (if alternate exists)
//!   Contingency → Emergency: 4h total without heartbeat
//!   Emergency: triggers safety Lockdown via callback
//!
//! Recovery: primary heartbeat restores Active from any state.

#![allow(dead_code)]

use fabric_types::governance::AcsState;

/// Tick thresholds (at 1kHz tick rate).
const DEGRADED_THRESHOLD_TICKS: u64 = 3_600_000;   // 1 hour
const CONTINGENCY_THRESHOLD_TICKS: u64 = 7_200_000; // 2 hours
const EMERGENCY_THRESHOLD_TICKS: u64 = 14_400_000;  // 4 hours

pub struct AcsStateMachine {
    state: AcsState,
    /// Tick of last heartbeat from primary authority.
    last_heartbeat_tick: u64,
    /// Whether an alternate authority exists (for Contingency transition).
    alternate_exists: bool,
    /// Total transitions (for diagnostics).
    total_transitions: u64,
    /// Whether Emergency was triggered (so caller can escalate safety).
    emergency_triggered: bool,
}

impl AcsStateMachine {
    pub const fn new() -> Self {
        Self {
            state: AcsState::Active,
            last_heartbeat_tick: 0,
            alternate_exists: false,
            total_transitions: 0,
            emergency_triggered: false,
        }
    }

    /// Get current ACS state.
    pub fn state(&self) -> AcsState {
        self.state
    }

    /// Get total transitions.
    pub fn total_transitions(&self) -> u64 {
        self.total_transitions
    }

    /// Check if emergency was triggered since last check, and clear the flag.
    pub fn take_emergency_trigger(&mut self) -> bool {
        let triggered = self.emergency_triggered;
        self.emergency_triggered = false;
        triggered
    }

    /// Set whether an alternate authority exists.
    pub fn set_alternate_exists(&mut self, exists: bool) {
        self.alternate_exists = exists;
    }

    /// Tick-based dead-man switch evaluation. Call each tick.
    pub fn tick(&mut self, current_tick: u64) {
        let elapsed = current_tick.saturating_sub(self.last_heartbeat_tick);

        match self.state {
            AcsState::Active => {
                if elapsed >= DEGRADED_THRESHOLD_TICKS {
                    self.transition_to(AcsState::Degraded, current_tick);
                }
            }
            AcsState::Degraded => {
                if elapsed >= CONTINGENCY_THRESHOLD_TICKS && self.alternate_exists {
                    self.transition_to(AcsState::Contingency, current_tick);
                } else if elapsed >= EMERGENCY_THRESHOLD_TICKS {
                    self.transition_to(AcsState::Emergency, current_tick);
                    self.emergency_triggered = true;
                }
            }
            AcsState::Contingency => {
                if elapsed >= EMERGENCY_THRESHOLD_TICKS {
                    self.transition_to(AcsState::Emergency, current_tick);
                    self.emergency_triggered = true;
                }
            }
            AcsState::Emergency => {
                // Already in emergency — no further degradation
            }
        }
    }

    /// Receive a heartbeat from the primary authority. Restores Active from any state.
    pub fn heartbeat(&mut self, current_tick: u64) {
        self.last_heartbeat_tick = current_tick;
        if self.state != AcsState::Active {
            self.transition_to(AcsState::Active, current_tick);
        }
    }

    /// Force a specific state (for testing).
    pub fn force_state(&mut self, state: AcsState, current_tick: u64) {
        self.transition_to(state, current_tick);
    }

    /// Reset to Active (for testing between OCRB tests).
    pub fn reset(&mut self) {
        self.state = AcsState::Active;
        self.last_heartbeat_tick = 0;
        self.alternate_exists = false;
        self.total_transitions = 0;
        self.emergency_triggered = false;
    }

    fn transition_to(&mut self, new_state: AcsState, _current_tick: u64) {
        if self.state != new_state {
            self.state = new_state;
            self.total_transitions += 1;
        }
    }
}
