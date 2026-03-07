//! USB HID Boot Protocol State Machine
//!
//! Drives a HID boot keyboard from discovery through enumeration to active
//! polling. Generates USB Setup Packets for each control transfer step.
//!
//! State Machine:
//! ```text
//! Disconnected -> GetDeviceDescriptor -> GetConfigDescriptor
//!   -> SetConfiguration -> SetProtocol(Boot) -> SetIdle -> Active -> Error
//! ```
//!
//! Reference: USB HID 1.11, Appendix B "Boot Interface Descriptors"
//!            USB 2.0, Chapter 9 "USB Device Framework"

#![allow(dead_code)]

use super::hid_desc::*;

// ============================================================================
// USB Standard Request Constants
// ============================================================================

/// Request type: host-to-device, standard, device recipient
const RT_STD_OUT_DEVICE: u8 = 0x00;
/// Request type: device-to-host, standard, device recipient
const RT_STD_IN_DEVICE: u8 = 0x80;
/// Request type: host-to-device, class, interface recipient
const RT_CLASS_OUT_IFACE: u8 = 0x21;
/// Request type: device-to-host, class, interface recipient
const RT_CLASS_IN_IFACE: u8 = 0xA1;

/// Standard requests
const USB_REQ_GET_DESCRIPTOR: u8 = 0x06;
const USB_REQ_SET_CONFIGURATION: u8 = 0x09;

// ============================================================================
// USB Setup Packet (8 bytes, USB 2.0 Section 9.3)
// ============================================================================

/// 8-byte USB Setup Packet for control transfers.
///
/// Matches the on-wire format expected by xHCI Setup Stage TRBs.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SetupPacket {
    pub bm_request_type: u8,
    pub b_request: u8,
    pub w_value: u16,
    pub w_index: u16,
    pub w_length: u16,
}

impl SetupPacket {
    /// Pack into 8 bytes for TRB parameter field.
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0] = self.bm_request_type;
        buf[1] = self.b_request;
        buf[2..4].copy_from_slice(&self.w_value.to_le_bytes());
        buf[4..6].copy_from_slice(&self.w_index.to_le_bytes());
        buf[6..8].copy_from_slice(&self.w_length.to_le_bytes());
        buf
    }

    /// Pack into u64 parameter for xHCI Setup Stage TRB.
    pub fn to_u64(&self) -> u64 {
        let bytes = self.to_bytes();
        u64::from_le_bytes(bytes)
    }
}

// ============================================================================
// Boot Protocol State Machine
// ============================================================================

/// Enumeration state for a HID boot keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidBootState {
    /// No device connected
    Disconnected,
    /// Waiting for GET_DESCRIPTOR(Device) response
    GetDeviceDescriptor,
    /// Waiting for GET_DESCRIPTOR(Configuration) response
    GetConfigDescriptor,
    /// Waiting for SET_CONFIGURATION response
    SetConfiguration,
    /// Waiting for SET_PROTOCOL(Boot) response
    SetProtocol,
    /// Waiting for SET_IDLE(0) response
    SetIdle,
    /// Keyboard is active, polling Interrupt IN endpoint
    Active,
    /// Unrecoverable error
    Error,
}

/// HID boot keyboard enumeration driver.
///
/// Tracks the state of a single USB keyboard through enumeration.
/// Each state generates the appropriate USB Setup Packet. The caller
/// (xHCI driver) submits the control transfer and advances the state
/// machine when the completion event arrives.
pub struct HidBootDriver {
    /// Current enumeration state
    state: HidBootState,
    /// xHCI slot ID (assigned by Enable Slot command)
    slot_id: u8,
    /// USB device address (assigned by Address Device)
    device_address: u8,
    /// Max packet size for EP0 (from device descriptor byte 7)
    max_packet_size0: u8,
    /// Parsed keyboard info (available after GetConfigDescriptor)
    keyboard_info: Option<HidBootKeyboardInfo>,
    /// Raw device descriptor buffer (18 bytes)
    device_desc_buf: [u8; 18],
    /// Raw configuration descriptor buffer (up to 256 bytes)
    config_desc_buf: [u8; 256],
    /// Actual length of configuration descriptor data received
    config_desc_len: usize,
    /// Error message for debugging
    error_msg: &'static str,
}

impl HidBootDriver {
    /// Create a new driver for a device at the given xHCI slot.
    pub fn new(slot_id: u8) -> Self {
        Self {
            state: HidBootState::Disconnected,
            slot_id,
            device_address: 0,
            max_packet_size0: 8, // Default per USB spec for initial transfers
            keyboard_info: None,
            device_desc_buf: [0u8; 18],
            config_desc_buf: [0u8; 256],
            config_desc_len: 0,
            error_msg: "",
        }
    }

    /// Get current state.
    pub fn state(&self) -> HidBootState {
        self.state
    }

    /// Get slot ID.
    pub fn slot_id(&self) -> u8 {
        self.slot_id
    }

    /// Get keyboard info (available once enumeration finds it).
    pub fn keyboard_info(&self) -> Option<&HidBootKeyboardInfo> {
        self.keyboard_info.as_ref()
    }

