use core::fmt;
use spin::Mutex;
use crate::io::{inb, outb};

const COM1: u16 = 0x3F8;

pub struct SerialPort {
    base: u16,
}

impl SerialPort {
    pub const fn new(base: u16) -> Self {
        Self { base }
    }

    pub fn init(&self) {
        unsafe {
            // Disable interrupts
            outb(self.base + 1, 0x00);
            // Enable DLAB (set baud rate divisor)
            outb(self.base + 3, 0x80);
            // Set divisor to 1 (115200 baud)
            outb(self.base, 0x01);     // Divisor low byte
            outb(self.base + 1, 0x00); // Divisor high byte
            // 8 bits, no parity, one stop bit, DLAB off
            outb(self.base + 3, 0x03);
            // Enable FIFO, clear them, 14-byte threshold
            outb(self.base + 2, 0xC7);
            // IRQs enabled, RTS/DSR set
            outb(self.base + 4, 0x0B);
        }
    }

    fn is_transmit_empty(&self) -> bool {
        unsafe { inb(self.base + 5) & 0x20 != 0 }
    }

    pub fn write_byte(&self, byte: u8) {
        while !self.is_transmit_empty() {
            core::hint::spin_loop();
        }
        unsafe {
            outb(self.base, byte);
        }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}

pub static SERIAL: Mutex<SerialPort> = Mutex::new(SerialPort::new(COM1));

pub fn init() {
    SERIAL.lock().init();
}

/// Write a single byte to COM1 (used by syscall handler for serial output).
pub fn write_byte(byte: u8) {
    SERIAL.lock().write_byte(byte);
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    SERIAL.lock().write_fmt(args).unwrap();
}
