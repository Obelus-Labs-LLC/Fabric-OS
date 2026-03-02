//! Safety State Machine — 5 states with tick-based de-escalation.
//!
//! States: Normal(0) → Elevated(1) → Chaos(3) → Lockdown(4)
//! Recovery: Lockdown → Safe(2) → Normal(0)
//!
//! Transitions:
//!   Normal → Elevated: anomaly burst (3+ anomalies in window)
//!   Elevated → Chaos: alarm burst (3+ alarms in window)
//!   Elevated → Normal: 5 min clear (300_000 ticks @ 1kHz)
//!   Any → Lockdown: force_lockdown()
//!   Lockdown → Safe: human_confirm()
//!   Safe → Normal: 30 min burn-down (1_800_000 ticks @ 1kHz)
//!   Chaos → Lockdown: manual or auto

#![allow(dead_code)]

use fabric_types::governance::SafetyState;

/// Tick thresholds (at 1kHz tick rate).
const ELEVATED_CLEAR_TICKS: u64 = 300_000;     // 5 minutes
const SAFE_BURNDOWN_TICKS: u64 = 1_800_000;    // 30 minutes
const ANOMALY_WINDOW_TICKS: u64 = 60_000;       // 1 minute window for anomaly burst
const ANOMALY_BURST_THRESHOLD: u32 = 3;
const ALARM_BURST_THRESHOLD: u32 = 3;

pub struct SafetyStateMachine {
    state: SafetyState,
    /// Tick at which the current state was entered.
    state_entered_tick: u64,
    /// Anomaly counter within current window.
    anomaly_count: u32,
    /// Tick at which anomaly window started.
    anomaly_window_start: u64,
    /// Alarm counter for Chaos escalation.
    alarm_count: u32,
    /// Total transitions (for diagnostics).
    total_transitions: u64,
}

impl SafetyStateMachine {
    pub const fn new() -> Self {
        Self {
            state: SafetyState::Normal,
            state_entered_tick: 0,
            anomaly_count: 0,
            anomaly_window_start: 0,
            alarm_count: 0,
            total_transitions: 0,
        }
    }

    /// Get current safety state.
    pub fn state(&self) -> SafetyState {
        self.state
    }

    /// Get total transitions.
    pub fn total_transitions(&self) -> u64 {
        self.total_transitions
    }

    /// Tick-based de-escalation check. Call each tick.
    pub fn tick(&mut self, current_tick: u64) {
        match self.state {
            SafetyState::Elevated => {
                if current_tick.saturating_sub(self.state_entered_tick) >= ELEVATED_CLEAR_TICKS {
                    self.transition_to(SafetyState::Normal, current_tick);
                }
            }
            SafetyState::Safe => {
                if current_tick.saturating_sub(self.state_entered_tick) >= SAFE_BURNDOWN_TICKS {
                    self.transition_to(SafetyState::Normal, current_tick);
                }
            }
            _ => {}
        }
    }

    /// Report an anomaly. May escalate Normal → Elevated.
    pub fn report_anomaly(&mut self, current_tick: u64) {
        // Reset window if expired
        if current_tick.saturating_sub(self.anomaly_window_start) >= ANOMALY_WINDOW_TICKS {
            self.anomaly_count = 0;
            self.anomaly_window_start = current_tick;
        }

        self.anomaly_count += 1;

        if self.state == SafetyState::Normal && self.anomaly_count >= ANOMALY_BURST_THRESHOLD {
            self.transition_to(SafetyState::Elevated, current_tick);
            self.anomaly_count = 0;
        }
    }

    /// Report an alarm. May escalate Elevated → Chaos.
    pub fn report_alarm(&mut self, current_tick: u64) {
        self.alarm_count += 1;

        if self.state == SafetyState::Elevated && self.alarm_count >= ALARM_BURST_THRESHOLD {
            self.transition_to(SafetyState::Chaos, current_tick);
            self.alarm_count = 0;
        }
    }

    /// Force transition to Lockdown from any state.
    pub fn force_lockdown(&mut self, current_tick: u64) {
        self.transition_to(SafetyState::Lockdown, current_tick);
    }

    /// Human confirmation: Lockdown → Safe.
    /// Returns false if not currently in Lockdown.
    pub fn human_confirm(&mut self, current_tick: u64) -> bool {
        if self.state == SafetyState::Lockdown {
            self.transition_to(SafetyState::Safe, current_tick);
            true
        } else {
            false
        }
    }

    /// Force a specific state (for testing only).
    pub fn force_state(&mut self, state: SafetyState, current_tick: u64) {
        self.transition_to(state, current_tick);
    }

    /// Reset to Normal (for testing between OCRB tests).
    pub fn reset(&mut self) {
        self.state = SafetyState::Normal;
        self.state_entered_tick = 0;
        self.anomaly_count = 0;
        self.anomaly_window_start = 0;
        self.alarm_count = 0;
        self.total_transitions = 0;
    }

    fn transition_to(&mut self, new_state: SafetyState, current_tick: u64) {
        if self.state != new_state {
            self.state = new_state;
            self.state_entered_tick = current_tick;
            self.total_transitions += 1;
        }
    }
}
