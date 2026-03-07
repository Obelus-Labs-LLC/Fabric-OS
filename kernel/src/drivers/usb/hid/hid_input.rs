//! USB HID Boot Protocol Keyboard Report Handler
//!
//! Decodes 8-byte keyboard reports per USB HID 1.11 Appendix B.1:
//!
//! ```text
//! Byte 0: Modifier keys (bitfield)
//!         [0] Left Ctrl   [1] Left Shift   [2] Left Alt   [3] Left GUI
//!         [4] Right Ctrl  [5] Right Shift  [6] Right Alt  [7] Right GUI
//! Byte 1: Reserved (0x00)
//! Bytes 2-7: Key codes (up to 6 simultaneous keys, USB HID Usage Table)
//!            0x00 = no key, 0x01 = ErrorRollOver, 0x02 = POSTFail
//! ```
//!
//! The handler tracks key state between reports to detect press/release events,
//! then translates USB HID usage codes to ASCII for the keyboard buffer.

#![allow(dead_code)]

// ============================================================================
// Modifier Key Bits (Report Byte 0)
// ============================================================================

pub const MOD_LEFT_CTRL: u8 = 1 << 0;
pub const MOD_LEFT_SHIFT: u8 = 1 << 1;
pub const MOD_LEFT_ALT: u8 = 1 << 2;
pub const MOD_LEFT_GUI: u8 = 1 << 3;
pub const MOD_RIGHT_CTRL: u8 = 1 << 4;
pub const MOD_RIGHT_SHIFT: u8 = 1 << 5;
pub const MOD_RIGHT_ALT: u8 = 1 << 6;
pub const MOD_RIGHT_GUI: u8 = 1 << 7;

// ============================================================================
// Special HID Usage Codes
// ============================================================================

/// No key pressed (empty slot)
pub const KEY_NONE: u8 = 0x00;
/// Error: too many keys pressed (phantom/rollover)
pub const KEY_ERR_ROLLOVER: u8 = 0x01;
/// Error: POST failure
pub const KEY_ERR_POST_FAIL: u8 = 0x02;
/// Error: undefined
pub const KEY_ERR_UNDEFINED: u8 = 0x03;

// Key usage codes (HID Usage Tables, Section 10 "Keyboard/Keypad Page")
pub const KEY_A: u8 = 0x04;
pub const KEY_Z: u8 = 0x1D;
pub const KEY_1: u8 = 0x1E;
pub const KEY_0: u8 = 0x27;
pub const KEY_ENTER: u8 = 0x28;
pub const KEY_ESCAPE: u8 = 0x29;
pub const KEY_BACKSPACE: u8 = 0x2A;
pub const KEY_TAB: u8 = 0x2B;
pub const KEY_SPACE: u8 = 0x2C;
pub const KEY_MINUS: u8 = 0x2D;
pub const KEY_EQUAL: u8 = 0x2E;
pub const KEY_LEFT_BRACKET: u8 = 0x2F;
pub const KEY_RIGHT_BRACKET: u8 = 0x30;
pub const KEY_BACKSLASH: u8 = 0x31;
pub const KEY_SEMICOLON: u8 = 0x33;
pub const KEY_APOSTROPHE: u8 = 0x34;
pub const KEY_GRAVE: u8 = 0x35;
pub const KEY_COMMA: u8 = 0x36;
pub const KEY_PERIOD: u8 = 0x37;
pub const KEY_SLASH: u8 = 0x38;
pub const KEY_CAPS_LOCK: u8 = 0x39;
pub const KEY_F1: u8 = 0x3A;
pub const KEY_F12: u8 = 0x45;
pub const KEY_DELETE: u8 = 0x4C;
pub const KEY_RIGHT_ARROW: u8 = 0x4F;
pub const KEY_LEFT_ARROW: u8 = 0x50;
pub const KEY_DOWN_ARROW: u8 = 0x51;
pub const KEY_UP_ARROW: u8 = 0x52;

