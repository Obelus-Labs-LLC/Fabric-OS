//! Capability Engine — Phase 1 of Fabric OS.
//!
//! Provides unforgeable capability tokens for access control. Every future
//! subsystem (IPC bus, scheduler, drivers, filesystem) validates capabilities
//! through this module.
//!
//! Public API:
//!   capability::init()       — Initialize HMAC key
//!   capability::create()     — Create a root capability
//!   capability::delegate()   — Delegate a child capability
//!   capability::validate()   — Validate a token (hot path for bus router)
//!   capability::revoke()     — Revoke a token and all descendants
//!   capability::tick()       — Advance the monotonic tick counter

#![allow(dead_code)]

pub mod hmac_engine;
pub mod nonce;
pub mod budget;
pub mod store;

use spin::Mutex;
use crate::serial_println;

pub use fabric_types::{CapabilityId, ResourceId, ProcessId, Perm, Budget};
pub use store::{CapabilityStore, CapabilityError};

/// Global capability store.
pub static STORE: Mutex<CapabilityStore> = Mutex::new(CapabilityStore::new());

/// Initialize the capability subsystem. Must be called after heap init.
pub fn init() {
    hmac_engine::init();
    serial_println!("[CAP] Capability engine initialized");
    serial_println!("[CAP] HMAC-SHA3-256 key derived from boot entropy (RDTSC)");
}

// === Convenience free functions — lock STORE once, perform operation, release ===

pub fn create(
    resource: ResourceId,
    permissions: Perm,
    owner: ProcessId,
    expires: Option<u32>,
    budget: Option<Budget>,
) -> Result<CapabilityId, CapabilityError> {
    STORE.lock().create(resource, permissions, owner, expires, budget)
}

pub fn delegate(
    parent_id: u64,
    new_owner: ProcessId,
    permissions: Perm,
    expires: Option<u32>,
    budget: Option<Budget>,
) -> Result<CapabilityId, CapabilityError> {
    STORE.lock().delegate(parent_id, new_owner, permissions, expires, budget)
}

pub fn validate(
    token_id: u64,
    required_perm: Perm,
    presented_nonce: u32,
) -> Result<(), CapabilityError> {
    STORE.lock().validate(token_id, required_perm, presented_nonce)
}

pub fn revoke(token_id: u64) -> Result<usize, CapabilityError> {
    STORE.lock().revoke(token_id)
}

pub fn tick() {
    STORE.lock().tick();
}

pub fn advance_ticks(n: u64) {
    STORE.lock().advance_ticks(n);
}

pub fn count() -> usize {
    STORE.lock().count()
}
