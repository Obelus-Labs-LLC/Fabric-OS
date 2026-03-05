//! xHCI Controller Initialization — Phase 21a.
//!
//! Implements the xHCI initialization sequence per spec section 4.2:
//! 1. Read capability registers
//! 2. Wait for CNR=0 (Controller Not Ready cleared)
//! 3. Reset via HCRST
//! 4. Allocate DCBAA, Command Ring, Event Ring
//! 5. Program registers
//! 6. Set Run/Stop → operational state
//!
//! Target: USBSTS.HCH=0, USBSTS.CNR=0 (controller running).

#![allow(dead_code)]

use crate::serial_println;
use crate::hal::driver_sdk::MmioRegion;
use crate::hal::dma::DMA_MANAGER;

use super::regs::*;
use super::context::*;

/// Maximum number of spin iterations waiting for a hardware bit.
const TIMEOUT_SPINS: u32 = 100_000;

/// Parsed capability parameters from the Capability Register space.
#[derive(Debug, Clone)]
pub struct XhciCapabilities {
    /// CAPLENGTH — offset from BAR0 to Operational Registers.
    pub cap_length: u8,
    /// HCIVERSION — xHCI spec version (e.g. 0x0100, 0x0110, 0x0120).
    pub hci_version: u16,
    /// Maximum device slots.
    pub max_slots: u8,
    /// Maximum interrupters.
    pub max_intrs: u16,
    /// Maximum root hub ports.
    pub max_ports: u8,
    /// IST — Isochronous Scheduling Threshold.
    pub ist: u8,
    /// Maximum scratchpad buffers needed.
    pub max_scratchpad_bufs: u16,
    /// 64-bit addressing capable.
    pub ac64: bool,
    /// Context size: false=32 bytes, true=64 bytes.
    pub csz: bool,
    /// Extended Capabilities pointer (DWORD offset from BAR0).
    pub xecp: u16,
    /// Doorbell Array offset from BAR0.
    pub db_offset: u32,
    /// Runtime Registers offset from BAR0.
    pub rts_offset: u32,
}

/// Read capability registers from MMIO.
pub fn read_capabilities(mmio: &MmioRegion) -> Option<XhciCapabilities> {
    let caplength = mmio.read8(CAP_CAPLENGTH)?;
    let hciversion = mmio.read16(CAP_HCIVERSION)?;
    let hcsparams1 = mmio.read32(CAP_HCSPARAMS1)?;
    let hcsparams2 = mmio.read32(CAP_HCSPARAMS2)?;
    let hccparams1 = mmio.read32(CAP_HCCPARAMS1)?;
    let dboff = mmio.read32(CAP_DBOFF)?;
    let rtsoff = mmio.read32(CAP_RTSOFF)?;

    Some(XhciCapabilities {
        cap_length: caplength,
        hci_version: hciversion,
        max_slots: hcs1_max_slots(hcsparams1),
        max_intrs: hcs1_max_intrs(hcsparams1),
        max_ports: hcs1_max_ports(hcsparams1),
        ist: hcs2_ist(hcsparams2),
        max_scratchpad_bufs: hcs2_max_scratchpad_bufs(hcsparams2),
        ac64: hcc1_ac64(hccparams1),
        csz: hcc1_csz(hccparams1),
        xecp: hcc1_xecp(hccparams1),
        db_offset: dboff & !0x3, // DWORD aligned
        rts_offset: rtsoff & !0x1F, // 32-byte aligned
    })
}

/// Wait for USBSTS.CNR to clear (Controller Not Ready → Ready).
/// Returns true if ready, false on timeout.
pub fn wait_cnr_clear(mmio: &MmioRegion, op_base: usize) -> bool {
    for _ in 0..TIMEOUT_SPINS {
        if let Some(sts) = mmio.read32(op_base + OP_USBSTS) {
            if sts & USBSTS_CNR == 0 {
                return true;
            }
        }
        // Spin — in a real OS we'd yield or use a timer.
        core::hint::spin_loop();
    }
    false
}

