//! WM Compositor — renders all windows with decorations onto the display.
//!
//! compose_and_present() iterates windows bottom-to-top by z-order, drawing
//! each window's decorations (title bar, close button, border) and client
//! surface content, then blits the compose surface to the hardware framebuffer.

#![allow(dead_code)]

use crate::display::compositor::Surface;
use crate::display::framebuffer::FramebufferInfo;
use crate::display::text::{FONT_WIDTH, FONT_HEIGHT, FONT_DATA};
use crate::display::Color;
use super::{TITLE_BAR_HEIGHT, CLOSE_BUTTON_SIZE, TASKBAR_HEIGHT, WindowId};
use super::WINDOW_TABLE;
use crate::display::DISPLAY;

// ── Theme Colors ─────────────────────────────────────────────────────

/// Desktop background color (Nord-inspired dark blue).
const DESKTOP_BG: Color = Color::new(0x2E, 0x34, 0x40);

/// Title bar color for focused windows.
const TITLE_FOCUSED: Color = Color::new(0x3B, 0x4C, 0x68);

/// Title bar color for unfocused windows.
const TITLE_UNFOCUSED: Color = Color::new(0x55, 0x55, 0x55);

/// Close button background color.
const CLOSE_BG: Color = Color::new(0xC0, 0x39, 0x2B);

/// Taskbar background color.
const TASKBAR_BG: Color = Color::new(0x1A, 0x1A, 0x2E);

// ── Clipped Drawing Helpers ──────────────────────────────────────────

/// Set a pixel with i32 coordinates, clipping to surface bounds.
fn set_clipped(surface: &mut Surface, x: i32, y: i32, packed: u32) {
    if x >= 0 && y >= 0 && (x as u32) < surface.width && (y as u32) < surface.height {
        surface.pixel(x as u32, y as u32, packed);
    }
}

/// Draw a filled rectangle with i32 coordinates, clipping to surface bounds.
fn draw_clipped_filled_rect(
    surface: &mut Surface,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    packed: u32,
) {
    for dy in 0..h as i32 {
        for dx in 0..w as i32 {
            set_clipped(surface, x + dx, y + dy, packed);
        }
    }
}

/// Draw a single character with i32 coordinates, clipping to surface bounds.
fn draw_clipped_char(surface: &mut Surface, x: i32, y: i32, ch: char, fg: u32, bg: u32) {
    let code = ch as u8;
    let glyph_idx = if code >= 0x20 && code <= 0x7E {
        (code - 0x20) as usize
    } else {
        0 // unprintable -> space
    };

    let glyph_offset = glyph_idx * FONT_HEIGHT as usize;

    for row in 0..FONT_HEIGHT as i32 {
        let byte = FONT_DATA[glyph_offset + row as usize];
        for col in 0..FONT_WIDTH as i32 {
            let bit = (byte >> (7 - col)) & 1;
            let color = if bit != 0 { fg } else { bg };
            set_clipped(surface, x + col, y + row, color);
        }
    }
}

/// Draw a string with i32 coordinates, clipping to surface bounds.
fn draw_clipped_string(surface: &mut Surface, x: i32, y: i32, s: &str, fg: u32, bg: u32) {
    let mut cx = x;
    for ch in s.chars() {
        if ch == '\n' {
            continue;
        }
        draw_clipped_char(surface, cx, y, ch, fg, bg);
        cx += FONT_WIDTH as i32;
    }
}

/// Blit a source surface onto the destination at i32 coordinates, clipping.
fn blit_surface_clipped(
    dst: &mut Surface,
    src: &Surface,
    dst_x: i32,
    dst_y: i32,
) {
    for row in 0..src.height as i32 {
        for col in 0..src.width as i32 {
            let px = src.get_pixel(col as u32, row as u32);
            set_clipped(dst, dst_x + col, dst_y + row, px);
        }
    }
}

// ── Window Drawing ───────────────────────────────────────────────────

/// Draw a single window with decorations onto the compose surface.
fn draw_window(
    compose: &mut Surface,
    window: &super::Window,
    fb: &FramebufferInfo,
) {
    let x = window.x;
    let y = window.y;
    let w = window.width;
    let h = window.height;

    if window.decorated {
        // Title bar
        let title_color = if window.focused { TITLE_FOCUSED } else { TITLE_UNFOCUSED };
        let title_packed = title_color.to_packed(fb);
        draw_clipped_filled_rect(compose, x, y, w, TITLE_BAR_HEIGHT, title_packed);

        // Title text (centered vertically in title bar)
        let text_y = y + ((TITLE_BAR_HEIGHT as i32 - FONT_HEIGHT as i32) / 2);
        let text_x = x + 4;
        let white = Color::WHITE.to_packed(fb);
        draw_clipped_string(compose, text_x, text_y, &window.title, white, title_packed);

        // Close button (right side of title bar)
        let close_x = x + w as i32 - CLOSE_BUTTON_SIZE as i32 - 2;
        let close_y = y + 2;
        let close_packed = CLOSE_BG.to_packed(fb);
        draw_clipped_filled_rect(compose, close_x, close_y, CLOSE_BUTTON_SIZE, CLOSE_BUTTON_SIZE, close_packed);

        // "X" in close button
        let x_char_x = close_x + ((CLOSE_BUTTON_SIZE as i32 - FONT_WIDTH as i32) / 2);
        let x_char_y = close_y + ((CLOSE_BUTTON_SIZE as i32 - FONT_HEIGHT as i32) / 2);
        draw_clipped_char(compose, x_char_x, x_char_y, 'X', white, close_packed);

        // Border (1px, white for focused, gray for unfocused)
        let border_color = if window.focused {
            Color::WHITE.to_packed(fb)
        } else {
            Color::GRAY.to_packed(fb)
        };

        // Total window area = title bar + client area
        let total_h = TITLE_BAR_HEIGHT + h;

        // Top border
        for dx in 0..w as i32 {
            set_clipped(compose, x + dx, y, border_color);
        }
        // Bottom border
        for dx in 0..w as i32 {
            set_clipped(compose, x + dx, y + total_h as i32 - 1, border_color);
        }
        // Left border
        for dy in 0..total_h as i32 {
            set_clipped(compose, x, y + dy, border_color);
        }
        // Right border
        for dy in 0..total_h as i32 {
            set_clipped(compose, x + w as i32 - 1, y + dy, border_color);
        }

        // Client area content (below title bar)
        let client_y = y + TITLE_BAR_HEIGHT as i32;
        blit_surface_clipped(compose, &window.surface, x, client_y);
    } else {
        // No decorations — just blit the surface directly
        blit_surface_clipped(compose, &window.surface, x, y);
    }
}

