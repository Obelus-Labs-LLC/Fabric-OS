//! Handle table — fixed-size slab for per-process handle → capability mapping.
//!
//! Each process owns a HandleTable with 256 slots. Handles are u64 values
//! encoding a slot index (bits 0-7) and generation counter (bits 8-23).
//! Generation counters prevent stale handle use after release.
//!
//! No heap allocation — the entire table is a fixed array in the PCB.

#![allow(dead_code)]

use fabric_types::HandleId;

/// Maximum handles per process.
pub const MAX_HANDLES: usize = 256;

/// Errors from handle operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum HandleError {
    /// No free slots available.
    TableFull,
    /// Handle index out of range.
    InvalidIndex,
    /// Handle generation mismatch (stale handle).
    StaleGeneration,
    /// Slot is not active.
    NotActive,
}

/// A single handle table entry.
#[derive(Clone, Copy)]
pub struct HandleEntry {
    /// The capability ID this handle maps to.
    cap_id: u64,
    /// Whether this slot is in use.
    active: bool,
    /// Generation counter — incremented on each release.
    generation: u16,
}

impl HandleEntry {
    const fn empty() -> Self {
        Self {
            cap_id: 0,
            active: false,
            generation: 0,
        }
    }
}

/// Per-process handle table. Fixed 256-slot slab, no heap allocation.
pub struct HandleTable {
    entries: [HandleEntry; MAX_HANDLES],
    count: usize,
}

impl HandleTable {
    pub const fn new() -> Self {
        Self {
            entries: [HandleEntry::empty(); MAX_HANDLES],
            count: 0,
        }
    }

    /// Allocate a handle for a capability ID. Returns the packed HandleId.
    pub fn alloc(&mut self, cap_id: u64) -> Result<HandleId, HandleError> {
        for i in 0..MAX_HANDLES {
            if !self.entries[i].active {
                self.entries[i].cap_id = cap_id;
                self.entries[i].active = true;
                // generation was set on last release (or 0 for fresh)
                self.count += 1;
                return Ok(HandleId::pack(i as u8, self.entries[i].generation));
            }
        }
        Err(HandleError::TableFull)
    }

    /// Resolve a handle to its capability ID.
    pub fn resolve(&self, handle: HandleId) -> Result<u64, HandleError> {
        let slot = handle.slot() as usize;
        if slot >= MAX_HANDLES {
            return Err(HandleError::InvalidIndex);
        }

        let entry = &self.entries[slot];
        if !entry.active {
            return Err(HandleError::NotActive);
        }
        if entry.generation != handle.generation() {
            return Err(HandleError::StaleGeneration);
        }

        Ok(entry.cap_id)
    }

    /// Release a handle, making its slot available for reuse.
    pub fn release(&mut self, handle: HandleId) -> Result<(), HandleError> {
        let slot = handle.slot() as usize;
        if slot >= MAX_HANDLES {
            return Err(HandleError::InvalidIndex);
        }

        let entry = &mut self.entries[slot];
        if !entry.active {
            return Err(HandleError::NotActive);
        }
        if entry.generation != handle.generation() {
            return Err(HandleError::StaleGeneration);
        }

        entry.active = false;
        entry.cap_id = 0;
        entry.generation = entry.generation.wrapping_add(1);
        self.count -= 1;

        Ok(())
    }

    /// Number of active handles.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Check if a slot is active (for cross-process isolation checks).
    pub fn is_active(&self, slot: u8) -> bool {
        let idx = slot as usize;
        idx < MAX_HANDLES && self.entries[idx].active
    }

    /// Clear all handles (for process cleanup).
    pub fn clear(&mut self) {
        for entry in &mut self.entries {
            *entry = HandleEntry::empty();
        }
        self.count = 0;
    }
}
