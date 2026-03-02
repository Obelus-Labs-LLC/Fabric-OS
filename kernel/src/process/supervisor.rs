//! Erlang-style supervision tree — crash handling, restart strategies,
//! and restart intensity tracking with escalation.

#![allow(dead_code)]

use alloc::vec::Vec;
use fabric_types::{ProcessId, ProcessState, SupervisionStrategy, Timestamp};

use super::pcb::ExitReason;
use super::scheduler::Scheduler;
use super::table::{ProcessError, ProcessTable};

/// Default restart intensity: 5 restarts in 60 ticks.
pub const DEFAULT_MAX_RESTARTS: u32 = 5;
pub const DEFAULT_INTENSITY_WINDOW: u64 = 60;

/// Tracks restart timestamps for intensity limiting.
pub struct RestartTracker {
    pub max_restarts: u32,
    pub window_ticks: u64,
    pub restart_timestamps: Vec<Timestamp>,
}

impl RestartTracker {
    pub fn new(max_restarts: u32, window_ticks: u64) -> Self {
        Self {
            max_restarts,
            window_ticks,
            restart_timestamps: Vec::with_capacity(max_restarts as usize + 1),
        }
    }

    /// Record a restart and return true if intensity has been **exceeded**.
    pub fn record_restart(&mut self, now: Timestamp) -> bool {
        // Prune timestamps outside the window
        let window = self.window_ticks;
        self.restart_timestamps
            .retain(|ts| now.0.saturating_sub(ts.0) < window);
        self.restart_timestamps.push(now);
        self.restart_timestamps.len() > self.max_restarts as usize
    }

    /// Count of recent restarts within the window.
    pub fn recent_count(&self, now: Timestamp) -> u32 {
        self.restart_timestamps
            .iter()
            .filter(|ts| now.0.saturating_sub(ts.0) < self.window_ticks)
            .count() as u32
    }

    pub fn clear(&mut self) {
        self.restart_timestamps.clear();
    }
}

impl Default for RestartTracker {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_RESTARTS, DEFAULT_INTENSITY_WINDOW)
    }
}

/// Handle a child crash according to the supervisor's strategy.
///
/// This is the core supervision logic. It:
/// 1. Checks restart intensity — exceeded → escalate (terminate supervisor)
/// 2. Applies the supervision strategy (OneForOne, OneForAll, RestForOne)
/// 3. Restarts the appropriate children
///
/// Returns Ok(restarted_count) or Err(IntensityExceeded).
pub fn handle_child_crash(
    supervisor_pid: ProcessId,
    crashed_pid: ProcessId,
    now: Timestamp,
    table: &mut ProcessTable,
    scheduler: &mut Scheduler,
) -> Result<u32, ProcessError> {
    // Get supervisor's strategy and check intensity
    let (strategy, exceeded) = {
        let sup = table.get_mut(supervisor_pid).ok_or(ProcessError::SupervisorNotFound)?;
        let exceeded = sup.restart_tracker.record_restart(now);
        (sup.strategy, exceeded)
    };

    if exceeded {
        // Intensity exceeded — terminate supervisor and escalate
        let parent_pid = {
            let sup = table.get_mut(supervisor_pid).ok_or(ProcessError::SupervisorNotFound)?;
            sup.state = ProcessState::Terminated;
            sup.exit_reason = Some(ExitReason::IntensityExceeded);
            scheduler.dequeue(supervisor_pid);
            sup.supervisor
        };

        // If supervisor has a parent (not KERNEL), escalate
        if parent_pid != ProcessId::KERNEL {
            // Recursively handle the escalation
            let _ = handle_child_crash(parent_pid, supervisor_pid, now, table, scheduler);
        }

        return Err(ProcessError::IntensityExceeded);
    }

    // Apply strategy
    let mut restarted = 0u32;

    match strategy {
        SupervisionStrategy::OneForOne => {
            // Restart only the crashed child
            restart_process(crashed_pid, now, table, scheduler);
            restarted = 1;
        }

        SupervisionStrategy::OneForAll => {
            // Restart all children of this supervisor
            let children: Vec<ProcessId> = {
                let sup = table.get(supervisor_pid).ok_or(ProcessError::SupervisorNotFound)?;
                sup.children.clone()
            };

            for &child_pid in &children {
                // Terminate if not already terminated
                if let Some(pcb) = table.get_mut(child_pid) {
                    if pcb.state != ProcessState::Terminated {
                        pcb.state = ProcessState::Terminated;
                        pcb.exit_reason = Some(ExitReason::Killed);
                        scheduler.dequeue(child_pid);
                    }
                }
            }

            // Now restart all
            for &child_pid in &children {
                restart_process(child_pid, now, table, scheduler);
                restarted += 1;
            }
        }

        SupervisionStrategy::RestForOne => {
            // Find the crashed child's spawn_order
            let crashed_order = table
                .get(crashed_pid)
                .map(|pcb| pcb.spawn_order)
                .unwrap_or(0);

            // Get children with spawn_order >= crashed
            let to_restart: Vec<ProcessId> = {
                let sup = table.get(supervisor_pid).ok_or(ProcessError::SupervisorNotFound)?;
                sup.children
                    .iter()
                    .filter_map(|&child_pid| {
                        table.get(child_pid).and_then(|pcb| {
                            if pcb.spawn_order >= crashed_order {
                                Some(child_pid)
                            } else {
                                None
                            }
                        })
                    })
                    .collect()
            };

            // Terminate affected children first
            for &child_pid in &to_restart {
                if let Some(pcb) = table.get_mut(child_pid) {
                    if pcb.state != ProcessState::Terminated {
                        pcb.state = ProcessState::Terminated;
                        pcb.exit_reason = Some(ExitReason::Killed);
                        scheduler.dequeue(child_pid);
                    }
                }
            }

            // Restart them
            for &child_pid in &to_restart {
                restart_process(child_pid, now, table, scheduler);
                restarted += 1;
            }
        }
    }

    Ok(restarted)
}

/// Restart a single process: reset state, re-enqueue in scheduler.
fn restart_process(
    pid: ProcessId,
    now: Timestamp,
    table: &mut ProcessTable,
    scheduler: &mut Scheduler,
) {
    if let Some(pcb) = table.get_mut(pid) {
        pcb.reset_for_restart(now);
        scheduler.enqueue(pid, pcb.effective_priority);
    }
}
