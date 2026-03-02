//! CR0.WP (Write Protect) hardware enforcement for constitution protection.
//!
//! After the constitution loads and the hash is verified, we set the CR0.WP bit.
//! This makes .rodata pages truly read-only at the hardware level — even Ring 0
//! code with raw pointers cannot overwrite the genesis rules without first
//! clearing WP.

#![allow(dead_code)]

use core::arch::asm;

/// CR0.WP bit position (bit 16).
const CR0_WP: u64 = 1 << 16;

/// Read the current CR0 register value.
#[inline]
fn read_cr0() -> u64 {
    let value: u64;
    unsafe {
        asm!("mov {}, cr0", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Write a value to the CR0 register.
#[inline]
unsafe fn write_cr0(value: u64) {
    asm!("mov cr0, {}", in(reg) value, options(nomem, nostack));
}

/// Enable CR0 Write Protect — .rodata becomes hardware-enforced read-only.
pub fn wp_enable() {
    let cr0 = read_cr0();
    if cr0 & CR0_WP == 0 {
        unsafe { write_cr0(cr0 | CR0_WP); }
    }
}

/// Disable CR0 Write Protect — only during constitutional amendment flow.
pub fn wp_disable() {
    let cr0 = read_cr0();
    if cr0 & CR0_WP != 0 {
        unsafe { write_cr0(cr0 & !CR0_WP); }
    }
}

/// Query whether WP is currently enabled.
pub fn wp_is_enabled() -> bool {
    read_cr0() & CR0_WP != 0
}
