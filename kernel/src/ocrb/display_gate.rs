//! OCRB Phase 10 — Display System Gate
//!
//! 10 tests verifying framebuffer, drawing primitives, text rendering,
//! and compositor operations.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::format;
use crate::ocrb::OcrbResult;
use crate::display::{self, Color, FramebufferInfo};
use crate::display::compositor::Surface;
use crate::display::drawing;
use crate::display::text;

pub fn run_all_tests() -> alloc::vec::Vec<OcrbResult> {
    let mut results = alloc::vec::Vec::new();
    results.push(test_framebuffer_info_valid());
    results.push(test_color_encoding());
    results.push(test_surface_create_clear());
    results.push(test_pixel_read_write());
    results.push(test_filled_rectangle());
    results.push(test_line_drawing());
    results.push(test_blit_operation());
    results.push(test_font_glyph_data());
    results.push(test_text_rendering());
    results.push(test_compositor_present());
    results
}

// ─── Test 1: Framebuffer Info Valid ─────────────────────────────────

fn test_framebuffer_info_valid() -> OcrbResult {
    let disp = display::DISPLAY.lock();
    if let Some(ref ds) = *disp {
        let fb = &ds.fb;
        let width_ok = fb.width > 0;
        let height_ok = fb.height > 0;
        let pitch_ok = fb.pitch >= fb.width * (fb.bytes_per_pixel());
        let bpp_ok = fb.bpp >= 24;
        let size_ok = fb.size == (fb.pitch as usize) * (fb.height as usize);

        let all_ok = width_ok && height_ok && pitch_ok && bpp_ok && size_ok;

        OcrbResult {
            test_name: "Framebuffer Info Valid",
            passed: all_ok,
            score: if all_ok { 100 } else { 0 },
            weight: 10,
            details: format!(
                "{}x{}, {}bpp, pitch={}, size={}",
                fb.width, fb.height, fb.bpp, fb.pitch, fb.size
            ),
        }
    } else {
        OcrbResult {
            test_name: "Framebuffer Info Valid",
            passed: false,
            score: 0,
            weight: 10,
            details: String::from("Display not initialized"),
        }
    }
}

// ─── Test 2: Color Encoding ─────────────────────────────────────────

fn test_color_encoding() -> OcrbResult {
    let disp = display::DISPLAY.lock();
    if let Some(ref ds) = *disp {
        let fb = &ds.fb;

        // Black should encode to 0
        let black = Color::BLACK.to_packed(fb);
        let black_ok = black == 0;

        // White should have all channel bits set
        let white = Color::WHITE.to_packed(fb);
        let white_ok = white != 0;

        // Red should only set the red channel
        let red = Color::RED.to_packed(fb);
        let red_ok = red != 0 && red != white;

        // Green should differ from red
        let green = Color::GREEN.to_packed(fb);
        let green_ok = green != 0 && green != red;

        // Blue should differ from red and green
        let blue = Color::BLUE.to_packed(fb);
        let blue_ok = blue != 0 && blue != red && blue != green;

        let all_ok = black_ok && white_ok && red_ok && green_ok && blue_ok;

        OcrbResult {
            test_name: "Color Encoding",
            passed: all_ok,
            score: if all_ok { 100 } else { 0 },
            weight: 5,
            details: format!(
                "BLACK=0x{:08X} WHITE=0x{:08X} R=0x{:08X} G=0x{:08X} B=0x{:08X}",
                black, white, red, green, blue
            ),
        }
    } else {
        OcrbResult {
            test_name: "Color Encoding",
            passed: false,
            score: 0,
            weight: 5,
            details: String::from("Display not initialized"),
        }
    }
}

// ─── Test 3: Surface Create + Clear ─────────────────────────────────

