//! Display subsystem — framebuffer, compositor, drawing, text rendering.
//!
//! Provides double-buffered rendering to the Limine-provided framebuffer.
//! All drawing operations target the back buffer (Surface); present() blits
//! the back buffer to the hardware framebuffer.
//!
//! Userspace display syscalls (Phase 10 stubs for Loom integration):
//!   sys_display_alloc_surface(width, height) -> SurfaceId
//!   sys_display_blit(surface_id, buffer_ptr, len)
//!   sys_display_present(surface_id)
//!
//! // TODO(Future): Multi-window compositor, z-ordering, damage tracking

#![allow(dead_code)]

pub mod framebuffer;
pub mod compositor;
pub mod drawing;
pub mod text;

use crate::sync::OrderedMutex;
use crate::serial_println;

pub use framebuffer::FramebufferInfo;
pub use compositor::Surface;

// ── Color ────────────────────────────────────────────────────────────

/// RGB color (no alpha — unused by framebuffer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Encode this color using the hardware pixel format shifts.
    pub fn to_packed(&self, info: &FramebufferInfo) -> u32 {
        ((self.r as u32) << info.red_shift)
            | ((self.g as u32) << info.green_shift)
            | ((self.b as u32) << info.blue_shift)
    }

    pub const BLACK:  Color = Color::new(0, 0, 0);
    pub const WHITE:  Color = Color::new(255, 255, 255);
    pub const RED:    Color = Color::new(255, 0, 0);
    pub const GREEN:  Color = Color::new(0, 255, 0);
    pub const BLUE:   Color = Color::new(0, 0, 255);
    pub const GRAY:   Color = Color::new(128, 128, 128);
    pub const YELLOW: Color = Color::new(255, 255, 0);
    pub const CYAN:   Color = Color::new(0, 255, 255);
}

// ── Surface Table (userspace display syscalls) ─────────────────────

/// Maximum number of userspace-allocated surfaces.
const MAX_SURFACES: usize = 8;

/// Opaque surface identifier returned to userspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct SurfaceId(pub u32);

/// Fixed-size table of surfaces for userspace allocation via syscalls.
pub struct SurfaceTable {
    slots: [Option<Surface>; MAX_SURFACES],
}

impl SurfaceTable {
    pub const fn new() -> Self {
        // const-init: 8 × None
        Self {
            slots: [None, None, None, None, None, None, None, None],
        }
    }

    /// Allocate a new surface. Returns SurfaceId or None if full/alloc fails.
    pub fn alloc(&mut self, width: u32, height: u32) -> Option<SurfaceId> {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_none() {
                if let Some(surface) = Surface::new(width, height) {
                    *slot = Some(surface);
                    return Some(SurfaceId(i as u32));
                }
                return None; // alloc failed
            }
        }
        None // all slots full
    }

    /// Get a mutable reference to a surface by ID.
    pub fn get_mut(&mut self, id: SurfaceId) -> Option<&mut Surface> {
        self.slots.get_mut(id.0 as usize)?.as_mut()
    }

    /// Get an immutable reference to a surface by ID.
    pub fn get(&self, id: SurfaceId) -> Option<&Surface> {
        self.slots.get(id.0 as usize)?.as_ref()
    }
}

/// Global surface table for userspace display syscalls.
pub static SURFACE_TABLE: OrderedMutex<SurfaceTable, { crate::sync::levels::DISPLAY }> =
    OrderedMutex::new(SurfaceTable::new());

// ── Global Display State ─────────────────────────────────────────────

/// Holds the framebuffer info and compositor surface.
pub struct DisplayState {
    pub fb: FramebufferInfo,
    pub surface: Surface,
}

/// Global display state, initialized in Phase 10.
pub static DISPLAY: OrderedMutex<Option<DisplayState>, { crate::sync::levels::DISPLAY }> =
    OrderedMutex::new(None);

// ── Initialization ───────────────────────────────────────────────────

/// Initialize the display subsystem with framebuffer info from Limine.
pub fn init(fb: FramebufferInfo) {
    serial_println!("[DISPLAY] Framebuffer: {}x{}, {}bpp, pitch={}, size={}",
        fb.width, fb.height, fb.bpp, fb.pitch, fb.size);
    serial_println!("[DISPLAY] Pixel format: R@{} G@{} B@{} ({})",
        fb.red_shift, fb.green_shift, fb.blue_shift,
        if fb.blue_shift == 0 && fb.green_shift == 8 && fb.red_shift == 16 { "BGRX" } else { "custom" });

    // Attempt to allocate back buffer
    let surface = match Surface::new(fb.width, fb.height) {
        Some(s) => {
            serial_println!("[DISPLAY] Back buffer: {} bytes allocated",
                (fb.width as usize) * (fb.height as usize) * 4);
            s
        }
        None => {
            serial_println!("[DISPLAY] WARNING: Back buffer allocation failed, using 1x1 fallback");
            Surface::new(1, 1).expect("cannot allocate 1x1 surface")
        }
    };

    let state = DisplayState { fb, surface };

    // Draw boot banner to surface
    let bg_packed = Color::BLACK.to_packed(&state.fb);
    let fg_packed = Color::GREEN.to_packed(&state.fb);
    let mut ds = state;
    ds.surface.clear(bg_packed);
    text::draw_string(&mut ds.surface, 8, 8, "FabricOS v0.6.0", fg_packed, bg_packed);
    text::draw_string(&mut ds.surface, 8, 28, "Display subsystem initialized", fg_packed, bg_packed);

    // Present to hardware
    compositor::present(&ds.surface, &ds.fb);

    *DISPLAY.lock() = Some(ds);

    serial_println!("[DISPLAY] Display subsystem initialized");
}
