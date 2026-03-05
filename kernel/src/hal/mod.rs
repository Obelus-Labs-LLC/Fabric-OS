//! Hardware Abstraction Layer — Phase 4 of Fabric OS.
//!
//! Defines the Driver trait, driver registry, synthetic interrupt dispatch,
//! and the driver dispatch loop. All drivers are "simulated userspace" —
//! supervised kernel processes communicating exclusively via the typed
//! message bus with capability enforcement.
//!
//! Public API:
//!   hal::init()              — Spawn and register all reference drivers
//!   hal::dispatch_pending()  — Process bus messages for registered drivers
//!   hal::dispatch_one()      — Dispatch messages for a single driver by resource

#![allow(dead_code)]

pub mod registry;
pub mod interrupt;
pub mod drivers;
pub mod driver_sdk;
pub mod dma;
pub mod irq_router;
pub mod pci_bind;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use crate::sync::OrderedMutex;
use fabric_types::{
    DeviceClass, DriverOp, DriverRequest, DriverResponse, DriverStatus,
    Intent, IntentCategory, Priority, EnergyClass,
    MessageHeader, ProcessId, ResourceId, TypeId, Timestamp,
    Perm, CapabilityId,
};
use crate::{bus, capability, process, serial_println};

pub use registry::DriverRegistry;

/// Global driver registry.
pub static REGISTRY: OrderedMutex<DriverRegistry, { crate::sync::levels::HAL }> =
    OrderedMutex::new(DriverRegistry::new());

/// Maximum ticks a single driver dispatch may consume before warning.
/// This is the Time-Slice Guard — prevents a hung driver from stalling the kernel.
const DISPATCH_TICK_BUDGET: u32 = 50;

// ─── Driver Trait ─────────────────────────────────────────────────

/// The core Driver trait. All simulated userspace drivers implement this.
///
/// Drivers are "pure" — they never touch bus or capability locks directly.
/// The kernel's dispatch loop handles all bus I/O externally. This prevents
/// circular deadlocks and keeps drivers sandboxed.
///
/// Lifecycle:
///   1. Kernel spawns process, creates capability, registers in REGISTRY
///   2. Kernel calls init() once after registration
///   3. For each bus message, kernel calls handle_message()
///   4. On termination, kernel calls shutdown()
///   5. On crash + restart, kernel calls init() again
pub trait Driver: Send {
    /// Human-readable driver name (for logging).
    fn name(&self) -> &'static str;

    /// Which device class this driver serves.
    fn device_class(&self) -> DeviceClass;

    /// The ResourceId this driver manages.
    fn resource_id(&self) -> ResourceId;

    /// Initialize the driver. Called once after spawn (and again after restart).
    fn init(&mut self) -> Result<(), DriverStatus>;

    /// Handle an incoming request. Returns a pure DriverResponse — the driver
    /// must NOT call bus::send() or touch any kernel locks.
    fn handle_message(&mut self, request: &DriverRequest, payload: &[u8]) -> DriverResponse;

    /// Provide read data for a Read operation. Called after handle_message returns
    /// Ok for a Read op. The driver copies data into the provided buffer.
    /// Returns the number of bytes written.
    fn read_data(&self, offset: u32, buf: &mut [u8]) -> u32 {
        let _ = (offset, buf);
        0
    }

    /// Clean shutdown. Called before process termination.
    fn shutdown(&mut self);
}

// ─── Initialization ───────────────────────────────────────────────

