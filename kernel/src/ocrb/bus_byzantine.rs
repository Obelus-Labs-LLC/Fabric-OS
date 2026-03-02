#![allow(dead_code)]

use alloc::{format, vec, vec::Vec};
use fabric_types::{MessageHeader, ProcessId, TypeId, Timestamp, Perm, ResourceId};
use crate::bus::{self, BusError, MonitorFilter};
use crate::capability::{self, CapabilityError};
use crate::capability::hmac_engine;
use crate::ocrb::OcrbResult;

pub fn run_all_tests() -> Vec<OcrbResult> {
    vec![
        test1_basic_send_receive(),
        test2_capability_validation(),
        test3_sequence_enforcement(),
        test4_hmac_integrity(),
        test5_bus_flood_backpressure(),
        test6_monitor_tap(),
        test7_audit_hash_chain(),
        test8_byzantine_message_reject(),
    ]
}

/// Helper: register two processes and create a WRITE capability for the sender.
/// Returns (cap_id_raw, sender_pid, receiver_pid).
fn setup_pair(sender: u32, receiver: u32) -> u64 {
    bus::register_process(ProcessId::new(sender)).expect("register sender");
    bus::register_process(ProcessId::new(receiver)).expect("register receiver");

    let cap_id = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | sender as u64),
        Perm::READ | Perm::WRITE,
        ProcessId::new(sender),
        None,
        None,
    ).expect("create ipc cap");

    cap_id.0
}

/// Helper: build a message header.
fn make_header(sender: u32, receiver: u32, cap_id: u64, seq: u64, payload_len: u32) -> MessageHeader {
    let mut h = MessageHeader::zeroed();
    h.version = MessageHeader::VERSION;
    h.msg_type = TypeId(1);
    h.sender = ProcessId::new(sender);
    h.receiver = ProcessId::new(receiver);
    h.capability_id = cap_id;
    h.sequence = seq;
    h.timestamp = Timestamp(0);
    h.payload_len = payload_len;
    h
}

/// Helper: clean up between tests.
fn cleanup() {
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
}

/// Test 1: Basic Send/Receive (weight: 15)
/// Send 100 messages pid1→pid2, verify all received with correct payloads.
fn test1_basic_send_receive() -> OcrbResult {
    cleanup();
    let cap_id = setup_pair(1, 2);
    let mut errors = 0u32;
    let mut received = 0u32;
    const BATCH: u32 = 30;

    // Send and receive in batches to stay within queue capacity (32)
    let mut msg_idx = 0u32;
    while msg_idx < 100 {
        let batch_end = core::cmp::min(msg_idx + BATCH, 100);

        // Send a batch
        for i in msg_idx..batch_end {
            let payload = format!("msg{:04}", i);
            let h = make_header(1, 2, cap_id, (i + 1) as u64, payload.len() as u32);
            if bus::send(&h, Some(payload.as_bytes()), i + 1).is_err() {
                errors += 1;
            }
        }

        // Drain the batch
        for i in msg_idx..batch_end {
            if let Some(env) = bus::receive(ProcessId::new(2)) {
                if env.header.sequence != (i + 1) as u64 {
                    errors += 1;
                }
                if let Some(slice) = env.payload {
                    let expected = format!("msg{:04}", i);
                    let guard = bus::BUS.lock();
                    let data = guard.payload(slice);
                    if data != expected.as_bytes() {
                        errors += 1;
                    }
                } else {
                    errors += 1;
                }
                received += 1;
            } else {
                errors += 1;
            }
        }

        msg_idx = batch_end;
    }

    cleanup();

    let score = if errors == 0 && received == 100 { 100 } else {
        100u32.saturating_sub(errors * 5) as u8
    };

    OcrbResult {
        test_name: "Basic Send/Receive",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("{}/100 received, {} errors", received, errors),
    }
}

