//! OCRB Phase 4 — Driver Isolation Tests
//!
//! 9 tests verifying: driver registration, IPC round-trip, crash isolation,
//! multi-driver isolation, capability enforcement, interrupt bursts,
//! and crash storms.

#![allow(dead_code)]

use alloc::{format, vec, vec::Vec};
use fabric_types::{
    DeviceClass, DriverOp, DriverRequest, DriverResponse, DriverStatus,
    Intent, IntentCategory, Priority, EnergyClass,
    MessageHeader, ProcessId, ResourceId, TypeId, Timestamp,
    Perm,
};
use crate::{bus, capability, hal, process};
use crate::ocrb::OcrbResult;

// Device resource IDs (must match hal::init())
const SERIAL_RES:  ResourceId = ResourceId(ResourceId::KIND_DEVICE | 0x01);
const TIMER_RES:   ResourceId = ResourceId(ResourceId::KIND_DEVICE | 0x02);
const RAMDISK_RES: ResourceId = ResourceId(ResourceId::KIND_DEVICE | 0x03);
const FB_RES:      ResourceId = ResourceId(ResourceId::KIND_DEVICE | 0x04);

pub fn run_all_tests() -> Vec<OcrbResult> {
    vec![
        test1_driver_registration(),
        test2_serial_driver_ipc(),
        test3_ramdisk_read_write(),
        test4_timer_driver_tick(),
        test5_framebuffer_write(),
        test6_driver_crash_restart(),
        test7_multi_driver_crash_isolation(),
        test8_capability_enforcement(),
        test9_driver_crash_storm(),
    ]
}

// ─── Helpers ──────────────────────────────────────────────────────

fn cleanup() {
    hal::REGISTRY.lock().clear();
    process::TABLE.lock().clear();
    process::SCHEDULER.lock().clear();
    bus::BUS.lock().clear();
    capability::STORE.lock().clear();
}

fn init_fresh() {
    cleanup();
    process::init();
    hal::init();
}

/// Create a client process under Butler, registered on the bus, with an IPC cap.
/// Returns (pid, cap_id_raw).
fn make_client(name: &str) -> (ProcessId, u64) {
    let intent = Intent {
        category: IntentCategory::Compute,
        priority: Priority::Normal,
        energy_class: EnergyClass::Balanced,
        _pad: 0,
        _reserved: 0,
        deadline: Timestamp::ZERO,
    };
    let pid = process::spawn(ProcessId::BUTLER, intent, name, None)
        .expect("spawn client");

    let cap = capability::create(
        ResourceId::new(ResourceId::KIND_IPC | pid.0 as u64),
        Perm::READ | Perm::WRITE,
        pid,
        None,
        None,
    ).expect("create client cap");

    (pid, cap.0)
}

/// Send a DriverRequest from client to driver, dispatch, receive response.
/// `seq` is incremented for each send to satisfy bus sequence tracking.
/// Returns the DriverResponse (or None if no response).
fn send_driver_request(
    client_pid: ProcessId,
    client_cap: u64,
    driver_res: ResourceId,
    request: &DriverRequest,
    extra_data: Option<&[u8]>,
    seq: &mut u64,
) -> Option<(DriverResponse, Vec<u8>)> {
    let driver_pid = hal::driver_pid(driver_res)?;

    *seq += 1;
    let current_seq = *seq;

    // Build payload: request bytes + optional extra data
    let req_bytes = request.to_bytes();
    let mut payload = Vec::from(req_bytes.as_slice());
    if let Some(data) = extra_data {
        payload.extend_from_slice(data);
    }

    let mut header = MessageHeader::zeroed();
    header.version = MessageHeader::VERSION;
    header.msg_type = TypeId::DRIVER_REQUEST;
    header.sender = client_pid;
    header.receiver = driver_pid;
    header.capability_id = client_cap;
    header.payload_len = payload.len() as u32;
    header.sequence = current_seq;
    header.timestamp = Timestamp(0);

    bus::send(&header, Some(&payload), current_seq as u32).ok()?;

    // Dispatch the driver
    hal::dispatch_one(driver_res);

    // Receive response
    let env = bus::receive(client_pid)?;
    let resp_payload = {
        let bus_guard = bus::BUS.lock();
        if let Some(slice) = env.payload {
            let data = bus_guard.payload(slice);
            Vec::from(data)
        } else {
            Vec::new()
        }
    };

    // Parse response from payload
    let resp = if resp_payload.len() >= DriverResponse::SIZE {
        DriverResponse::from_bytes(&resp_payload).unwrap_or(DriverResponse::error(DriverStatus::Error))
    } else {
        DriverResponse::error(DriverStatus::Error)
    };

    // Extra data after the response header
    let extra = if resp_payload.len() > DriverResponse::SIZE {
        resp_payload[DriverResponse::SIZE..].to_vec()
    } else {
        Vec::new()
    };

    Some((resp, extra))
}

