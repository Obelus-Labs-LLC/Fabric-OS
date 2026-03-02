//! Synthetic interrupt dispatch — simulates hardware interrupt delivery
//! via the typed message bus.
//!
//! In real hardware, interrupts would arrive via IDT entries. Here, the
//! kernel generates INTERRUPT_EVENT messages and routes them to the
//! appropriate driver via bus::send().

#![allow(dead_code)]

use fabric_types::{
    DriverOp, DriverRequest, DeviceClass, MessageHeader, ProcessId,
    ResourceId, TypeId, Timestamp,
};
use crate::bus;
use super::REGISTRY;

/// Dispatch a synthetic interrupt to a driver by resource ID.
///
/// Constructs a DriverRequest with DriverOp::Interrupt, wraps it in a
/// bus message from KERNEL, and delivers it to the driver's inbox.
/// The driver will process it during the next dispatch_pending() call.
pub fn dispatch_synthetic(resource_id: ResourceId) -> Result<(), &'static str> {
    // Look up driver PID and capability
    let pid = {
        let reg = REGISTRY.lock();
        match reg.get(resource_id) {
            Some(entry) => entry.pid,
            None => return Err("driver not found for resource"),
        }
    };

    // Build the interrupt request payload
    let request = DriverRequest {
        operation: DriverOp::Interrupt,
        device_class: DeviceClass::Timer, // will be overridden by driver's own class
        _pad: [0; 2],
        offset: 0,
        length: 0,
        flags: 0,
        _reserved: [0; 16],
    };
    let payload = request.to_bytes();

    // Build message header — from KERNEL to driver
    let mut header = MessageHeader::zeroed();
    header.version = MessageHeader::VERSION;
    header.msg_type = TypeId::INTERRUPT_EVENT;
    header.sender = ProcessId::KERNEL;
    header.receiver = pid;
    header.payload_len = payload.len() as u32;
    header.sequence = 1;
    header.timestamp = Timestamp(0);

    // Use the driver's capability for the send
    if let Some(cap_id) = super::get_driver_cap(pid) {
        header.capability_id = cap_id.0;
    }

    bus::send(&header, Some(&payload), 1).map_err(|_| "bus send failed for interrupt")
}

/// Dispatch a burst of synthetic interrupts to a driver.
/// Returns the number successfully sent.
pub fn dispatch_burst(resource_id: ResourceId, count: u32) -> u32 {
    let mut sent = 0u32;
    for _ in 0..count {
        if dispatch_synthetic(resource_id).is_ok() {
            sent += 1;
        }
    }
    sent
}
