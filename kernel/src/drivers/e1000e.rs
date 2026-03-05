//! Intel e1000e Gigabit Ethernet Driver — Phase 20A.
//!
//! Supports I217/I218/I219 family NICs commonly found in Intel desktop
//! and laptop platforms (e.g., Dell Inspiron 5558/5559).
//! Uses MMIO via MmioRegion, DMA descriptor rings via DmaManager,
//! and registers IRQ via IrqRouter.

#![allow(dead_code)]

use crate::serial_println;
use crate::pci::PciDevice;
use crate::hal::driver_sdk::{MmioRegion, DmaBuffer};
use crate::hal::dma::DMA_MANAGER;
use crate::memory::{PhysAddr, VirtAddr, PAGE_SIZE, hhdm_offset, page_table::PageTableFlags};
use crate::network::nic_trait::NicDriver;

// ============================================================================
// PCI Device IDs — Intel e1000e family
// ============================================================================

/// Intel vendor ID.
pub const VENDOR_INTEL: u16 = 0x8086;

/// Supported Intel e1000e device IDs (I217/I218/I219 family).
pub const DEVICE_IDS: [u16; 6] = [
    0x153A, // I217-LM
    0x155A, // I218-V
    0x15B8, // I219-V (Skylake)
    0x1570, // I219-LM (Skylake)
    0x15B7, // I219-LM (Kaby Lake)
    0x15D7, // I219-LM (Cannon Lake)
];

/// Additional Intel e1000e device IDs (extended coverage for Broadwell/Haswell).
pub const DEVICE_IDS_EXT: [u16; 6] = [
    0x15A0, // I218-LM (Broadwell)
    0x15A1, // I218-V (Broadwell)
    0x15A2, // I218-LM (Broadwell)
    0x15A3, // I218-V (Broadwell)
    0x156F, // I219-LM (Broadwell)
    0x1533, // I210 (common in Mini-ITX boards)
];

/// Realtek vendor ID.
pub const VENDOR_REALTEK: u16 = 0x10EC;

/// Known Realtek Ethernet device IDs (not yet driven — for detection/logging).
pub const REALTEK_DEVICE_IDS: [u16; 4] = [
    0x8168, // RTL8111/8168/8411 (most common PCIe GbE)
    0x8169, // RTL8169 (older PCIe GbE)
    0x8136, // RTL8101/8102E (Fast Ethernet)
    0x8161, // RTL8111GR
];

/// Check if a PCI device is a supported Intel e1000e NIC.
pub fn is_e1000e(dev: &PciDevice) -> bool {
    if dev.vendor_id != VENDOR_INTEL {
        return false;
    }
    DEVICE_IDS.contains(&dev.device_id) || DEVICE_IDS_EXT.contains(&dev.device_id)
}

/// Check if a PCI device is a Realtek Ethernet NIC (detected but not yet driven).
pub fn is_realtek_nic(dev: &PciDevice) -> bool {
    dev.vendor_id == VENDOR_REALTEK && REALTEK_DEVICE_IDS.contains(&dev.device_id)
}

/// Check if a PCI device is any Ethernet controller (class 0x02, subclass 0x00).
pub fn is_ethernet_controller(dev: &PciDevice) -> bool {
    dev.class_code == 0x02 && dev.subclass == 0x00
}

/// Return a human-readable name for a known NIC vendor/device.
pub fn nic_name(dev: &PciDevice) -> &'static str {
    match dev.vendor_id {
        VENDOR_INTEL => {
            if DEVICE_IDS.contains(&dev.device_id) || DEVICE_IDS_EXT.contains(&dev.device_id) {
                "Intel e1000e"
            } else {
                "Intel (unknown NIC)"
            }
        }
        VENDOR_REALTEK => {
            match dev.device_id {
                0x8168 => "Realtek RTL8168/8111",
                0x8169 => "Realtek RTL8169",
                0x8136 => "Realtek RTL8101/8102E",
                0x8161 => "Realtek RTL8111GR",
                _ => "Realtek (unknown NIC)",
            }
        }
        0x1AF4 => "VirtIO-net (QEMU)",
        _ => "Unknown NIC",
    }
}

// ============================================================================
// MMIO Register Offsets
// ============================================================================

