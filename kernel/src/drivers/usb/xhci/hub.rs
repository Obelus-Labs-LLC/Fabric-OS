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
}