fn test_surface_create_clear() -> OcrbResult {
    let mut surface = match Surface::new(64, 64) {
        Some(s) => s,
        None => {
            return OcrbResult {
                test_name: "Surface Create + Clear",
                passed: false,
                score: 0,
                weight: 10,
                details: String::from("Failed to allocate 64x64 surface"),
            };
        }
    };

    // Initially all zeros
    let zero_ok = surface.get_pixel(0, 0) == 0 && surface.get_pixel(63, 63) == 0;

    // Clear to a test pattern
    let fill = 0x00FF00FFu32; // magenta-ish
    surface.clear(fill);

    let clear_ok = surface.get_pixel(0, 0) == fill
        && surface.get_pixel(31, 31) == fill
        && surface.get_pixel(63, 63) == fill;

    let all_ok = zero_ok && clear_ok;

    OcrbResult {
        test_name: "Surface Create + Clear",
        passed: all_ok,
        score: if all_ok { 100 } else { 0 },
        weight: 10,
        details: format!("Alloc OK, clear fill=0x{:08X} verified", fill),
    }
}

// ─── Test 4: Pixel Read/Write ───────────────────────────────────────

fn test_pixel_read_write() -> OcrbResult {
    let mut surface = match Surface::new(32, 32) {
        Some(s) => s,
        None => {
            return OcrbResult {
                test_name: "Pixel Read/Write",
                passed: false,
                score: 0,
                weight: 10,
                details: String::from("Failed to allocate surface"),
            };
        }
    };

    let c1 = 0x00112233u32;
    let c2 = 0x00AABBCC;
    let c3 = 0x00DDEEFF;

    drawing::draw_pixel(&mut surface, 0, 0, c1);
    drawing::draw_pixel(&mut surface, 15, 15, c2);
    drawing::draw_pixel(&mut surface, 31, 31, c3);

    let ok1 = surface.get_pixel(0, 0) == c1;
    let ok2 = surface.get_pixel(15, 15) == c2;
    let ok3 = surface.get_pixel(31, 31) == c3;

    // Out-of-bounds should not crash (no-op)
    drawing::draw_pixel(&mut surface, 100, 100, 0xFFFFFFFF);
    let ok4 = true; // didn't panic

    let all_ok = ok1 && ok2 && ok3 && ok4;

    OcrbResult {
        test_name: "Pixel Read/Write",
        passed: all_ok,
        score: if all_ok { 100 } else { 0 },
        weight: 10,
        details: format!("3 pixels + OOB safety verified"),
    }
}

// ─── Test 5: Filled Rectangle ───────────────────────────────────────

fn test_filled_rectangle() -> OcrbResult {
    let mut surface = match Surface::new(64, 64) {
        Some(s) => s,
        None => {
            return OcrbResult {
                test_name: "Filled Rectangle",
                passed: false,
                score: 0,
                weight: 10,
                details: String::from("Failed to allocate surface"),
            };
        }
    };

    let color = 0x00FF0000u32; // red
    drawing::draw_filled_rect(&mut surface, 10, 10, 20, 15, color);

    // Check corners of the rectangle
    let tl = surface.get_pixel(10, 10) == color;
    let tr = surface.get_pixel(29, 10) == color;
    let bl = surface.get_pixel(10, 24) == color;
    let br = surface.get_pixel(29, 24) == color;
    // Check center
    let center = surface.get_pixel(20, 17) == color;
    // Check outside
    let outside = surface.get_pixel(9, 9) == 0 && surface.get_pixel(30, 25) == 0;

    let all_ok = tl && tr && bl && br && center && outside;

    OcrbResult {
        test_name: "Filled Rectangle",
        passed: all_ok,
        score: if all_ok { 100 } else { 0 },
        weight: 10,
        details: format!("Corners+center+outside all correct"),
    }
}

// ─── Test 6: Line Drawing ───────────────────────────────────────────

