//! Drawing primitives — pixel, rect, line, blit, clear.
//!
//! All operations target a Surface (back buffer). Call compositor::present()
//! after drawing to update the hardware framebuffer.

#![allow(dead_code)]

use super::compositor::Surface;

/// Draw a single pixel.
pub fn draw_pixel(surface: &mut Surface, x: u32, y: u32, packed: u32) {
    surface.pixel(x, y, packed);
}

/// Draw a rectangle outline (1px border).
pub fn draw_rect(surface: &mut Surface, x: u32, y: u32, w: u32, h: u32, packed: u32) {
    if w == 0 || h == 0 {
        return;
    }
    // Top and bottom edges
    for dx in 0..w {
        surface.pixel(x + dx, y, packed);
        surface.pixel(x + dx, y + h - 1, packed);
    }
    // Left and right edges (skip corners already drawn)
    for dy in 1..h.saturating_sub(1) {
        surface.pixel(x, y + dy, packed);
        surface.pixel(x + w - 1, y + dy, packed);
    }
}

/// Draw a filled rectangle.
pub fn draw_filled_rect(surface: &mut Surface, x: u32, y: u32, w: u32, h: u32, packed: u32) {
    for dy in 0..h {
        for dx in 0..w {
            surface.pixel(x + dx, y + dy, packed);
        }
    }
}

/// Draw a line using Bresenham's algorithm.
pub fn draw_line(surface: &mut Surface, x0: i32, y0: i32, x1: i32, y1: i32, packed: u32) {
    let mut x = x0;
    let mut y = y0;

    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx: i32 = if x0 < x1 { 1 } else { -1 };
    let sy: i32 = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if x >= 0 && y >= 0 {
            surface.pixel(x as u32, y as u32, packed);
        }

        if x == x1 && y == y1 {
            break;
        }

        let e2 = 2 * err;
        if e2 >= dy {
            if x == x1 {
                break;
            }
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            if y == y1 {
                break;
            }
            err += dx;
            y += sy;
        }
    }
}

/// Blit a source pixel buffer onto the surface at (dst_x, dst_y).
///
/// `src` is row-major packed u32 pixels, `src_width` x `src_height`.
pub fn blit(
    surface: &mut Surface,
    src: &[u32],
    src_width: u32,
    src_height: u32,
    dst_x: u32,
    dst_y: u32,
) {
    for row in 0..src_height {
        for col in 0..src_width {
            let src_idx = (row * src_width + col) as usize;
            if src_idx < src.len() {
                surface.pixel(dst_x + col, dst_y + row, src[src_idx]);
            }
        }
    }
}

/// Clear the entire surface to a packed color.
pub fn clear(surface: &mut Surface, packed: u32) {
    surface.clear(packed);
}