/// Test 2: Capability Validation on Send (weight: 15)
fn test2_capability_validation() -> OcrbResult {
    cleanup();
    let mut correct = 0u32;
    let total = 5u32;

    // Setup: pid 1 and pid 2 registered
    bus::register_process(ProcessId::new(1)).unwrap();
    bus::register_process(ProcessId::new(2)).unwrap();

    // Case (a): Invalid capability_id (non-existent)
    let h = make_header(1, 2, 99999, 1, 0);
    match bus::send(&h, None, 1) {
        Err(BusError::CapabilityInvalid(_)) => correct += 1,
        _ => {}
    }

    // Case (b): Capability owned by pid 3 but sender is pid 1
    bus::register_process(ProcessId::new(3)).unwrap();
    let other_cap = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 3),
        Perm::READ | Perm::WRITE,
        ProcessId::new(3), // owned by pid 3
        None,
        None,
    ).unwrap();
    let h = make_header(1, 2, other_cap.0, 1, 0);
    match bus::send(&h, None, 1) {
        Err(BusError::OwnerMismatch) => correct += 1,
        _ => {}
    }

    // Case (c): Capability with only READ permission (no WRITE)
    let read_cap = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 1),
        Perm::READ, // no WRITE
        ProcessId::new(1),
        None,
        None,
    ).unwrap();
    let h = make_header(1, 2, read_cap.0, 1, 0);
    match bus::send(&h, None, 1) {
        Err(BusError::CapabilityInvalid(CapabilityError::InsufficientPermission)) => correct += 1,
        _ => {}
    }

    // Case (d): Expired capability
    let exp_cap = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 10),
        Perm::READ | Perm::WRITE,
        ProcessId::new(1),
        Some(1), // expires in 1 tick
        None,
    ).unwrap();
    capability::advance_ticks(10); // well past expiry
    let h = make_header(1, 2, exp_cap.0, 1, 0);
    match bus::send(&h, None, 1) {
        Err(BusError::CapabilityInvalid(CapabilityError::Expired)) => correct += 1,
        _ => {}
    }

    // Case (e): Valid capability should succeed
    let valid_cap = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 20),
        Perm::READ | Perm::WRITE,
        ProcessId::new(1),
        None,
        None,
    ).unwrap();
    let h = make_header(1, 2, valid_cap.0, 1, 0);
    match bus::send(&h, None, 1) {
        Ok(()) => correct += 1,
        _ => {}
    }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };

    OcrbResult {
        test_name: "Capability Validation",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("{}/{} cases correct", correct, total),
    }
}

/// Test 3: Sequence Number Enforcement (weight: 15)
fn test3_sequence_enforcement() -> OcrbResult {
    cleanup();
    let cap_id = setup_pair(1, 2);
    let mut correct = 0u32;
    let total = 4u32;

    // Send seq 1, 2, 3 (should all pass)
    for seq in 1..=3u64 {
        let h = make_header(1, 2, cap_id, seq, 0);
        bus::send(&h, None, seq as u32).unwrap();
    }

    // (a) Replay: seq 3 again
    let h = make_header(1, 2, cap_id, 3, 0);
    match bus::send(&h, None, 4) {
        Err(BusError::SequenceReplay) => correct += 1,
        _ => {}
    }

    // (b) Regression: seq 2
    let h = make_header(1, 2, cap_id, 2, 0);
    match bus::send(&h, None, 5) {
        Err(BusError::SequenceReplay) => correct += 1,
        _ => {}
    }

    // (c) Gap: seq 5 (skipped 4)
    let h = make_header(1, 2, cap_id, 5, 0);
    match bus::send(&h, None, 6) {
        Err(BusError::SequenceGap { .. }) => correct += 1,
        _ => {}
    }

    // (d) Correct next: seq 4
    let h = make_header(1, 2, cap_id, 4, 0);
    match bus::send(&h, None, 7) {
        Ok(()) => correct += 1,
        _ => {}
    }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };

    OcrbResult {
        test_name: "Sequence Enforcement",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("{}/{} cases correct", correct, total),
    }
}

/// Test 4: HMAC Integrity (weight: 15)
fn test4_hmac_integrity() -> OcrbResult {
    cleanup();
    let cap_id = setup_pair(1, 2);
    let mut verified = 0u32;
    let mut errors = 0u32;
    const BATCH: u32 = 25;

    // Send 50 messages in batches, verify HMACs after each batch
    let mut msg_idx = 0u32;
    while msg_idx < 50 {
        let batch_end = core::cmp::min(msg_idx + BATCH, 50);

        // Send a batch
        for i in msg_idx..batch_end {
            let payload = format!("data{}", i);
            let h = make_header(1, 2, cap_id, (i + 1) as u64, payload.len() as u32);
            bus::send(&h, Some(payload.as_bytes()), i + 1).unwrap();
        }

        // Receive and verify HMACs for this batch
        for i in msg_idx..batch_end {
            if let Some(env) = bus::receive(ProcessId::new(2)) {
                let active = env.header.active_bytes();
                let payload = format!("data{}", i);

                let mut combined = Vec::with_capacity(40 + payload.len());
                combined.extend_from_slice(&active);
                combined.extend_from_slice(payload.as_bytes());

                if hmac_engine::verify(&combined, &env.hmac) {
                    verified += 1;
                } else {
                    errors += 1;
                }
            } else {
                errors += 1;
            }
        }

        msg_idx = batch_end;
    }

    // Tamper detection: send one more, tamper with received envelope
    let h = make_header(1, 2, cap_id, 51, 4);
    bus::send(&h, Some(b"test"), 51).unwrap();
    if let Some(mut env) = bus::receive(ProcessId::new(2)) {
        env.header.sender = ProcessId::new(99);
        let tampered_active = env.header.active_bytes();
        let mut combined = Vec::with_capacity(44);
        combined.extend_from_slice(&tampered_active);
        combined.extend_from_slice(b"test");

        if !hmac_engine::verify(&combined, &env.hmac) {
            verified += 1; // tamper correctly detected
        } else {
            errors += 1;
        }
    }

    cleanup();

    let total = 51u32;
    let score = if verified == total && errors == 0 { 100 } else {
        100u32.saturating_sub(errors * 5) as u8
    };

    OcrbResult {
        test_name: "HMAC Integrity",
        passed: score >= 80,
        score,
        weight: 15,
        details: format!("{}/{} HMACs verified, {} errors", verified, total, errors),
    }
}