/// Device Control Register
pub const REG_CTRL: usize = 0x0000;
/// Device Status Register
pub const REG_STATUS: usize = 0x0008;
/// Interrupt Cause Read
pub const REG_ICR: usize = 0x00C0;
/// Interrupt Mask Set/Read
pub const REG_IMS: usize = 0x00D0;
/// Interrupt Mask Clear
pub const REG_IMC: usize = 0x00D8;
/// Receive Control Register
pub const REG_RCTL: usize = 0x0100;
/// Transmit Control Register
pub const REG_TCTL: usize = 0x0400;

/// Receive Descriptor Base Address Low
pub const REG_RDBAL: usize = 0x2800;
/// Receive Descriptor Base Address High
pub const REG_RDBAH: usize = 0x2804;
/// Receive Descriptor Length
pub const REG_RDLEN: usize = 0x2808;
/// Receive Descriptor Head
pub const REG_RDH: usize = 0x2810;
/// Receive Descriptor Tail
pub const REG_RDT: usize = 0x2818;

/// Transmit Descriptor Base Address Low
pub const REG_TDBAL: usize = 0x3800;
/// Transmit Descriptor Base Address High
pub const REG_TDBAH: usize = 0x3804;
/// Transmit Descriptor Length
pub const REG_TDLEN: usize = 0x3808;
/// Transmit Descriptor Head
pub const REG_TDH: usize = 0x3810;
/// Transmit Descriptor Tail
pub const REG_TDT: usize = 0x3818;

/// Receive Address Low (MAC bytes 0-3)
pub const REG_RAL: usize = 0x5400;
/// Receive Address High (MAC bytes 4-5 + flags)
pub const REG_RAH: usize = 0x5404;
/// Multicast Table Array (128 entries × 4 bytes)
pub const REG_MTA: usize = 0x5200;

// ============================================================================
// Control Bits
// ============================================================================

/// CTRL: Set Link Up
pub const CTRL_SLU: u32 = 1 << 6;
/// CTRL: Device Reset
pub const CTRL_RST: u32 = 1 << 26;

/// RCTL: Receiver Enable
pub const RCTL_EN: u32 = 1 << 1;
/// RCTL: Store Bad Packets
pub const RCTL_SBP: u32 = 1 << 2;
/// RCTL: Unicast Promiscuous Enable
pub const RCTL_UPE: u32 = 1 << 3;
/// RCTL: Multicast Promiscuous Enable
pub const RCTL_MPE: u32 = 1 << 4;
/// RCTL: Broadcast Accept Mode
pub const RCTL_BAM: u32 = 1 << 15;
/// RCTL: Buffer Size 2048 bytes (BSIZE=0, BSEX=0)
pub const RCTL_BSIZE_2048: u32 = 0;
/// RCTL: Strip Ethernet CRC
pub const RCTL_SECRC: u32 = 1 << 26;

/// TCTL: Transmit Enable
pub const TCTL_EN: u32 = 1 << 1;
/// TCTL: Pad Short Packets
pub const TCTL_PSP: u32 = 1 << 3;
/// TCTL: Collision Threshold (default 0x10)
pub const TCTL_CT_SHIFT: u32 = 4;
/// TCTL: Collision Distance (default 0x40)
pub const TCTL_COLD_SHIFT: u32 = 12;

/// IMS: Transmit Descriptor Written Back
pub const IMS_TXDW: u32 = 1 << 0;
/// IMS: Receive Descriptor Minimum Threshold Hit
pub const IMS_RXDMT0: u32 = 1 << 4;
/// IMS: Receiver FIFO Overrun
pub const IMS_RXO: u32 = 1 << 6;
/// IMS: Receiver Timer Interrupt
pub const IMS_RXT0: u32 = 1 << 7;
/// IMS: Link Status Change
pub const IMS_LSC: u32 = 1 << 2;

// ============================================================================
// TX/RX Descriptor Structs
// ============================================================================

/// Number of TX descriptors in the ring.
pub const TX_RING_SIZE: usize = 32;
/// Number of RX descriptors in the ring.
pub const RX_RING_SIZE: usize = 32;
/// Size of each TX/RX packet buffer.
pub const BUFFER_SIZE: usize = 2048;

/// Transmit Descriptor (legacy format, 16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct E1000eTxDesc {
    pub addr: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

/// TX Command bits.
pub const TX_CMD_EOP: u8 = 1 << 0;  // End of Packet
pub const TX_CMD_IFCS: u8 = 1 << 1; // Insert FCS/CRC
pub const TX_CMD_RS: u8 = 1 << 3;   // Report Status

/// TX Status bits.
pub const TX_STATUS_DD: u8 = 1 << 0; // Descriptor Done