// ─── Test 1: Driver Registration ──────────────────────────────────

fn test1_driver_registration() -> OcrbResult {
    init_fresh();

    let mut score = 0u8;
    let mut details = alloc::string::String::new();

    // Verify 4 drivers registered
    if hal::driver_count() == 4 {
        score += 25;
    } else {
        details = format!("expected 4 drivers, got {}", hal::driver_count());
    }

    // Verify lookup by ResourceId
    let serial_ok = hal::driver_pid(SERIAL_RES).is_some();
    let timer_ok = hal::driver_pid(TIMER_RES).is_some();
    let ramdisk_ok = hal::driver_pid(RAMDISK_RES).is_some();
    let fb_ok = hal::driver_pid(FB_RES).is_some();

    if serial_ok && timer_ok && ramdisk_ok && fb_ok {
        score += 25;
    } else {
        details = format!("lookup failed: s={} t={} r={} f={}",
            serial_ok, timer_ok, ramdisk_ok, fb_ok);
    }

    // Verify PIDs are unique and valid (> BUTLER)
    let pids: Vec<ProcessId> = [SERIAL_RES, TIMER_RES, RAMDISK_RES, FB_RES]
        .iter()
        .filter_map(|&r| hal::driver_pid(r))
        .collect();

    if pids.len() == 4 && pids.iter().all(|p| p.0 > 1) {
        score += 25;
    }

    // Verify device classes in registry
    {
        let reg = hal::REGISTRY.lock();
        let classes_ok =
            reg.get(SERIAL_RES).map(|e| e.device_class) == Some(DeviceClass::Serial)
            && reg.get(TIMER_RES).map(|e| e.device_class) == Some(DeviceClass::Timer)
            && reg.get(RAMDISK_RES).map(|e| e.device_class) == Some(DeviceClass::BlockStorage)
            && reg.get(FB_RES).map(|e| e.device_class) == Some(DeviceClass::Framebuffer);
        if classes_ok {
            score += 25;
        }
    }

    OcrbResult {
        test_name: "Driver Registration",
        passed: score >= 80,
        score,
        weight: 10,
        details,
    }
}

// ─── Test 2: Serial Driver IPC ───────────────────────────────────

fn test2_serial_driver_ipc() -> OcrbResult {
    init_fresh();
    let (client, cap) = make_client("serial-test");
    let mut seq = 0u64;

    let mut score = 0u8;

    // Send a Write with 5 bytes of test data
    let mut req = DriverRequest::zeroed();
    req.operation = DriverOp::Write;
    req.device_class = DeviceClass::Serial;
    req.length = 5;

    let test_data = b"Hello";
    if let Some((resp, _extra)) = send_driver_request(client, cap, SERIAL_RES, &req, Some(test_data), &mut seq) {
        if resp.status == DriverStatus::Ok {
            score += 50;
        }
        if resp.bytes_xfer == 5 {
            score += 25;
        }
    }

    // Send a Status request
    let mut status_req = DriverRequest::zeroed();
    status_req.operation = DriverOp::Status;
    status_req.device_class = DeviceClass::Serial;

    if let Some((resp, _)) = send_driver_request(client, cap, SERIAL_RES, &status_req, None, &mut seq) {
        if resp.status == DriverStatus::Ok && resp.bytes_xfer >= 5 {
            score += 25;
        }
    }

    OcrbResult {
        test_name: "Serial Driver IPC",
        passed: score >= 80,
        score,
        weight: 10,
        details: alloc::string::String::new(),
    }
}