/// Test 5: Bus Flood / Backpressure (weight: 10)
fn test5_bus_flood_backpressure() -> OcrbResult {
    cleanup();
    let cap_id = setup_pair(10, 20);
    let mut correct = 0u32;
    let total = 3u32;

    // Fill queue: send 32 messages (queue capacity)
    let mut sent = 0u32;
    for i in 1..=32u32 {
        let h = make_header(10, 20, cap_id, i as u64, 0);
        if bus::send(&h, None, i).is_ok() {
            sent += 1;
        }
    }
    if sent == 32 { correct += 1; }

    // 33rd should fail with ReceiverQueueFull
    let h = make_header(10, 20, cap_id, 33, 0);
    match bus::send(&h, None, 33) {
        Err(BusError::ReceiverQueueFull) => correct += 1,
        _ => {}
    }

    // Drain 10, then send 10 more — should succeed
    for _ in 0..10 {
        bus::receive(ProcessId::new(20));
    }
    let mut refill_ok = true;
    // After queue-full rejection, sequence 33 was consumed by the sequence tracker
    // (it passed seq check but failed at queue push). So next expected is 33.
    // Actually the send for seq 33 failed at ReceiverQueueFull, but sequence check
    // passed first. So last accepted sequence was 33, next expected is 34.
    // Wait - the queue-full happens AFTER sequence check succeeds and is recorded.
    // So we need to continue from seq 34.
    // But the nonce for cap validation was 33, so next nonce is 34.
    for i in 0..10u32 {
        let seq = 34 + i as u64;
        let nonce = 34 + i;
        let h = make_header(10, 20, cap_id, seq, 0);
        if bus::send(&h, None, nonce).is_err() {
            refill_ok = false;
            break;
        }
    }
    if refill_ok { correct += 1; }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };

    OcrbResult {
        test_name: "Bus Flood / Backpressure",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("{}/{} cases correct", correct, total),
    }
}

/// Test 6: Monitor Tap Read-Only (weight: 10)
fn test6_monitor_tap() -> OcrbResult {
    cleanup();

    // Register 3 processes
    bus::register_process(ProcessId::new(1)).unwrap();
    bus::register_process(ProcessId::new(2)).unwrap();
    bus::register_process(ProcessId::new(3)).unwrap();

    // Create caps for pid 1 and pid 3
    let cap1 = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 1),
        Perm::READ | Perm::WRITE,
        ProcessId::new(1),
        None,
        None,
    ).unwrap();
    let cap3 = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 3),
        Perm::READ | Perm::WRITE,
        ProcessId::new(3),
        None,
        None,
    ).unwrap();

    // Register monitor: only messages from pid 1
    let tap_id = bus::register_monitor(MonitorFilter {
        sender: Some(ProcessId::new(1)),
        receiver: None,
        msg_type: None,
    }).unwrap();

    // Send 20 from pid 1 → pid 2
    for i in 1..=20u32 {
        let h = make_header(1, 2, cap1.0, i as u64, 0);
        bus::send(&h, None, i).unwrap();
    }

    // Send 10 from pid 3 → pid 2
    for i in 1..=10u32 {
        let h = make_header(3, 2, cap3.0, i as u64, 0);
        bus::send(&h, None, i).unwrap();
    }

    // Drain monitor — should have exactly 20 entries (only pid 1)
    let mut monitor_count = 0u32;
    let mut wrong_sender = 0u32;
    let mut bus = bus::BUS.lock();
    loop {
        match bus.monitor_drain(tap_id) {
            Some(env) => {
                monitor_count += 1;
                if env.header.sender != ProcessId::new(1) {
                    wrong_sender += 1;
                }
            }
            None => break,
        }
    }
    drop(bus);

    cleanup();

    let score = if monitor_count == 20 && wrong_sender == 0 { 100 } else {
        let deviation = (monitor_count as i32 - 20).unsigned_abs();
        100u32.saturating_sub(deviation * 5 + wrong_sender * 10) as u8
    };

    OcrbResult {
        test_name: "Monitor Tap Read-Only",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("{}/20 monitor events, {} wrong sender", monitor_count, wrong_sender),
    }
}

