//! Central bus router — capability-validated message routing.
//!
//! The router is the integration hub: it validates capabilities, checks
//! sequence numbers, signs messages with HMAC, routes to per-process queues,
//! notifies monitors, and appends to the audit log.

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use fabric_types::{MessageHeader, ProcessId, Timestamp};
use fabric_types::audit::AuditAction;
use crate::capability::{self, CapabilityError};
use crate::capability::hmac_engine;

use super::arena::PayloadArena;
use super::audit::AuditLog;
use super::monitor::{MonitorFilter, MonitorRegistry};
use super::queue::{Envelope, InboxQueue};
use super::sequence::{SequenceError, SequenceTracker};

/// Maximum registered processes.
const MAX_PROCESSES: usize = 64;

/// Error types for bus operations.
#[derive(Debug)]
pub enum BusError {
    // Routing errors
    SenderNotRegistered,
    ReceiverNotRegistered,
    ReceiverQueueFull,
    SelfSendDenied,

    // Security errors
    CapabilityInvalid(CapabilityError),
    OwnerMismatch,
    HmacVerificationFailed,
    SequenceReplay,
    SequenceGap { expected: u64, got: u64 },

    // Validation errors
    InvalidVersion,
    InvalidSender,
    PayloadLengthMismatch,
    PayloadTooLarge,

    // Resource errors
    ArenaFull,
    MonitorLimitReached,
    MonitorNotFound,
    ProcessAlreadyRegistered,
    ProcessLimitReached,

    // Governance
    PolicyDenied,

    // Internal
    NotInitialized,
}

/// The central bus router.
pub struct BusRouter {
    inboxes: BTreeMap<u32, InboxQueue>,
    arena: PayloadArena,
    sequences: SequenceTracker,
    monitors: MonitorRegistry,
    audit: AuditLog,
    current_tick: u64,
    total_sent: u64,
    total_rejected: u64,
}

impl BusRouter {
    pub const fn new() -> Self {
        Self {
            inboxes: BTreeMap::new(),
            arena: PayloadArena::new(),
            sequences: SequenceTracker::new(),
            monitors: MonitorRegistry::new(),
            audit: AuditLog::new(),
            current_tick: 0,
            total_sent: 0,
            total_rejected: 0,
        }
    }

    /// Initialize the bus (heap-allocate the arena).
    pub fn init(&mut self) {
        self.arena.init();
    }

    /// Advance the monotonic tick counter.
    pub fn tick(&mut self) {
        self.current_tick += 1;
    }

    /// Advance ticks by N.
    pub fn advance_ticks(&mut self, n: u64) {
        self.current_tick += n;
    }

    /// Get current tick.
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// Register a process to receive messages.
    pub fn register_process(&mut self, pid: ProcessId) -> Result<(), BusError> {
        if pid.0 == 0 {
            return Err(BusError::InvalidSender); // pid 0 is kernel
        }
        if self.inboxes.contains_key(&pid.0) {
            return Err(BusError::ProcessAlreadyRegistered);
        }
        if self.inboxes.len() >= MAX_PROCESSES {
            return Err(BusError::ProcessLimitReached);
        }
        self.inboxes.insert(pid.0, InboxQueue::new());
        Ok(())
    }

    /// Unregister a process (drops its inbox).
    pub fn unregister_process(&mut self, pid: ProcessId) -> Result<(), BusError> {
        if self.inboxes.remove(&pid.0).is_none() {
            return Err(BusError::ReceiverNotRegistered);
        }
        self.sequences.remove(pid);
        Ok(())
    }

