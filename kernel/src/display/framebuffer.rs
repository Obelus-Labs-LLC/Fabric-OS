//! Framebuffer hardware access — maps the Limine-provided framebuffer.
//!
//! FramebufferInfo stores metadata (resolution, pitch, pixel format) and
//! provides raw pixel write/blit/clear operations on the hardware buffer.
//! The address is already virtual (Limine HHDM-mapped).

#![allow(dead_code)]

use super::Color;

/// Hardware framebuffer metadata and raw access.
pub struct FramebufferInfo {
    /// Virtual address of the hardware framebuffer memory.
    pub base: *mut u8,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row (may include padding beyond width * bytes_per_pixel).
    pub pitch: u32,
    /// Bits per pixel (typically 32).
    pub bpp: u16,
    /// Bit shift for the red channel.
    pub red_shift: u8,
    /// Bit shift for the green channel.
    pub green_shift: u8,
    /// Bit shift for the blue channel.
    pub blue_shift: u8,
    /// Total framebuffer size in bytes (pitch * height).
    pub size: usize,
}

// Safety: The framebuffer pointer is valid for the lifetime of the kernel
// and is only accessed through synchronized methods.
unsafe impl Send for FramebufferInfo {}
unsafe impl Sync for FramebufferInfo {}

impl FramebufferInfo {
    /// Create a new FramebufferInfo from Limine framebuffer data.
    pub fn new(
        base: *mut u8,
        width: u64,
        height: u64,
        pitch: u64,
        bpp: u16,
        red_shift: u8,
        green_shift: u8,
        blue_shift: u8,
    ) -> Self {
        Self {
            base,
            width: width as u32,
            height: height as u32,
            pitch: pitch as u32,
            bpp,
            red_shift,
            green_shift,
            blue_shift,
            size: (pitch as usize) * (height as usize),
        }
    }

    /// Bytes per pixel (bpp / 8).
    pub fn bytes_per_pixel(&self) -> u32 {
        (self.bpp as u32) / 8
    }

    /// Write a single pixel to the hardware framebuffer.
    ///
    /// `packed` is the color encoded with the hardware pixel format shifts.
    pub fn put_pixel(&self, x: u32, y: u32, packed: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y as usize) * (self.pitch as usize) + (x as usize) * (self.bytes_per_pixel() as usize);
        if offset + 3 < self.size {
            unsafe {
                let ptr = self.base.add(offset) as *mut u32;
                ptr.write_volatile(packed);
            }
        }
    }

    /// Read a pixel from the hardware framebuffer.
    pub fn get_pixel(&self, x: u32, y: u32) -> u32 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        let offset = (y as usize) * (self.pitch as usize) + (x as usize) * (self.bytes_per_pixel() as usize);
        if offset + 3 < self.size {
            unsafe {
                let ptr = self.base.add(offset) as *const u32;
                ptr.read_volatile()
            }
        } else {
            0
        }
    }

    /// Blit a packed pixel buffer to the hardware framebuffer at (x, y).
    ///
    /// `src` is a row-major array of packed u32 pixels.
    /// `src_width` and `src_height` describe the source dimensions.
    pub fn blit_buffer(&self, src: &[u32], src_width: u32, src_height: u32, dst_x: u32, dst_y: u32) {
        for row in 0..src_height {
            let dy = dst_y + row;
            if dy >= self.height {
                break;
            }
            for col in 0..src_width {
                let dx = dst_x + col;
                if dx >= self.width {
                    break;
                }
                let src_idx = (row * src_width + col) as usize;
                if src_idx < src.len() {
                    self.put_pixel(dx, dy, src[src_idx]);
                }
            }
        }
    }

    /// Clear the entire hardware framebuffer to a packed color.
    pub fn clear(&self, packed: u32) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.put_pixel(x, y, packed);
            }
        }
    }

    /// Encode a Color into a packed u32 using this framebuffer's shifts.
    pub fn pack_color(&self, color: Color) -> u32 {
        color.to_packed(self)
    }
}