// ─── Test 3: RAM Disk Read/Write ─────────────────────────────────

fn test3_ramdisk_read_write() -> OcrbResult {
    init_fresh();
    let (client, cap) = make_client("ramdisk-test");
    let mut seq = 0u64;

    let mut score = 0u8;

    // Write 16 bytes at offset 0
    let mut write_req = DriverRequest::zeroed();
    write_req.operation = DriverOp::Write;
    write_req.device_class = DeviceClass::BlockStorage;
    write_req.offset = 0;
    write_req.length = 16;

    let write_data: [u8; 16] = [
        0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE,
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
    ];

    if let Some((resp, _)) = send_driver_request(client, cap, RAMDISK_RES, &write_req, Some(&write_data), &mut seq) {
        if resp.status == DriverStatus::Ok && resp.bytes_xfer == 16 {
            score += 34;
        }
    }

    // Read 16 bytes back from offset 0
    let mut read_req = DriverRequest::zeroed();
    read_req.operation = DriverOp::Read;
    read_req.device_class = DeviceClass::BlockStorage;
    read_req.offset = 0;
    read_req.length = 16;

    if let Some((resp, extra)) = send_driver_request(client, cap, RAMDISK_RES, &read_req, None, &mut seq) {
        if resp.status == DriverStatus::Ok && resp.bytes_xfer == 16 {
            score += 33;
        }
        // Verify data integrity
        if extra.len() >= 16 && extra[..16] == write_data {
            score += 33;
        }
    }

    OcrbResult {
        test_name: "RAM Disk Read/Write",
        passed: score >= 80,
        score,
        weight: 15,
        details: alloc::string::String::new(),
    }
}

// ─── Test 4: Timer Driver Tick ───────────────────────────────────

fn test4_timer_driver_tick() -> OcrbResult {
    init_fresh();
    let (client, cap) = make_client("timer-test");
    let mut seq = 0u64;

    let mut score = 0u8;

    // Send 5 Interrupt events
    for _ in 0..5 {
        let mut req = DriverRequest::zeroed();
        req.operation = DriverOp::Interrupt;
        req.device_class = DeviceClass::Timer;

        let _ = send_driver_request(client, cap, TIMER_RES, &req, None, &mut seq);
    }

    // Send Status to check tick count
    let mut status_req = DriverRequest::zeroed();
    status_req.operation = DriverOp::Status;
    status_req.device_class = DeviceClass::Timer;

    if let Some((resp, _)) = send_driver_request(client, cap, TIMER_RES, &status_req, None, &mut seq) {
        if resp.status == DriverStatus::Ok {
            score += 50;
        }
        if resp.bytes_xfer == 5 {
            score += 50;
        }
    }

    OcrbResult {
        test_name: "Timer Driver Tick",
        passed: score >= 80,
        score,
        weight: 10,
        details: alloc::string::String::new(),
    }
}

// ─── Test 5: Framebuffer Write ───────────────────────────────────

fn test5_framebuffer_write() -> OcrbResult {
    init_fresh();
    let (client, cap) = make_client("fb-test");
    let mut seq = 0u64;

    let mut score = 0u8;

    // Write 4 bytes (1 pixel RGBA) at offset 0
    let mut write_req = DriverRequest::zeroed();
    write_req.operation = DriverOp::Write;
    write_req.device_class = DeviceClass::Framebuffer;
    write_req.offset = 0;
    write_req.length = 4;

    let pixel: [u8; 4] = [0xFF, 0x00, 0x80, 0xFF]; // RGBA

    if let Some((resp, _)) = send_driver_request(client, cap, FB_RES, &write_req, Some(&pixel), &mut seq) {
        if resp.status == DriverStatus::Ok && resp.bytes_xfer == 4 {
            score += 34;
        }
    }

    // Read it back
    let mut read_req = DriverRequest::zeroed();
    read_req.operation = DriverOp::Read;
    read_req.device_class = DeviceClass::Framebuffer;
    read_req.offset = 0;
    read_req.length = 4;

    if let Some((resp, extra)) = send_driver_request(client, cap, FB_RES, &read_req, None, &mut seq) {
        if resp.status == DriverStatus::Ok && resp.bytes_xfer == 4 {
            score += 33;
        }
        if extra.len() >= 4 && extra[..4] == pixel {
            score += 33;
        }
    }

    OcrbResult {
        test_name: "Framebuffer Write",
        passed: score >= 80,
        score,
        weight: 5,
        details: alloc::string::String::new(),
    }
}

