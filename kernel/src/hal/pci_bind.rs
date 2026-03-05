//! PCI Driver Binding — match PCI devices to drivers and manage lifecycle.
//!
//! Provides PciDeviceId for device matching (with wildcard support),
//! the PciDriver trait for hardware drivers, and PciDriverTable for
//! registration and binding.

#![allow(dead_code)]

use spin::Mutex;
use crate::pci::PciDevice;
use super::driver_sdk::DriverResources;

/// Maximum registered PCI drivers.
pub const MAX_PCI_DRIVERS: usize = 16;

/// PCI device identifier for driver matching.
///
/// Use 0xFFFF for vendor/device or 0xFF for class/subclass as wildcards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciDeviceId {
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub subclass: u8,
}

impl PciDeviceId {
    pub const WILDCARD_16: u16 = 0xFFFF;
    pub const WILDCARD_8: u8 = 0xFF;

    pub const fn new(vendor: u16, device: u16, class: u8, subclass: u8) -> Self {
        Self { vendor, device, class, subclass }
    }

    /// Check if this ID matches a discovered PCI device.
    /// Wildcard values match any device field.
    pub fn matches(&self, dev: &PciDevice) -> bool {
        (self.vendor == Self::WILDCARD_16 || self.vendor == dev.vendor_id) &&
        (self.device == Self::WILDCARD_16 || self.device == dev.device_id) &&
        (self.class == Self::WILDCARD_8 || self.class == dev.class_code) &&
        (self.subclass == Self::WILDCARD_8 || self.subclass == dev.subclass)
    }
}

/// Bus/Device/Function address for a bound PCI device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciBdf {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciBdf {
    pub const fn new(bus: u8, device: u8, function: u8) -> Self {
        Self { bus, device, function }
    }
}

/// Trait for PCI hardware drivers.
///
/// Unlike the message-bus `Driver` trait in hal/mod.rs, PciDriver is
/// hardware-oriented: it probes, attaches to, and directly handles
/// interrupts from PCI devices.
pub trait PciDriver: Send {
    /// Human-readable driver name.
    fn name(&self) -> &'static str;

    /// List of supported PCI device IDs.
    fn supported_devices(&self) -> &[PciDeviceId];

    /// Quick check if this driver can handle the device.
    /// Called during bus enumeration before full attach.
    fn probe(&self, dev: &PciDevice) -> bool;

    /// Attach to a PCI device. Set up MMIO/PIO/DMA resources.
    fn attach(&mut self, dev: &PciDevice, resources: &mut DriverResources) -> Result<(), &'static str>;

    /// Detach from the device. Release all resources.
    fn detach(&mut self);

    /// Handle an interrupt from the device.
    fn interrupt(&mut self);
}

/// Entry in the PCI driver table.
pub struct PciDriverEntry {
    pub driver: &'static mut dyn PciDriver,
    pub bound_bdf: Option<PciBdf>,
    pub active: bool,
}

/// Table of registered PCI drivers.
///
/// Drivers register themselves at boot, then bind_all() matches them
/// against discovered PCI devices.
pub struct PciDriverTable {
    entries: [Option<PciDriverTableSlot>; MAX_PCI_DRIVERS],
    count: usize,
}

/// Internal slot — uses static references since drivers are kernel-lifetime objects.
struct PciDriverTableSlot {
    name: &'static str,
    supported: &'static [PciDeviceId],
    bound_bdf: Option<PciBdf>,
    resource_id: u32,
}

impl PciDriverTable {
    pub const fn new() -> Self {
        // Can't use [None; N] for non-Copy types, so use array init
        Self {
            entries: [
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
            ],
            count: 0,
        }
    }

    /// Register a PCI driver by name and supported device list.
    pub fn register(
        &mut self,
        name: &'static str,
        supported: &'static [PciDeviceId],
        resource_id: u32,
    ) -> Result<usize, &'static str> {
        if self.count >= MAX_PCI_DRIVERS {
            return Err("PCI driver table full");
        }

        for (i, slot) in self.entries.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(PciDriverTableSlot {
                    name,
                    supported,
                    bound_bdf: None,
                    resource_id,
                });
                self.count += 1;
                return Ok(i);
            }
        }

        Err("PCI driver table inconsistency")
    }

    /// Unregister a driver by index.
    pub fn unregister(&mut self, index: usize) -> bool {
        if index < MAX_PCI_DRIVERS {
            if let Some(_) = self.entries[index].take() {
                self.count -= 1;
                return true;
            }
        }
        false
    }

    /// Attempt to bind all registered drivers to discovered PCI devices.
    ///
    /// For each unbound driver, iterates through devices looking for a
    /// match. Returns the number of successful bindings.
    pub fn bind_all(&mut self, devices: &[PciDevice]) -> usize {
        let mut bound = 0;

        for slot in self.entries.iter_mut().flatten() {
            if slot.bound_bdf.is_some() {
                continue; // already bound
            }

            for dev in devices {
                let mut matched = false;
                for id in slot.supported {
                    if id.matches(dev) {
                        matched = true;
                        break;
                    }
                }

                if matched {
                    slot.bound_bdf = Some(PciBdf::new(dev.bus, dev.device, dev.function));
                    bound += 1;
                    break; // one device per driver
                }
            }
        }

        bound
    }

    /// Unbind a driver by index. Returns the BDF if it was bound.
    pub fn unbind(&mut self, index: usize) -> Option<PciBdf> {
        if index < MAX_PCI_DRIVERS {
            if let Some(ref mut slot) = self.entries[index] {
                return slot.bound_bdf.take();
            }
        }
        None
    }

    /// Get the number of registered drivers.
    pub fn driver_count(&self) -> usize {
        self.count
    }

    /// Get the number of bound (active) drivers.
    pub fn bound_count(&self) -> usize {
        self.entries.iter()
            .flatten()
            .filter(|s| s.bound_bdf.is_some())
            .count()
    }

    /// Check if a specific BDF is bound to any driver.
    pub fn is_bound(&self, bdf: PciBdf) -> bool {
        self.entries.iter()
            .flatten()
            .any(|s| s.bound_bdf == Some(bdf))
    }

    /// Get driver name by index.
    pub fn driver_name(&self, index: usize) -> Option<&'static str> {
        if index < MAX_PCI_DRIVERS {
            self.entries[index].as_ref().map(|s| s.name)
        } else {
            None
        }
    }

    /// Get driver resource_id by index.
    pub fn driver_resource_id(&self, index: usize) -> Option<u32> {
        if index < MAX_PCI_DRIVERS {
            self.entries[index].as_ref().map(|s| s.resource_id)
        } else {
            None
        }
    }
}

/// Global PCI driver table.
pub static PCI_DRIVER_TABLE: Mutex<PciDriverTable> = Mutex::new(PciDriverTable::new());
