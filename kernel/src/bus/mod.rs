//! Typed Message Bus — Phase 2 of Fabric OS.
//!
//! Provides capability-authenticated, HMAC-signed message routing between
//! processes. Every message is validated, sequence-tracked, and audit-logged.
//!
//! Public API:
//!   bus::init()              — Initialize bus (allocate arena)
//!   bus::register_process()  — Register a process for messaging
//!   bus::send()              — Send a capability-validated message
//!   bus::receive()           — Receive next message for a process
//!   bus::register_monitor()  — Attach a read-only monitor tap

#![allow(dead_code)]

pub mod arena;
pub mod audit;
pub mod monitor;
pub mod queue;
pub mod router;
pub mod sequence;

use fabric_types::{MessageHeader, ProcessId};
use crate::sync::OrderedMutex;
use crate::serial_println;

pub use router::{BusRouter, BusError};
pub use queue::Envelope;
pub use monitor::MonitorFilter;

/// Global bus router instance.
pub static BUS: OrderedMutex<BusRouter, { crate::sync::levels::BUS }> =
    OrderedMutex::new(BusRouter::new());

/// Initialize the message bus subsystem. Must be called after heap init.
pub fn init() {
    BUS.lock().init();
    serial_println!("[BUS] Message bus initialized");
    serial_println!("[BUS] Arena: 256 KiB | Queues: 32/process | Audit: 512 entries");
}

// === Convenience free functions ===

pub fn register_process(pid: ProcessId) -> Result<(), BusError> {
    BUS.lock().register_process(pid)
}

pub fn send(header: &MessageHeader, payload: Option<&[u8]>, nonce: u32) -> Result<(), BusError> {
    // Phase 5A: Policy pre-check BEFORE acquiring BUS lock.
    // Lock ordering: GOVERNANCE(5) < TABLE(6) < STORE(7) < BUS(9)
    crate::governance::evaluate_policy(header)?;

    // TD-003: Capability validation BEFORE BUS lock (STORE level 7 < BUS level 9).
    // Previously done inside BusRouter::send(), which violated lock ordering.
    if header.capability_id != 0 {
        crate::capability::validate(
            header.capability_id,
            fabric_types::Perm::WRITE,
            nonce,
        ).map_err(BusError::CapabilityInvalid)?;

        // Ownership check: cap owner must match sender
        let store = crate::capability::STORE.lock();
        match store.get_token_info(header.capability_id) {
            Some((owner, _)) if owner != header.sender => {
                return Err(BusError::OwnerMismatch);
            }
            None => {
                return Err(BusError::CapabilityInvalid(
                    crate::capability::CapabilityError::NotFound,
                ));
            }
            _ => {} // Valid
        }
    }

    // All pre-checks passed — proceed with BUS-locked pipeline
    let mut bus = BUS.lock();
    bus.send(header, payload, nonce)
}

pub fn receive(pid: ProcessId) -> Option<Envelope> {
    BUS.lock().receive(pid)
}

pub fn register_monitor(filter: MonitorFilter) -> Result<u32, BusError> {
    BUS.lock().register_monitor(filter)
}

pub fn verify_audit_chain() -> (usize, bool) {
    BUS.lock().verify_audit_chain()
}