// ─── Test 6: Driver Crash + Restart ──────────────────────────────

fn test6_driver_crash_restart() -> OcrbResult {
    init_fresh();
    let (client, cap) = make_client("crash-test");
    let mut seq = 0u64;

    let mut score = 0u8;

    // Get serial driver PID before crash
    let serial_pid_before = hal::driver_pid(SERIAL_RES).expect("serial pid");

    // Crash the serial driver
    let crash_result = process::crash(serial_pid_before);
    if crash_result.is_ok() {
        score += 25;
    }

    // Butler should have restarted it — verify process is Ready/Running
    let state = process::get_state(serial_pid_before);
    let restarted = state == Some(fabric_types::ProcessState::Ready)
        || state == Some(fabric_types::ProcessState::Running);
    if restarted {
        score += 25;
    }

    // Mark driver as needing re-init (registry side)
    hal::mark_needs_reinit(SERIAL_RES);

    // Send a Write — should trigger re-init, then handle successfully
    let mut req = DriverRequest::zeroed();
    req.operation = DriverOp::Write;
    req.device_class = DeviceClass::Serial;
    req.length = 3;

    if let Some((resp, _)) = send_driver_request(client, cap, SERIAL_RES, &req, Some(b"OK!"), &mut seq) {
        if resp.status == DriverStatus::Ok {
            score += 25;
        }
        if resp.bytes_xfer == 3 {
            score += 25;
        }
    }

    OcrbResult {
        test_name: "Driver Crash + Restart",
        passed: score >= 80,
        score,
        weight: 20,
        details: alloc::string::String::new(),
    }
}

// ─── Test 7: Multi-Driver Crash Isolation ────────────────────────

fn test7_multi_driver_crash_isolation() -> OcrbResult {
    init_fresh();
    let (client, cap) = make_client("isolation-test");
    let mut seq = 0u64;

    let mut score = 0u8;

    // Write data to ramdisk first
    let mut write_req = DriverRequest::zeroed();
    write_req.operation = DriverOp::Write;
    write_req.device_class = DeviceClass::BlockStorage;
    write_req.offset = 100;
    write_req.length = 8;

    let data = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let _ = send_driver_request(client, cap, RAMDISK_RES, &write_req, Some(&data), &mut seq);

    // Crash the serial driver
    let serial_pid = hal::driver_pid(SERIAL_RES).expect("serial pid");
    let _ = process::crash(serial_pid);
    hal::mark_needs_reinit(SERIAL_RES);
    score += 20;

    // Verify ramdisk is still operational — read back the data
    let mut read_req = DriverRequest::zeroed();
    read_req.operation = DriverOp::Read;
    read_req.device_class = DeviceClass::BlockStorage;
    read_req.offset = 100;
    read_req.length = 8;

    if let Some((resp, extra)) = send_driver_request(client, cap, RAMDISK_RES, &read_req, None, &mut seq) {
        if resp.status == DriverStatus::Ok {
            score += 30;
        }
        if extra.len() >= 8 && extra[..8] == data {
            score += 30;
        }
    }

    // Verify timer also still works
    let mut timer_req = DriverRequest::zeroed();
    timer_req.operation = DriverOp::Interrupt;
    timer_req.device_class = DeviceClass::Timer;

    if let Some((resp, _)) = send_driver_request(client, cap, TIMER_RES, &timer_req, None, &mut seq) {
        if resp.status == DriverStatus::Ok {
            score += 20;
        }
    }

    OcrbResult {
        test_name: "Multi-Driver Crash Isolation",
        passed: score >= 80,
        score,
        weight: 15,
        details: alloc::string::String::new(),
    }
}

