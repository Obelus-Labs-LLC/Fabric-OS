//! IRQ Router — dynamic interrupt dispatch with shared IRQ support.
//!
//! Replaces hardcoded IDT vector matching with a registration table.
//! Supports vectors 32-47 (standard x86 IRQ range) with up to 4
//! shared handlers per vector for PCI IRQ sharing.

#![allow(dead_code)]

use crate::sync::OrderedMutex;

/// Maximum shared handlers per IRQ vector.
pub const MAX_SHARED: usize = 4;

/// Number of routable IRQ vectors (32-47).
pub const IRQ_VECTOR_COUNT: usize = 16;

/// Base IRQ vector number.
pub const IRQ_VECTOR_BASE: u8 = 32;

/// A registered interrupt handler.
#[derive(Clone, Copy, Debug)]
pub struct IrqHandler {
    pub driver_name: &'static str,
    pub resource_id: u32,
    pub active: bool,
}

impl IrqHandler {
    const fn empty() -> Self {
        Self {
            driver_name: "",
            resource_id: 0,
            active: false,
        }
    }
}

/// Slot for one IRQ vector: holds up to MAX_SHARED handlers.
#[derive(Clone, Copy, Debug)]
pub struct IrqSlot {
    handlers: [IrqHandler; MAX_SHARED],
    count: u8,
}

impl IrqSlot {
    const fn empty() -> Self {
        Self {
            handlers: [IrqHandler::empty(); MAX_SHARED],
            count: 0,
        }
    }
}

/// Dynamic IRQ dispatch table.
///
/// Maps IRQ vectors 32-47 to registered handlers. Multiple drivers
/// can share a single IRQ vector (common with PCI).
pub struct IrqRouter {
    slots: [IrqSlot; IRQ_VECTOR_COUNT],
}

impl IrqRouter {
    pub const fn new() -> Self {
        Self {
            slots: [IrqSlot::empty(); IRQ_VECTOR_COUNT],
        }
    }

    /// Convert an absolute vector number to a slot index.
    fn vector_to_index(vector: u8) -> Option<usize> {
        if vector >= IRQ_VECTOR_BASE && vector < IRQ_VECTOR_BASE + IRQ_VECTOR_COUNT as u8 {
            Some((vector - IRQ_VECTOR_BASE) as usize)
        } else {
            None
        }
    }

    /// Register a handler for the given IRQ vector.
    ///
    /// Returns Err if the vector is out of range or all 4 shared slots are full.
    pub fn register(&mut self, vector: u8, handler: IrqHandler) -> Result<(), &'static str> {
        let idx = Self::vector_to_index(vector)
            .ok_or("IRQ vector out of range (must be 32-47)")?;

        let slot = &mut self.slots[idx];
        if slot.count as usize >= MAX_SHARED {
            return Err("IRQ vector full (max 4 shared handlers)");
        }

        // Find first inactive entry
        for h in &mut slot.handlers {
            if !h.active {
                *h = handler;
                h.active = true;
                slot.count += 1;
                return Ok(());
            }
        }

        Err("IRQ slot inconsistency")
    }

    /// Unregister a handler by resource_id from the given vector.
    pub fn unregister(&mut self, vector: u8, resource_id: u32) -> bool {
        let idx = match Self::vector_to_index(vector) {
            Some(i) => i,
            None => return false,
        };

        let slot = &mut self.slots[idx];
        for h in &mut slot.handlers {
            if h.active && h.resource_id == resource_id {
                h.active = false;
                h.driver_name = "";
                h.resource_id = 0;
                slot.count -= 1;
                return true;
            }
        }
        false
    }

    /// Get all active handlers for the given vector.
    ///
    /// Returns a slice of up to MAX_SHARED handlers. Callers should
    /// iterate and call each handler's interrupt routine.
    pub fn dispatch(&self, vector: u8) -> &[IrqHandler; MAX_SHARED] {
        let idx = match Self::vector_to_index(vector) {
            Some(i) => i,
            None => return &EMPTY_HANDLERS,
        };
        &self.slots[idx].handlers
    }

    /// Get the number of registered handlers for a vector.
    pub fn handler_count(&self, vector: u8) -> usize {
        let idx = match Self::vector_to_index(vector) {
            Some(i) => i,
            None => return 0,
        };
        self.slots[idx].count as usize
    }

    /// Get total number of registered handlers across all vectors.
    pub fn total_handlers(&self) -> usize {
        self.slots.iter().map(|s| s.count as usize).sum()
    }
}

/// Empty handler array for out-of-range vectors.
static EMPTY_HANDLERS: [IrqHandler; MAX_SHARED] = [IrqHandler::empty(); MAX_SHARED];

/// Global IRQ router.
pub static IRQ_ROUTER: OrderedMutex<IrqRouter, { crate::sync::levels::HAL }> =
    OrderedMutex::new(IrqRouter::new());