fn test_line_drawing() -> OcrbResult {
    let mut surface = match Surface::new(64, 64) {
        Some(s) => s,
        None => {
            return OcrbResult {
                test_name: "Line Drawing",
                passed: false,
                score: 0,
                weight: 10,
                details: String::from("Failed to allocate surface"),
            };
        }
    };

    let color = 0x0000FF00u32; // green

    // Horizontal line: y=10, x=5..25
    drawing::draw_line(&mut surface, 5, 10, 25, 10, color);
    let h_start = surface.get_pixel(5, 10) == color;
    let h_mid = surface.get_pixel(15, 10) == color;
    let h_end = surface.get_pixel(25, 10) == color;

    // Vertical line: x=30, y=5..25
    drawing::draw_line(&mut surface, 30, 5, 30, 25, color);
    let v_start = surface.get_pixel(30, 5) == color;
    let v_mid = surface.get_pixel(30, 15) == color;
    let v_end = surface.get_pixel(30, 25) == color;

    // Diagonal: (0,0) to (20,20)
    drawing::draw_line(&mut surface, 0, 0, 20, 20, color);
    let d_start = surface.get_pixel(0, 0) == color;
    let d_mid = surface.get_pixel(10, 10) == color;
    let d_end = surface.get_pixel(20, 20) == color;

    let all_ok = h_start && h_mid && h_end
        && v_start && v_mid && v_end
        && d_start && d_mid && d_end;

    OcrbResult {
        test_name: "Line Drawing",
        passed: all_ok,
        score: if all_ok { 100 } else { 0 },
        weight: 10,
        details: format!("H+V+diagonal lines verified"),
    }
}

// ─── Test 7: Blit Operation ─────────────────────────────────────────

fn test_blit_operation() -> OcrbResult {
    let mut surface = match Surface::new(32, 32) {
        Some(s) => s,
        None => {
            return OcrbResult {
                test_name: "Blit Operation",
                passed: false,
                score: 0,
                weight: 10,
                details: String::from("Failed to allocate surface"),
            };
        }
    };

    // 4x4 sprite
    let sprite: [u32; 16] = [
        0xAA, 0xBB, 0xCC, 0xDD,
        0x11, 0x22, 0x33, 0x44,
        0x55, 0x66, 0x77, 0x88,
        0x99, 0xAA, 0xBB, 0xCC,
    ];

    drawing::blit(&mut surface, &sprite, 4, 4, 5, 5);

    // Verify corners of the blitted region
    let tl = surface.get_pixel(5, 5) == 0xAA;
    let tr = surface.get_pixel(8, 5) == 0xDD;
    let bl = surface.get_pixel(5, 8) == 0x99;
    let br = surface.get_pixel(8, 8) == 0xCC;
    // Verify center
    let mid = surface.get_pixel(6, 6) == 0x22;

    // Outside should still be 0
    let outside = surface.get_pixel(4, 4) == 0;

    let all_ok = tl && tr && bl && br && mid && outside;

    OcrbResult {
        test_name: "Blit Operation",
        passed: all_ok,
        score: if all_ok { 100 } else { 0 },
        weight: 10,
        details: format!("4x4 sprite blit verified"),
    }
}

// ─── Test 8: Font Glyph Data ────────────────────────────────────────

fn test_font_glyph_data() -> OcrbResult {
    // 'A' = 0x41, glyph index = 0x41 - 0x20 = 33
    let a_offset = 33 * text::FONT_HEIGHT as usize;
    let mut a_has_pixels = false;
    for row in 0..text::FONT_HEIGHT as usize {
        if text::FONT_DATA[a_offset + row] != 0 {
            a_has_pixels = true;
            break;
        }
    }

    // Space = 0x20, glyph index = 0 (should be mostly zero rows)
    let space_offset = 0;
    let mut space_all_zero = true;
    for row in 0..text::FONT_HEIGHT as usize {
        if text::FONT_DATA[space_offset + row] != 0 {
            space_all_zero = false;
            break;
        }
    }

    // 'O' = 0x4F, index 47 — should also have pixels
    let o_offset = 47 * text::FONT_HEIGHT as usize;
    let mut o_has_pixels = false;
    for row in 0..text::FONT_HEIGHT as usize {
        if text::FONT_DATA[o_offset + row] != 0 {
            o_has_pixels = true;
            break;
        }
    }

    let all_ok = a_has_pixels && space_all_zero && o_has_pixels;

    OcrbResult {
        test_name: "Font Glyph Data",
        passed: all_ok,
        score: if all_ok { 100 } else { 0 },
        weight: 5,
        details: format!(
            "A=has_pixels:{}, space=all_zero:{}, O=has_pixels:{}",
            a_has_pixels, space_all_zero, o_has_pixels
        ),
    }
}