/// Initialize the HAL subsystem — spawn and register all reference drivers.
/// Must be called after process::init() and bus::init().
pub fn init() {
    use drivers::{serial_driver, timer_driver, ramdisk_driver, framebuffer_driver};

    // Device resource IDs
    let serial_res  = ResourceId::new(ResourceId::KIND_DEVICE | 0x01);
    let timer_res   = ResourceId::new(ResourceId::KIND_DEVICE | 0x02);
    let ramdisk_res = ResourceId::new(ResourceId::KIND_DEVICE | 0x03);
    let fb_res      = ResourceId::new(ResourceId::KIND_DEVICE | 0x04);

    // Spawn driver processes under Butler
    let serial_pid = process::spawn(
        ProcessId::BUTLER,
        make_driver_intent(IntentCategory::Io, Priority::High),
        "serial-com1",
        None,
    ).expect("spawn serial driver");

    let timer_pid = process::spawn(
        ProcessId::BUTLER,
        make_driver_intent(IntentCategory::Io, Priority::Critical),
        "timer-pit",
        None,
    ).expect("spawn timer driver");

    let ramdisk_pid = process::spawn(
        ProcessId::BUTLER,
        make_driver_intent(IntentCategory::Storage, Priority::Normal),
        "ramdisk-0",
        None,
    ).expect("spawn ramdisk driver");

    let fb_pid = process::spawn(
        ProcessId::BUTLER,
        make_driver_intent(IntentCategory::Display, Priority::Normal),
        "framebuffer-0",
        None,
    ).expect("spawn framebuffer driver");

    // Create KIND_DEVICE capabilities
    let serial_cap = capability::create(
        serial_res, Perm::READ | Perm::WRITE, serial_pid, None, None,
    ).expect("create serial cap");
    let timer_cap = capability::create(
        timer_res, Perm::READ | Perm::WRITE, timer_pid, None, None,
    ).expect("create timer cap");
    let ramdisk_cap = capability::create(
        ramdisk_res, Perm::READ | Perm::WRITE, ramdisk_pid, None, None,
    ).expect("create ramdisk cap");
    let fb_cap = capability::create(
        fb_res, Perm::READ | Perm::WRITE, fb_pid, None, None,
    ).expect("create fb cap");

    // Store capability IDs in PCBs
    {
        let mut table = process::TABLE.lock();
        if let Some(pcb) = table.get_mut(serial_pid)  { pcb.capabilities.push(serial_cap); }
        if let Some(pcb) = table.get_mut(timer_pid)    { pcb.capabilities.push(timer_cap); }
        if let Some(pcb) = table.get_mut(ramdisk_pid)  { pcb.capabilities.push(ramdisk_cap); }
        if let Some(pcb) = table.get_mut(fb_pid)       { pcb.capabilities.push(fb_cap); }
    }

    // Register in HAL registry with trait objects
    let mut reg = REGISTRY.lock();
    reg.register(serial_pid, serial_res, DeviceClass::Serial,
        Box::new(serial_driver::SerialDriver::new())).unwrap();
    reg.register(timer_pid, timer_res, DeviceClass::Timer,
        Box::new(timer_driver::TimerDriver::new())).unwrap();
    reg.register(ramdisk_pid, ramdisk_res, DeviceClass::BlockStorage,
        Box::new(ramdisk_driver::RamDiskDriver::new())).unwrap();
    reg.register(fb_pid, fb_res, DeviceClass::Framebuffer,
        Box::new(framebuffer_driver::FramebufferDriver::new())).unwrap();

    // Initialize each driver
    for entry in reg.entries_mut() {
        match entry.driver.init() {
            Ok(()) => {
                entry.initialized = true;
                serial_println!("[HAL] Driver '{}' initialized ({})",
                    entry.driver.name(), entry.pid);
            }
            Err(status) => {
                serial_println!("[HAL] Driver '{}' init FAILED: {:?}",
                    entry.driver.name(), status);
            }
        }
    }
    drop(reg);

    serial_println!("[HAL] HAL initialized with 4 drivers");
}

// ─── Dispatch ─────────────────────────────────────────────────────

