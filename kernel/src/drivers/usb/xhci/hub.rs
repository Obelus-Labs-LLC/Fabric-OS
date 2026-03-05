//! xHCI Root Hub Emulation — Phase 21b.
//!
//! The xHCI root hub is a virtual hub exposed by the host controller.
//! Each physical USB connector is split into a USB 2.0 port (for LS/FS/HS)
//! and a USB 3.0 port (for SS/SS+). The Extended Capabilities registers
//! define which xHCI port numbers map to USB 2.0 vs USB 3.0.
//!
//! This module provides:
//! - Protocol detection (USB 2.0/3.0 port routing via xECP)
//! - Full port scanning with state machine tracking
//! - Port Status Change Event handling
//! - Port reset and speed negotiation
//! - Connection/disconnection event processing

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;
use crate::serial_println;
use crate::hal::driver_sdk::MmioRegion;

use super::regs::*;
use super::context::*;
use super::init::XhciCapabilities;
use super::port::*;

// ============================================================================
// Extended Capability: Supported Protocol
// ============================================================================

/// xHCI Extended Capability ID for "Supported Protocol" (USB 2.0 or 3.0).
const XECP_ID_SUPPORTED_PROTOCOL: u8 = 2;

/// Parsed Supported Protocol capability.
///
/// Tells us which xHCI port range is USB 2.0 and which is USB 3.0.
/// Found by walking the Extended Capabilities list from xECP offset.
#[derive(Debug, Clone)]
pub struct SupportedProtocol {
    /// Major revision: 2 = USB 2.0, 3 = USB 3.0.
    pub major: u8,
    /// Minor revision (e.g., 0x00 = USB 3.0, 0x10 = USB 3.1).
    pub minor: u8,
    /// First 1-based port number covered by this protocol.
    pub port_offset: u8,
    /// Number of ports covered.
    pub port_count: u8,
    /// Protocol slot type (from DW2).
    pub slot_type: u8,
}

impl SupportedProtocol {
    /// Is this a USB 2.0 protocol capability?
    pub fn is_usb2(&self) -> bool {
        self.major == 2
    }

    /// Is this a USB 3.0 protocol capability?
    pub fn is_usb3(&self) -> bool {
        self.major == 3
    }

    /// Convert 1-based port_offset to 0-based range.
    /// Returns (start_index, end_index_exclusive).
    pub fn port_range_0based(&self) -> (u8, u8) {
        let start = self.port_offset.saturating_sub(1);
        let end = start + self.port_count;
        (start, end)
    }
}

/// Walk the xHCI Extended Capabilities list and extract Supported Protocol entries.
///
/// The xECP pointer from HCCPARAMS1 gives the DWORD offset from BAR0
/// to the first Extended Capability. Each capability has:
///   DW0: [7:0] ID, [15:8] Next pointer (DWORDs), [31:16] cap-specific
///   DW1+: cap-specific data
///
/// For Supported Protocol (ID=2):
///   DW0: [31:24] Major, [23:16] Minor
///   DW2: [7:0] Compatible Port Offset, [15:8] Compatible Port Count
///   DW2: [27:24] Protocol Slot Type
pub fn read_supported_protocols(
    mmio: &MmioRegion,
    xecp_offset: u16,
) -> Vec<SupportedProtocol> {
    let mut protocols = Vec::new();

    if xecp_offset == 0 {
        return protocols;
    }

    // xECP is in DWORDs from BAR0
    let mut offset = (xecp_offset as usize) * 4;
    let mut iterations = 0;
    const MAX_ITERATIONS: usize = 256; // Safety limit

    while offset != 0 && iterations < MAX_ITERATIONS {
        iterations += 1;

        let dw0 = match mmio.read32(offset) {
            Some(v) => v,
            None => break,
        };

        let cap_id = (dw0 & 0xFF) as u8;
        let next_ptr = ((dw0 >> 8) & 0xFF) as usize; // DWORDs to next cap

        if cap_id == XECP_ID_SUPPORTED_PROTOCOL {
            let major = ((dw0 >> 24) & 0xFF) as u8;
            let minor = ((dw0 >> 16) & 0xFF) as u8;

            // DW2 has port offset and count
            let dw2 = mmio.read32(offset + 8).unwrap_or(0);
            let port_offset = (dw2 & 0xFF) as u8;
            let port_count = ((dw2 >> 8) & 0xFF) as u8;
            let slot_type = ((dw2 >> 24) & 0xF) as u8;

            protocols.push(SupportedProtocol {
                major,
                minor,
                port_offset,
                port_count,
                slot_type,
            });
        }

        if next_ptr == 0 {
            break;
        }
        offset += next_ptr * 4;
    }

    protocols
}

// ============================================================================
// USB Standard Request Types
// ============================================================================

/// USB Setup Packet — 8-byte control request per USB 2.0 spec §9.3.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct UsbSetupPacket {
    /// Characteristics of request (direction, type, recipient).
    pub bm_request_type: u8,
    /// Specific request code.
    pub b_request: u8,
    /// Word-sized field that varies according to request.
    pub w_value: u16,
    /// Word-sized field that varies according to request (often port number).
    pub w_index: u16,
    /// Number of bytes to transfer if there is a data stage.
    pub w_length: u16,
}

// bmRequestType direction bit
const USB_DIR_IN: u8 = 0x80;
const USB_DIR_OUT: u8 = 0x00;

// bmRequestType recipient field
const USB_RECIP_DEVICE: u8 = 0x00;
const USB_RECIP_OTHER: u8 = 0x03; // "other" = port for hub class

// Standard USB request codes (bRequest)
const USB_REQ_GET_STATUS: u8 = 0x00;
const USB_REQ_CLEAR_FEATURE: u8 = 0x01;
const USB_REQ_SET_FEATURE: u8 = 0x03;
const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;

