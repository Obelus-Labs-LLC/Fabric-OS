//! Timer (PIT) driver — tracks synthetic tick events.
//!
//! Handles Interrupt (increment tick counter), Status (return tick count),
//! and Read (return tick count as bytes).

#![allow(dead_code)]

use fabric_types::{DeviceClass, DriverOp, DriverRequest, DriverResponse, DriverStatus, ResourceId};
use crate::hal::Driver;

/// Simulated PIT timer driver.
pub struct TimerDriver {
    resource: ResourceId,
    tick_count: u64,
    ready: bool,
}

impl TimerDriver {
    pub fn new() -> Self {
        Self {
            resource: ResourceId::new(ResourceId::KIND_DEVICE | 0x02),
            tick_count: 0,
            ready: false,
        }
    }
}

impl Driver for TimerDriver {
    fn name(&self) -> &'static str {
        "timer-pit"
    }

    fn device_class(&self) -> DeviceClass {
        DeviceClass::Timer
    }

    fn resource_id(&self) -> ResourceId {
        self.resource
    }

    fn init(&mut self) -> Result<(), DriverStatus> {
        self.tick_count = 0;
        self.ready = true;
        Ok(())
    }

    fn handle_message(&mut self, request: &DriverRequest, _payload: &[u8]) -> DriverResponse {
        if !self.ready {
            return DriverResponse::error(DriverStatus::DeviceNotReady);
        }

        match request.operation {
            DriverOp::Interrupt => {
                self.tick_count += 1;
                DriverResponse::ok(self.tick_count as u32)
            }

            DriverOp::Status => {
                DriverResponse::ok(self.tick_count as u32)
            }

            DriverOp::Read => {
                // Return tick count as 8-byte value via read_data
                DriverResponse::ok(8)
            }

            _ => DriverResponse::error(DriverStatus::InvalidRequest),
        }
    }

    fn read_data(&self, _offset: u32, buf: &mut [u8]) -> u32 {
        if buf.len() >= 8 {
            let bytes = self.tick_count.to_le_bytes();
            buf[..8].copy_from_slice(&bytes);
            8
        } else {
            0
        }
    }

    fn shutdown(&mut self) {
        self.ready = false;
    }
}
