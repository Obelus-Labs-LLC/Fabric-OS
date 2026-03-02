//! Intent-aware scheduler with multi-level priority queues.
//!
//! Five priority levels (Background..Critical). Higher levels drain first.
//! Round-robin within a level. Aging prevents starvation. Intent-aware
//! boosts for I/O-waking and deadline-approaching processes.

#![allow(dead_code)]

use alloc::vec::Vec;
use fabric_types::{IntentCategory, Priority, ProcessId, ProcessState, Timestamp};

use super::pcb::ProcessControlBlock;
use super::table::ProcessTable;

/// Time slice per priority level (in ticks).
pub const SLICE_CRITICAL: u32   = 20;
pub const SLICE_HIGH: u32       = 15;
pub const SLICE_NORMAL: u32     = 10;
pub const SLICE_LOW: u32        = 5;
pub const SLICE_BACKGROUND: u32 = 2;

/// After this many ticks without being scheduled, a process gets a +1 boost.
pub const AGING_INTERVAL: u64 = 100;

/// Deadline urgency: within this many ticks of deadline, boost to Critical.
pub const DEADLINE_URGENCY: u64 = 10;

/// Number of priority levels.
const NUM_LEVELS: usize = 5;

/// Get time slice for a given priority level.
pub fn slice_for_priority(p: u8) -> u32 {
    match p {
        4 => SLICE_CRITICAL,
        3 => SLICE_HIGH,
        2 => SLICE_NORMAL,
        1 => SLICE_LOW,
        _ => SLICE_BACKGROUND,
    }
}