/// Receive Descriptor (legacy format, 16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct E1000eRxDesc {
    pub addr: u64,
    pub length: u16,
    pub checksum: u16,
    pub status: u8,
    pub errors: u8,
    pub special: u16,
}

/// RX Status bits.
pub const RX_STATUS_DD: u8 = 1 << 0;  // Descriptor Done
pub const RX_STATUS_EOP: u8 = 1 << 1; // End of Packet

// ============================================================================
// Driver Struct
// ============================================================================

/// Intel e1000e Gigabit Ethernet driver instance.
pub struct E1000eDriver {
    /// MMIO region for register access.
    pub mmio: MmioRegion,
    /// MAC address read from hardware.
    pub mac: [u8; 6],
    /// IRQ vector assigned by PCI/IOAPIC.
    pub irq_vector: u8,

    // TX state
    tx_descs_dma: DmaBuffer,
    tx_buffers_dma: DmaBuffer,
    tx_tail: u16,

    // RX state
    rx_descs_dma: DmaBuffer,
    rx_buffers_dma: DmaBuffer,
    rx_tail: u16,
    /// Software-tracked RX head for poll_rx/recycle_rx.
    rx_head_sw: u16,

    /// Link status.
    pub link_up: bool,
    /// Whether the driver completed initialization.
    pub initialized: bool,
}

