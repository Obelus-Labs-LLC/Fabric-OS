//! PCI Bus Enumeration — scan PCI configuration space via I/O ports.
//!
//! Uses legacy PCI configuration mechanism #1:
//!   Port 0xCF8: CONFIG_ADDRESS (bus/device/function/offset)
//!   Port 0xCFC: CONFIG_DATA (read/write config registers)

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;
use crate::io::{inl, outl};
use crate::serial_println;

const CONFIG_ADDRESS: u16 = 0x0CF8;
const CONFIG_DATA: u16 = 0x0CFC;

/// PCI device descriptor discovered during bus scan.
#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub header_type: u8,
    pub irq_line: u8,
    pub bars: [u32; 6],
}

impl PciDevice {
    /// Return true if this is a virtio device (vendor 0x1AF4).
    pub fn is_virtio(&self) -> bool {
        self.vendor_id == 0x1AF4
    }

    /// Return true if this is a virtio-net device (vendor 0x1AF4, device 0x1000).
    pub fn is_virtio_net(&self) -> bool {
        self.vendor_id == 0x1AF4 && self.device_id == 0x1000
    }

    /// Get BAR0 as an I/O port base (low bit set = I/O space).
    pub fn bar0_io_base(&self) -> Option<u16> {
        let bar = self.bars[0];
        if bar & 1 != 0 {
            Some((bar & 0xFFFC) as u16)
        } else {
            None
        }
    }
}

/// Build a CONFIG_ADDRESS value for the given bus/device/function/offset.
fn config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let bus = bus as u32;
    let device = device as u32;
    let function = function as u32;
    let offset = (offset & 0xFC) as u32; // align to dword
    (1 << 31) | (bus << 16) | (device << 11) | (function << 8) | offset
}

/// Read a 32-bit value from PCI configuration space.
pub fn config_read_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    unsafe {
        outl(CONFIG_ADDRESS, config_address(bus, device, function, offset));
        inl(CONFIG_DATA)
    }
}

/// Read a 16-bit value from PCI configuration space.
pub fn config_read_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let dword = config_read_u32(bus, device, function, offset & 0xFC);
    let shift = ((offset & 2) * 8) as u32;
    ((dword >> shift) & 0xFFFF) as u16
}

/// Read an 8-bit value from PCI configuration space.
pub fn config_read_u8(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let dword = config_read_u32(bus, device, function, offset & 0xFC);
    let shift = ((offset & 3) * 8) as u32;
    ((dword >> shift) & 0xFF) as u8
}

/// Write a 32-bit value to PCI configuration space.
pub fn config_write_u32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    unsafe {
        outl(CONFIG_ADDRESS, config_address(bus, device, function, offset));
        outl(CONFIG_DATA, value);
    }
}

/// Scan a PCI bus and return all discovered devices.
pub fn scan_bus(bus: u8) -> Vec<PciDevice> {
    let mut devices = Vec::new();

    for device in 0..32u8 {
        let vendor = config_read_u16(bus, device, 0, 0x00);
        if vendor == 0xFFFF {
            continue; // no device
        }

        let header_type = config_read_u8(bus, device, 0, 0x0E);
        let max_functions = if header_type & 0x80 != 0 { 8 } else { 1 };

        for function in 0..max_functions {
            let vendor_id = config_read_u16(bus, device, function, 0x00);
            if vendor_id == 0xFFFF {
                continue;
            }

            let device_id = config_read_u16(bus, device, function, 0x02);
            let class_code = config_read_u8(bus, device, function, 0x0B);
            let subclass = config_read_u8(bus, device, function, 0x0A);
            let hdr_type = config_read_u8(bus, device, function, 0x0E) & 0x7F;
            let irq_line = config_read_u8(bus, device, function, 0x3C);

            let mut bars = [0u32; 6];
            let bar_count = if hdr_type == 0 { 6 } else { 2 };
            for i in 0..bar_count {
                bars[i] = config_read_u32(bus, device, function, 0x10 + (i as u8) * 4);
            }

            devices.push(PciDevice {
                bus,
                device,
                function,
                vendor_id,
                device_id,
                class_code,
                subclass,
                header_type: hdr_type,
                irq_line,
                bars,
            });
        }
    }

    devices
}

/// Initialize PCI: scan bus 0 and log discovered devices.
pub fn init() -> Vec<PciDevice> {
    let devices = scan_bus(0);
    serial_println!("[PCI] Bus scan: found {} devices", devices.len());
    for dev in &devices {
        serial_println!("[PCI] {:02x}:{:02x}.{} {:04x}:{:04x} class={:02x}:{:02x} IRQ={} BAR0=0x{:08x}",
            dev.bus, dev.device, dev.function,
            dev.vendor_id, dev.device_id,
            dev.class_code, dev.subclass,
            dev.irq_line, dev.bars[0]);
    }
    devices
}
