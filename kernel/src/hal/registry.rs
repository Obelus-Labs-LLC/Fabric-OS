//! Driver Registry — maps devices to driver processes.
//!
//! Central lookup for device -> driver PID + trait object. The dispatch
//! loop uses this to route bus messages to the correct driver implementation.

#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use fabric_types::{ProcessId, ResourceId};
use fabric_types::device::DeviceClass;

use super::Driver;

/// Maximum registered drivers.
pub const MAX_DRIVERS: usize = 16;

/// A registered driver entry.
pub struct DriverEntry {
    pub pid: ProcessId,
    pub resource_id: ResourceId,
    pub device_class: DeviceClass,
    pub driver: Box<dyn Driver>,
    pub initialized: bool,
    /// Monotonic sequence counter for response messages from this driver.
    pub response_seq: u64,
}

/// Errors from registry operations.
#[derive(Debug)]
pub enum RegistryError {
    RegistryFull,
    AlreadyRegistered,
    NotFound,
    DriverInitFailed,
}

/// The driver registry.
pub struct DriverRegistry {
    entries: BTreeMap<u64, DriverEntry>,
}

impl DriverRegistry {
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Register a driver. Returns Err if registry full or resource already registered.
    pub fn register(
        &mut self,
        pid: ProcessId,
        resource_id: ResourceId,
        device_class: DeviceClass,
        driver: Box<dyn Driver>,
    ) -> Result<(), RegistryError> {
        if self.entries.len() >= MAX_DRIVERS {
            return Err(RegistryError::RegistryFull);
        }
        if self.entries.contains_key(&resource_id.0) {
            return Err(RegistryError::AlreadyRegistered);
        }
        self.entries.insert(resource_id.0, DriverEntry {
            pid,
            resource_id,
            device_class,
            driver,
            initialized: false,
            response_seq: 0,
        });
        Ok(())
    }

    /// Unregister a driver by resource ID.
    pub fn unregister(&mut self, resource_id: ResourceId) -> Option<DriverEntry> {
        self.entries.remove(&resource_id.0)
    }

    /// Look up a driver by resource ID.
    pub fn get(&self, resource_id: ResourceId) -> Option<&DriverEntry> {
        self.entries.get(&resource_id.0)
    }

    /// Get mutable reference to a driver by resource ID.
    pub fn get_mut(&mut self, resource_id: ResourceId) -> Option<&mut DriverEntry> {
        self.entries.get_mut(&resource_id.0)
    }

    /// Look up a driver by its ProcessId.
    pub fn get_by_pid(&self, pid: ProcessId) -> Option<&DriverEntry> {
        self.entries.values().find(|e| e.pid == pid)
    }

    /// Get mutable reference by ProcessId.
    pub fn get_by_pid_mut(&mut self, pid: ProcessId) -> Option<&mut DriverEntry> {
        self.entries.values_mut().find(|e| e.pid == pid)
    }

    /// Number of registered drivers.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Collect all (resource_key, pid) pairs (for iteration without holding borrow).
    pub fn all_pids(&self) -> Vec<(u64, ProcessId)> {
        self.entries.iter().map(|(&k, e)| (k, e.pid)).collect()
    }

    /// Get mutable iterator over all entries (for init loop).
    pub fn entries_mut(&mut self) -> impl Iterator<Item = &mut DriverEntry> {
        self.entries.values_mut()
    }

    /// Look up a driver by raw resource key (u64).
    /// Used by dispatch loop which snapshots keys via all_pids().
    pub fn get_by_resource_key_mut(&mut self, key: u64) -> Option<&mut DriverEntry> {
        self.entries.get_mut(&key)
    }

    /// Clear all entries (for testing).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