impl E1000eDriver {
    /// Extract MMIO base address from BAR0.
    ///
    /// BAR0 bit 0 = 0 means memory-mapped (MMIO).
    /// Bits 1-2 indicate 32-bit (00) or 64-bit (10) BAR.
    /// For 64-bit BARs, the upper 32 bits come from BAR1.
    pub fn bar0_mmio_base(dev: &PciDevice) -> Option<u64> {
        let bar0 = dev.bars[0];
        // Bit 0 must be 0 for MMIO (not I/O space)
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
            _ => None, // Reserved
        }
    }

    /// Initialize the e1000e driver from a PCI device.
    ///
    /// Performs the full hardware initialization sequence:
    /// 1. Extract MMIO base from BAR0
    /// 2. Map MMIO pages with NO_CACHE flags
    /// 3. Reset the device
    /// 4. Read MAC address from RAL/RAH
    /// 5. Allocate DMA rings for TX/RX descriptors + packet buffers
    /// 6. Program descriptor ring registers
    /// 7. Clear Multicast Table Array
    /// 8. Enable TX, RX, and interrupts
    /// 9. Set link up
    pub fn init_from_pci(dev: &PciDevice) -> Option<Self> {
        let mmio_phys = Self::bar0_mmio_base(dev)?;
        serial_println!("[E1000E] BAR0 MMIO phys = 0x{:x}", mmio_phys);

        // Map MMIO pages (e1000e uses 128KB of register space, map 32 pages)
        let mmio_pages = 32; // 128KB
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
                    serial_println!("[E1000E] Failed to map MMIO page {}", i);
                    return None;
                }
            }
        }

        let mmio = MmioRegion::new(mmio_virt_base as usize, mmio_pages * PAGE_SIZE);

        // --- Device Reset ---
        let ctrl = mmio.read32(REG_CTRL).unwrap_or(0);
        mmio.write32(REG_CTRL, ctrl | CTRL_RST);
        // Busy-wait for reset to complete (~1ms)
        for _ in 0..100_000 {
            core::hint::spin_loop();
        }
        // Disable all interrupts during setup
        mmio.write32(REG_IMC, 0xFFFFFFFF);

        // --- Read MAC address from RAL/RAH ---
        let ral = mmio.read32(REG_RAL).unwrap_or(0);
        let rah = mmio.read32(REG_RAH).unwrap_or(0);
        let mac = [
            (ral & 0xFF) as u8,
            ((ral >> 8) & 0xFF) as u8,
            ((ral >> 16) & 0xFF) as u8,
            ((ral >> 24) & 0xFF) as u8,
            (rah & 0xFF) as u8,
            ((rah >> 8) & 0xFF) as u8,
        ];
        serial_println!(
            "[E1000E] MAC = {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );

        // --- Allocate DMA for TX descriptor ring ---
        let tx_ring_bytes = TX_RING_SIZE * core::mem::size_of::<E1000eTxDesc>();
        let tx_descs_dma = DMA_MANAGER.lock().alloc(tx_ring_bytes, 0)?;
        // Zero the TX descriptor ring
        unsafe {
            core::ptr::write_bytes(tx_descs_dma.virt as *mut u8, 0, tx_descs_dma.size);
        }

        // --- Allocate DMA for TX packet buffers ---
        let tx_bufs_bytes = TX_RING_SIZE * BUFFER_SIZE;
        let tx_buffers_dma = DMA_MANAGER.lock().alloc(tx_bufs_bytes, 0)?;
        unsafe {
            core::ptr::write_bytes(tx_buffers_dma.virt as *mut u8, 0, tx_buffers_dma.size);
        }

        // Initialize TX descriptors — each points to its buffer
        let tx_descs = tx_descs_dma.virt as *mut E1000eTxDesc;
        for i in 0..TX_RING_SIZE {
            let buf_phys = tx_buffers_dma.phys + i * BUFFER_SIZE;
            unsafe {
                (*tx_descs.add(i)).addr = buf_phys as u64;
                (*tx_descs.add(i)).status = TX_STATUS_DD; // Mark as done (available)
            }
        }

        // --- Allocate DMA for RX descriptor ring ---
        let rx_ring_bytes = RX_RING_SIZE * core::mem::size_of::<E1000eRxDesc>();
        let rx_descs_dma = DMA_MANAGER.lock().alloc(rx_ring_bytes, 0)?;
        unsafe {
            core::ptr::write_bytes(rx_descs_dma.virt as *mut u8, 0, rx_descs_dma.size);
        }

        // --- Allocate DMA for RX packet buffers ---
        let rx_bufs_bytes = RX_RING_SIZE * BUFFER_SIZE;
        let rx_buffers_dma = DMA_MANAGER.lock().alloc(rx_bufs_bytes, 0)?;
        unsafe {
            core::ptr::write_bytes(rx_buffers_dma.virt as *mut u8, 0, rx_buffers_dma.size);
        }

        // Initialize RX descriptors — each points to its buffer
        let rx_descs = rx_descs_dma.virt as *mut E1000eRxDesc;
        for i in 0..RX_RING_SIZE {
            let buf_phys = rx_buffers_dma.phys + i * BUFFER_SIZE;
            unsafe {
                (*rx_descs.add(i)).addr = buf_phys as u64;
                (*rx_descs.add(i)).status = 0;
            }
        }

        // --- Program TX Descriptor Ring ---
        mmio.write32(REG_TDBAL, (tx_descs_dma.phys & 0xFFFFFFFF) as u32);
        mmio.write32(REG_TDBAH, ((tx_descs_dma.phys >> 32) & 0xFFFFFFFF) as u32);
        mmio.write32(REG_TDLEN, tx_ring_bytes as u32);
        mmio.write32(REG_TDH, 0); // Head = 0
        mmio.write32(REG_TDT, 0); // Tail = 0

        // --- Program RX Descriptor Ring ---
        mmio.write32(REG_RDBAL, (rx_descs_dma.phys & 0xFFFFFFFF) as u32);
        mmio.write32(REG_RDBAH, ((rx_descs_dma.phys >> 32) & 0xFFFFFFFF) as u32);
        mmio.write32(REG_RDLEN, rx_ring_bytes as u32);
        mmio.write32(REG_RDH, 0); // Head = 0
        // Tail = N-1: hardware owns all descriptors
        mmio.write32(REG_RDT, (RX_RING_SIZE - 1) as u32);

        // --- Clear Multicast Table Array (128 entries) ---
        for i in 0..128 {
            mmio.write32(REG_MTA + i * 4, 0);
        }

        // --- Enable Transmit ---
        let tctl = TCTL_EN
            | TCTL_PSP
            | (0x10 << TCTL_CT_SHIFT)   // Collision threshold
            | (0x40 << TCTL_COLD_SHIFT); // Collision distance
        mmio.write32(REG_TCTL, tctl);

        // --- Enable Receive ---
        let rctl = RCTL_EN
            | RCTL_BAM         // Accept broadcast
            | RCTL_BSIZE_2048  // 2KB buffers
            | RCTL_SECRC;      // Strip CRC
        mmio.write32(REG_RCTL, rctl);

        // --- Enable Interrupts ---
        let ims = IMS_TXDW | IMS_RXDMT0 | IMS_RXO | IMS_RXT0 | IMS_LSC;
        mmio.write32(REG_IMS, ims);

        // --- Set Link Up ---
        let ctrl = mmio.read32(REG_CTRL).unwrap_or(0);
        mmio.write32(REG_CTRL, ctrl | CTRL_SLU);

        // Check link status
        let status = mmio.read32(REG_STATUS).unwrap_or(0);
        let link_up = status & 0x2 != 0;

        serial_println!(
            "[E1000E] Initialized: TX={} RX={} descriptors, link={}",
            TX_RING_SIZE, RX_RING_SIZE,
            if link_up { "UP" } else { "DOWN" }
        );

        let irq_vector = dev.irq_line;

        Some(Self {
            mmio,
            mac,
            irq_vector,
            tx_descs_dma,
            tx_buffers_dma,
            tx_tail: 0,
            rx_descs_dma,
            rx_buffers_dma,
            rx_tail: (RX_RING_SIZE - 1) as u16,
            rx_head_sw: 0,
            link_up,
            initialized: true,
        })
    }
}

