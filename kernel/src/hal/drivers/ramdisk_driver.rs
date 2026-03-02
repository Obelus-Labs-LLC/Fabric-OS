//! RAM Disk driver — heap-backed 64 KiB block device.
//!
//! Handles Read and Write with offset/length bounds checking.
//! Status returns total capacity in bytes_xfer.

#![allow(dead_code)]

use alloc::boxed::Box;
use fabric_types::{DeviceClass, DriverOp, DriverRequest, DriverResponse, DriverStatus, ResourceId};
use crate::hal::Driver;

/// Size of the RAM disk in bytes (64 KiB).
pub const RAMDISK_SIZE: usize = 64 * 1024;

/// Heap-backed RAM disk driver.
pub struct RamDiskDriver {
    resource: ResourceId,
    buffer: Box<[u8]>,
    ready: bool,
}

impl RamDiskDriver {
    pub fn new() -> Self {
        Self {
            resource: ResourceId::new(ResourceId::KIND_DEVICE | 0x03),
            buffer: alloc::vec![0u8; RAMDISK_SIZE].into_boxed_slice(),
            ready: false,
        }
    }
}

impl Driver for RamDiskDriver {
    fn name(&self) -> &'static str {
        "ramdisk-0"
    }

    fn device_class(&self) -> DeviceClass {
        DeviceClass::BlockStorage
    }

    fn resource_id(&self) -> ResourceId {
        self.resource
    }

    fn init(&mut self) -> Result<(), DriverStatus> {
        // Zero the buffer on init (clean slate after crash+restart)
        for byte in self.buffer.iter_mut() {
            *byte = 0;
        }
        self.ready = true;
        Ok(())
    }

    fn handle_message(&mut self, request: &DriverRequest, payload: &[u8]) -> DriverResponse {
        if !self.ready {
            return DriverResponse::error(DriverStatus::DeviceNotReady);
        }

        let offset = request.offset as usize;
        let length = request.length as usize;

        match request.operation {
            DriverOp::Write => {
                // Bounds check
                if offset + length > RAMDISK_SIZE {
                    return DriverResponse::error(DriverStatus::InvalidRequest);
                }

                // Write data from payload (after DriverRequest header)
                let data_offset = DriverRequest::SIZE;
                let data = if payload.len() > data_offset {
                    &payload[data_offset..]
                } else {
                    &[]
                };

                let copy_len = length.min(data.len());
                self.buffer[offset..offset + copy_len].copy_from_slice(&data[..copy_len]);
                DriverResponse::ok(copy_len as u32)
            }

            DriverOp::Read => {
                // Bounds check
                if offset + length > RAMDISK_SIZE {
                    return DriverResponse::error(DriverStatus::InvalidRequest);
                }
                // Data will be provided via read_data()
                DriverResponse::ok(length as u32)
            }

            DriverOp::Status => {
                DriverResponse::ok(RAMDISK_SIZE as u32)
            }

            _ => DriverResponse::error(DriverStatus::InvalidRequest),
        }
    }

    fn read_data(&self, offset: u32, buf: &mut [u8]) -> u32 {
        let off = offset as usize;
        let len = buf.len().min(RAMDISK_SIZE.saturating_sub(off));
        if len > 0 {
            buf[..len].copy_from_slice(&self.buffer[off..off + len]);
        }
        len as u32
    }

    fn shutdown(&mut self) {
        self.ready = false;
    }
}
