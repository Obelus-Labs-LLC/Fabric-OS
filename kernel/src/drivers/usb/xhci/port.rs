//! xHCI Port State Machine — Phase 21b.
//!
//! Models the USB port lifecycle per xHCI spec section 4.19:
//!   Powered → Disconnected → Disabled → Reset → Enabled → Suspended
//!
//! Each port tracks its state, speed, protocol (USB 2.0 or 3.0),
//! and change events. The state machine drives hardware transitions
//! by writing PORTSC with proper W1C bit preservation.

#![allow(dead_code)]

use crate::serial_println;
use crate::hal::driver_sdk::MmioRegion;

use super::regs::*;

// ============================================================================
// Port State
// ============================================================================

/// Port state as observed from PORTSC register bits.
///
/// Follows the xHCI port state diagram (spec figure 4-25/4-27):
///   PoweredOff → Disconnected → Disabled → Reset → Enabled
///   Enabled → Suspended → Enabled (resume)
///   Any → Error (on hardware fault)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortState {
    /// Port has no power applied (PP=0).
    PoweredOff,
    /// Port is powered but no device connected (PP=1, CCS=0).
    Disconnected,
    /// Device connected but port not enabled (CCS=1, PED=0, not in reset).
    Disabled,
    /// Port is undergoing reset (PR=1).
    Resetting,
    /// Port is enabled and link is active (PED=1, PLS=U0).
    Enabled,
    /// Port link is in a suspend state (PLS=U3).
    Suspended,
    /// Port encountered an error (over-current, compliance mode, etc.).
    Error,
}

impl PortState {
    /// Human-readable state name.
    pub fn as_str(&self) -> &'static str {
        match self {
            PortState::PoweredOff => "PoweredOff",
            PortState::Disconnected => "Disconnected",
            PortState::Disabled => "Disabled",
            PortState::Resetting => "Resetting",
            PortState::Enabled => "Enabled",
            PortState::Suspended => "Suspended",
            PortState::Error => "Error",
        }
    }
}

// ============================================================================
// USB Speed
// ============================================================================

/// Negotiated USB device speed (from PORTSC Speed field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    /// 1.5 Mbps (USB 1.0 Low Speed).
    Low,
    /// 12 Mbps (USB 1.1 Full Speed).
    Full,
    /// 480 Mbps (USB 2.0 High Speed).
    High,
    /// 5 Gbps (USB 3.0 SuperSpeed).
    Super,
    /// 10 Gbps (USB 3.1 SuperSpeed+).
    SuperPlus,
    /// Speed not yet determined or unknown.
    Unknown,
}

impl UsbSpeed {
    /// Decode speed from PORTSC Speed field value.
    pub fn from_portsc(speed_val: u8) -> Self {
        match speed_val {
            SPEED_LOW => UsbSpeed::Low,
            SPEED_FULL => UsbSpeed::Full,
            SPEED_HIGH => UsbSpeed::High,
            SPEED_SUPER => UsbSpeed::Super,
            SPEED_SUPER_PLUS => UsbSpeed::SuperPlus,
            _ => UsbSpeed::Unknown,
        }
    }

    /// Human-readable speed string.
    pub fn as_str(&self) -> &'static str {
        match self {
            UsbSpeed::Low => "Low (1.5 Mbps)",
            UsbSpeed::Full => "Full (12 Mbps)",
            UsbSpeed::High => "High (480 Mbps)",
            UsbSpeed::Super => "Super (5 Gbps)",
            UsbSpeed::SuperPlus => "Super+ (10 Gbps)",
            UsbSpeed::Unknown => "Unknown",
        }
    }

    /// Bandwidth in Mbps.
    pub fn bandwidth_mbps(&self) -> u32 {
        match self {
            UsbSpeed::Low => 1,
            UsbSpeed::Full => 12,
            UsbSpeed::High => 480,
            UsbSpeed::Super => 5000,
            UsbSpeed::SuperPlus => 10000,
            UsbSpeed::Unknown => 0,
        }
    }

    /// Whether this speed requires a USB 3.0 capable port.
    pub fn is_superspeed(&self) -> bool {
        matches!(self, UsbSpeed::Super | UsbSpeed::SuperPlus)
    }
}

// ============================================================================
// USB Protocol
// ============================================================================

/// USB protocol version for the port's routing.
///
/// xHCI controllers multiplex USB 2.0 and USB 3.0 ports.
/// Each physical connector has a USB2 port (for LS/FS/HS devices)
/// and a USB3 port (for SS/SS+ devices). The Extended Capability
/// registers define the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbProtocol {
    /// USB 2.0 port (handles Low/Full/High speed devices).
    Usb2,
    /// USB 3.0 port (handles SuperSpeed/SuperSpeed+ devices).
    Usb3,
    /// Protocol not determined.
    Unknown,
}