// Hub class descriptor type codes (wValue high byte in GET_DESCRIPTOR)
/// Hub descriptor type (USB 2.0).
const USB_DT_HUB: u8 = 0x29;
/// SuperSpeed hub descriptor type (USB 3.0).
const USB_DT_SS_HUB: u8 = 0x2A;

// ============================================================================
// Hub Feature Selectors
// ============================================================================

/// C_HUB_LOCAL_POWER — acknowledge hub power change.
const C_HUB_LOCAL_POWER: u16 = 0;
/// C_HUB_OVER_CURRENT — acknowledge hub over-current change.
const C_HUB_OVER_CURRENT: u16 = 1;

// ============================================================================
// Port Feature Selectors (USB 2.0 spec Table 11-17)
// ============================================================================

const PORT_ENABLE: u16 = 1;
const PORT_SUSPEND: u16 = 2;
const PORT_RESET: u16 = 4;
const PORT_POWER: u16 = 8;

// Port change feature selectors (acknowledge change bits)
const C_PORT_CONNECTION: u16 = 16;
const C_PORT_ENABLE: u16 = 17;
const C_PORT_SUSPEND: u16 = 18;
const C_PORT_OVER_CURRENT: u16 = 19;
const C_PORT_RESET: u16 = 20;

// USB 3.0 additional port feature selectors
const PORT_LINK_STATE: u16 = 5;
const PORT_U1_TIMEOUT: u16 = 23;
const PORT_U2_TIMEOUT: u16 = 24;
const C_PORT_LINK_STATE: u16 = 25;
const C_PORT_CONFIG_ERR: u16 = 26;
const BH_PORT_RESET: u16 = 28;
const C_BH_PORT_RESET: u16 = 29;

// ============================================================================
// USB Standard Port Status Word (wPortStatus + wPortChange)
// ============================================================================
//
// These are the STANDARD USB format bits, NOT xHCI PORTSC.
// The root hub must translate xHCI PORTSC → USB standard format.

/// Port status/change response as returned by GET_STATUS (Port).
#[derive(Debug, Clone, Copy)]
pub struct UsbPortStatus {
    /// wPortStatus — current port state (USB 2.0 spec Table 11-21).
    pub status: u16,
    /// wPortChange — change bits since last acknowledgment (Table 11-22).
    pub change: u16,
}

impl UsbPortStatus {
    /// Serialize to 4 bytes (little-endian) for USB control transfer response.
    pub fn to_le_bytes(&self) -> [u8; 4] {
        let s = self.status.to_le_bytes();
        let c = self.change.to_le_bytes();
        [s[0], s[1], c[0], c[1]]
    }
}

// ── USB 2.0 wPortStatus bits (Table 11-21) ──────────────────────────

const USB_PORT_STAT_CONNECTION: u16  = 1 << 0;
const USB_PORT_STAT_ENABLE: u16     = 1 << 1;
const USB_PORT_STAT_SUSPEND: u16    = 1 << 2;
const USB_PORT_STAT_OVERCURRENT: u16 = 1 << 3;
const USB_PORT_STAT_RESET: u16      = 1 << 4;
const USB_PORT_STAT_POWER: u16      = 1 << 8;  // USB 2.0 only
const USB_PORT_STAT_LOW_SPEED: u16  = 1 << 9;
const USB_PORT_STAT_HIGH_SPEED: u16 = 1 << 10;

// ── USB 3.0 wPortStatus bits (USB 3.2 spec Table 10-10) ────────────

/// Link state field — bits [8:5].
const USB_SS_PORT_STAT_LINK_STATE: u16 = 0x01E0;
/// Port power — bit 9.
const USB_SS_PORT_STAT_POWER: u16 = 1 << 9;
/// Speed field — bits [12:10].
const USB_SS_PORT_STAT_SPEED: u16 = 0x1C00;

// ── USB 2.0 wPortChange bits (Table 11-22) ──────────────────────────

const USB_PORT_STAT_C_CONNECTION: u16  = 1 << 0;
const USB_PORT_STAT_C_ENABLE: u16     = 1 << 1;
const USB_PORT_STAT_C_SUSPEND: u16    = 1 << 2;
const USB_PORT_STAT_C_OVERCURRENT: u16 = 1 << 3;
const USB_PORT_STAT_C_RESET: u16      = 1 << 4;

// ── USB 3.0 additional wPortChange bits (Table 10-11) ───────────────

/// BH (warm) port reset change — bit 5.
const USB_PORT_STAT_C_BH_RESET: u16   = 1 << 5;
/// Port link state change — bit 6.
const USB_PORT_STAT_C_LINK_STATE: u16  = 1 << 6;
/// Port config error change — bit 7.
const USB_PORT_STAT_C_CONFIG_ERR: u16  = 1 << 7;

// ============================================================================
// PORTSC → USB Port Status Conversion
// ============================================================================