/// Process pending bus messages for all registered drivers.
///
/// For each driver: receive messages from its inbox, deserialize the
/// DriverRequest, call handle_message(), and send a DriverResponse back.
///
/// Drivers never touch locks — they return pure data. The kernel handles
/// all bus::send() calls, preventing circular deadlocks.
pub fn dispatch_pending() {
    // Snapshot PIDs to avoid holding REGISTRY during bus ops
    let driver_pids: Vec<(u64, ProcessId)> = REGISTRY.lock().all_pids();

    for (res_key, pid) in driver_pids {
        // Drain this driver's inbox
        loop {
            // Receive one message (locks BUS, then releases)
            let envelope = bus::receive(pid);
            let envelope = match envelope {
                Some(e) => e,
                None => break,
            };

            // Extract payload bytes from arena
            let (request, payload_bytes) = {
                let bus_guard = bus::BUS.lock();
                let payload = if let Some(slice) = envelope.payload {
                    let data = bus_guard.payload(slice);
                    let mut buf = Vec::new();
                    buf.extend_from_slice(data);
                    buf
                } else {
                    Vec::new()
                };
                drop(bus_guard);

                let req = DriverRequest::from_bytes(&payload)
                    .unwrap_or(DriverRequest::zeroed());
                (req, payload)
            };

            // Dispatch to driver (locks REGISTRY, then releases)
            let (response, sender, read_data, resp_seq) = {
                let mut reg = REGISTRY.lock();
                if let Some(entry) = reg.get_by_resource_key_mut(res_key) {
                    // Re-init if driver was restarted
                    if !entry.initialized {
                        let _ = entry.driver.init();
                        entry.initialized = true;
                    }

                    let resp = entry.driver.handle_message(&request, &payload_bytes);

                    // For Read operations, get the data from the driver
                    let rdata = if request.operation == DriverOp::Read
                        && resp.status == DriverStatus::Ok
                        && resp.bytes_xfer > 0
                    {
                        let mut buf = vec![0u8; resp.bytes_xfer as usize];
                        entry.driver.read_data(request.offset, &mut buf);
                        Some(buf)
                    } else {
                        None
                    };

                    // Increment response sequence counter
                    entry.response_seq += 1;
                    let seq = entry.response_seq;

                    (resp, envelope.header.sender, rdata, seq)
                } else {
                    continue;
                }
            };

            // Send response back to original sender
            let resp_bytes = response.to_bytes();
            let resp_payload: Vec<u8> = if let Some(mut rd) = read_data {
                // Prepend response header, then read data
                let mut combined = Vec::from(resp_bytes.as_slice());
                combined.append(&mut rd);
                combined
            } else {
                Vec::from(resp_bytes.as_slice())
            };

            let mut resp_header = MessageHeader::zeroed();
            resp_header.version = MessageHeader::VERSION;
            resp_header.msg_type = TypeId::DRIVER_RESPONSE;
            resp_header.sender = pid;
            resp_header.receiver = sender;
            resp_header.payload_len = resp_payload.len() as u32;
            resp_header.sequence = resp_seq;
            resp_header.timestamp = Timestamp(0);
            // Use the driver's capability for sending
            if let Some(cap_id) = get_driver_cap(pid) {
                resp_header.capability_id = cap_id.0;
            }

            let _ = bus::send(&resp_header, Some(&resp_payload), resp_seq as u32);
        }
    }
}