/// Maximum number of simultaneous keys in a boot report
const MAX_KEYS: usize = 6;

// ============================================================================
// Key Event
// ============================================================================

/// A key press or release event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    /// HID usage code (0x04 = A, 0x28 = Enter, etc.)
    pub usage: u8,
    /// true = pressed, false = released
    pub pressed: bool,
    /// Modifier state at time of event
    pub modifiers: u8,
}

impl KeyEvent {
    /// Is any Shift held?
    pub fn shift(&self) -> bool {
        (self.modifiers & (MOD_LEFT_SHIFT | MOD_RIGHT_SHIFT)) != 0
    }

    /// Is any Ctrl held?
    pub fn ctrl(&self) -> bool {
        (self.modifiers & (MOD_LEFT_CTRL | MOD_RIGHT_CTRL)) != 0
    }

    /// Is any Alt held?
    pub fn alt(&self) -> bool {
        (self.modifiers & (MOD_LEFT_ALT | MOD_RIGHT_ALT)) != 0
    }

    /// Is any GUI (Windows/Super) key held?
    pub fn gui(&self) -> bool {
        (self.modifiers & (MOD_LEFT_GUI | MOD_RIGHT_GUI)) != 0
    }

    /// Translate to ASCII character (0 if no mapping).
    pub fn to_ascii(&self) -> u8 {
        if !self.pressed {
            return 0;
        }
        usage_to_ascii(self.usage, self.shift())
    }
}

// ============================================================================
// Boot Report Parser
// ============================================================================

/// Stateful parser for 8-byte HID boot keyboard reports.
///
/// Tracks the previous report to detect key press/release transitions.
/// Call `process_report()` for each 8-byte interrupt transfer, then
/// drain key events from the returned slice.
pub struct BootKeyboardParser {
    /// Previous report's key codes (bytes 2-7)
    prev_keys: [u8; MAX_KEYS],
    /// Previous modifier state
    prev_modifiers: u8,
    /// Event buffer (worst case: 6 releases + 6 presses + modifier changes)
    events: [KeyEvent; 16],
    /// Number of valid events
    event_count: usize,
}

impl BootKeyboardParser {
    /// Create a new parser with empty state.
    pub const fn new() -> Self {
        Self {
            prev_keys: [0u8; MAX_KEYS],
            prev_modifiers: 0,
            events: [KeyEvent { usage: 0, pressed: false, modifiers: 0 }; 16],
            event_count: 0,
        }
    }

    /// Process an 8-byte boot protocol keyboard report.
    ///
    /// Returns a slice of key events (presses and releases) detected
    /// by comparing this report to the previous one.
    ///
    /// Returns `None` if the report is malformed or contains error codes.
    pub fn process_report(&mut self, report: &[u8]) -> Option<&[KeyEvent]> {
        if report.len() < 8 {
            return None;
        }

        let modifiers = report[0];
        // report[1] is reserved, should be 0x00
        let keys = &report[2..8];

        // Check for error condition (all keys = ErrorRollOver)
        if keys.iter().all(|&k| k == KEY_ERR_ROLLOVER) {
            return None; // Phantom state — too many keys, ignore
        }

        self.event_count = 0;

        // Detect modifier changes (8 individual bits)
        for bit in 0..8u8 {
            let mask = 1u8 << bit;
            let was = (self.prev_modifiers & mask) != 0;
            let now = (modifiers & mask) != 0;
            if was != now {
                self.push_event(KeyEvent {
                    usage: 0xE0 + bit, // HID modifier usage codes start at 0xE0
                    pressed: now,
                    modifiers,
                });
            }
        }

        // Copy prev_keys to avoid borrowing self during push_event
        let prev_keys = self.prev_keys;

        // Detect key releases: keys in prev_keys but not in current keys
        for &prev_key in &prev_keys {
            if prev_key >= KEY_A && !keys.contains(&prev_key) {
                self.push_event(KeyEvent {
                    usage: prev_key,
                    pressed: false,
                    modifiers,
                });
            }
        }

        // Detect key presses: keys in current but not in prev_keys
        for &key in keys {
            if key >= KEY_A && !prev_keys.contains(&key) {
                self.push_event(KeyEvent {
                    usage: key,
                    pressed: true,
                    modifiers,
                });
            }
        }

        // Save state for next report
        self.prev_modifiers = modifiers;
        self.prev_keys.copy_from_slice(keys);

        Some(&self.events[..self.event_count])
    }