// ── Taskbar Drawing ──────────────────────────────────────────────────

/// Draw the taskbar at the bottom of the screen.
fn draw_taskbar(
    compose: &mut Surface,
    fb: &FramebufferInfo,
    focused_id: Option<WindowId>,
) {
    let screen_h = compose.height as i32;
    let taskbar_y = screen_h - TASKBAR_HEIGHT as i32;

    // Taskbar background
    let taskbar_packed = TASKBAR_BG.to_packed(fb);
    draw_clipped_filled_rect(compose, 0, taskbar_y, compose.width, TASKBAR_HEIGHT, taskbar_packed);

    // Draw window titles in taskbar
    let wt = WINDOW_TABLE.lock();
    let sorted = wt.sorted_by_z();
    let mut tx = 8i32;
    let text_y = taskbar_y + ((TASKBAR_HEIGHT as i32 - FONT_HEIGHT as i32) / 2);

    for wid in sorted.iter() {
        if let Some(win) = wt.get(*wid) {
            let (fg, bg) = if focused_id == Some(*wid) {
                // Focused window: white on lighter background
                let highlight = Color::new(0x3B, 0x4C, 0x68).to_packed(fb);
                (Color::WHITE.to_packed(fb), highlight)
            } else {
                (Color::GRAY.to_packed(fb), taskbar_packed)
            };

            // Draw a highlight box for focused items
            if focused_id == Some(*wid) {
                let title_pixel_w = (win.title.len() as u32 + 2) * FONT_WIDTH;
                draw_clipped_filled_rect(compose, tx - 4, taskbar_y + 2, title_pixel_w + 8, TASKBAR_HEIGHT - 4, bg);
            }

            // Draw " Title "
            draw_clipped_char(compose, tx, text_y, ' ', fg, bg);
            tx += FONT_WIDTH as i32;
            draw_clipped_string(compose, tx, text_y, &win.title, fg, bg);
            tx += (win.title.len() as i32) * FONT_WIDTH as i32;
            draw_clipped_char(compose, tx, text_y, ' ', fg, bg);
            tx += FONT_WIDTH as i32 + 8; // gap between entries
        }
    }
}

// ── Main Compose Entry Point ─────────────────────────────────────────

/// Compose all windows and present to the hardware framebuffer.
///
/// This is the main rendering entry point called after any visual change.
/// It clears the compose surface, draws the desktop background, renders
/// all windows bottom-to-top by z-order, draws the taskbar, then blits
/// the final image to the hardware framebuffer.
pub fn compose_and_present() {
    let mut display_lock = DISPLAY.lock();
    let ds = match display_lock.as_mut() {
        Some(ds) => ds,
        None => return, // display not initialized
    };

    let fb = &ds.fb;
    let compose = &mut ds.surface;

    // Clear to desktop background
    let bg_packed = DESKTOP_BG.to_packed(fb);
    compose.clear(bg_packed);

    // Get window data from the window table
    let wt = WINDOW_TABLE.lock();
    let sorted = wt.sorted_by_z();
    let focused_id = wt.focused_id;

    // Draw windows bottom-to-top
    for wid in sorted.iter() {
        if let Some(win) = wt.get(*wid) {
            draw_window(compose, win, fb);
        }
    }

    // Must drop window table lock before drawing taskbar (which re-acquires it)
    drop(wt);

    // Draw taskbar
    draw_taskbar(compose, fb, focused_id);

    // Present to hardware framebuffer
    crate::display::compositor::present(compose, fb);
}

/// Compose and present without acquiring the DISPLAY lock (caller holds it).
/// Used when the display state is already locked.
pub fn compose_with_surface(compose: &mut Surface, fb: &FramebufferInfo) {
    // Clear to desktop background
    let bg_packed = DESKTOP_BG.to_packed(fb);
    compose.clear(bg_packed);

    // Get window data
    let wt = WINDOW_TABLE.lock();
    let sorted = wt.sorted_by_z();
    let focused_id = wt.focused_id;

    // Draw windows bottom-to-top
    for wid in sorted.iter() {
        if let Some(win) = wt.get(*wid) {
            draw_window(compose, win, fb);
        }
    }

    drop(wt);

    // Draw taskbar
    draw_taskbar(compose, fb, focused_id);

    // Present to hardware framebuffer
    crate::display::compositor::present(compose, fb);
}