    /// Get error message (when state == Error).
    pub fn error_msg(&self) -> &'static str {
        self.error_msg
    }

    /// Is the keyboard active and ready for polling?
    pub fn is_active(&self) -> bool {
        self.state == HidBootState::Active
    }

    // ========================================================================
    // State Machine — call start_enumeration(), then advance() after each
    // control transfer completes successfully.
    // ========================================================================

    /// Begin enumeration: transition to GetDeviceDescriptor.
    /// Returns the Setup Packet for the first control transfer.
    pub fn start_enumeration(&mut self) -> SetupPacket {
        self.state = HidBootState::GetDeviceDescriptor;
        // GET_DESCRIPTOR(Device), 18 bytes
        SetupPacket {
            bm_request_type: RT_STD_IN_DEVICE,
            b_request: USB_REQ_GET_DESCRIPTOR,
            w_value: (DESC_TYPE_DEVICE as u16) << 8, // Descriptor type in high byte
            w_index: 0,
            w_length: 18,
        }
    }

    /// Feed the data stage response and advance the state machine.
    ///
    /// `data`: response bytes from the Data Stage of the control transfer.
    /// Returns: the next Setup Packet to send, or None if terminal state.
    pub fn advance(&mut self, data: &[u8]) -> Option<SetupPacket> {
        match self.state {
            HidBootState::GetDeviceDescriptor => {
                self.handle_device_descriptor(data)
            }
            HidBootState::GetConfigDescriptor => {
                self.handle_config_descriptor(data)
            }
            HidBootState::SetConfiguration => {
                self.handle_set_configuration(data)
            }
            HidBootState::SetProtocol => {
                self.handle_set_protocol(data)
            }
            HidBootState::SetIdle => {
                self.handle_set_idle(data);
                None // Terminal: keyboard is now Active
            }
            _ => None,
        }
    }

    /// Handle device descriptor response.
    fn handle_device_descriptor(&mut self, data: &[u8]) -> Option<SetupPacket> {
        if data.len() < 18 {
            self.state = HidBootState::Error;
            self.error_msg = "Device descriptor too short";
            return None;
        }

        self.device_desc_buf[..18].copy_from_slice(&data[..18]);

        if let Some(desc) = UsbDeviceDescriptor::parse(data) {
            self.max_packet_size0 = desc.b_max_packet_size0;
        }

        // Next: GET_DESCRIPTOR(Configuration, index 0), request full length
        // First request 9 bytes to get wTotalLength, then request full length.
        // For simplicity, request up to 256 bytes (covers most keyboards).
        self.state = HidBootState::GetConfigDescriptor;
        Some(SetupPacket {
            bm_request_type: RT_STD_IN_DEVICE,
            b_request: USB_REQ_GET_DESCRIPTOR,
            w_value: (DESC_TYPE_CONFIGURATION as u16) << 8, // Config index 0
            w_index: 0,
            w_length: 256, // Request full config
        })
    }

    /// Handle configuration descriptor response.
    fn handle_config_descriptor(&mut self, data: &[u8]) -> Option<SetupPacket> {
        if data.len() < 9 {
            self.state = HidBootState::Error;
            self.error_msg = "Config descriptor too short";
            return None;
        }

        let copy_len = data.len().min(256);
        self.config_desc_buf[..copy_len].copy_from_slice(&data[..copy_len]);
        self.config_desc_len = copy_len;

        // Parse to find HID boot keyboard
        match find_hid_boot_keyboard(&self.config_desc_buf[..copy_len]) {
            Some(info) => {
                let config_value = info.config_value;
                self.keyboard_info = Some(info);

                // Next: SET_CONFIGURATION
                self.state = HidBootState::SetConfiguration;
                Some(SetupPacket {
                    bm_request_type: RT_STD_OUT_DEVICE,
                    b_request: USB_REQ_SET_CONFIGURATION,
                    w_value: config_value as u16,
                    w_index: 0,
                    w_length: 0,
                })
            }
            None => {
                self.state = HidBootState::Error;
                self.error_msg = "No HID boot keyboard found in config";
                None
            }
        }
    }

    /// Handle SET_CONFIGURATION completion (status stage only, no data).
    fn handle_set_configuration(&mut self, _data: &[u8]) -> Option<SetupPacket> {
        let iface = match &self.keyboard_info {
            Some(info) => info.interface_number,
            None => {
                self.state = HidBootState::Error;
                self.error_msg = "No keyboard info for SET_PROTOCOL";
                return None;
            }
        };

        // Next: SET_PROTOCOL(Boot) — HID class request to interface
        self.state = HidBootState::SetProtocol;
        Some(SetupPacket {
            bm_request_type: RT_CLASS_OUT_IFACE,
            b_request: HID_REQ_SET_PROTOCOL,
            w_value: HID_PROTOCOL_BOOT_VALUE, // 0 = Boot protocol
            w_index: iface as u16,
            w_length: 0,
        })
    }

    /// Handle SET_PROTOCOL(Boot) completion.
    fn handle_set_protocol(&mut self, _data: &[u8]) -> Option<SetupPacket> {
        let iface = match &self.keyboard_info {
            Some(info) => info.interface_number,
            None => {
                self.state = HidBootState::Error;
                self.error_msg = "No keyboard info for SET_IDLE";
                return None;
            }
        };

        // Next: SET_IDLE(0, 0) — indefinite idle (only report on change)
        self.state = HidBootState::SetIdle;
        Some(SetupPacket {
            bm_request_type: RT_CLASS_OUT_IFACE,
            b_request: HID_REQ_SET_IDLE,
            w_value: 0, // Duration=0 (indefinite), Report ID=0
            w_index: iface as u16,
            w_length: 0,
        })
    }

    /// Handle SET_IDLE completion.
    fn handle_set_idle(&mut self, _data: &[u8]) {
        self.state = HidBootState::Active;
    }

    /// Signal device disconnection — reset state.
    pub fn disconnect(&mut self) {
        self.state = HidBootState::Disconnected;
        self.keyboard_info = None;
        self.config_desc_len = 0;
        self.error_msg = "";
    }

    /// Signal an error from the xHCI layer (stall, transaction error, etc.)
    pub fn signal_error(&mut self, msg: &'static str) {
        self.state = HidBootState::Error;
        self.error_msg = msg;
    }
}
