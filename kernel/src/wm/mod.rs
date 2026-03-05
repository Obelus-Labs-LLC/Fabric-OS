//! Window Manager — kernel-managed overlapping windows with z-ordering.
//!
//! Provides window lifecycle (create/destroy), focus management (Alt+Tab),
//! z-order stacking, per-window event queues, and a compositor that renders
//! all windows with decorations onto the display surface.
//!
//! Syscalls 29-34 expose WM to userspace. The keyboard IRQ handler routes
//! input events to the focused window's event queue.

#![allow(dead_code)]

pub mod event;
pub mod compositor;

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::OrderedMutex;
use fabric_types::ProcessId;
use crate::display::compositor::Surface;
use self::event::{WmEvent, WmEventQueue};

// ── Constants ────────────────────────────────────────────────────────

/// Maximum number of managed windows.
pub const MAX_WINDOWS: usize = 32;

/// Title bar height in pixels.
pub const TITLE_BAR_HEIGHT: u32 = 24;

/// Close button size in pixels (width = height).
pub const CLOSE_BUTTON_SIZE: u32 = 20;

/// Taskbar height in pixels at bottom of screen.
pub const TASKBAR_HEIGHT: u32 = 32;

// ── WindowId ─────────────────────────────────────────────────────────

/// Opaque window identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct WindowId(pub u32);

// ── Window ───────────────────────────────────────────────────────────

/// A managed window with its own surface, position, and event queue.
pub struct Window {
    /// Unique window identifier.
    pub id: WindowId,
    /// PID of the owning process.
    pub owner_pid: ProcessId,
    /// Window title (displayed in title bar and taskbar).
    pub title: String,
    /// X position on screen (can be negative for partially offscreen).
    pub x: i32,
    /// Y position on screen.
    pub y: i32,
    /// Client area width in pixels.
    pub width: u32,
    /// Client area height in pixels.
    pub height: u32,
    /// Z-order (higher = on top).
    pub z_order: u32,
    /// Whether the window is visible.
    pub visible: bool,
    /// Whether the window has keyboard focus.
    pub focused: bool,
    /// Whether to draw decorations (title bar, border, close button).
    pub decorated: bool,
    /// Client area pixel buffer.
    pub surface: Surface,
    /// Per-window event queue for input delivery.
    pub event_queue: WmEventQueue,
}

// ── WindowTable ──────────────────────────────────────────────────────

/// Fixed-size table of managed windows.
pub struct WindowTable {
    slots: [Option<Window>; MAX_WINDOWS],
    next_id: u32,
    pub focused_id: Option<WindowId>,
}

impl WindowTable {
    pub const fn new() -> Self {
        // const-init: 32 x None
        const NONE: Option<Window> = None;
        Self {
            slots: [NONE; MAX_WINDOWS],
            next_id: 1,
            focused_id: None,
        }
    }

    /// Create a new window. Returns WindowId or None if full/alloc fails.
    pub fn create(
        &mut self,
        owner_pid: ProcessId,
        title: String,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Option<WindowId> {
        // Find empty slot
        let slot_idx = self.slots.iter().position(|s| s.is_none())?;

        // Allocate surface for client area
        let surface = Surface::new(width, height)?;

        let id = WindowId(self.next_id);
        self.next_id += 1;

        // Z-order: above all existing windows
        let max_z = self.slots.iter()
            .filter_map(|s| s.as_ref())
            .map(|w| w.z_order)
            .max()
            .unwrap_or(0);

        let window = Window {
            id,
            owner_pid,
            title,
            x,
            y,
            width,
            height,
            z_order: max_z + 1,
            visible: true,
            focused: false,
            decorated: true,
            surface,
            event_queue: WmEventQueue::new(),
        };

        self.slots[slot_idx] = Some(window);
        Some(id)
    }

    /// Destroy a window by ID. Returns true if found and removed.
    pub fn destroy(&mut self, id: WindowId) -> bool {
        for slot in self.slots.iter_mut() {
            if let Some(ref w) = slot {
                if w.id == id {
                    *slot = None;
                    // Clear focus if this was the focused window
                    if self.focused_id == Some(id) {
                        self.focused_id = None;
                    }
                    return true;
                }
            }
        }
        false
    }

    /// Get an immutable reference to a window by ID.
    pub fn get(&self, id: WindowId) -> Option<&Window> {
        self.slots.iter()
            .filter_map(|s| s.as_ref())
            .find(|w| w.id == id)
    }

    /// Get a mutable reference to a window by ID.
    pub fn get_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.slots.iter_mut()
            .filter_map(|s| s.as_mut())
            .find(|w| w.id == id)
    }