/// Convert xHCI PORTSC register value to USB standard port status.
///
/// The xHCI controller uses its own register format (PORTSC bits),
/// but the USB hub class interface requires standard USB format.
/// This translation is what Linux xhci-hub.c:xhci_get_port_status() does.
///
/// Key differences between USB 2.0 and USB 3.0 status words:
///   USB 2.0: bit 8 = POWER, bits 9-10 = speed indicators
///   USB 3.0: bits [8:5] = link state, bit 9 = POWER, bits [12:10] = speed
pub fn portsc_to_usb_status(portsc: u32, protocol: UsbProtocol) -> UsbPortStatus {
    let mut status: u16 = 0;
    let mut change: u16 = 0;

    // --- wPortStatus (common bits) ---

    if portsc & PORTSC_CCS != 0 {
        status |= USB_PORT_STAT_CONNECTION;
    }
    if portsc & PORTSC_PED != 0 {
        status |= USB_PORT_STAT_ENABLE;
    }
    if portsc & PORTSC_OCA != 0 {
        status |= USB_PORT_STAT_OVERCURRENT;
    }
    if portsc & PORTSC_PR != 0 {
        status |= USB_PORT_STAT_RESET;
    }

    let pls = portsc_pls(portsc);
    let speed = portsc_speed(portsc);

    // --- Protocol-specific status bits ---

    if protocol == UsbProtocol::Usb3 {
        // USB 3.0: Power at bit 9, link state at bits [8:5], speed at [12:10]
        if portsc & PORTSC_PP != 0 {
            status |= USB_SS_PORT_STAT_POWER;
        }
        status |= ((pls as u16) << 5) & USB_SS_PORT_STAT_LINK_STATE;
        let usb_speed: u16 = match speed {
            SPEED_SUPER => 1,      // SuperSpeed
            SPEED_SUPER_PLUS => 2, // SuperSpeed+
            _ => 0,
        };
        status |= (usb_speed << 10) & USB_SS_PORT_STAT_SPEED;
    } else {
        // USB 2.0: Power at bit 8, suspend from PLS, speed indicators
        if portsc & PORTSC_PP != 0 {
            status |= USB_PORT_STAT_POWER;
        }
        if pls == PLS_U3 {
            status |= USB_PORT_STAT_SUSPEND;
        }
        match speed {
            SPEED_LOW => status |= USB_PORT_STAT_LOW_SPEED,
            SPEED_HIGH => status |= USB_PORT_STAT_HIGH_SPEED,
            _ => {} // Full speed has no indicator bit
        }
    }

    // --- wPortChange (common bits) ---

    if portsc & PORTSC_CSC != 0 {
        change |= USB_PORT_STAT_C_CONNECTION;
    }
    if portsc & PORTSC_PEC != 0 {
        change |= USB_PORT_STAT_C_ENABLE;
    }
    if portsc & PORTSC_OCC != 0 {
        change |= USB_PORT_STAT_C_OVERCURRENT;
    }
    if portsc & PORTSC_PRC != 0 {
        change |= USB_PORT_STAT_C_RESET;
    }

    // --- Protocol-specific change bits ---

    if protocol == UsbProtocol::Usb3 {
        if portsc & PORTSC_WRC != 0 {
            change |= USB_PORT_STAT_C_BH_RESET;
        }
        if portsc & PORTSC_PLC != 0 {
            change |= USB_PORT_STAT_C_LINK_STATE;
        }
        if portsc & PORTSC_CEC != 0 {
            change |= USB_PORT_STAT_C_CONFIG_ERR;
        }
    } else {
        // USB 2.0: PLC maps to suspend change
        if portsc & PORTSC_PLC != 0 {
            change |= USB_PORT_STAT_C_SUSPEND;
        }
    }

    UsbPortStatus { status, change }
}

// ============================================================================
// Hub Descriptor (USB 2.0 spec §11.23.2.1)
// ============================================================================

/// Maximum descriptor size: 7 fixed bytes + ceil(MaxPorts+1 / 8) * 2.
/// For 15 ports (Dell Inspiron 5558 typical max): 7 + 2*2 = 11 bytes.
const HUB_DESC_MAX_SIZE: usize = 71; // 7 + 2*ceil(256/8) theoretical max

/// USB 2.0 Hub Descriptor.
///
/// Variable-length descriptor that describes a hub's characteristics.
/// The DeviceRemovable and PortPwrCtrlMask fields are bitmasks whose
/// size depends on the number of ports.
///
/// Layout:
///   [0]   bDescLength
///   [1]   bDescriptorType (0x29)
///   [2]   bNbrPorts
///   [3:4] wHubCharacteristics (LE)
///   [5]   bPwrOn2PwrGood (× 2ms)
///   [6]   bHubContrCurrent (mA)
///   [7+]  DeviceRemovable (variable)
///   [7+N] PortPwrCtrlMask (variable, all 0xFF for compat)
#[derive(Debug)]
pub struct HubDescriptor {
    /// Raw descriptor bytes, ready for USB transfer.
    pub data: [u8; HUB_DESC_MAX_SIZE],
    /// Actual length of the descriptor.
    pub length: u8,
}

impl HubDescriptor {
    /// Build a hub descriptor for the root hub.
    ///
    /// - `num_ports`: total number of ports
    /// - `per_port_power`: true if controller supports per-port power switching
    /// - `removable_mask`: bitmask bytes (bit N = port N non-removable)
    pub fn build(num_ports: u8, per_port_power: bool, removable_mask: &[u8]) -> Self {
        let mut data = [0u8; HUB_DESC_MAX_SIZE];

        // Variable field sizes: ceil((num_ports + 1) / 8) bytes each
        let var_bytes = ((num_ports as usize + 1) + 7) / 8;
        let desc_length = 7 + var_bytes * 2;

        // bDescLength
        data[0] = desc_length as u8;
        // bDescriptorType
        data[1] = USB_DT_HUB;
        // bNbrPorts
        data[2] = num_ports;

        // wHubCharacteristics (little-endian)
        //   Bits [1:0]: 00=ganged power, 01=per-port power
        //   Bit  [2]:   0 = not compound device
        //   Bits [4:3]: 00=global OC, 01=per-port OC
        //   Bits [6:5]: TT think time (0 for root hub)
        //   Bit  [7]:   0 = no port indicators
        let mut characteristics: u16 = 0;
        if per_port_power {
            characteristics |= 0x01; // Individual port power switching
        }
        characteristics |= 0x08; // Individual port over-current protection
        data[3] = (characteristics & 0xFF) as u8;
        data[4] = ((characteristics >> 8) & 0xFF) as u8;

        // bPwrOn2PwrGood: 10 × 2ms = 20ms (typical root hub value)
        data[5] = 10;
        // bHubContrCurrent: 0mA (root hub doesn't consume bus current)
        data[6] = 0;

        // DeviceRemovable: bit N = 1 means port N is non-removable.
        // Bit 0 is reserved (always 0). Port 1 = bit 1, etc.
        for (i, &byte) in removable_mask.iter().enumerate() {
            if 7 + i < desc_length {
                data[7 + i] = byte;
            }
        }

        // PortPwrCtrlMask: all 0xFF for backward compatibility (USB 2.0 §11.23.2.1)
        let mask_offset = 7 + var_bytes;
        for i in 0..var_bytes {
            if mask_offset + i < desc_length {
                data[mask_offset + i] = 0xFF;
            }
        }

        HubDescriptor {
            data,
            length: desc_length as u8,
        }
    }