impl UsbProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            UsbProtocol::Usb2 => "USB 2.0",
            UsbProtocol::Usb3 => "USB 3.0",
            UsbProtocol::Unknown => "Unknown",
        }
    }
}

// ============================================================================
// Port Status Change Flags
// ============================================================================

/// Bitmask of pending change events on a port.
///
/// Each flag corresponds to a W1C (write-1-to-clear) bit in PORTSC.
/// Reading these tells us what changed; writing 1 acknowledges/clears them.
#[derive(Debug, Clone, Copy, Default)]
pub struct PortChanges {
    /// Connect Status Change — device was plugged/unplugged.
    pub connect_change: bool,
    /// Port Enabled/Disabled Change.
    pub enable_change: bool,
    /// Over-current Change.
    pub overcurrent_change: bool,
    /// Port Reset Change — reset completed.
    pub reset_change: bool,
    /// Port Link State Change.
    pub link_state_change: bool,
    /// Warm Port Reset Change (USB3 only).
    pub warm_reset_change: bool,
    /// Port Config Error Change.
    pub config_error_change: bool,
}

impl PortChanges {
    /// Decode change flags from a PORTSC register value.
    pub fn from_portsc(val: u32) -> Self {
        Self {
            connect_change: val & PORTSC_CSC != 0,
            enable_change: val & PORTSC_PEC != 0,
            overcurrent_change: val & PORTSC_OCC != 0,
            reset_change: val & PORTSC_PRC != 0,
            link_state_change: val & PORTSC_PLC != 0,
            warm_reset_change: val & PORTSC_WRC != 0,
            config_error_change: val & PORTSC_CEC != 0,
        }
    }

    /// Whether any change flag is set.
    pub fn any(&self) -> bool {
        self.connect_change
            || self.enable_change
            || self.overcurrent_change
            || self.reset_change
            || self.link_state_change
            || self.warm_reset_change
            || self.config_error_change
    }
}

// ============================================================================
// Port Info — snapshot of a single port's state
// ============================================================================

/// Complete status snapshot for one root hub port.
#[derive(Debug, Clone)]
pub struct PortInfo {
    /// 0-indexed port number.
    pub index: u8,
    /// Current state machine position.
    pub state: PortState,
    /// Negotiated speed (valid when Enabled).
    pub speed: UsbSpeed,
    /// USB 2.0 or 3.0 protocol routing.
    pub protocol: UsbProtocol,
    /// Device physically connected (CCS=1).
    pub connected: bool,
    /// Port enabled and link active (PED=1).
    pub enabled: bool,
    /// Port powered (PP=1).
    pub powered: bool,
    /// Port Link State value.
    pub link_state: u8,
    /// Raw PORTSC register value.
    pub portsc_raw: u32,
    /// Pending change flags.
    pub changes: PortChanges,
    /// Over-current condition active.
    pub overcurrent: bool,
}

impl PortInfo {
    /// Determine port state from PORTSC bits.
    fn derive_state(portsc: u32) -> PortState {
        let powered = portsc & PORTSC_PP != 0;
        let connected = portsc & PORTSC_CCS != 0;
        let enabled = portsc & PORTSC_PED != 0;
        let resetting = portsc & PORTSC_PR != 0;
        let overcurrent = portsc & PORTSC_OCA != 0;
        let pls = portsc_pls(portsc);

        if overcurrent {
            return PortState::Error;
        }
        if !powered {
            return PortState::PoweredOff;
        }
        if resetting {
            return PortState::Resetting;
        }
        if !connected {
            return PortState::Disconnected;
        }
        if enabled {
            if pls == PLS_U3 {
                return PortState::Suspended;
            }
            return PortState::Enabled;
        }
        PortState::Disabled
    }
}

// ============================================================================
// Port Operations — hardware register access
// ============================================================================

/// Read port status and return a PortInfo snapshot.
///
/// `port_index` is 0-based. Reads the PORTSC register at
/// op_base + 0x400 + port_index * 0x10.
pub fn read_port_status(
    mmio: &MmioRegion,
    op_base: usize,
    port_index: u8,
    protocol: UsbProtocol,
) -> Option<PortInfo> {
    let offset = op_base + portsc_offset(port_index);
    let portsc = mmio.read32(offset)?;

    let connected = portsc & PORTSC_CCS != 0;
    let enabled = portsc & PORTSC_PED != 0;
    let powered = portsc & PORTSC_PP != 0;
    let overcurrent = portsc & PORTSC_OCA != 0;
    let speed_val = portsc_speed(portsc);
    let link_state = portsc_pls(portsc);

    Some(PortInfo {
        index: port_index,
        state: PortInfo::derive_state(portsc),
        speed: if enabled { UsbSpeed::from_portsc(speed_val) } else { UsbSpeed::Unknown },
        protocol,
        connected,
        enabled,
        powered,
        link_state,
        portsc_raw: portsc,
        changes: PortChanges::from_portsc(portsc),
        overcurrent,
    })
}

