//! Fabric OS — Stable Interface Types
//!
//! 64-byte cache-line-aligned wire formats for the Typed Message Bus
//! and Capability Engine. See INTERFACE_CONTRACT.md for the authoritative spec.
//!
//! This crate is `no_std` and has zero dependencies.

#![no_std]
#![allow(dead_code)]

pub mod ids;
pub mod capability;
pub mod message;
pub mod audit;

// Re-export core types at crate root for convenience
pub use ids::{CapabilityId, ResourceId, ProcessId, TypeId, Timestamp};
pub use capability::{CapabilityToken, Perm, Budget};
pub use message::MessageHeader;
pub use audit::{AuditEntry, AuditAction};