    /// Send a message through the bus.
    ///
    /// Validation order:
    /// 1. Version check
    /// 2. Sender/receiver registered
    /// 3. No self-send
    /// 4. Payload length consistency
    /// 5. Capability validation (via capability::validate)
    /// 6. Capability ownership check
    /// 7. Sequence number check
    /// 8. Allocate payload in arena
    /// 9. Compute HMAC
    /// 10. Push to receiver's inbox
    /// 11. Notify monitors
    /// 12. Append audit entry
    pub fn send(
        &mut self,
        header: &MessageHeader,
        payload: Option<&[u8]>,
        nonce: u32,
    ) -> Result<(), BusError> {
        let ts = Timestamp(self.current_tick);

        // 1. Version check
        if header.version != MessageHeader::VERSION {
            self.reject(header, AuditAction::MessageRejected, ts);
            return Err(BusError::InvalidVersion);
        }

        // 2. Sender registered?
        if header.sender.0 == 0 {
            self.reject(header, AuditAction::MessageRejected, ts);
            return Err(BusError::InvalidSender);
        }
        if !self.inboxes.contains_key(&header.sender.0) {
            self.reject(header, AuditAction::MessageRejected, ts);
            return Err(BusError::SenderNotRegistered);
        }

        // 2b. Receiver registered?
        if !self.inboxes.contains_key(&header.receiver.0) {
            self.reject(header, AuditAction::MessageRejected, ts);
            return Err(BusError::ReceiverNotRegistered);
        }

        // 3. No self-send
        if header.sender == header.receiver {
            self.reject(header, AuditAction::MessageRejected, ts);
            return Err(BusError::SelfSendDenied);
        }

        // 4. Payload length consistency
        let payload_data = match (header.payload_len, payload) {
            (0, None) | (0, Some(&[])) => None,
            (len, Some(data)) if len as usize == data.len() => {
                if data.len() > super::arena::MAX_PAYLOAD_SIZE {
                    self.reject(header, AuditAction::MessageRejected, ts);
                    return Err(BusError::PayloadTooLarge);
                }
                Some(data)
            }
            _ => {
                self.reject(header, AuditAction::MessageRejected, ts);
                return Err(BusError::PayloadLengthMismatch);
            }
        };

        // 5. Capability validation
        if header.capability_id == 0 {
            self.reject(header, AuditAction::CapDenied, ts);
            return Err(BusError::CapabilityInvalid(CapabilityError::NotFound));
        }

        // Validate capability (locks STORE briefly, then releases)
        if let Err(e) = capability::validate(
            header.capability_id,
            fabric_types::Perm::WRITE,
            nonce,
        ) {
            self.audit.append(
                header.sender,
                AuditAction::CapDenied,
                header.receiver,
                header.msg_type,
                header.capability_id,
                header.sequence,
                ts,
            );
            self.total_rejected += 1;
            return Err(BusError::CapabilityInvalid(e));
        }

        // 6. Ownership check — cap owner must match sender
        {
            let store = capability::STORE.lock();
            match store.get_token_info(header.capability_id) {
                Some((owner, _resource)) => {
                    if owner != header.sender {
                        drop(store);
                        self.audit.append(
                            header.sender,
                            AuditAction::CapDenied,
                            header.receiver,
                            header.msg_type,
                            header.capability_id,
                            header.sequence,
                            ts,
                        );
                        self.total_rejected += 1;
                        return Err(BusError::OwnerMismatch);
                    }
                }
                None => {
                    drop(store);
                    self.reject(header, AuditAction::CapDenied, ts);
                    return Err(BusError::CapabilityInvalid(CapabilityError::NotFound));
                }
            }
        }

        // 7. Sequence number check
        if let Err(seq_err) = self.sequences.check(header.sender, header.sequence) {
            match seq_err {
                SequenceError::Replay => {
                    self.audit.append(
                        header.sender,
                        AuditAction::SequenceViolation,
                        header.receiver,
                        header.msg_type,
                        header.capability_id,
                        header.sequence,
                        ts,
                    );
                    self.total_rejected += 1;
                    return Err(BusError::SequenceReplay);
                }
                SequenceError::Gap { expected, got } => {
                    self.audit.append(
                        header.sender,
                        AuditAction::SequenceViolation,
                        header.receiver,
                        header.msg_type,
                        header.capability_id,
                        header.sequence,
                        ts,
                    );
                    self.total_rejected += 1;
                    return Err(BusError::SequenceGap { expected, got });
                }
            }
        }

        // 8. Allocate payload in arena
        let arena_slice = if let Some(data) = payload_data {
            match self.arena.allocate(data) {
                Some(slice) => Some(slice),
                None => {
                    self.reject(header, AuditAction::MessageRejected, ts);
                    return Err(BusError::ArenaFull);
                }
            }
        } else {
            None
        };

        // 9. Compute HMAC over header active_bytes + payload
        let active = header.active_bytes();
        let hmac = if let Some(data) = payload_data {
            // Concatenate active_bytes + payload for HMAC
            let mut combined = alloc::vec::Vec::with_capacity(40 + data.len());
            combined.extend_from_slice(&active);
            combined.extend_from_slice(data);
            hmac_engine::sign(&combined)
        } else {
            hmac_engine::sign(&active)
        };

        // 10. Push envelope to receiver's inbox
        let envelope = Envelope {
            header: *header,
            hmac,
            payload: arena_slice,
        };

        let inbox = self.inboxes.get_mut(&header.receiver.0).unwrap();
        if !inbox.push(envelope) {
            self.audit.append(
                header.sender,
                AuditAction::QueueFull,
                header.receiver,
                header.msg_type,
                header.capability_id,
                header.sequence,
                ts,
            );
            self.total_rejected += 1;
            return Err(BusError::ReceiverQueueFull);
        }

        // 11. Notify matching monitors
        self.monitors.notify(&envelope);

        // 12. Append audit entry
        self.audit.append(
            header.sender,
            AuditAction::MessageSent,
            header.receiver,
            header.msg_type,
            header.capability_id,
            header.sequence,
            ts,
        );

        self.total_sent += 1;
        Ok(())
    }