// ============================================================================
// NicDriver Trait Implementation
// ============================================================================

impl NicDriver for E1000eDriver {
    fn name(&self) -> &'static str {
        "e1000e"
    }

    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send_packet(&mut self, data: &[u8]) -> bool {
        if !self.initialized || data.len() > BUFFER_SIZE {
            return false;
        }

        let idx = self.tx_tail as usize;
        let descs = self.tx_descs_dma.virt as *mut E1000eTxDesc;

        // Check if this descriptor is available (DD bit set)
        let desc_status = unsafe { (*descs.add(idx)).status };
        if desc_status & TX_STATUS_DD == 0 {
            return false; // Descriptor still in use by hardware
        }

        // Copy packet data to the TX buffer
        let buf_virt = self.tx_buffers_dma.virt + idx * BUFFER_SIZE;
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr(),
                buf_virt as *mut u8,
                data.len(),
            );
        }

        // Set up the descriptor
        unsafe {
            let desc = &mut *descs.add(idx);
            desc.length = data.len() as u16;
            desc.cmd = TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS;
            desc.status = 0; // Clear DD — hardware will set it when done
        }

        // Advance tail
        self.tx_tail = ((idx + 1) % TX_RING_SIZE) as u16;
        self.mmio.write32(REG_TDT, self.tx_tail as u32);

        true
    }

    fn poll_rx(&mut self) -> Option<(usize, usize)> {
        let idx = self.rx_head_sw as usize;
        let descs = self.rx_descs_dma.virt as *const E1000eRxDesc;

        let desc = unsafe { &*descs.add(idx) };

        // Check if hardware has written to this descriptor (DD bit)
        if desc.status & RX_STATUS_DD == 0 {
            return None;
        }

        let len = desc.length as usize;
        // Return virtual address of the received data
        let buf_virt = self.rx_buffers_dma.virt + idx * BUFFER_SIZE;

        // Advance software head (but don't recycle yet — caller must copy first)
        self.rx_head_sw = ((idx + 1) % RX_RING_SIZE) as u16;

        Some((buf_virt, len))
    }

    fn recycle_rx(&mut self) {
        // Recycle the previously polled descriptor
        // rx_head_sw was already advanced by poll_rx, so the descriptor
        // we want to recycle is (rx_head_sw - 1 + RX_RING_SIZE) % RX_RING_SIZE
        let recycled_idx = if self.rx_head_sw == 0 {
            RX_RING_SIZE - 1
        } else {
            (self.rx_head_sw - 1) as usize
        };

        let descs = self.rx_descs_dma.virt as *mut E1000eRxDesc;
        unsafe {
            let desc = &mut *descs.add(recycled_idx);
            desc.status = 0; // Clear status — hardware can reuse
            desc.length = 0;
        }

        // Update RX tail — give this descriptor back to hardware
        self.rx_tail = recycled_idx as u16;
        self.mmio.write32(REG_RDT, self.rx_tail as u32);
    }

    fn handle_irq(&mut self) -> bool {
        // Read and clear ICR (Interrupt Cause Read is auto-clear on read)
        let icr = self.mmio.read32(REG_ICR).unwrap_or(0);
        if icr == 0 {
            return false; // Not our interrupt
        }

        // Link Status Change
        if icr & IMS_LSC != 0 {
            let status = self.mmio.read32(REG_STATUS).unwrap_or(0);
            self.link_up = status & 0x2 != 0;
            serial_println!(
                "[E1000E] Link status change: {}",
                if self.link_up { "UP" } else { "DOWN" }
            );
        }

        true
    }
}

// Safety: E1000eDriver uses DMA buffers and MMIO which are not thread-local.
// Access is protected by the ACTIVE_NIC mutex.
unsafe impl Send for E1000eDriver {}