/// Test 7: Audit Log Hash Chain (weight: 10)
fn test7_audit_hash_chain() -> OcrbResult {
    cleanup();
    let cap_id = setup_pair(1, 2);
    const BATCH: u32 = 30;

    // Send 200 messages in batches to generate audit entries (draining between batches)
    let mut msg_idx = 1u32;
    while msg_idx <= 200 {
        let batch_end = core::cmp::min(msg_idx + BATCH, 201);

        for i in msg_idx..batch_end {
            let h = make_header(1, 2, cap_id, i as u64, 0);
            bus::send(&h, None, i).unwrap();
        }

        // Drain receiver to free queue space
        while bus::receive(ProcessId::new(2)).is_some() {}

        msg_idx = batch_end;
    }

    // Verify chain integrity
    let (count, valid) = bus::verify_audit_chain();
    let chain_ok = valid && count > 0;

    // Tamper with one entry and verify detection
    let tamper_detected = {
        let mut bus = bus::BUS.lock();
        let audit = bus.audit_log_mut();
        let wp = audit.write_pos();
        let tamper_idx = if wp > 10 { wp - 10 } else { 0 };
        if let Some(entry) = &mut audit.entries_mut()[tamper_idx] {
            entry.prev_hash[0] ^= 0xFF;
        }
        let (_, valid_after) = audit.verify_chain();
        !valid_after
    };

    cleanup();

    let score = if chain_ok && tamper_detected { 100 } else { 0 };

    OcrbResult {
        test_name: "Audit Log Hash Chain",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("chain_ok={}, tamper_detected={}, entries={}", chain_ok, tamper_detected, count),
    }
}

/// Test 8: Byzantine Message Reject (weight: 10)
fn test8_byzantine_message_reject() -> OcrbResult {
    cleanup();
    let mut correct = 0u32;
    let total = 6u32;

    // Setup
    bus::register_process(ProcessId::new(1)).unwrap();
    bus::register_process(ProcessId::new(2)).unwrap();
    let cap_id = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 1),
        Perm::READ | Perm::WRITE,
        ProcessId::new(1),
        None,
        None,
    ).unwrap();

    // (a) Invalid version
    let mut h = make_header(1, 2, cap_id.0, 1, 0);
    h.version = 0;
    match bus::send(&h, None, 1) {
        Err(BusError::InvalidVersion) => correct += 1,
        _ => {}
    }

    // (b) Sender = pid 0 (kernel)
    let h = make_header(0, 2, cap_id.0, 1, 0);
    match bus::send(&h, None, 2) {
        Err(BusError::InvalidSender) => correct += 1,
        _ => {}
    }

    // (c) Self-send
    let self_cap = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | 50),
        Perm::READ | Perm::WRITE,
        ProcessId::new(1),
        None,
        None,
    ).unwrap();
    let h = make_header(1, 1, self_cap.0, 1, 0);
    match bus::send(&h, None, 3) {
        Err(BusError::SelfSendDenied) => correct += 1,
        _ => {}
    }

    // (d) payload_len=100 but payload=None
    let h = make_header(1, 2, cap_id.0, 1, 100);
    match bus::send(&h, None, 4) {
        Err(BusError::PayloadLengthMismatch) => correct += 1,
        _ => {}
    }

    // (e) payload_len=0 but payload=Some(data)
    let h = make_header(1, 2, cap_id.0, 1, 0);
    match bus::send(&h, Some(b"surprise"), 5) {
        Err(BusError::PayloadLengthMismatch) => correct += 1,
        _ => {}
    }

    // (f) capability_id=0 (null)
    let h = make_header(1, 2, 0, 1, 0);
    match bus::send(&h, None, 6) {
        Err(BusError::CapabilityInvalid(_)) => correct += 1,
        _ => {}
    }

    // Verify bus is still functional after all rejections
    let h = make_header(1, 2, cap_id.0, 1, 0);
    let still_works = bus::send(&h, None, 7).is_ok();
    if !still_works {
        // Bus broken after rejections — that's bad, subtract from score
        correct = correct.saturating_sub(1);
    }

    cleanup();

    let score = if correct == total { 100 } else { (correct * 100 / total) as u8 };

    OcrbResult {
        test_name: "Byzantine Message Reject",
        passed: score >= 80,
        score,
        weight: 10,
        details: format!("{}/{} rejections correct, bus_ok={}", correct, total, still_works),
    }
}