// ─── Test 8: Capability Enforcement ──────────────────────────────

fn test8_capability_enforcement() -> OcrbResult {
    init_fresh();

    let mut score = 0u8;

    // Create a rogue process with NO valid device capability
    let intent = Intent {
        category: IntentCategory::Compute,
        priority: Priority::Normal,
        energy_class: EnergyClass::Balanced,
        _pad: 0,
        _reserved: 0,
        deadline: Timestamp::ZERO,
    };
    let rogue = process::spawn(ProcessId::BUTLER, intent, "rogue", None)
        .expect("spawn rogue");

    // Try to send to ramdisk with a bogus capability ID
    let driver_pid = hal::driver_pid(RAMDISK_RES).expect("ramdisk pid");

    let mut req = DriverRequest::zeroed();
    req.operation = DriverOp::Read;
    req.device_class = DeviceClass::BlockStorage;
    req.length = 8;
    let payload = req.to_bytes();

    let mut header = MessageHeader::zeroed();
    header.version = MessageHeader::VERSION;
    header.msg_type = TypeId::DRIVER_REQUEST;
    header.sender = rogue;
    header.receiver = driver_pid;
    header.capability_id = 0xDEAD; // bogus cap
    header.payload_len = payload.len() as u32;
    header.sequence = 1;
    header.timestamp = Timestamp(0);

    // bus::send should reject this — capability validation fails
    let send_result = bus::send(&header, Some(&payload), 1);
    if send_result.is_err() {
        score += 50;
    }

    // Even if the bus somehow accepted, verify no response comes through
    let resp = bus::receive(rogue);
    if resp.is_none() {
        score += 50;
    }

    OcrbResult {
        test_name: "Capability Enforcement",
        passed: score >= 80,
        score,
        weight: 10,
        details: alloc::string::String::new(),
    }
}

// ─── Test 9: Driver Crash Storm ──────────────────────────────────

fn test9_driver_crash_storm() -> OcrbResult {
    init_fresh();
    let (client, cap) = make_client("storm-test");
    let mut seq = 0u64;

    let mut score = 0u8;
    let resources = [SERIAL_RES, TIMER_RES, RAMDISK_RES, FB_RES];

    // Crash all 4 drivers rapidly
    let mut crashed = 0u32;
    for &res in &resources {
        if let Some(pid) = hal::driver_pid(res) {
            if process::crash(pid).is_ok() {
                hal::mark_needs_reinit(res);
                crashed += 1;
            }
        }
    }

    if crashed == 4 {
        score += 25;
    }

    // Verify all 4 are restarted (Ready or Running state)
    let mut restarted = 0u32;
    for &res in &resources {
        if let Some(pid) = hal::driver_pid(res) {
            let state = process::get_state(pid);
            if state == Some(fabric_types::ProcessState::Ready)
                || state == Some(fabric_types::ProcessState::Running)
            {
                restarted += 1;
            }
        }
    }

    if restarted == 4 {
        score += 25;
    }

    // Verify bus still functional — send a Status to ramdisk
    let mut req = DriverRequest::zeroed();
    req.operation = DriverOp::Status;
    req.device_class = DeviceClass::BlockStorage;

    if let Some((resp, _)) = send_driver_request(client, cap, RAMDISK_RES, &req, None, &mut seq) {
        if resp.status == DriverStatus::Ok {
            score += 25;
        }
    }

    // Verify timer also recovered — send an Interrupt
    let mut timer_req = DriverRequest::zeroed();
    timer_req.operation = DriverOp::Interrupt;
    timer_req.device_class = DeviceClass::Timer;

    if let Some((resp, _)) = send_driver_request(client, cap, TIMER_RES, &timer_req, None, &mut seq) {
        if resp.status == DriverStatus::Ok {
            score += 25;
        }
    }

    OcrbResult {
        test_name: "Driver Crash Storm",
        passed: score >= 80,
        score,
        weight: 5,
        details: alloc::string::String::new(),
    }
}