/// Write to PORTSC preserving W1C change bits (don't accidentally clear them).
///
/// When modifying PORTSC, we must mask out all W1C bits to avoid
/// inadvertently clearing pending status changes. Only set the W1C bits
/// we explicitly want to clear.
fn write_portsc_preserve(
    mmio: &MmioRegion,
    op_base: usize,
    port_index: u8,
    set_bits: u32,
    clear_bits: u32,
) {
    let offset = op_base + portsc_offset(port_index);
    if let Some(current) = mmio.read32(offset) {
        // Mask out W1C bits to preserve them, then apply set/clear
        let preserved = current & !PORTSC_CHANGE_BITS & !clear_bits;
        mmio.write32(offset, preserved | set_bits);
    }
}

/// Clear all pending change bits on a port (acknowledge events).
pub fn clear_change_bits(mmio: &MmioRegion, op_base: usize, port_index: u8) {
    let offset = op_base + portsc_offset(port_index);
    if let Some(current) = mmio.read32(offset) {
        // Write 1 to all change bits to clear them.
        // Preserve non-W1C bits exactly as they are.
        let preserved = current & !PORTSC_CHANGE_BITS;
        mmio.write32(offset, preserved | PORTSC_CHANGE_BITS);
    }
}

/// Clear a specific change bit.
pub fn clear_change_bit(mmio: &MmioRegion, op_base: usize, port_index: u8, bit: u32) {
    let offset = op_base + portsc_offset(port_index);
    if let Some(current) = mmio.read32(offset) {
        let preserved = current & !PORTSC_CHANGE_BITS;
        mmio.write32(offset, preserved | bit);
    }
}

/// Initiate a port reset (USB 2.0 style — PORTSC.PR = 1).
///
/// After reset completes, the hardware sets PRC=1 (Port Reset Change).
/// For USB 3.0 ports, a warm reset uses PORTSC.WPR instead.
///
/// Returns true if the reset was initiated.
pub fn port_reset(mmio: &MmioRegion, op_base: usize, port_index: u8) -> bool {
    serial_println!("[PORT] Initiating reset on port {}", port_index);

    let offset = op_base + portsc_offset(port_index);
    if let Some(current) = mmio.read32(offset) {
        // Set PR=1 while preserving W1C bits
        let preserved = current & !PORTSC_CHANGE_BITS;
        mmio.write32(offset, preserved | PORTSC_PR);
        true
    } else {
        false
    }
}

/// Initiate a warm port reset (USB 3.0 only — PORTSC.WPR = 1).
///
/// Warm reset is used when the USB 3.0 link is in SS.Inactive or
/// Compliance Mode. After completion, WRC=1 is set.
pub fn warm_port_reset(mmio: &MmioRegion, op_base: usize, port_index: u8) -> bool {
    serial_println!("[PORT] Initiating warm reset on port {} (USB3)", port_index);

    let offset = op_base + portsc_offset(port_index);
    if let Some(current) = mmio.read32(offset) {
        let preserved = current & !PORTSC_CHANGE_BITS;
        mmio.write32(offset, preserved | PORTSC_WPR);
        true
    } else {
        false
    }
}

/// Wait for port reset to complete (PRC=1 or timeout).
///
/// Returns the updated PortInfo if reset completed, None on timeout.
pub fn wait_port_reset_complete(
    mmio: &MmioRegion,
    op_base: usize,
    port_index: u8,
    protocol: UsbProtocol,
) -> Option<PortInfo> {
    const RESET_TIMEOUT: u32 = 200_000;

    for _ in 0..RESET_TIMEOUT {
        let offset = op_base + portsc_offset(port_index);
        if let Some(portsc) = mmio.read32(offset) {
            // Check PR still set (reset in progress)
            if portsc & PORTSC_PR == 0 {
                // Reset completed — check PRC
                if portsc & PORTSC_PRC != 0 {
                    // Clear PRC
                    clear_change_bit(mmio, op_base, port_index, PORTSC_PRC);
                    return read_port_status(mmio, op_base, port_index, protocol);
                }
            }
        }
        core::hint::spin_loop();
    }

    serial_println!("[PORT] WARNING: Port {} reset timeout", port_index);
    None
}

