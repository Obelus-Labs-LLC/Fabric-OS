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

/// Hysteresis: weighted signal window for multi-signal escalation.
const HYSTERESIS_WINDOW_TICKS: u64 = 1_000;     // 1 second window
const HYSTERESIS_ELEVATED_THRESHOLD: u32 = 150;  // Weighted sum to trigger Normal→Elevated
const HYSTERESIS_CHAOS_THRESHOLD: u32 = 300;      // Weighted sum to trigger Elevated→Chaos
const HYSTERESIS_INSTANT_THRESHOLD: u32 = 80;     // Single signal weight that triggers instant escalation
const TRANSITION_COOLDOWN_TICKS: u64 = 5_000;    // 5 second cooldown between escalations

/// Signal types for weighted hysteresis scoring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SafetySignal {
    /// Memory integrity failure (hash mismatch). Weight: 100
    MemoryIntegrity = 0,
    /// Instruction pointer anomaly (code hijack). Weight: 90
    InstructionPointer = 1,
    /// ACS heartbeat timeout. Weight: 80
    AcsTimeout = 2,
    /// Capability validation failure burst. Weight: 60
    CapabilityFailure = 3,
    /// Council drift detection. Weight: 40
    CouncilDrift = 4,
    /// I/O frequency anomaly. Weight: 20
    IoAnomaly = 5,
}

impl SafetySignal {
    /// Get the hysteresis weight for this signal type.
    pub const fn weight(self) -> u32 {
        match self {
            SafetySignal::MemoryIntegrity => 100,
            SafetySignal::InstructionPointer => 90,
            SafetySignal::AcsTimeout => 80,
            SafetySignal::CapabilityFailure => 60,
            SafetySignal::CouncilDrift => 40,
            SafetySignal::IoAnomaly => 20,
        }
    }
}

/// Maximum number of signals tracked in the hysteresis window.
const MAX_SIGNALS: usize = 32;

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
    /// Hysteresis signal buffer: (tick, weight) pairs.
    signal_buffer: [(u64, u32); MAX_SIGNALS],
    /// Number of signals in the buffer.
    signal_count: usize,
    /// Tick of last escalation (for cooldown).
    last_escalation_tick: u64,
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
            signal_buffer: [(0, 0); MAX_SIGNALS],
            signal_count: 0,
            last_escalation_tick: 0,
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

    /// Report a weighted safety signal (hysteresis-based escalation).
    ///
    /// Signals are accumulated in a sliding window. If the weighted sum
    /// exceeds the threshold, the state escalates. A single signal with
    /// weight >= 80 triggers instant escalation. Cooldown prevents flapping.
    pub fn report_signal(&mut self, signal: SafetySignal, current_tick: u64) {
        let weight = signal.weight();

        // Instant escalation for critical signals
        if weight >= HYSTERESIS_INSTANT_THRESHOLD {
            if self.state == SafetyState::Normal {
                if current_tick.saturating_sub(self.last_escalation_tick) >= TRANSITION_COOLDOWN_TICKS {
                    self.transition_to(SafetyState::Elevated, current_tick);
                    self.last_escalation_tick = current_tick;
                }
            } else if self.state == SafetyState::Elevated {
                if current_tick.saturating_sub(self.last_escalation_tick) >= TRANSITION_COOLDOWN_TICKS {
                    self.transition_to(SafetyState::Chaos, current_tick);
                    self.last_escalation_tick = current_tick;
                }
            }
            return;
        }

        // Evict expired signals from the window
        let window_start = current_tick.saturating_sub(HYSTERESIS_WINDOW_TICKS);
        let mut write_idx = 0;
        for read_idx in 0..self.signal_count {
            if self.signal_buffer[read_idx].0 >= window_start {
                self.signal_buffer[write_idx] = self.signal_buffer[read_idx];
                write_idx += 1;
            }
        }
        self.signal_count = write_idx;

        // Add new signal
        if self.signal_count < MAX_SIGNALS {
            self.signal_buffer[self.signal_count] = (current_tick, weight);
            self.signal_count += 1;
        }

        // Compute weighted sum within window
        let mut weighted_sum: u32 = 0;
        for i in 0..self.signal_count {
            weighted_sum = weighted_sum.saturating_add(self.signal_buffer[i].1);
        }

        // Check escalation thresholds (with cooldown)
        if current_tick.saturating_sub(self.last_escalation_tick) >= TRANSITION_COOLDOWN_TICKS {
            if self.state == SafetyState::Normal && weighted_sum >= HYSTERESIS_ELEVATED_THRESHOLD {
                self.transition_to(SafetyState::Elevated, current_tick);
                self.last_escalation_tick = current_tick;
                self.signal_count = 0; // Reset window after escalation
            } else if self.state == SafetyState::Elevated && weighted_sum >= HYSTERESIS_CHAOS_THRESHOLD {
                self.transition_to(SafetyState::Chaos, current_tick);
                self.last_escalation_tick = current_tick;
                self.signal_count = 0;
            }
        }
    }

    /// Get the current weighted signal sum (for diagnostics/testing).
    pub fn signal_weighted_sum(&self, current_tick: u64) -> u32 {
        let window_start = current_tick.saturating_sub(HYSTERESIS_WINDOW_TICKS);
        let mut sum: u32 = 0;
        for i in 0..self.signal_count {
            if self.signal_buffer[i].0 >= window_start {
                sum = sum.saturating_add(self.signal_buffer[i].1);
            }
        }
        sum
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
        self.signal_buffer = [(0, 0); MAX_SIGNALS];
        self.signal_count = 0;
        self.last_escalation_tick = 0;
    }

    fn transition_to(&mut self, new_state: SafetyState, current_tick: u64) {
        if self.state != new_state {
            self.state = new_state;
            self.state_entered_tick = current_tick;
            self.total_transitions += 1;
        }
    }
}
