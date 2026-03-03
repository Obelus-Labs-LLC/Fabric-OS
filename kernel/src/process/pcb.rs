//! Process Control Block — kernel-internal process state.
//!
//! The PCB extends the shared wire types from `fabric_types::process` with
//! kernel-only fields: behavioral profile, supervision tree links, scheduling
//! metadata, capability references, handle table, and address space.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use fabric_types::{
    CapabilityId, Intent, ProcessId, ProcessState, SupervisionStrategy, Timestamp,
};

use super::supervisor::RestartTracker;
use crate::handle::HandleTable;
use crate::address_space::AddressSpace;
use crate::memory::VirtAddr;

/// Exit reason when a process terminates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    Normal,
    Crash,
    Killed,
    IntensityExceeded,
}

/// Behavioral telemetry tracked by the kernel.
/// All values are fixed-point u32 (no FPU). Values marked *1000 are
/// scaled — e.g. anomaly_score=500 means 0.5.
pub struct BehavioralProfile {
    pub total_ticks_run: u64,
    pub total_bursts: u64,
    pub avg_burst_ticks: u32,
    pub total_messages_sent: u64,
    pub total_memory_allocated: u64,
    pub anomaly_score: u32,
    pub last_updated: Timestamp,
}

impl BehavioralProfile {
    pub const fn new() -> Self {
        Self {
            total_ticks_run: 0,
            total_bursts: 0,
            avg_burst_ticks: 0,
            total_messages_sent: 0,
            total_memory_allocated: 0,
            anomaly_score: 0,
            last_updated: Timestamp::ZERO,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// The full process control block — kernel-internal only.
pub struct ProcessControlBlock {
    // Identity
    pub pid: ProcessId,
    pub intent: Intent,
    pub description: String,
    pub state: ProcessState,

    // Scheduling
    pub effective_priority: u8,
    pub base_priority: u8,
    pub time_slice_remaining: u32,
    pub total_ticks_run: u64,
    pub blocked_on: Option<ProcessId>,
    pub last_scheduled: Timestamp,
    pub wake_tick: Timestamp,

    // Capabilities
    pub capabilities: Vec<CapabilityId>,

    // Supervision tree
    pub supervisor: ProcessId,
    pub children: Vec<ProcessId>,
    pub strategy: SupervisionStrategy,
    pub spawn_order: u32,
    pub restart_tracker: RestartTracker,

    // Behavioral profile
    pub profile: BehavioralProfile,

    // Lifecycle
    pub exit_reason: Option<ExitReason>,
    pub created_at: Timestamp,

    // Phase 6: Per-process handle table (fixed 256-slot slab, no heap)
    pub handle_table: HandleTable,

    // Phase 6: Per-process address space (PML4 page table)
    pub address_space: Option<AddressSpace>,

    // Phase 7: CPU context for preemptive scheduling and Ring 3 execution.
    /// Saved RSP pointing to a SavedContext on the kernel stack.
    pub saved_rsp: u64,
    /// Base of the per-process kernel stack (2 pages). None for kernel-only processes.
    pub kernel_stack_base: Option<VirtAddr>,
    /// Top of the per-process kernel stack (RSP0 for Ring 3→0 transitions).
    pub kernel_stack_top: u64,
    /// Whether this process has been scheduled at least once.
    pub has_run: bool,
    /// Whether this is a userspace process (Ring 3).
    pub is_user: bool,
    /// Exit code from sys_exit (Phase 7).
    pub exit_code: u64,
}

impl ProcessControlBlock {
    pub fn new(
        pid: ProcessId,
        intent: Intent,
        description: String,
        supervisor: ProcessId,
        strategy: SupervisionStrategy,
        spawn_order: u32,
        created_at: Timestamp,
    ) -> Self {
        let base_priority = intent.priority as u8;
        Self {
            pid,
            intent,
            description,
            state: ProcessState::Ready,
            effective_priority: base_priority,
            base_priority,
            time_slice_remaining: 0,
            total_ticks_run: 0,
            blocked_on: None,
            last_scheduled: Timestamp::ZERO,
            wake_tick: Timestamp::ZERO,
            capabilities: Vec::new(),
            supervisor,
            children: Vec::new(),
            strategy,
            spawn_order,
            restart_tracker: RestartTracker::default(),
            profile: BehavioralProfile::new(),
            exit_reason: None,
            created_at,
            handle_table: HandleTable::new(),
            address_space: None,
            saved_rsp: 0,
            kernel_stack_base: None,
            kernel_stack_top: 0,
            has_run: false,
            is_user: false,
            exit_code: 0,
        }
    }

    /// Reset a process for restart (preserves identity and supervision links).
    pub fn reset_for_restart(&mut self, now: Timestamp) {
        self.state = ProcessState::Ready;
        self.effective_priority = self.base_priority;
        self.time_slice_remaining = 0;
        self.blocked_on = None;
        self.last_scheduled = Timestamp::ZERO;
        self.wake_tick = Timestamp::ZERO;
        self.profile.reset();
        self.exit_reason = None;
        self.created_at = now;
        self.handle_table.clear();
        // Address space is preserved across restarts (same process identity)
        // Phase 7: Reset execution context but preserve kernel stack allocation
        self.saved_rsp = 0;
        self.has_run = false;
        // kernel_stack_base/kernel_stack_top/is_user preserved (same stack reused)
    }
}