/// The scheduler manages run queues and selects the next process to run.
pub struct Scheduler {
    run_queues: [Vec<ProcessId>; NUM_LEVELS],
    current: Option<ProcessId>,
    current_tick: Timestamp,
    total_decisions: u64,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            run_queues: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            current: None,
            current_tick: Timestamp::ZERO,
            total_decisions: 0,
        }
    }

    /// Advance the monotonic tick counter.
    pub fn tick(&mut self) {
        self.current_tick = Timestamp(self.current_tick.0 + 1);
    }

    /// Advance by N ticks.
    pub fn advance_ticks(&mut self, n: u64) {
        self.current_tick = Timestamp(self.current_tick.0 + n);
    }

    /// Get current tick.
    pub fn current_tick(&self) -> Timestamp {
        self.current_tick
    }

    /// Get the currently running process.
    pub fn current(&self) -> Option<ProcessId> {
        self.current
    }

    /// Get total scheduling decisions made.
    pub fn total_decisions(&self) -> u64 {
        self.total_decisions
    }

    /// Enqueue a process into the appropriate run queue.
    pub fn enqueue(&mut self, pid: ProcessId, effective_priority: u8) {
        let level = (effective_priority as usize).min(NUM_LEVELS - 1);
        // Don't double-enqueue
        if !self.run_queues[level].contains(&pid) {
            self.run_queues[level].push(pid);
        }
    }

    /// Remove a process from all run queues.
    pub fn dequeue(&mut self, pid: ProcessId) {
        for queue in &mut self.run_queues {
            queue.retain(|&p| p != pid);
        }
        if self.current == Some(pid) {
            self.current = None;
        }
    }

    /// Select the next process to run. Scans from Critical down to Background.
    /// Returns the selected PID and assigns it as `current`.
    pub fn schedule_next(&mut self, table: &mut ProcessTable) -> Option<ProcessId> {
        // If current has time_slice_remaining and is still Running, keep it
        if let Some(current_pid) = self.current {
            if let Some(pcb) = table.get(current_pid) {
                if pcb.state == ProcessState::Running && pcb.time_slice_remaining > 0 {
                    return Some(current_pid);
                }
            }
        }

        // Move current back to its queue if it was running
        if let Some(current_pid) = self.current.take() {
            if let Some(pcb) = table.get_mut(current_pid) {
                if pcb.state == ProcessState::Running {
                    pcb.state = ProcessState::Ready;
                    let level = (pcb.effective_priority as usize).min(NUM_LEVELS - 1);
                    if !self.run_queues[level].contains(&current_pid) {
                        self.run_queues[level].push(current_pid);
                    }
                }
            }
        }

        // Scan from highest priority down
        for level in (0..NUM_LEVELS).rev() {
            if let Some(pid) = self.run_queues[level].first().copied() {
                self.run_queues[level].remove(0);

                if let Some(pcb) = table.get_mut(pid) {
                    pcb.state = ProcessState::Running;
                    pcb.time_slice_remaining = slice_for_priority(pcb.effective_priority);
                    pcb.last_scheduled = self.current_tick;
                    self.current = Some(pid);
                    self.total_decisions += 1;
                    return Some(pid);
                }
            }
        }

        None // Idle
    }

    /// Apply aging: boost starved processes by +1 priority level.
    pub fn run_aging(&mut self, table: &mut ProcessTable) {
        let now = self.current_tick.0;
        // Collect candidates first to avoid borrow issues
        let mut to_boost: Vec<(ProcessId, u8, u8)> = Vec::new();

        for level in 0..(NUM_LEVELS - 1) {
            for &pid in &self.run_queues[level] {
                if let Some(pcb) = table.get(pid) {
                    if pcb.state == ProcessState::Ready {
                        let idle_ticks = now.saturating_sub(pcb.last_scheduled.0);
                        if idle_ticks >= AGING_INTERVAL && (pcb.effective_priority as usize) < NUM_LEVELS - 1 {
                            let new_priority = pcb.effective_priority + 1;
                            to_boost.push((pid, level as u8, new_priority));
                        }
                    }
                }
            }
        }

        for (pid, old_level, new_priority) in to_boost {
            // Remove from old queue
            self.run_queues[old_level as usize].retain(|&p| p != pid);
            // Update PCB
            if let Some(pcb) = table.get_mut(pid) {
                pcb.effective_priority = new_priority;
            }
            // Add to new queue
            let new_level = (new_priority as usize).min(NUM_LEVELS - 1);
            self.run_queues[new_level].push(pid);
        }
    }

    /// Apply intent-aware scheduling boosts.
    /// Called when a process transitions to Ready (e.g., after unblock).
    pub fn apply_intent_boost(&self, pcb: &mut ProcessControlBlock) {
        let now = self.current_tick.0;

        // Background category: capped at Low
        if pcb.intent.category == IntentCategory::Compute
            && pcb.intent.priority == Priority::Background
        {
            pcb.effective_priority = pcb.effective_priority.min(Priority::Low as u8);
        }

        // I/O waking boost: Io/Network/Storage get +1 after unblocking
        match pcb.intent.category {
            IntentCategory::Io | IntentCategory::Network | IntentCategory::Storage => {
                if pcb.wake_tick.0 == now {
                    pcb.effective_priority = (pcb.effective_priority + 1).min(Priority::Critical as u8);
                }
            }
            _ => {}
        }

        // Display boost: +1 for first 5 ticks after waking
        if pcb.intent.category == IntentCategory::Display {
            if now.saturating_sub(pcb.wake_tick.0) < 5 {
                pcb.effective_priority = (pcb.effective_priority + 1).min(Priority::Critical as u8);
            }
        }

        // Deadline urgency: boost to Critical if within DEADLINE_URGENCY ticks
        if pcb.intent.deadline.0 > 0 {
            let remaining = pcb.intent.deadline.0.saturating_sub(now);
            if remaining <= DEADLINE_URGENCY {
                pcb.effective_priority = Priority::Critical as u8;
            }
        }
    }

    /// Consume one tick from the current process's time slice.
    /// Returns true if the process still has slice remaining.
    pub fn consume_tick(&mut self, table: &mut ProcessTable) -> bool {
        if let Some(pid) = self.current {
            if let Some(pcb) = table.get_mut(pid) {
                if pcb.time_slice_remaining > 0 {
                    pcb.time_slice_remaining -= 1;
                    pcb.total_ticks_run += 1;
                    pcb.profile.total_ticks_run += 1;
                    return pcb.time_slice_remaining > 0;
                }
            }
        }
        false
    }

    /// Clear all scheduler state (for testing).
    pub fn clear(&mut self) {
        for queue in &mut self.run_queues {
            queue.clear();
        }
        self.current = None;
        self.current_tick = Timestamp::ZERO;
        self.total_decisions = 0;
    }

    /// Count of processes across all run queues.
    pub fn queued_count(&self) -> usize {
        self.run_queues.iter().map(|q| q.len()).sum()
    }

    /// Get the queue contents for a specific priority level (for testing).
    pub fn queue_at(&self, level: u8) -> &[ProcessId] {
        let idx = (level as usize).min(NUM_LEVELS - 1);
        &self.run_queues[idx]
    }
}