    /// Get the descriptor bytes as a slice.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data[..self.length as usize]
    }
}

// ============================================================================
// SuperSpeed Hub Descriptor (USB 3.0 spec §10.13.2.1)
// ============================================================================

/// Fixed-size SuperSpeed hub descriptor (12 bytes).
const SS_HUB_DESC_SIZE: usize = 12;

/// USB 3.0 SuperSpeed Hub Descriptor.
///
/// Unlike the USB 2.0 hub descriptor, this has a fixed size of 12 bytes.
///
/// Layout:
///   [0]    bDescLength (12)
///   [1]    bDescriptorType (0x2A)
///   [2]    bNbrPorts
///   [3:4]  wHubCharacteristics (LE)
///   [5]    bPwrOn2PwrGood (× 2ms)
///   [6]    bHubContrCurrent (mA)
///   [7]    bHubHdrDecLat (hub header decode latency)
///   [8:9]  wHubDelay (ns)
///   [10:11] DeviceRemovable (16-bit bitmask)
#[derive(Debug)]
pub struct SuperSpeedHubDescriptor {
    /// Raw descriptor bytes, ready for USB transfer.
    pub data: [u8; SS_HUB_DESC_SIZE],
}

impl SuperSpeedHubDescriptor {
    /// Build a SuperSpeed hub descriptor for the root hub.
    ///
    /// - `num_ports`: number of USB 3.0 ports
    /// - `per_port_power`: true if controller supports per-port power switching
    /// - `removable_mask`: 16-bit bitmask of non-removable ports
    pub fn build(num_ports: u8, per_port_power: bool, removable_mask: u16) -> Self {
        let mut data = [0u8; SS_HUB_DESC_SIZE];

        // bDescLength
        data[0] = SS_HUB_DESC_SIZE as u8;
        // bDescriptorType
        data[1] = USB_DT_SS_HUB;
        // bNbrPorts
        data[2] = num_ports;

        // wHubCharacteristics (little-endian)
        let mut characteristics: u16 = 0;
        if per_port_power {
            characteristics |= 0x01; // Individual port power switching
        }
        characteristics |= 0x08; // Individual port over-current protection
        data[3] = (characteristics & 0xFF) as u8;
        data[4] = ((characteristics >> 8) & 0xFF) as u8;

        // bPwrOn2PwrGood: 10 × 2ms = 20ms
        data[5] = 10;
        // bHubContrCurrent: 0mA
        data[6] = 0;
        // bHubHdrDecLat: 0 (root hub has no decode latency)
        data[7] = 0;
        // wHubDelay: 0ns (root hub has no hub delay)
        data[8] = 0;
        data[9] = 0;

        // DeviceRemovable: 16-bit bitmask (little-endian)
        data[10] = (removable_mask & 0xFF) as u8;
        data[11] = ((removable_mask >> 8) & 0xFF) as u8;

        SuperSpeedHubDescriptor { data }
    }

    /// Get the descriptor bytes as a slice.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

// ============================================================================
// Hub Request Result
// ============================================================================

/// Result of handling a hub class request.
#[derive(Debug)]
pub enum HubRequestResult {
    /// Request completed successfully with data to return.
    Data(Vec<u8>),
    /// Request completed successfully with no data (status-only).
    Ok,
    /// Request was not recognized or is not applicable (STALL endpoint).
    Stall,
}

// ============================================================================
// Root Hub
// ============================================================================

/// Root Hub — manages all root hub ports on an xHCI controller.
///
/// Tracks USB 2.0/3.0 protocol routing, port states, and
/// provides methods for scanning, resetting, and handling events.
pub struct RootHub {
    /// All port status snapshots (indexed by 0-based port number).
    pub ports: Vec<PortInfo>,
    /// Supported Protocol capabilities from xECP.
    pub protocols: Vec<SupportedProtocol>,
    /// Number of USB 2.0 ports.
    pub usb2_port_count: u8,
    /// Number of USB 3.0 ports.
    pub usb3_port_count: u8,
    /// Total ports from capabilities.
    pub max_ports: u8,
}

impl RootHub {
    /// Initialize the root hub: detect protocols and scan all ports.
    pub fn init(
        mmio: &MmioRegion,
        caps: &XhciCapabilities,
        op_base: usize,
    ) -> Self {
        serial_println!("[HUB] Initializing root hub ({} ports)", caps.max_ports);

        // Read Supported Protocol extended capabilities
        let protocols = read_supported_protocols(mmio, caps.xecp);

        let mut usb2_count: u8 = 0;
        let mut usb3_count: u8 = 0;

        for proto in &protocols {
            serial_println!(
                "[HUB] Protocol: USB {}.{} ports {}-{} ({} ports, slot_type={})",
                proto.major, proto.minor,
                proto.port_offset,
                proto.port_offset as u16 + proto.port_count as u16 - 1,
                proto.port_count,
                proto.slot_type,
            );
            if proto.is_usb2() {
                usb2_count += proto.port_count;
            } else if proto.is_usb3() {
                usb3_count += proto.port_count;
            }
        }

        serial_println!("[HUB] USB 2.0 ports: {}, USB 3.0 ports: {}", usb2_count, usb3_count);

        // Scan all ports
        let mut ports = Vec::with_capacity(caps.max_ports as usize);
        for i in 0..caps.max_ports {
            let protocol = Self::protocol_for_port_static(&protocols, i);
            if let Some(info) = read_port_status(mmio, op_base, i, protocol) {
                ports.push(info);
            }
        }

        let hub = Self {
            ports,
            protocols,
            usb2_port_count: usb2_count,
            usb3_port_count: usb3_count,
            max_ports: caps.max_ports,
        };

        hub.log_status();
        hub
    }

