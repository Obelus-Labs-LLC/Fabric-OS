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

use spin::Mutex;
use fabric_types::{MessageHeader, ProcessId};
use crate::serial_println;

pub use router::{BusRouter, BusError};
pub use queue::Envelope;
pub use monitor::MonitorFilter;

/// Global bus router instance.
pub static BUS: Mutex<BusRouter> = Mutex::new(BusRouter::new());

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
    // Lock ordering: GOVERNANCE < TABLE < STORE < BUS
    crate::governance::evaluate_policy(header)?;

    // Policy allows — proceed with normal 12-step pipeline
    let mut bus = BUS.lock();
    let result = bus.send(header, payload, nonce);
    if result.is_err() {
        // If send fails after policy pass, that's a bus-level rejection (not policy)
    }
    result
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