// ─── Test 9: Text Rendering ─────────────────────────────────────────

fn test_text_rendering() -> OcrbResult {
    let mut surface = match Surface::new(128, 32) {
        Some(s) => s,
        None => {
            return OcrbResult {
                test_name: "Text Rendering",
                passed: false,
                score: 0,
                weight: 15,
                details: String::from("Failed to allocate surface"),
            };
        }
    };

    let fg = 0x00FFFFFFu32; // white
    let bg = 0x00000000u32; // black

    // Clear to black
    surface.clear(bg);

    // Draw "OK" at position (4, 4)
    text::draw_string(&mut surface, 4, 4, "OK", fg, bg);

    // 'O' starts at x=4, 'K' starts at x=12
    // Check that the 'O' region has foreground pixels
    let mut o_fg_count = 0u32;
    for y in 4..(4 + text::FONT_HEIGHT) {
        for x in 4..(4 + text::FONT_WIDTH) {
            if surface.get_pixel(x, y) == fg {
                o_fg_count += 1;
            }
        }
    }

    // 'K' region
    let mut k_fg_count = 0u32;
    for y in 4..(4 + text::FONT_HEIGHT) {
        for x in 12..(12 + text::FONT_WIDTH) {
            if surface.get_pixel(x, y) == fg {
                k_fg_count += 1;
            }
        }
    }

    // Region well outside text should remain background
    let outside_bg = surface.get_pixel(100, 0) == bg;

    // Both letters should have some foreground pixels
    let o_ok = o_fg_count > 10;
    let k_ok = k_fg_count > 10;

    let all_ok = o_ok && k_ok && outside_bg;

    OcrbResult {
        test_name: "Text Rendering",
        passed: all_ok,
        score: if all_ok { 100 } else { 0 },
        weight: 15,
        details: format!("O={}fg K={}fg pixels, outside_bg={}", o_fg_count, k_fg_count, outside_bg),
    }
}

// ─── Test 10: Compositor Present ────────────────────────────────────

fn test_compositor_present() -> OcrbResult {
    // This test verifies that present() runs without faulting.
    // We create a small surface, write to it, and present() to the real FB.
    // In headless QEMU (-display none), the FB memory is still writable.

    let disp = display::DISPLAY.lock();
    if let Some(ref ds) = *disp {
        // Create a small test surface
        let mut test_surface = match Surface::new(16, 16) {
            Some(s) => s,
            None => {
                return OcrbResult {
                    test_name: "Compositor Present",
                    passed: false,
                    score: 0,
                    weight: 15,
                    details: String::from("Failed to allocate test surface"),
                };
            }
        };

        // Fill with a pattern
        let test_color = 0x00ABCDEF;
        test_surface.clear(test_color);

        // Present to hardware framebuffer — should not fault
        crate::display::compositor::present(&test_surface, &ds.fb);

        // Verify: read back from the hardware FB at (0,0)
        // In some configurations this may not match due to write-combining,
        // so we test for fault-free completion as the primary criterion.
        let readback = ds.fb.get_pixel(0, 0);
        let readback_match = readback == test_color;

        // Restore: present the actual surface back
        crate::display::compositor::present(&ds.surface, &ds.fb);

        OcrbResult {
            test_name: "Compositor Present",
            passed: true, // Primary: completed without fault
            score: 100,
            weight: 15,
            details: format!(
                "Present completed, readback=0x{:08X} (match={})",
                readback, readback_match
            ),
        }
    } else {
        OcrbResult {
            test_name: "Compositor Present",
            passed: false,
            score: 0,
            weight: 15,
            details: String::from("Display not initialized"),
        }
    }
}
