//! Compositor — single full-screen surface with double buffering.
//!
//! All drawing operations target the Surface (back buffer). When ready,
//! present() blits the entire back buffer to the hardware framebuffer.

#![allow(dead_code)]

extern crate alloc;

use alloc::boxed::Box;
use super::framebuffer::FramebufferInfo;

/// Heap-allocated back buffer for double-buffered rendering.
///
/// Pixels are stored as packed u32 values using the hardware pixel format.
/// Layout: row-major, buffer[y * width + x].
pub struct Surface {
    /// Packed pixel data (width * height entries).
    pub buffer: Box<[u32]>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Whether the surface has been modified since last present().
    pub dirty: bool,
}

impl Surface {
    /// Allocate a new surface. Returns None if heap allocation fails.
    pub fn new(width: u32, height: u32) -> Option<Self> {
        let len = (width as usize) * (height as usize);
        if len == 0 {
            return None;
        }
        // Try to allocate; on failure return None
        let buffer = alloc::vec![0u32; len].into_boxed_slice();
        Some(Self {
            buffer,
            width,
            height,
            dirty: false,
        })
    }

    /// Set a pixel in the back buffer.
    pub fn pixel(&mut self, x: u32, y: u32, packed: u32) {
        if x < self.width && y < self.height {
            let idx = (y as usize) * (self.width as usize) + (x as usize);
            self.buffer[idx] = packed;
            self.dirty = true;
        }
    }

    /// Read a pixel from the back buffer.
    pub fn get_pixel(&self, x: u32, y: u32) -> u32 {
        if x < self.width && y < self.height {
            let idx = (y as usize) * (self.width as usize) + (x as usize);
            self.buffer[idx]
        } else {
            0
        }
    }

    /// Clear the entire back buffer to a packed color.
    pub fn clear(&mut self, packed: u32) {
        for px in self.buffer.iter_mut() {
            *px = packed;
        }
        self.dirty = true;
    }

    /// Total size of the back buffer in bytes.
    pub fn size_bytes(&self) -> usize {
        self.buffer.len() * 4
    }
}

/// Blit the entire back buffer to the hardware framebuffer.
///
/// Copies each pixel from the surface to the framebuffer, respecting the
/// framebuffer's pitch (which may differ from width * bytes_per_pixel).
pub fn present(surface: &Surface, fb: &FramebufferInfo) {
    let w = surface.width.min(fb.width);
    let h = surface.height.min(fb.height);
    let bpp = fb.bytes_per_pixel() as usize;

    for y in 0..h {
        let src_offset = (y as usize) * (surface.width as usize);
        let dst_row_base = (y as usize) * (fb.pitch as usize);

        for x in 0..w {
            let packed = surface.buffer[src_offset + x as usize];
            let dst_offset = dst_row_base + (x as usize) * bpp;
            if dst_offset + 3 < fb.size {
                unsafe {
                    let ptr = fb.base.add(dst_offset) as *mut u32;
                    ptr.write_volatile(packed);
                }
            }
        }
    }
}