    /// Determine which USB protocol a port belongs to.
    fn protocol_for_port_static(protocols: &[SupportedProtocol], port_index: u8) -> UsbProtocol {
        // Port numbers in xECP are 1-based
        let port_num = port_index + 1;
        for proto in protocols {
            let start = proto.port_offset;
            let end = start + proto.port_count;
            if port_num >= start && port_num < end {
                if proto.is_usb2() {
                    return UsbProtocol::Usb2;
                } else if proto.is_usb3() {
                    return UsbProtocol::Usb3;
                }
            }
        }
        UsbProtocol::Unknown
    }

    /// Determine which USB protocol a port belongs to.
    pub fn protocol_for_port(&self, port_index: u8) -> UsbProtocol {
        Self::protocol_for_port_static(&self.protocols, port_index)
    }

    /// Re-scan all ports and update cached state.
    pub fn refresh_all(&mut self, mmio: &MmioRegion, op_base: usize) {
        self.ports.clear();
        for i in 0..self.max_ports {
            let protocol = self.protocol_for_port(i);
            if let Some(info) = read_port_status(mmio, op_base, i, protocol) {
                self.ports.push(info);
            }
        }
    }

    /// Refresh a single port's cached state.
    pub fn refresh_port(&mut self, mmio: &MmioRegion, op_base: usize, port_index: u8) {
        let protocol = self.protocol_for_port(port_index);
        if let Some(info) = read_port_status(mmio, op_base, port_index, protocol) {
            if (port_index as usize) < self.ports.len() {
                self.ports[port_index as usize] = info;
            }
        }
    }

    /// Get ports that have a device connected.
    pub fn connected_ports(&self) -> Vec<&PortInfo> {
        self.ports.iter().filter(|p| p.connected).collect()
    }

    /// Get ports that are enabled (link active).
    pub fn enabled_ports(&self) -> Vec<&PortInfo> {
        self.ports.iter().filter(|p| p.enabled).collect()
    }

    /// Get ports with pending change events.
    pub fn ports_with_changes(&self) -> Vec<&PortInfo> {
        self.ports.iter().filter(|p| p.changes.any()).collect()
    }

    /// Handle a Port Status Change Event from the Event Ring.
    ///
    /// The event TRB parameter field contains the port ID (1-based) in bits [31:24].
    /// We read the port status, process the change, and clear the change bits.
    ///
    /// Returns the updated PortInfo, or None if the port ID is invalid.
    pub fn handle_port_status_change(
        &mut self,
        mmio: &MmioRegion,
        op_base: usize,
        event_trb: &Trb,
    ) -> Option<PortInfo> {
        // Port ID is in bits [31:24] of the parameter field (low 32 bits)
        let port_id = ((event_trb.parameter & 0xFF00_0000) >> 24) as u8;

        if port_id == 0 || port_id > self.max_ports {
            serial_println!("[HUB] WARNING: Invalid port ID {} in status change event", port_id);
            return None;
        }

        let port_index = port_id - 1; // Convert to 0-based
        let protocol = self.protocol_for_port(port_index);

        // Read current port status
        let info = read_port_status(mmio, op_base, port_index, protocol)?;

        serial_println!("[HUB] Port Status Change: port {}", port_index);
        log_port_status(&info);

        // Process specific changes
        if info.changes.connect_change {
            if info.connected {
                serial_println!("[HUB] Device CONNECTED on port {} ({})",
                    port_index, protocol.as_str());
            } else {
                serial_println!("[HUB] Device DISCONNECTED from port {}", port_index);
            }
            clear_change_bit(mmio, op_base, port_index, PORTSC_CSC);
        }

        if info.changes.reset_change {
            if info.enabled {
                serial_println!("[HUB] Port {} reset complete — {} at {}",
                    port_index, info.state.as_str(), info.speed.as_str());
            }
            clear_change_bit(mmio, op_base, port_index, PORTSC_PRC);
        }

        if info.changes.enable_change {
            serial_println!("[HUB] Port {} enable change — enabled={}",
                port_index, info.enabled);
            clear_change_bit(mmio, op_base, port_index, PORTSC_PEC);
        }

        if info.changes.overcurrent_change {
            serial_println!("[HUB] WARNING: Port {} over-current change — OCA={}",
                port_index, info.overcurrent);
            clear_change_bit(mmio, op_base, port_index, PORTSC_OCC);
        }

        if info.changes.warm_reset_change {
            serial_println!("[HUB] Port {} warm reset complete", port_index);
            clear_change_bit(mmio, op_base, port_index, PORTSC_WRC);
        }

        if info.changes.link_state_change {
            clear_change_bit(mmio, op_base, port_index, PORTSC_PLC);
        }

        if info.changes.config_error_change {
            serial_println!("[HUB] WARNING: Port {} config error", port_index);
            clear_change_bit(mmio, op_base, port_index, PORTSC_CEC);
        }

        // Update cached state
        self.refresh_port(mmio, op_base, port_index);
        let updated = self.ports.get(port_index as usize)?.clone();
        Some(updated)
    }