    /// Receive the next message for a process.
    pub fn receive(&mut self, pid: ProcessId) -> Option<Envelope> {
        let inbox = self.inboxes.get_mut(&pid.0)?;
        inbox.pop()
    }

    /// Peek at the next message without consuming it.
    pub fn peek(&self, pid: ProcessId) -> Option<&Envelope> {
        let inbox = self.inboxes.get(&pid.0)?;
        inbox.peek()
    }

    /// Get payload bytes for an arena slice.
    pub fn payload(&self, slice: super::arena::ArenaSlice) -> &[u8] {
        self.arena.get(slice)
    }

    /// Register a monitor tap.
    pub fn register_monitor(&mut self, filter: MonitorFilter) -> Result<u32, BusError> {
        self.monitors
            .register(filter)
            .ok_or(BusError::MonitorLimitReached)
    }

    /// Unregister a monitor tap.
    pub fn unregister_monitor(&mut self, tap_id: u32) -> Result<(), BusError> {
        if self.monitors.unregister(tap_id) {
            Ok(())
        } else {
            Err(BusError::MonitorNotFound)
        }
    }

    /// Drain one event from a monitor.
    pub fn monitor_drain(&mut self, tap_id: u32) -> Option<Envelope> {
        self.monitors.drain(tap_id)
    }

    /// Get pending monitor event count.
    pub fn monitor_pending(&self, tap_id: u32) -> usize {
        self.monitors.pending_count(tap_id)
    }

    /// Verify the audit log hash chain.
    pub fn verify_audit_chain(&self) -> (usize, bool) {
        self.audit.verify_chain()
    }

    /// Get bus statistics: (total_sent, total_rejected).
    pub fn stats(&self) -> (u64, u64) {
        (self.total_sent, self.total_rejected)
    }

    /// Get the audit log (for testing).
    pub fn audit_log_mut(&mut self) -> &mut AuditLog {
        &mut self.audit
    }

    /// Get pending message count for a process.
    pub fn pending_count(&self, pid: ProcessId) -> usize {
        self.inboxes
            .get(&pid.0)
            .map(|q| q.len())
            .unwrap_or(0)
    }

    /// Check if process is registered.
    pub fn is_registered(&self, pid: ProcessId) -> bool {
        self.inboxes.contains_key(&pid.0)
    }

    /// Clear all state (for testing between OCRB tests).
    pub fn clear(&mut self) {
        self.inboxes.clear();
        self.arena.clear();
        self.sequences.clear();
        self.monitors.clear();
        self.audit.clear();
        self.current_tick = 0;
        self.total_sent = 0;
        self.total_rejected = 0;
    }

    // --- Internal helpers ---

    /// Record a policy-denied rejection in the audit log.
    pub fn reject_with_policy(&mut self, header: &MessageHeader) {
        let ts = Timestamp(self.current_tick);
        self.audit.append(
            header.sender,
            AuditAction::PolicyViolation,
            header.receiver,
            header.msg_type,
            header.capability_id,
            header.sequence,
            ts,
        );
        self.total_rejected += 1;
    }

    fn reject(&mut self, header: &MessageHeader, action: AuditAction, ts: Timestamp) {
        self.audit.append(
            header.sender,
            action,
            header.receiver,
            header.msg_type,
            header.capability_id,
            header.sequence,
            ts,
        );
        self.total_rejected += 1;
    }
}
