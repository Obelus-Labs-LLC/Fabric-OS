//! Virtual gamepad input abstraction.
//!
//! Supports 4 concurrent gamepads with standard button/axis layout.
//! Keyboard-to-gamepad mapping enables testing without USB HID.
//! Future phases add USB HID driver for physical controllers.

#![allow(dead_code)]

use crate::sync::OrderedMutex;
use crate::serial_println;

/// Maximum concurrent gamepads.
pub const MAX_GAMEPADS: usize = 4;

/// Gamepad button flags (bitfield in u16).
pub struct ButtonFlags;

impl ButtonFlags {
    pub const A: u16        = 1 << 0;
    pub const B: u16        = 1 << 1;
    pub const X: u16        = 1 << 2;
    pub const Y: u16        = 1 << 3;
    pub const START: u16    = 1 << 4;
    pub const SELECT: u16   = 1 << 5;
    pub const LB: u16       = 1 << 6;
    pub const RB: u16       = 1 << 7;
    pub const D_UP: u16     = 1 << 8;
    pub const D_DOWN: u16   = 1 << 9;
    pub const D_LEFT: u16   = 1 << 10;
    pub const D_RIGHT: u16  = 1 << 11;
}

/// Analog axis state.
#[derive(Clone, Copy, Debug, Default)]
pub struct AxisState {
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
    pub left_trigger: u8,
    pub right_trigger: u8,
}

impl AxisState {
    pub const fn new() -> Self {
        AxisState {
            left_x: 0,
            left_y: 0,
            right_x: 0,
            right_y: 0,
            left_trigger: 0,
            right_trigger: 0,
        }
    }
}

/// Complete gamepad state.
#[derive(Clone, Debug)]
pub struct GamepadState {
    pub buttons: u16,
    pub axes: AxisState,
    pub connected: bool,
}

impl GamepadState {
    pub const fn new() -> Self {
        GamepadState {
            buttons: 0,
            axes: AxisState::new(),
            connected: false,
        }
    }

    /// Set a button (OR into bitfield).
    pub fn press(&mut self, button: u16) {
        self.buttons |= button;
    }

    /// Clear a button.
    pub fn release(&mut self, button: u16) {
        self.buttons &= !button;
    }

    /// Check if a button is pressed.
    pub fn is_pressed(&self, button: u16) -> bool {
        self.buttons & button != 0
    }
}

/// Gamepad event types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamepadEvent {
    ButtonDown(u16),
    ButtonUp(u16),
    AxisMove { axis: u8, value: i16 },
}

/// Table of gamepads (up to 4 concurrent).
pub struct GamepadTable {
    pads: [Option<GamepadState>; MAX_GAMEPADS],
}

impl GamepadTable {
    pub const fn new() -> Self {
        const NONE: Option<GamepadState> = None;
        GamepadTable {
            pads: [NONE; MAX_GAMEPADS],
        }
    }

    /// Connect a gamepad at the given slot.
    pub fn connect(&mut self, slot: usize) -> Option<&mut GamepadState> {
        if slot >= MAX_GAMEPADS {
            return None;
        }
        self.pads[slot] = Some(GamepadState {
            buttons: 0,
            axes: AxisState::new(),
            connected: true,
        });
        self.pads[slot].as_mut()
    }

    /// Disconnect a gamepad.
    pub fn disconnect(&mut self, slot: usize) -> bool {
        if slot >= MAX_GAMEPADS {
            return false;
        }
        if self.pads[slot].is_some() {
            self.pads[slot] = None;
            true
        } else {
            false
        }
    }

    /// Get gamepad state (immutable).
    pub fn get(&self, slot: usize) -> Option<&GamepadState> {
        if slot >= MAX_GAMEPADS {
            return None;
        }
        self.pads[slot].as_ref()
    }

    /// Get gamepad state (mutable).
    pub fn get_mut(&mut self, slot: usize) -> Option<&mut GamepadState> {
        if slot >= MAX_GAMEPADS {
            return None;
        }
        self.pads[slot].as_mut()
    }

    /// Count connected gamepads.
    pub fn count(&self) -> usize {
        self.pads.iter().filter(|p| p.is_some()).count()
    }

    /// Map PS/2 scancode set 1 to gamepad slot 0.
    /// Call with pressed=true for make codes, false for break codes.
    pub fn update_from_keyboard(&mut self, scancode: u8, pressed: bool) {
        let pad = match self.pads[0].as_mut() {
            Some(p) if p.connected => p,
            _ => return,
        };

        match scancode {
            // WASD -> left analog stick
            0x11 => { // W
                pad.axes.left_y = if pressed { -32767 } else { 0 };
            }
            0x1F => { // S
                pad.axes.left_y = if pressed { 32767 } else { 0 };
            }
            0x1E => { // A
                pad.axes.left_x = if pressed { -32767 } else { 0 };
            }
            0x20 => { // D
                pad.axes.left_x = if pressed { 32767 } else { 0 };
            }

            // Arrow keys -> D-pad
            0x48 => { // Up
                if pressed { pad.press(ButtonFlags::D_UP); } else { pad.release(ButtonFlags::D_UP); }
            }
            0x50 => { // Down
                if pressed { pad.press(ButtonFlags::D_DOWN); } else { pad.release(ButtonFlags::D_DOWN); }
            }
            0x4B => { // Left
                if pressed { pad.press(ButtonFlags::D_LEFT); } else { pad.release(ButtonFlags::D_LEFT); }
            }
            0x4D => { // Right
                if pressed { pad.press(ButtonFlags::D_RIGHT); } else { pad.release(ButtonFlags::D_RIGHT); }
            }

            // Action buttons
            0x39 => { // Space -> A
                if pressed { pad.press(ButtonFlags::A); } else { pad.release(ButtonFlags::A); }
            }
            0x1C => { // Enter -> Start
                if pressed { pad.press(ButtonFlags::START); } else { pad.release(ButtonFlags::START); }
            }
            0x2A => { // Left Shift -> B
                if pressed { pad.press(ButtonFlags::B); } else { pad.release(ButtonFlags::B); }
            }
            0x1D => { // Left Ctrl -> X
                if pressed { pad.press(ButtonFlags::X); } else { pad.release(ButtonFlags::X); }
            }
            0x38 => { // Left Alt -> Y
                if pressed { pad.press(ButtonFlags::Y); } else { pad.release(ButtonFlags::Y); }
            }
            0x0F => { // Tab -> Select
                if pressed { pad.press(ButtonFlags::SELECT); } else { pad.release(ButtonFlags::SELECT); }
            }

            _ => {} // Unmapped key
        }
    }
}

/// Global gamepad table.
pub static GAMEPAD_TABLE: OrderedMutex<GamepadTable, { crate::sync::levels::INPUT }> =
    OrderedMutex::new(GamepadTable::new());

/// Initialize gamepad subsystem. Auto-connects slot 0 for keyboard mapping.
pub fn init() {
    let mut table = GAMEPAD_TABLE.lock();
    table.connect(0);
    serial_println!("[GAMEPAD] Slot 0 connected (keyboard-mapped)");
}