/// Power on a port (set PP=1).
pub fn power_on(mmio: &MmioRegion, op_base: usize, port_index: u8) {
    write_portsc_preserve(mmio, op_base, port_index, PORTSC_PP, 0);
    serial_println!("[PORT] Port {} powered ON", port_index);
}

/// Power off a port (clear PP=0).
pub fn power_off(mmio: &MmioRegion, op_base: usize, port_index: u8) {
    write_portsc_preserve(mmio, op_base, port_index, 0, PORTSC_PP);
    serial_println!("[PORT] Port {} powered OFF", port_index);
}

/// Set the Port Link State (PLS) with Link Write Strobe (LWS=1).
///
/// Used to transition links to specific states (e.g., U0 to resume,
/// U3 to suspend, RxDetect for disconnect).
pub fn set_link_state(mmio: &MmioRegion, op_base: usize, port_index: u8, pls: u8) {
    let offset = op_base + portsc_offset(port_index);
    if let Some(current) = mmio.read32(offset) {
        // Build new PORTSC: preserve non-W1C bits, set LWS + new PLS
        let preserved = current & !PORTSC_CHANGE_BITS & !PORTSC_PLS_MASK;
        let new_pls = ((pls as u32) << PORTSC_PLS_SHIFT) & PORTSC_PLS_MASK;
        mmio.write32(offset, preserved | new_pls | PORTSC_LWS);
    }
}

/// Suspend a port by setting PLS=U3 (via LWS).
pub fn suspend_port(mmio: &MmioRegion, op_base: usize, port_index: u8) {
    serial_println!("[PORT] Suspending port {} (PLS=U3)", port_index);
    set_link_state(mmio, op_base, port_index, PLS_U3);
}

/// Resume a suspended port by setting PLS=U0.
///
/// For USB 3.0 ports, this triggers the link to return to U0.
/// For USB 2.0 ports, set PLS=Resume (15) first, then U0 after 20ms.
pub fn resume_port(mmio: &MmioRegion, op_base: usize, port_index: u8, protocol: UsbProtocol) {
    serial_println!("[PORT] Resuming port {} ({})", port_index, protocol.as_str());
    match protocol {
        UsbProtocol::Usb3 => {
            // USB 3.0: direct transition to U0
            set_link_state(mmio, op_base, port_index, PLS_U0);
        }
        UsbProtocol::Usb2 | UsbProtocol::Unknown => {
            // USB 2.0: set Resume first, then U0
            set_link_state(mmio, op_base, port_index, PLS_RESUME);
            // In a real driver we'd wait 20ms here, then set U0
            // For now, busy-wait a short time
            for _ in 0..50_000 {
                core::hint::spin_loop();
            }
            set_link_state(mmio, op_base, port_index, PLS_U0);
        }
    }
}

/// Disable a port by clearing PED (set PED=0 preserving W1C bits).
///
/// Note: On USB 3.0 ports, you can't software-disable by clearing PED;
/// instead you must set PLS=Disabled (5) via LWS.
pub fn disable_port(mmio: &MmioRegion, op_base: usize, port_index: u8, protocol: UsbProtocol) {
    serial_println!("[PORT] Disabling port {} ({})", port_index, protocol.as_str());
    match protocol {
        UsbProtocol::Usb3 => {
            // USB 3.0: set PLS=Disabled
            set_link_state(mmio, op_base, port_index, PLS_DISABLED);
        }
        UsbProtocol::Usb2 | UsbProtocol::Unknown => {
            // USB 2.0: write PED=1 (it's a W1C bit when written as 1 it clears)
            let offset = op_base + portsc_offset(port_index);
            if let Some(current) = mmio.read32(offset) {
                let preserved = current & !PORTSC_CHANGE_BITS;
                mmio.write32(offset, preserved | PORTSC_PED);
            }
        }
    }
}

/// Log a human-readable port status line.
pub fn log_port_status(info: &PortInfo) {
    serial_println!(
        "[PORT] Port {:2}: {} | {} | {} | conn={} en={} pwr={} PLS={} OC={}",
        info.index,
        info.state.as_str(),
        info.speed.as_str(),
        info.protocol.as_str(),
        info.connected as u8,
        info.enabled as u8,
        info.powered as u8,
        info.link_state,
        info.overcurrent as u8,
    );

    if info.changes.any() {
        serial_println!(
            "[PORT]        Changes: CSC={} PEC={} OCC={} PRC={} PLC={} WRC={} CEC={}",
            info.changes.connect_change as u8,
            info.changes.enable_change as u8,
            info.changes.overcurrent_change as u8,
            info.changes.reset_change as u8,
            info.changes.link_state_change as u8,
            info.changes.warm_reset_change as u8,
            info.changes.config_error_change as u8,
        );
    }
}