    /// Reset a port and wait for completion.
    ///
    /// For USB 2.0 ports, uses standard reset (PR=1).
    /// For USB 3.0 ports in SS.Inactive/Compliance, uses warm reset (WPR=1).
    ///
    /// Returns the port info after reset, with negotiated speed.
    pub fn reset_port(
        &mut self,
        mmio: &MmioRegion,
        op_base: usize,
        port_index: u8,
    ) -> Option<PortInfo> {
        let protocol = self.protocol_for_port(port_index);
        let info = read_port_status(mmio, op_base, port_index, protocol)?;

        if !info.connected {
            serial_println!("[HUB] Cannot reset port {} — no device connected", port_index);
            return None;
        }

        // Determine reset type
        let use_warm_reset = protocol == UsbProtocol::Usb3
            && (info.link_state == PLS_INACTIVE || info.link_state == PLS_COMPLIANCE);

        if use_warm_reset {
            warm_port_reset(mmio, op_base, port_index);
        } else {
            port_reset(mmio, op_base, port_index);
        }

        // Wait for reset to complete
        let result = wait_port_reset_complete(mmio, op_base, port_index, protocol);

        if let Some(ref updated) = result {
            serial_println!(
                "[HUB] Port {} reset result: {} speed={}",
                port_index, updated.state.as_str(), updated.speed.as_str()
            );
            self.refresh_port(mmio, op_base, port_index);
        }

        result
    }

    /// Reset all connected ports and report results.
    ///
    /// Returns a Vec of (port_index, speed) for successfully reset ports.
    pub fn reset_connected_ports(
        &mut self,
        mmio: &MmioRegion,
        op_base: usize,
    ) -> Vec<(u8, UsbSpeed)> {
        let mut results = Vec::new();

        // Collect connected port indices first (avoid borrow issues)
        let connected: Vec<u8> = self.ports.iter()
            .filter(|p| p.connected && !p.enabled)
            .map(|p| p.index)
            .collect();

        for port_index in connected {
            if let Some(info) = self.reset_port(mmio, op_base, port_index) {
                if info.enabled {
                    results.push((port_index, info.speed));
                }
            }
        }

        results
    }

    /// Process any pending Port Status Change Events from the event ring.
    ///
    /// Dequeues events of type TRB_TYPE_PORT_STATUS_CHANGE and
    /// handles each one. Returns the number of events processed.
    pub fn process_pending_events(
        &mut self,
        mmio: &MmioRegion,
        op_base: usize,
        event_ring: &mut TrbRing,
    ) -> u32 {
        let mut count = 0;

        while event_ring.event_ready() {
            if let Some(trb) = event_ring.dequeue_event() {
                if trb.trb_type() == TRB_TYPE_PORT_STATUS_CHANGE {
                    self.handle_port_status_change(mmio, op_base, &trb);
                    count += 1;
                }
                // Other event types are ignored here (handled elsewhere)
            }
        }

        count
    }

    /// Log status of all ports.
    pub fn log_status(&self) {
        serial_println!("[HUB] Root Hub: {} ports ({} USB2, {} USB3)",
            self.max_ports, self.usb2_port_count, self.usb3_port_count);

        let connected = self.ports.iter().filter(|p| p.connected).count();
        let enabled = self.ports.iter().filter(|p| p.enabled).count();
        serial_println!("[HUB] Connected: {}, Enabled: {}", connected, enabled);

        for info in &self.ports {
            // Only log ports that are powered and interesting
            if info.powered && (info.connected || info.state != PortState::Disconnected) {
                log_port_status(info);
            }
        }
    }

    /// Log a compact one-line summary for each connected device.
    pub fn log_connected_devices(&self) {
        let connected: Vec<&PortInfo> = self.connected_ports();
        if connected.is_empty() {
            serial_println!("[HUB] No devices connected");
            return;
        }

        serial_println!("[HUB] Connected devices:");
        for info in &connected {
            serial_println!(
                "[HUB]   Port {:2}: {} {} (state={}, PLS={})",
                info.index,
                info.protocol.as_str(),
                info.speed.as_str(),
                info.state.as_str(),
                info.link_state,
            );
        }
    }

    // ========================================================================
    // USB Hub Device Interface — Standard Hub Request Handler
    // ========================================================================

    /// Build a DeviceRemovable bitmask from PORTSC.DR bits.
    ///
    /// Bit 0 is reserved. Bit N corresponds to port N (1-based).
    /// A set bit means the port is NON-removable.
    fn build_removable_mask(&self, mmio: &MmioRegion, op_base: usize) -> Vec<u8> {
        let num_bytes = ((self.max_ports as usize + 1) + 7) / 8;
        let mut mask = alloc::vec![0u8; num_bytes];

        for i in 0..self.max_ports {
            let offset = op_base + portsc_offset(i);
            if let Some(portsc) = mmio.read32(offset) {
                // PORTSC.DR (bit 30): 1 = Device is non-removable
                if portsc & PORTSC_DR != 0 {
                    let bit_index = (i + 1) as usize; // port 1 = bit 1
                    mask[bit_index / 8] |= 1 << (bit_index % 8);
                }
            }
        }

        mask
    }

    /// Build a 16-bit removable mask for SuperSpeed hub descriptor.
    fn build_ss_removable_mask(&self, mmio: &MmioRegion, op_base: usize) -> u16 {
        let mut mask: u16 = 0;

        for i in 0..self.max_ports {
            if self.protocol_for_port(i) != UsbProtocol::Usb3 {
                continue;
            }
            let offset = op_base + portsc_offset(i);
            if let Some(portsc) = mmio.read32(offset) {
                if portsc & PORTSC_DR != 0 {
                    let bit_index = (i + 1) as u16;
                    if bit_index < 16 {
                        mask |= 1 << bit_index;
                    }
                }
            }
        }

        mask
    }

