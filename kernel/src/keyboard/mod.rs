//! PS/2 Keyboard Driver — scancode set 1 to ASCII, with IRQ1 ring buffer.
//!
//! Reads scancodes from port 0x60, translates to ASCII via lookup table,
//! stores in a 64-entry ring buffer accessible from userspace via syscall.

#![allow(dead_code)]

use spin::Mutex;
use crate::io::{inb, outb};
use crate::serial_println;

/// PS/2 keyboard I/O ports.
const KBD_DATA_PORT: u16 = 0x60;
const KBD_STATUS_PORT: u16 = 0x64;

/// Keyboard ring buffer size.
const BUFFER_SIZE: usize = 64;

/// Ring buffer for keyboard input (ASCII characters).
pub struct KeyboardBuffer {
    buf: [u8; BUFFER_SIZE],
    read_idx: usize,
    write_idx: usize,
}

impl KeyboardBuffer {
    pub const fn new() -> Self {
        Self {
            buf: [0; BUFFER_SIZE],
            read_idx: 0,
            write_idx: 0,
        }
    }

    /// Push an ASCII character into the buffer.
    pub fn push(&mut self, ch: u8) {
        let next_write = (self.write_idx + 1) % BUFFER_SIZE;
        if next_write != self.read_idx {
            self.buf[self.write_idx] = ch;
            self.write_idx = next_write;
        }
        // else: buffer full, drop the character
    }

    /// Pop an ASCII character from the buffer. Returns None if empty.
    pub fn pop(&mut self) -> Option<u8> {
        if self.read_idx == self.write_idx {
            return None;
        }
        let ch = self.buf[self.read_idx];
        self.read_idx = (self.read_idx + 1) % BUFFER_SIZE;
        Some(ch)
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.read_idx == self.write_idx
    }

    /// Number of characters available.
    pub fn len(&self) -> usize {
        if self.write_idx >= self.read_idx {
            self.write_idx - self.read_idx
        } else {
            BUFFER_SIZE - self.read_idx + self.write_idx
        }
    }
}

/// Global keyboard buffer.
pub static KEYBOARD_BUFFER: Mutex<KeyboardBuffer> = Mutex::new(KeyboardBuffer::new());

/// Scancode set 1 → ASCII lookup table (make codes only, index = scancode).
/// 0 = no ASCII mapping (special key or unmapped).
static SCANCODE_TABLE: [u8; 128] = [
    0,   27,  b'1', b'2', b'3', b'4', b'5', b'6',  // 0x00-0x07
    b'7', b'8', b'9', b'0', b'-', b'=', 8,   b'\t', // 0x08-0x0F (BS=8, TAB)
    b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i',  // 0x10-0x17
    b'o', b'p', b'[', b']', b'\n', 0,   b'a', b's',  // 0x18-0x1F (Enter, LCtrl)
    b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';',  // 0x20-0x27
    b'\'', b'`', 0,   b'\\', b'z', b'x', b'c', b'v', // 0x28-0x2F (LShift)
    b'b', b'n', b'm', b',', b'.', b'/', 0,   b'*',  // 0x30-0x37 (RShift, KP*)
    0,   b' ', 0,   0,   0,   0,   0,   0,    // 0x38-0x3F (LAlt, Space, Caps, F1-F5)
    0,   0,   0,   0,   0,   0,   0,   0,    // 0x40-0x47 (F6-F10, Num, Scroll, KP7-KP8)
    0,   0,   0,   0,   0,   0,   0,   0,    // 0x48-0x4F
    0,   0,   0,   0,   0,   0,   0,   0,    // 0x50-0x57
    0,   0,   0,   0,   0,   0,   0,   0,    // 0x58-0x5F
    0,   0,   0,   0,   0,   0,   0,   0,    // 0x60-0x67
    0,   0,   0,   0,   0,   0,   0,   0,    // 0x68-0x6F
    0,   0,   0,   0,   0,   0,   0,   0,    // 0x70-0x77
    0,   0,   0,   0,   0,   0,   0,   0,    // 0x78-0x7F
];

/// Handle a keyboard IRQ — read scancode and translate to ASCII.
/// Called from the IDT vector 33 handler.
pub fn keyboard_irq_handler() {
    let scancode = unsafe { inb(KBD_DATA_PORT) };

    // Ignore break codes (key release, bit 7 set)
    if scancode & 0x80 != 0 {
        return;
    }

    let ascii = SCANCODE_TABLE[scancode as usize];
    if ascii != 0 {
        KEYBOARD_BUFFER.lock().push(ascii);
    }
}

/// Initialize the PS/2 keyboard controller.
pub fn init() {
    // Flush any pending data from the controller
    while unsafe { inb(KBD_STATUS_PORT) } & 1 != 0 {
        unsafe { inb(KBD_DATA_PORT); }
    }

    // Enable keyboard interrupt (bit 0 of controller config byte)
    // Send command 0x20 to read config, then 0x60 to write it back
    unsafe {
        outb(KBD_STATUS_PORT, 0x20); // read config byte
        while inb(KBD_STATUS_PORT) & 1 == 0 {} // wait for data
        let mut config = inb(KBD_DATA_PORT);
        config |= 1; // enable IRQ1
        config &= !0x10; // enable keyboard clock
        outb(KBD_STATUS_PORT, 0x60); // write config byte
        outb(KBD_DATA_PORT, config);
    }

    serial_println!("[KBD] PS/2 keyboard initialized");
}