/// Wait for USBCMD.HCRST to self-clear after reset.
/// Returns true if cleared, false on timeout.
pub fn wait_reset_complete(mmio: &MmioRegion, op_base: usize) -> bool {
    for _ in 0..TIMEOUT_SPINS {
        if let Some(cmd) = mmio.read32(op_base + OP_USBCMD) {
            if cmd & USBCMD_HCRST == 0 {
                return true;
            }
        }
        core::hint::spin_loop();
    }
    false
}

/// Wait for USBSTS.HCH to clear (controller running).
/// Returns true if running, false on timeout.
pub fn wait_running(mmio: &MmioRegion, op_base: usize) -> bool {
    for _ in 0..TIMEOUT_SPINS {
        if let Some(sts) = mmio.read32(op_base + OP_USBSTS) {
            if sts & USBSTS_HCH == 0 {
                return true;
            }
        }
        core::hint::spin_loop();
    }
    false
}

/// Phase 1: Halt the controller if it's running.
/// Set USBCMD.RS=0, then wait for USBSTS.HCH=1.
pub fn halt_controller(mmio: &MmioRegion, op_base: usize) -> bool {
    if let Some(cmd) = mmio.read32(op_base + OP_USBCMD) {
        // Clear Run/Stop
        mmio.write32(op_base + OP_USBCMD, cmd & !USBCMD_RS);
    }

    // Wait for HCH=1 (halted)
    for _ in 0..TIMEOUT_SPINS {
        if let Some(sts) = mmio.read32(op_base + OP_USBSTS) {
            if sts & USBSTS_HCH != 0 {
                return true;
            }
        }
        core::hint::spin_loop();
    }
    false
}

/// Phase 2: Reset the controller via USBCMD.HCRST.
pub fn reset_controller(mmio: &MmioRegion, op_base: usize) -> bool {
    serial_println!("[XHCI] Resetting controller...");

    // Set HCRST
    if let Some(cmd) = mmio.read32(op_base + OP_USBCMD) {
        mmio.write32(op_base + OP_USBCMD, cmd | USBCMD_HCRST);
    }

    // Wait for HCRST to self-clear
    if !wait_reset_complete(mmio, op_base) {
        serial_println!("[XHCI] ERROR: HCRST did not clear (timeout)");
        return false;
    }

    // Wait for CNR to clear
    if !wait_cnr_clear(mmio, op_base) {
        serial_println!("[XHCI] ERROR: CNR did not clear after reset (timeout)");
        return false;
    }

    serial_println!("[XHCI] Reset complete");
    true
}

/// Phase 3: Program MaxSlotsEn in CONFIG register.
pub fn set_max_slots(mmio: &MmioRegion, op_base: usize, max_slots: u8) {
    // Read current CONFIG, set MaxSlotsEn in low 8 bits
    let config = mmio.read32(op_base + OP_CONFIG).unwrap_or(0);
    let new_config = (config & !0xFF) | (max_slots as u32);
    mmio.write32(op_base + OP_CONFIG, new_config);
    serial_println!("[XHCI] MaxSlotsEn = {}", max_slots);
}

/// Phase 4: Allocate and program DCBAA.
/// Returns (virt_addr, phys_addr) of the DCBAA, or None on failure.
pub fn setup_dcbaa(
    mmio: &MmioRegion,
    op_base: usize,
    max_slots: u8,
) -> Option<(usize, usize)> {
    let size = dcbaa_size(max_slots);
    // DCBAA must be 64-byte aligned — DMA allocator handles this.
    let dma = DMA_MANAGER.lock().alloc(size, 0)?;

    // Zero the DCBAA
    unsafe {
        core::ptr::write_bytes(dma.virt as *mut u8, 0, dma.size);
    }

    // Program DCBAAP
    let phys = dma.phys as u64;
    mmio.write32(op_base + OP_DCBAAP_LO, (phys & 0xFFFF_FFFF) as u32);
    mmio.write32(op_base + OP_DCBAAP_HI, ((phys >> 32) & 0xFFFF_FFFF) as u32);

    serial_println!(
        "[XHCI] DCBAA: virt={:#x}, phys={:#x}, size={} ({} slots)",
        dma.virt, dma.phys, size, max_slots
    );
    Some((dma.virt, dma.phys))
}