    fn push_event(&mut self, event: KeyEvent) {
        if self.event_count < self.events.len() {
            self.events[self.event_count] = event;
            self.event_count += 1;
        }
    }

    /// Reset parser state (e.g., on device disconnect).
    pub fn reset(&mut self) {
        self.prev_keys = [0u8; MAX_KEYS];
        self.prev_modifiers = 0;
        self.event_count = 0;
    }
}

// ============================================================================
// HID Usage Code → ASCII Translation
// ============================================================================

/// Translate HID usage code to ASCII.
/// `shifted`: true if Shift is held.
/// Returns 0 for non-printable or unmapped keys.
pub fn usage_to_ascii(usage: u8, shifted: bool) -> u8 {
    match usage {
        // Letters: A-Z (0x04 - 0x1D)
        KEY_A..=KEY_Z => {
            let base = b'a' + (usage - KEY_A);
            if shifted { base - 32 } else { base }
        }

        // Numbers: 1-9 (0x1E - 0x26)
        0x1E..=0x26 => {
            if shifted {
                // Shift+1 = !, Shift+2 = @, etc.
                SHIFT_NUMBERS[(usage - 0x1E) as usize]
            } else {
                b'1' + (usage - 0x1E)
            }
        }

        // 0 (0x27)
        0x27 => if shifted { b')' } else { b'0' },

        // Special keys
        KEY_ENTER => b'\n',
        KEY_ESCAPE => 0x1B,
        KEY_BACKSPACE => 0x08,
        KEY_TAB => b'\t',
        KEY_SPACE => b' ',

        // Punctuation
        KEY_MINUS => if shifted { b'_' } else { b'-' },
        KEY_EQUAL => if shifted { b'+' } else { b'=' },
        KEY_LEFT_BRACKET => if shifted { b'{' } else { b'[' },
        KEY_RIGHT_BRACKET => if shifted { b'}' } else { b']' },
        KEY_BACKSLASH => if shifted { b'|' } else { b'\\' },
        KEY_SEMICOLON => if shifted { b':' } else { b';' },
        KEY_APOSTROPHE => if shifted { b'"' } else { b'\'' },
        KEY_GRAVE => if shifted { b'~' } else { b'`' },
        KEY_COMMA => if shifted { b'<' } else { b',' },
        KEY_PERIOD => if shifted { b'>' } else { b'.' },
        KEY_SLASH => if shifted { b'?' } else { b'/' },

        _ => 0, // Unmapped (F-keys, arrows, etc.)
    }
}

/// Shift+number symbols: ! @ # $ % ^ & * (
const SHIFT_NUMBERS: [u8; 9] = [b'!', b'@', b'#', b'$', b'%', b'^', b'&', b'*', b'('];

// ============================================================================
// LED Output Report (Byte 0, host-to-device)
// ============================================================================

/// LED indicator bits for SET_REPORT output report.
pub const LED_NUM_LOCK: u8 = 1 << 0;
pub const LED_CAPS_LOCK: u8 = 1 << 1;
pub const LED_SCROLL_LOCK: u8 = 1 << 2;
pub const LED_COMPOSE: u8 = 1 << 3;
pub const LED_KANA: u8 = 1 << 4;

/// Build a 1-byte LED output report for SET_REPORT.
pub fn build_led_report(num: bool, caps: bool, scroll: bool) -> u8 {
    let mut leds = 0u8;
    if num { leds |= LED_NUM_LOCK; }
    if caps { leds |= LED_CAPS_LOCK; }
    if scroll { leds |= LED_SCROLL_LOCK; }
    leds
}