/// Process pending messages for a single driver by resource ID.
/// Used in STRESS tests for targeted dispatch.
pub fn dispatch_one(resource_id: ResourceId) {
    let (res_key, pid) = {
        let reg = REGISTRY.lock();
        match reg.get(resource_id) {
            Some(entry) => (resource_id.0, entry.pid),
            None => return,
        }
    };

    // Process one message
    let envelope = match bus::receive(pid) {
        Some(e) => e,
        None => return,
    };

    let (request, payload_bytes) = {
        let bus_guard = bus::BUS.lock();
        let payload = if let Some(slice) = envelope.payload {
            let data = bus_guard.payload(slice);
            let mut buf = Vec::new();
            buf.extend_from_slice(data);
            buf
        } else {
            Vec::new()
        };
        drop(bus_guard);
        let req = DriverRequest::from_bytes(&payload).unwrap_or(DriverRequest::zeroed());
        (req, payload)
    };

    let (response, sender, read_data, resp_seq) = {
        let mut reg = REGISTRY.lock();
        if let Some(entry) = reg.get_by_resource_key_mut(res_key) {
            if !entry.initialized {
                let _ = entry.driver.init();
                entry.initialized = true;
            }
            let resp = entry.driver.handle_message(&request, &payload_bytes);
            let rdata = if request.operation == DriverOp::Read
                && resp.status == DriverStatus::Ok
                && resp.bytes_xfer > 0
            {
                let mut buf = vec![0u8; resp.bytes_xfer as usize];
                entry.driver.read_data(request.offset, &mut buf);
                Some(buf)
            } else {
                None
            };
            entry.response_seq += 1;
            let seq = entry.response_seq;
            (resp, envelope.header.sender, rdata, seq)
        } else {
            return;
        }
    };

    let resp_bytes = response.to_bytes();
    let resp_payload: Vec<u8> = if let Some(mut rd) = read_data {
        let mut combined = Vec::from(resp_bytes.as_slice());
        combined.append(&mut rd);
        combined
    } else {
        Vec::from(resp_bytes.as_slice())
    };

    let mut resp_header = MessageHeader::zeroed();
    resp_header.version = MessageHeader::VERSION;
    resp_header.msg_type = TypeId::DRIVER_RESPONSE;
    resp_header.sender = pid;
    resp_header.receiver = sender;
    resp_header.payload_len = resp_payload.len() as u32;
    resp_header.sequence = resp_seq;
    resp_header.timestamp = Timestamp(0);
    if let Some(cap_id) = get_driver_cap(pid) {
        resp_header.capability_id = cap_id.0;
    }
    let _ = bus::send(&resp_header, Some(&resp_payload), resp_seq as u32);
}

// ─── Helpers ──────────────────────────────────────────────────────

fn make_driver_intent(category: IntentCategory, priority: Priority) -> Intent {
    Intent {
        category,
        priority,
        energy_class: EnergyClass::Balanced,
        _pad: 0,
        _reserved: 0,
        deadline: Timestamp::ZERO,
    }
}

/// Get a driver's first capability ID from its PCB.
fn get_driver_cap(pid: ProcessId) -> Option<CapabilityId> {
    let table = process::TABLE.lock();
    table.get(pid).and_then(|pcb| pcb.capabilities.first().copied())
}

/// Mark a driver as needing re-initialization (e.g., after crash + restart).
pub fn mark_needs_reinit(resource_id: ResourceId) {
    if let Some(entry) = REGISTRY.lock().get_mut(resource_id) {
        entry.initialized = false;
    }
}

/// Get the PID for a driver by resource ID.
pub fn driver_pid(resource_id: ResourceId) -> Option<ProcessId> {
    REGISTRY.lock().get(resource_id).map(|e| e.pid)
}

/// Get the number of registered drivers.
pub fn driver_count() -> usize {
    REGISTRY.lock().count()
}

// ─── Driver Framework (Phase 19) ─────────────────────────────────

/// Initialize the driver framework subsystems.
///
/// Sets up the DMA manager, IRQ router (pre-registers timer on vector 32),
/// and PCI driver table. Called from main.rs Phase 19 boot block.
pub fn driver_framework_init() {
    use irq_router::{IrqHandler, IRQ_ROUTER};

    // Pre-register timer IRQ (vector 32)
    let timer_handler = IrqHandler {
        driver_name: "timer",
        resource_id: 0x0002,
        active: true,
    };
    let _ = IRQ_ROUTER.lock().register(32, timer_handler);

    serial_println!("[HAL] Driver framework initialized");
    serial_println!("[HAL]   DMA manager: {} slots", dma::MAX_DMA_BUFFERS);
    serial_println!("[HAL]   IRQ router: vectors 32-47, {} handlers/vector",
        irq_router::MAX_SHARED);
    serial_println!("[HAL]   PCI driver table: {} slots", pci_bind::MAX_PCI_DRIVERS);
}