/// Phase 5: Allocate and program the Command Ring.
/// Returns the TrbRing descriptor, or None on failure.
pub fn setup_command_ring(
    mmio: &MmioRegion,
    op_base: usize,
) -> Option<TrbRing> {
    let dma_size = TrbRing::dma_size(RING_SIZE);
    let dma = DMA_MANAGER.lock().alloc(dma_size, 0)?;

    let mut ring = TrbRing::new(dma.virt, dma.phys, RING_SIZE);
    ring.init_producer();

    // Program CRCR: physical address of ring | RCS=1 (initial cycle state)
    let crcr_val = (dma.phys as u64 & CRCR_PTR_MASK) | CRCR_RCS;
    mmio.write32(op_base + OP_CRCR_LO, (crcr_val & 0xFFFF_FFFF) as u32);
    mmio.write32(op_base + OP_CRCR_HI, ((crcr_val >> 32) & 0xFFFF_FFFF) as u32);

    serial_println!(
        "[XHCI] Command Ring: virt={:#x}, phys={:#x}, {} TRBs",
        dma.virt, dma.phys, RING_SIZE
    );
    Some(ring)
}

/// Phase 6: Allocate and program the primary Event Ring (Interrupter 0).
/// Returns (event_ring, erst_virt, erst_phys) or None on failure.
pub fn setup_event_ring(
    mmio: &MmioRegion,
    rts_base: usize,
) -> Option<(TrbRing, usize, usize)> {
    // Allocate Event Ring segment
    let ring_dma_size = TrbRing::dma_size(RING_SIZE);
    let ring_dma = DMA_MANAGER.lock().alloc(ring_dma_size, 0)?;

    let mut ring = TrbRing::new(ring_dma.virt, ring_dma.phys, RING_SIZE);
    ring.init_consumer();

    // Allocate Event Ring Segment Table (1 entry = 16 bytes, but must be 64-byte aligned)
    let erst_size = core::mem::size_of::<EventRingSegmentTableEntry>();
    // Allocate at least 64 bytes for alignment
    let erst_alloc = 64.max(erst_size);
    let erst_dma = DMA_MANAGER.lock().alloc(erst_alloc, 0)?;

    // Fill ERST entry 0
    let entry = EventRingSegmentTableEntry::new(ring_dma.phys as u64, RING_SIZE as u16);
    unsafe {
        core::ptr::write_volatile(erst_dma.virt as *mut EventRingSegmentTableEntry, entry);
    }

    // Program Interrupter 0 registers
    let ir0 = rts_base + interrupter_offset(0);

    // ERSTSZ = 1 (one segment)
    mmio.write32(ir0 + IR_ERSTSZ, 1);

    // ERDP = start of event ring (with EHB=0)
    let erdp = ring_dma.phys as u64;
    mmio.write32(ir0 + IR_ERDP_LO, (erdp & 0xFFFF_FFFF) as u32);
    mmio.write32(ir0 + IR_ERDP_HI, ((erdp >> 32) & 0xFFFF_FFFF) as u32);

    // ERSTBA = ERST physical address (must write AFTER ERSTSZ and ERDP)
    let erstba = erst_dma.phys as u64;
    mmio.write32(ir0 + IR_ERSTBA_LO, (erstba & 0xFFFF_FFFF) as u32);
    mmio.write32(ir0 + IR_ERSTBA_HI, ((erstba >> 32) & 0xFFFF_FFFF) as u32);

    serial_println!(
        "[XHCI] Event Ring: virt={:#x}, phys={:#x}, {} TRBs",
        ring_dma.virt, ring_dma.phys, RING_SIZE
    );
    serial_println!(
        "[XHCI] ERST: virt={:#x}, phys={:#x}, 1 segment",
        erst_dma.virt, erst_dma.phys
    );

    Some((ring, erst_dma.virt, erst_dma.phys))
}

