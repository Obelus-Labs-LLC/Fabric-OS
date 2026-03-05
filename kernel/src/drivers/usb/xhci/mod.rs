//! xHCI Host Controller Driver — Phase 21a/21b.
//!
//! XhciController orchestrates full xHCI initialization:
//! PCI detection → BAR0 mapping → hardware reset → ring setup → operational state.
//! Root hub emulation with USB 2.0/3.0 port routing and state machine.
//!
//! Uses the same MMIO/DMA patterns as the e1000e Ethernet driver (Phase 20A).

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;
use crate::serial_println;
use crate::pci::{self, PciDevice};
use crate::hal::driver_sdk::MmioRegion;
use crate::memory::{PhysAddr, VirtAddr, PAGE_SIZE, hhdm_offset, page_table::PageTableFlags};

pub mod regs;
pub mod context;
pub mod init;
pub mod port;
pub mod hub;

use regs::*;
use context::TrbRing;
use init::XhciCapabilities;
use hub::RootHub;

// ============================================================================
// XHCI Controller
// ============================================================================

/// Full xHCI Host Controller instance.
///
/// Holds all state needed to manage the controller:
/// MMIO region, parsed capabilities, DMA ring descriptors,
/// and computed register space offsets.
pub struct XhciController {
    /// MMIO region mapped from BAR0.
    pub mmio: MmioRegion,
    /// Parsed capability parameters.
    pub caps: XhciCapabilities,
    /// Operational Register base offset from MMIO base.
    pub op_base: usize,
    /// Runtime Register base offset from MMIO base.
    pub rts_base: usize,
    /// Doorbell Array base offset from MMIO base.
    pub db_base: usize,
    /// Command Ring.
    pub command_ring: TrbRing,
    /// Primary Event Ring (Interrupter 0).
    pub event_ring: TrbRing,
    /// DCBAA virtual address.
    pub dcbaa_virt: usize,
    /// DCBAA physical address.
    pub dcbaa_phys: usize,
    /// ERST virtual address.
    pub erst_virt: usize,
    /// ERST physical address.
    pub erst_phys: usize,
    /// Root hub — port management, protocol routing, event handling.
    pub root_hub: Option<RootHub>,
    /// Number of connected ports detected.
    pub connected_ports: u8,
    /// Whether the controller reached operational state.
    pub running: bool,
}

impl XhciController {
    /// Extract MMIO base address from BAR0 (same logic as e1000e).
    ///
    /// BAR0 bit 0 = 0 means memory-mapped (MMIO).
    /// Bits 1-2: 00 = 32-bit BAR, 10 = 64-bit BAR.
    fn bar0_mmio_base(dev: &PciDevice) -> Option<u64> {
        let bar0 = dev.bars[0];
        if bar0 & 1 != 0 {
            return None; // I/O BAR, not MMIO
        }

        let bar_type = (bar0 >> 1) & 0x3;
        match bar_type {
            0b00 => {
                // 32-bit BAR
                Some((bar0 & 0xFFFFFFF0) as u64)
            }
            0b10 => {
                // 64-bit BAR — upper 32 bits from BAR1
                let high = dev.bars[1] as u64;
                let low = (bar0 & 0xFFFFFFF0) as u64;
                Some(low | (high << 32))
            }
            _ => None,
        }
    }

