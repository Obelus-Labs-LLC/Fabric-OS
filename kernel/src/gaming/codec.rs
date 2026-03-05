//! Simple media codecs: pixel formats, video frames, RLE compression.
//!
//! Provides VideoFrame with per-pixel access and conversion to the
//! window manager's packed pixel format. RLE codec enables basic
//! frame delta compression for streaming.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use alloc::vec;

/// Pixel format for video frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// 3 bytes per pixel: R, G, B
    Rgb888,
    /// 4 bytes per pixel: R, G, B, A
    Rgba8888,
    /// 3 bytes per pixel: B, G, R
    Bgr888,
}

/// Audio codec identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioCodec {
    /// Signed 16-bit little-endian PCM.
    PcmS16Le,
}

/// A video frame with pixel data.
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}

impl VideoFrame {
    /// Create a new zeroed video frame.
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        let bpp = Self::bytes_per_pixel_for(format);
        let size = (width as usize) * (height as usize) * bpp;
        VideoFrame {
            width,
            height,
            format,
            data: vec![0u8; size],
        }
    }

    /// Bytes per pixel for a given format.
    pub fn bytes_per_pixel_for(format: PixelFormat) -> usize {
        match format {
            PixelFormat::Rgb888 => 3,
            PixelFormat::Rgba8888 => 4,
            PixelFormat::Bgr888 => 3,
        }
    }

    /// Bytes per pixel for this frame's format.
    pub fn bytes_per_pixel(&self) -> usize {
        Self::bytes_per_pixel_for(self.format)
    }

    /// Get pixel offset in data array.
    fn pixel_offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let bpp = self.bytes_per_pixel();
        Some((y as usize * self.width as usize + x as usize) * bpp)
    }

    /// Read pixel as (R, G, B). Returns (0,0,0) if out of bounds.
    pub fn pixel_at(&self, x: u32, y: u32) -> (u8, u8, u8) {
        let offset = match self.pixel_offset(x, y) {
            Some(o) => o,
            None => return (0, 0, 0),
        };

        match self.format {
            PixelFormat::Rgb888 => {
                (self.data[offset], self.data[offset + 1], self.data[offset + 2])
            }
            PixelFormat::Rgba8888 => {
                (self.data[offset], self.data[offset + 1], self.data[offset + 2])
            }
            PixelFormat::Bgr888 => {
                (self.data[offset + 2], self.data[offset + 1], self.data[offset])
            }
        }
    }

    /// Set pixel as (R, G, B).
    pub fn set_pixel(&mut self, x: u32, y: u32, r: u8, g: u8, b: u8) {
        let offset = match self.pixel_offset(x, y) {
            Some(o) => o,
            None => return,
        };

        match self.format {
            PixelFormat::Rgb888 => {
                self.data[offset] = r;
                self.data[offset + 1] = g;
                self.data[offset + 2] = b;
            }
            PixelFormat::Rgba8888 => {
                self.data[offset] = r;
                self.data[offset + 1] = g;
                self.data[offset + 2] = b;
                self.data[offset + 3] = 255; // full alpha
            }
            PixelFormat::Bgr888 => {
                self.data[offset] = b;
                self.data[offset + 1] = g;
                self.data[offset + 2] = r;
            }
        }
    }

    /// Convert pixel at (x,y) to packed u32 for WM Surface.
    /// Format: (R << 16) | (G << 8) | B — matches display module convention.
    pub fn to_surface_packed(&self, x: u32, y: u32) -> u32 {
        let (r, g, b) = self.pixel_at(x, y);
        ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
    }

    /// Fill entire frame with a solid color.
    pub fn fill(&mut self, r: u8, g: u8, b: u8) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.set_pixel(x, y, r, g, b);
            }
        }
    }
}

/// Run-length encode a byte slice.
/// Output format: [count, value] pairs. Max run length 255.
pub fn rle_encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut i = 0;

    while i < data.len() {
        let value = data[i];
        let mut count: u8 = 1;

        while i + (count as usize) < data.len()
            && data[i + count as usize] == value
            && count < 255
        {
            count += 1;
        }

        result.push(count);
        result.push(value);
        i += count as usize;
    }

    result
}

/// Decode a run-length encoded byte slice.
pub fn rle_decode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();

    let mut i = 0;
    while i + 1 < data.len() {
        let count = data[i] as usize;
        let value = data[i + 1];
        for _ in 0..count {
            result.push(value);
        }
        i += 2;
    }

    result
}