/// Phase 7: Enable interrupts on Interrupter 0.
pub fn enable_interrupts(mmio: &MmioRegion, op_base: usize, rts_base: usize) {
    let ir0 = rts_base + interrupter_offset(0);

    // Set IMAN.IE = 1 (Interrupt Enable on Interrupter 0)
    let iman = mmio.read32(ir0 + IR_IMAN).unwrap_or(0);
    mmio.write32(ir0 + IR_IMAN, iman | IMAN_IE);

    // Set IMOD (moderation: interval=160 = 40us at 250ns units)
    mmio.write32(ir0 + IR_IMOD, 160);

    // Set USBCMD.INTE = 1 (global interrupt enable)
    let cmd = mmio.read32(op_base + OP_USBCMD).unwrap_or(0);
    mmio.write32(op_base + OP_USBCMD, cmd | USBCMD_INTE);

    serial_println!("[XHCI] Interrupts enabled (INTE=1, IMAN.IE=1)");
}

/// Phase 8: Start the controller — set USBCMD.RS=1.
/// Returns true if controller reaches operational state.
pub fn start_controller(mmio: &MmioRegion, op_base: usize) -> bool {
    serial_println!("[XHCI] Starting controller (RS=1)...");

    let cmd = mmio.read32(op_base + OP_USBCMD).unwrap_or(0);
    mmio.write32(op_base + OP_USBCMD, cmd | USBCMD_RS);

    // Wait for HCH to clear (controller running)
    if !wait_running(mmio, op_base) {
        serial_println!("[XHCI] ERROR: Controller did not start (HCH still set)");
        return false;
    }

    // Verify final status
    let sts = mmio.read32(op_base + OP_USBSTS).unwrap_or(0xFFFF_FFFF);
    let hch = sts & USBSTS_HCH != 0;
    let cnr = sts & USBSTS_CNR != 0;
    let hse = sts & USBSTS_HSE != 0;
    let hce = sts & USBSTS_HCE != 0;

    serial_println!(
        "[XHCI] Status: HCH={}, CNR={}, HSE={}, HCE={}",
        hch as u8, cnr as u8, hse as u8, hce as u8
    );

    if !hch && !cnr && !hse && !hce {
        serial_println!("[XHCI] *** Controller OPERATIONAL ***");
        true
    } else {
        serial_println!("[XHCI] ERROR: Controller not in expected state");
        false
    }
}

/// Log port status for all root hub ports.
pub fn scan_ports(mmio: &MmioRegion, op_base: usize, max_ports: u8) {
    serial_println!("[XHCI] Scanning {} root hub ports:", max_ports);
    for port in 0..max_ports {
        let offset = op_base + portsc_offset(port);
        if let Some(portsc) = mmio.read32(offset) {
            let connected = portsc & PORTSC_CCS != 0;
            let enabled = portsc & PORTSC_PED != 0;
            let powered = portsc & PORTSC_PP != 0;
            let speed = portsc_speed(portsc);
            let pls = portsc_pls(portsc);

            if connected {
                let speed_str = match speed {
                    SPEED_LOW => "Low (1.5 Mbps)",
                    SPEED_FULL => "Full (12 Mbps)",
                    SPEED_HIGH => "High (480 Mbps)",
                    SPEED_SUPER => "Super (5 Gbps)",
                    SPEED_SUPER_PLUS => "Super+ (10 Gbps)",
                    _ => "Unknown",
                };
                serial_println!(
                    "  Port {}: CONNECTED speed={} enabled={} PLS={} powered={}",
                    port, speed_str, enabled, pls, powered
                );
            } else if powered {
                serial_println!("  Port {}: empty (powered, PLS={})", port, pls);
            }
        }
    }
}