    /// Handle a standard USB hub class request.
    ///
    /// This emulates the root hub as a standard USB hub device,
    /// translating USB hub class requests into xHCI register operations.
    ///
    /// Called by the USB subsystem when it sends a control transfer
    /// to device address 0, endpoint 0 (the root hub).
    ///
    /// Supported requests:
    ///   GET_DESCRIPTOR  Hub (0x29) / SuperSpeed Hub (0x2A)
    ///   GET_STATUS      Hub / Port
    ///   SET_FEATURE     Port (Power, Reset, Suspend, BH Reset, Link State)
    ///   CLEAR_FEATURE   Port (Enable, Suspend, Power, C_PORT_*)
    pub fn handle_hub_request(
        &mut self,
        mmio: &MmioRegion,
        op_base: usize,
        setup: &UsbSetupPacket,
    ) -> HubRequestResult {
        let recipient = setup.bm_request_type & 0x1F;
        let direction = setup.bm_request_type & 0x80;

        match (setup.b_request, recipient) {
            // ── GET_DESCRIPTOR (Hub or Device recipient) ────────────
            (USB_REQ_GET_DESCRIPTOR, USB_RECIP_DEVICE) if direction == USB_DIR_IN => {
                let desc_type = (setup.w_value >> 8) as u8;
                match desc_type {
                    USB_DT_HUB => {
                        let removable = self.build_removable_mask(mmio, op_base);
                        let desc = HubDescriptor::build(
                            self.max_ports,
                            true, // Assume PPC=1 for root hubs
                            &removable,
                        );
                        let len = core::cmp::min(
                            desc.length as usize,
                            setup.w_length as usize,
                        );
                        serial_println!("[HUB] GET_DESCRIPTOR Hub: {} bytes", len);
                        HubRequestResult::Data(desc.as_bytes()[..len].to_vec())
                    }
                    USB_DT_SS_HUB => {
                        let removable = self.build_ss_removable_mask(mmio, op_base);
                        let desc = SuperSpeedHubDescriptor::build(
                            self.usb3_port_count,
                            true,
                            removable,
                        );
                        let len = core::cmp::min(
                            SS_HUB_DESC_SIZE,
                            setup.w_length as usize,
                        );
                        serial_println!("[HUB] GET_DESCRIPTOR SS Hub: {} bytes", len);
                        HubRequestResult::Data(desc.as_bytes()[..len].to_vec())
                    }
                    _ => {
                        serial_println!(
                            "[HUB] GET_DESCRIPTOR: unsupported type 0x{:02X}",
                            desc_type,
                        );
                        HubRequestResult::Stall
                    }
                }
            }

            // ── GET_STATUS (Hub) ───────────────────────────────────
            (USB_REQ_GET_STATUS, USB_RECIP_DEVICE) if direction == USB_DIR_IN => {
                // Hub status word (USB 2.0 spec §11.24.2.6):
                //   Bit 0: Local Power Source (1 = good)
                //   Bit 1: Over-current (1 = active)
                let mut hub_status: u16 = 0x0001; // Local power good (self-powered)
                let any_oc = self.ports.iter().any(|p| p.overcurrent);
                if any_oc {
                    hub_status |= 0x0002;
                }

                // Hub change word: no pending hub-level changes for root hub
                let hub_change: u16 = 0;

                let mut data = Vec::with_capacity(4);
                data.extend_from_slice(&hub_status.to_le_bytes());
                data.extend_from_slice(&hub_change.to_le_bytes());
                serial_println!("[HUB] GET_STATUS Hub: status=0x{:04X}", hub_status);
                HubRequestResult::Data(data)
            }

            // ── GET_STATUS (Port) ──────────────────────────────────
            (USB_REQ_GET_STATUS, USB_RECIP_OTHER) if direction == USB_DIR_IN => {
                let port_num = (setup.w_index & 0xFF) as u8; // 1-based
                if port_num == 0 || port_num > self.max_ports {
                    serial_println!("[HUB] GET_STATUS: invalid port {}", port_num);
                    return HubRequestResult::Stall;
                }

                let port_index = port_num - 1;
                let offset = op_base + portsc_offset(port_index);
                let portsc = match mmio.read32(offset) {
                    Some(v) => v,
                    None => return HubRequestResult::Stall,
                };

                let protocol = self.protocol_for_port(port_index);
                let usb_status = portsc_to_usb_status(portsc, protocol);

                serial_println!(
                    "[HUB] GET_STATUS Port {}: status=0x{:04X} change=0x{:04X}",
                    port_num, usb_status.status, usb_status.change,
                );
                HubRequestResult::Data(usb_status.to_le_bytes().to_vec())
            }

            // ── SET_FEATURE (Port) ─────────────────────────────────
            (USB_REQ_SET_FEATURE, USB_RECIP_OTHER) if direction == USB_DIR_OUT => {
                let port_num = (setup.w_index & 0xFF) as u8;
                if port_num == 0 || port_num > self.max_ports {
                    serial_println!("[HUB] SET_FEATURE: invalid port {}", port_num);
                    return HubRequestResult::Stall;
                }

                let port_index = port_num - 1;
                let feature = setup.w_value;

                match feature {
                    PORT_POWER => {
                        serial_println!("[HUB] SET_FEATURE PORT_POWER port {}", port_num);
                        power_on(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    PORT_RESET => {
                        serial_println!("[HUB] SET_FEATURE PORT_RESET port {}", port_num);
                        port_reset(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    PORT_SUSPEND => {
                        serial_println!("[HUB] SET_FEATURE PORT_SUSPEND port {}", port_num);
                        suspend_port(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    BH_PORT_RESET => {
                        serial_println!("[HUB] SET_FEATURE BH_PORT_RESET port {}", port_num);
                        warm_port_reset(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    PORT_LINK_STATE => {
                        let link_state = ((setup.w_index >> 8) & 0xFF) as u8;
                        serial_println!(
                            "[HUB] SET_FEATURE PORT_LINK_STATE port {} PLS={}",
                            port_num, link_state,
                        );
                        set_link_state(mmio, op_base, port_index, link_state);
                        HubRequestResult::Ok
                    }
                    PORT_U1_TIMEOUT | PORT_U2_TIMEOUT => {
                        // U1/U2 timeout: configured via PORTPMSC, accept but no-op
                        serial_println!(
                            "[HUB] SET_FEATURE U{}_TIMEOUT port {} (no-op)",
                            if feature == PORT_U1_TIMEOUT { 1 } else { 2 },
                            port_num,
                        );
                        HubRequestResult::Ok
                    }
                    _ => {
                        serial_println!(
                            "[HUB] SET_FEATURE: unsupported feature {} port {}",
                            feature, port_num,
                        );
                        HubRequestResult::Stall
                    }
                }
            }

            // ── CLEAR_FEATURE (Port) ───────────────────────────────
            (USB_REQ_CLEAR_FEATURE, USB_RECIP_OTHER) if direction == USB_DIR_OUT => {
                let port_num = (setup.w_index & 0xFF) as u8;
                if port_num == 0 || port_num > self.max_ports {
                    serial_println!("[HUB] CLEAR_FEATURE: invalid port {}", port_num);
                    return HubRequestResult::Stall;
                }

                let port_index = port_num - 1;
                let feature = setup.w_value;
                let protocol = self.protocol_for_port(port_index);

                match feature {
                    PORT_ENABLE => {
                        serial_println!("[HUB] CLEAR_FEATURE PORT_ENABLE port {}", port_num);
                        disable_port(mmio, op_base, port_index, protocol);
                        HubRequestResult::Ok
                    }
                    PORT_SUSPEND => {
                        serial_println!(
                            "[HUB] CLEAR_FEATURE PORT_SUSPEND port {} (resume)",
                            port_num,
                        );
                        resume_port(mmio, op_base, port_index, protocol);
                        HubRequestResult::Ok
                    }
                    PORT_POWER => {
                        serial_println!("[HUB] CLEAR_FEATURE PORT_POWER port {}", port_num);
                        power_off(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    C_PORT_CONNECTION => {
                        clear_change_bit(mmio, op_base, port_index, PORTSC_CSC);
                        self.refresh_port(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    C_PORT_ENABLE => {
                        clear_change_bit(mmio, op_base, port_index, PORTSC_PEC);
                        self.refresh_port(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    C_PORT_SUSPEND => {
                        clear_change_bit(mmio, op_base, port_index, PORTSC_PLC);
                        self.refresh_port(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    C_PORT_OVER_CURRENT => {
                        clear_change_bit(mmio, op_base, port_index, PORTSC_OCC);
                        self.refresh_port(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    C_PORT_RESET => {
                        clear_change_bit(mmio, op_base, port_index, PORTSC_PRC);
                        self.refresh_port(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    C_PORT_LINK_STATE => {
                        clear_change_bit(mmio, op_base, port_index, PORTSC_PLC);
                        self.refresh_port(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    C_PORT_CONFIG_ERR => {
                        clear_change_bit(mmio, op_base, port_index, PORTSC_CEC);
                        self.refresh_port(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    C_BH_PORT_RESET => {
                        clear_change_bit(mmio, op_base, port_index, PORTSC_WRC);
                        self.refresh_port(mmio, op_base, port_index);
                        HubRequestResult::Ok
                    }
                    _ => {
                        serial_println!(
                            "[HUB] CLEAR_FEATURE: unsupported feature {} port {}",
                            feature, port_num,
                        );
                        HubRequestResult::Stall
                    }
                }
            }

            // ── SET_FEATURE (Hub) ──────────────────────────────────
            (USB_REQ_SET_FEATURE, USB_RECIP_DEVICE) if direction == USB_DIR_OUT => {
                serial_println!(
                    "[HUB] SET_FEATURE Hub: feature={} (no-op)",
                    setup.w_value,
                );
                HubRequestResult::Ok
            }

            // ── CLEAR_FEATURE (Hub) ────────────────────────────────
            (USB_REQ_CLEAR_FEATURE, USB_RECIP_DEVICE) if direction == USB_DIR_OUT => {
                match setup.w_value {
                    C_HUB_LOCAL_POWER => {
                        serial_println!("[HUB] CLEAR_FEATURE C_HUB_LOCAL_POWER");
                        HubRequestResult::Ok
                    }
                    C_HUB_OVER_CURRENT => {
                        serial_println!("[HUB] CLEAR_FEATURE C_HUB_OVER_CURRENT");
                        HubRequestResult::Ok
                    }
                    _ => {
                        serial_println!(
                            "[HUB] CLEAR_FEATURE Hub: unsupported feature {}",
                            setup.w_value,
                        );
                        HubRequestResult::Stall
                    }
                }
            }

            // ── Unrecognized request ───────────────────────────────
            _ => {
                serial_println!(
                    "[HUB] Unsupported request: type=0x{:02X} req=0x{:02X} \
                     val=0x{:04X} idx=0x{:04X}",
                    setup.bm_request_type, setup.b_request,
                    setup.w_value, setup.w_index,
                );
                HubRequestResult::Stall
            }
        }
    }

    /// Get USB standard port status for a specific port.
    ///
    /// Convenience wrapper that reads the PORTSC register and translates
    /// to standard USB port status format.
    pub fn get_port_status(
        &self,
        mmio: &MmioRegion,
        op_base: usize,
        port_index: u8,
    ) -> Option<UsbPortStatus> {
        let offset = op_base + portsc_offset(port_index);
        let portsc = mmio.read32(offset)?;
        let protocol = self.protocol_for_port(port_index);
        Some(portsc_to_usb_status(portsc, protocol))
    }
}
