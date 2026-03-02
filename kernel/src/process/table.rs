//! Process table — BTreeMap-backed registry of all live process control blocks.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use fabric_types::ProcessId;
use super::pcb::ProcessControlBlock;

/// Maximum simultaneous processes.
pub const MAX_PROCESSES: usize = 1024;

/// Error types for process operations.
#[derive(Debug)]
pub enum ProcessError {
    TableFull,
    NotFound,
    AlreadyExists,
    InvalidState,
    SupervisorNotFound,
    MaxChildrenReached,
    IntensityExceeded,
    NotSupervisor,
    InvalidPid,
    BusRegistrationFailed,
}

/// The process table holding all live PCBs.
pub struct ProcessTable {
    processes: BTreeMap<u32, ProcessControlBlock>,
    next_pid: u32,
}

impl ProcessTable {
    pub const fn new() -> Self {
        Self {
            processes: BTreeMap::new(),
            next_pid: 1,
        }
    }

    /// Allocate the next unique PID.
    pub fn alloc_pid(&mut self) -> u32 {
        let pid = self.next_pid;
        self.next_pid += 1;
        pid
    }

    /// Insert a PCB into the table.
    pub fn insert(&mut self, pcb: ProcessControlBlock) -> Result<(), ProcessError> {
        if self.processes.len() >= MAX_PROCESSES {
            return Err(ProcessError::TableFull);
        }
        if self.processes.contains_key(&pcb.pid.0) {
            return Err(ProcessError::AlreadyExists);
        }
        self.processes.insert(pcb.pid.0, pcb);
        Ok(())
    }

    /// Get an immutable reference to a PCB.
    pub fn get(&self, pid: ProcessId) -> Option<&ProcessControlBlock> {
        self.processes.get(&pid.0)
    }

    /// Get a mutable reference to a PCB.
    pub fn get_mut(&mut self, pid: ProcessId) -> Option<&mut ProcessControlBlock> {
        self.processes.get_mut(&pid.0)
    }

    /// Remove a PCB from the table.
    pub fn remove(&mut self, pid: ProcessId) -> Option<ProcessControlBlock> {
        self.processes.remove(&pid.0)
    }

    /// Check if a process exists.
    pub fn contains(&self, pid: ProcessId) -> bool {
        self.processes.contains_key(&pid.0)
    }

    /// Number of live processes.
    pub fn count(&self) -> usize {
        self.processes.len()
    }

    /// Iterate over all PIDs.
    pub fn pids(&self) -> impl Iterator<Item = ProcessId> + '_ {
        self.processes.keys().map(|&k| ProcessId::new(k))
    }

    /// Clear the entire table (for testing).
    pub fn clear(&mut self) {
        self.processes.clear();
        self.next_pid = 1;
    }
}