    /// Set focus to a window. Unfocuses the previous window (sends Blur),
    /// raises the new window to front, and sends Focus event.
    pub fn set_focus(&mut self, id: WindowId) {
        // Unfocus old window
        if let Some(old_id) = self.focused_id {
            if old_id != id {
                if let Some(old_win) = self.get_mut(old_id) {
                    old_win.focused = false;
                    old_win.event_queue.push(WmEvent::WindowBlur);
                }
            }
        }

        // Raise to front
        self.raise_to_front(id);

        // Focus new window
        if let Some(win) = self.get_mut(id) {
            win.focused = true;
            win.event_queue.push(WmEvent::WindowFocus);
        }

        self.focused_id = Some(id);
    }

    /// Raise a window to the front (highest z-order).
    pub fn raise_to_front(&mut self, id: WindowId) {
        let max_z = self.slots.iter()
            .filter_map(|s| s.as_ref())
            .map(|w| w.z_order)
            .max()
            .unwrap_or(0);

        if let Some(win) = self.get_mut(id) {
            win.z_order = max_z + 1;
        }
    }

    /// Lower a window to the back (lowest z-order).
    pub fn lower_to_back(&mut self, id: WindowId) {
        let min_z = self.slots.iter()
            .filter_map(|s| s.as_ref())
            .map(|w| w.z_order)
            .min()
            .unwrap_or(1);

        if let Some(win) = self.get_mut(id) {
            win.z_order = if min_z > 0 { min_z - 1 } else { 0 };
        }
    }

    /// Cycle focus to the next window (Alt+Tab behavior).
    /// Cycles through windows in creation order, wrapping around.
    pub fn cycle_focus(&mut self) {
        let mut ids: Vec<WindowId> = self.slots.iter()
            .filter_map(|s| s.as_ref())
            .filter(|w| w.visible)
            .map(|w| w.id)
            .collect();

        if ids.is_empty() {
            return;
        }

        // Sort by window ID for consistent cycling order
        ids.sort_by_key(|id| id.0);

        let current_idx = self.focused_id
            .and_then(|fid| ids.iter().position(|&id| id == fid));

        let next_idx = match current_idx {
            Some(idx) => (idx + 1) % ids.len(),
            None => 0,
        };

        self.set_focus(ids[next_idx]);
    }

    /// Return window IDs sorted by z-order (bottom to top).
    pub fn sorted_by_z(&self) -> Vec<WindowId> {
        let mut windows: Vec<(u32, WindowId)> = self.slots.iter()
            .filter_map(|s| s.as_ref())
            .filter(|w| w.visible)
            .map(|w| (w.z_order, w.id))
            .collect();

        windows.sort_by_key(|&(z, _)| z);
        windows.into_iter().map(|(_, id)| id).collect()
    }

    /// Count of active (non-None) windows.
    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// Get all window IDs owned by a given PID.
    pub fn windows_for_pid(&self, pid: ProcessId) -> Vec<WindowId> {
        self.slots.iter()
            .filter_map(|s| s.as_ref())
            .filter(|w| w.owner_pid == pid)
            .map(|w| w.id)
            .collect()
    }
}

/// Global window table.
pub static WINDOW_TABLE: OrderedMutex<WindowTable, { crate::sync::levels::INPUT }> =
    OrderedMutex::new(WindowTable::new());