    /// Initialize an xHCI controller from a PCI device.
    ///
    /// Full hardware initialization sequence:
    /// 1. Extract MMIO base from BAR0
    /// 2. Map MMIO pages with NO_CACHE flags
    /// 3. Read capability registers
    /// 4. Halt + Reset controller
    /// 5. Program MaxSlotsEn
    /// 6. Allocate + program DCBAA
    /// 7. Allocate + program Command Ring
    /// 8. Allocate + program Event Ring
    /// 9. Enable interrupts
    /// 10. Start controller (RS=1)
    /// 11. Scan root hub ports
    pub fn init_from_pci(dev: &PciDevice) -> Option<Self> {
        serial_println!("[XHCI] Initializing {:02x}:{:02x}.{} ({:04x}:{:04x})",
            dev.bus, dev.device, dev.function, dev.vendor_id, dev.device_id);

        // Step 1: Extract MMIO base from BAR0
        let mmio_phys = Self::bar0_mmio_base(dev)?;
        serial_println!("[XHCI] BAR0 MMIO phys = {:#x}", mmio_phys);

        // Step 2: Map MMIO pages (xHCI typically needs 64KB+, map 16 pages = 64KB)
        let mmio_pages = 16;
        let mmio_virt_base = mmio_phys + hhdm_offset();
        for i in 0..mmio_pages {
            let phys = PhysAddr::new(mmio_phys + (i * PAGE_SIZE) as u64);
            let virt = VirtAddr::new(mmio_virt_base + (i * PAGE_SIZE) as u64);
            let flags = PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::NO_CACHE;
            match crate::memory::mapper::map(virt, phys, flags) {
                Ok(()) => {}
                Err(crate::memory::mapper::MapError::AlreadyMapped) => {}
                Err(_) => {
                    serial_println!("[XHCI] Failed to map MMIO page {}", i);
                    return None;
                }
            }
        }

        let mmio = MmioRegion::new(mmio_virt_base as usize, mmio_pages * PAGE_SIZE);

        // Step 3: Read capability registers
        let caps = init::read_capabilities(&mmio)?;
        serial_println!("[XHCI] xHCI version {}.{}.{}",
            (caps.hci_version >> 8) & 0xFF,
            (caps.hci_version >> 4) & 0xF,
            caps.hci_version & 0xF);
        serial_println!("[XHCI] MaxSlots={} MaxIntrs={} MaxPorts={} AC64={} CSZ={}",
            caps.max_slots, caps.max_intrs, caps.max_ports, caps.ac64, caps.csz);

        let op_base = caps.cap_length as usize;
        let rts_base = caps.rts_offset as usize;
        let db_base = caps.db_offset as usize;

        serial_println!("[XHCI] OpBase={:#x} RtsBase={:#x} DbBase={:#x}",
            op_base, rts_base, db_base);

        // Step 4: Wait for CNR=0, then Halt + Reset
        if !init::wait_cnr_clear(&mmio, op_base) {
            serial_println!("[XHCI] ERROR: Controller not ready (CNR stuck)");
            return None;
        }

        if !init::halt_controller(&mmio, op_base) {
            serial_println!("[XHCI] ERROR: Failed to halt controller");
            return None;
        }

        if !init::reset_controller(&mmio, op_base) {
            serial_println!("[XHCI] ERROR: Failed to reset controller");
            return None;
        }

        // Step 5: Program MaxSlotsEn
        init::set_max_slots(&mmio, op_base, caps.max_slots);

        // Step 6: Allocate and program DCBAA
        let (dcbaa_virt, dcbaa_phys) = init::setup_dcbaa(&mmio, op_base, caps.max_slots)?;

        // Step 7: Allocate and program Command Ring
        let command_ring = init::setup_command_ring(&mmio, op_base)?;

        // Step 8: Allocate and program Event Ring
        let (event_ring, erst_virt, erst_phys) = init::setup_event_ring(&mmio, rts_base)?;

        // Step 9: Enable interrupts
        init::enable_interrupts(&mmio, op_base, rts_base);

        // Step 10: Start controller
        let running = init::start_controller(&mmio, op_base);

        if running {
            serial_println!("[XHCI] === Controller OPERATIONAL (HCH=0, CNR=0) ===");
        } else {
            serial_println!("[XHCI] WARNING: Controller did not reach operational state");
        }

        // Step 11: Initialize root hub (port scanning + protocol detection)
        let root_hub = if running {
            Some(RootHub::init(&mmio, &caps, op_base))
        } else {
            // Fallback: basic port scan without root hub
            init::scan_ports(&mmio, op_base, caps.max_ports);
            None
        };

        let connected_ports = root_hub.as_ref()
            .map(|rh| rh.connected_ports().len() as u8)
            .unwrap_or(0);

        Some(Self {
            mmio,
            caps,
            op_base,
            rts_base,
            db_base,
            command_ring,
            event_ring,
            dcbaa_virt,
            dcbaa_phys,
            erst_virt,
            erst_phys,
            root_hub,
            connected_ports,
            running,
        })
    }

    /// Ring the host controller command doorbell.
    /// This notifies the HC that new commands are on the Command Ring.
    pub fn ring_command_doorbell(&self) {
        let db_offset = self.db_base + doorbell_offset(0);
        self.mmio.write32(db_offset, doorbell_value(DB_TARGET_HC_COMMAND, 0));
    }

    /// Read the current USBSTS register.
    pub fn read_status(&self) -> u32 {
        self.mmio.read32(self.op_base + OP_USBSTS).unwrap_or(0xFFFF_FFFF)
    }

    /// Check if the controller is currently running (HCH=0).
    pub fn is_running(&self) -> bool {
        let sts = self.read_status();
        sts & USBSTS_HCH == 0
    }
}

// ============================================================================
// PCI Probe
// ============================================================================

/// Check if a PCI device is an xHCI controller.
///
/// Reads prog_if from PCI config space (offset 0x09) since PciDevice
/// doesn't store it. xHCI = class 0x0C, subclass 0x03, prog_if 0x30.
pub fn is_xhci_device(dev: &PciDevice) -> bool {
    if dev.class_code != PCI_CLASS_SERIAL_BUS || dev.subclass != PCI_SUBCLASS_USB {
        return false;
    }
    let prog_if = pci::config_read_u8(dev.bus, dev.device, dev.function, 0x09);
    prog_if == PCI_PROGIF_XHCI
}

/// Probe PCI bus for xHCI controllers and initialize the first one found.
///
/// Returns the initialized controller, or None if no xHCI hardware is present.
pub fn probe_pci(devices: &[PciDevice]) -> Option<XhciController> {
    let mut xhci_count = 0u32;

    for dev in devices {
        if is_xhci_device(dev) {
            xhci_count += 1;
            serial_println!(
                "[XHCI] Found xHCI controller #{}: {:02x}:{:02x}.{} {:04x}:{:04x} BAR0={:#010x}",
                xhci_count, dev.bus, dev.device, dev.function,
                dev.vendor_id, dev.device_id, dev.bars[0]
            );

            // Initialize the first xHCI controller found
            if xhci_count == 1 {
                match XhciController::init_from_pci(dev) {
                    Some(ctrl) => return Some(ctrl),
                    None => {
                        serial_println!("[XHCI] WARNING: Failed to initialize xHCI controller");
                    }
                }
            }
        }
    }

    if xhci_count == 0 {
        serial_println!("[XHCI] No xHCI controllers found on PCI bus");
    } else if xhci_count > 1 {
        serial_println!("[XHCI] {} xHCI controllers found (only first initialized)", xhci_count);
    }

    None
}
