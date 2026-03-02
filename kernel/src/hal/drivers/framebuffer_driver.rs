//! Framebuffer driver — heap-backed 64x64 pixel buffer (4 BPP = 16 KiB).
//!
//! Same Read/Write pattern as RAM disk but for pixel data.
//! Status returns dimensions encoded as (width << 16 | height).

#![allow(dead_code)]

use alloc::boxed::Box;
use fabric_types::{DeviceClass, DriverOp, DriverRequest, DriverResponse, DriverStatus, ResourceId};
use crate::hal::Driver;

/// Framebuffer dimensions.
pub const FB_WIDTH: u32 = 64;
pub const FB_HEIGHT: u32 = 64;
pub const FB_BPP: u32 = 4; // bytes per pixel (RGBA)
pub const FB_SIZE: usize = (FB_WIDTH * FB_HEIGHT * FB_BPP) as usize; // 16384 bytes

/// Heap-backed framebuffer driver.
pub struct FramebufferDriver {
    resource: ResourceId,
    buffer: Box<[u8]>,
    ready: bool,
}

impl FramebufferDriver {
    pub fn new() -> Self {
        Self {
            resource: ResourceId::new(ResourceId::KIND_DEVICE | 0x04),
            buffer: alloc::vec![0u8; FB_SIZE].into_boxed_slice(),
            ready: false,
        }
    }
}

impl Driver for FramebufferDriver {
    fn name(&self) -> &'static str {
        "framebuffer-0"
    }

    fn device_class(&self) -> DeviceClass {
        DeviceClass::Framebuffer
    }

    fn resource_id(&self) -> ResourceId {
        self.resource
    }

    fn init(&mut self) -> Result<(), DriverStatus> {
        // Clear framebuffer on init
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
                if offset + length > FB_SIZE {
                    return DriverResponse::error(DriverStatus::InvalidRequest);
                }

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
                if offset + length > FB_SIZE {
                    return DriverResponse::error(DriverStatus::InvalidRequest);
                }
                DriverResponse::ok(length as u32)
            }

            DriverOp::Status => {
                // Encode dimensions: (width << 16) | height
                let dims = (FB_WIDTH << 16) | FB_HEIGHT;
                DriverResponse::ok(dims)
            }

            _ => DriverResponse::error(DriverStatus::InvalidRequest),
        }
    }

    fn read_data(&self, offset: u32, buf: &mut [u8]) -> u32 {
        let off = offset as usize;
        let len = buf.len().min(FB_SIZE.saturating_sub(off));
        if len > 0 {
            buf[..len].copy_from_slice(&self.buffer[off..off + len]);
        }
        len as u32
    }

    fn shutdown(&mut self) {
        self.ready = false;
    }
}
