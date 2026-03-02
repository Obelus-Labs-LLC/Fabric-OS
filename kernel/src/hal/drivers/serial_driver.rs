//! Serial COM1 driver — wraps the hardware serial port for bus-based I/O.
//!
//! Handles Write (output bytes to COM1) and Status (returns device ready).
//! Read returns InvalidRequest — no interrupt-driven RX yet.

#![allow(dead_code)]

use fabric_types::{DeviceClass, DriverOp, DriverRequest, DriverResponse, DriverStatus, ResourceId};
use crate::hal::Driver;

/// Serial COM1 driver.
pub struct SerialDriver {
    resource: ResourceId,
    bytes_written: u32,
    ready: bool,
}

impl SerialDriver {
    pub fn new() -> Self {
        Self {
            resource: ResourceId::new(ResourceId::KIND_DEVICE | 0x01),
            bytes_written: 0,
            ready: false,
        }
    }
}

impl Driver for SerialDriver {
    fn name(&self) -> &'static str {
        "serial-com1"
    }

    fn device_class(&self) -> DeviceClass {
        DeviceClass::Serial
    }

    fn resource_id(&self) -> ResourceId {
        self.resource
    }

    fn init(&mut self) -> Result<(), DriverStatus> {
        // Hardware already initialized by serial::init() at boot.
        // Just mark ourselves as ready.
        self.ready = true;
        self.bytes_written = 0;
        Ok(())
    }

    fn handle_message(&mut self, request: &DriverRequest, payload: &[u8]) -> DriverResponse {
        if !self.ready {
            return DriverResponse::error(DriverStatus::DeviceNotReady);
        }

        match request.operation {
            DriverOp::Write => {
                // Write payload bytes beyond the DriverRequest header to COM1
                let data_offset = DriverRequest::SIZE;
                let data = if payload.len() > data_offset {
                    &payload[data_offset..]
                } else {
                    &[]
                };

                let len = data.len().min(request.length as usize);
                for &byte in &data[..len] {
                    crate::serial::SERIAL.lock().write_byte(byte);
                }
                self.bytes_written += len as u32;
                DriverResponse::ok(len as u32)
            }

            DriverOp::Status => {
                // Return bytes_written in the bytes_xfer field
                DriverResponse::ok(self.bytes_written)
            }

            DriverOp::Read => {
                // No RX support yet
                DriverResponse::error(DriverStatus::InvalidRequest)
            }

            _ => DriverResponse::error(DriverStatus::InvalidRequest),
        }
    }

    fn shutdown(&mut self) {
        self.ready = false;
    }
}
