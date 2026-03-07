//! USB HID Descriptor Parser
//!
//! Parses USB device, configuration, interface, endpoint, and HID-class
//! descriptors to identify HID boot protocol keyboards.
//!
//! Reference: USB HID 1.11, Section 6.2 "Report Descriptor"
//!            USB 2.0, Chapter 9 "USB Device Framework"

#![allow(dead_code)]

// ============================================================================
// USB Descriptor Type Codes (USB 2.0, Table 9-5)
// ============================================================================

/// Standard descriptor types
pub const DESC_TYPE_DEVICE: u8 = 0x01;
pub const DESC_TYPE_CONFIGURATION: u8 = 0x02;
pub const DESC_TYPE_STRING: u8 = 0x03;
pub const DESC_TYPE_INTERFACE: u8 = 0x04;
pub const DESC_TYPE_ENDPOINT: u8 = 0x05;

/// HID-class descriptor types (HID 1.11, Section 7.1)
pub const DESC_TYPE_HID: u8 = 0x21;
pub const DESC_TYPE_HID_REPORT: u8 = 0x22;
pub const DESC_TYPE_HID_PHYSICAL: u8 = 0x23;

// ============================================================================
// USB Class/Subclass/Protocol for HID (HID 1.11, Section 4.1)
// ============================================================================

/// USB HID class code
pub const USB_CLASS_HID: u8 = 0x03;

/// HID subclass: No subclass
pub const HID_SUBCLASS_NONE: u8 = 0x00;
/// HID subclass: Boot interface (supports simplified boot protocol)
pub const HID_SUBCLASS_BOOT: u8 = 0x01;

/// HID protocol: None
pub const HID_PROTOCOL_NONE: u8 = 0x00;
/// HID protocol: Keyboard
pub const HID_PROTOCOL_KEYBOARD: u8 = 0x01;
/// HID protocol: Mouse
pub const HID_PROTOCOL_MOUSE: u8 = 0x02;

// ============================================================================
// HID Class Requests (HID 1.11, Section 7.2)
// ============================================================================

/// GET_REPORT (class-specific, device-to-host)
pub const HID_REQ_GET_REPORT: u8 = 0x01;
/// GET_IDLE (class-specific, device-to-host)
pub const HID_REQ_GET_IDLE: u8 = 0x02;
/// GET_PROTOCOL (class-specific, device-to-host)
pub const HID_REQ_GET_PROTOCOL: u8 = 0x03;
/// SET_REPORT (class-specific, host-to-device)
pub const HID_REQ_SET_REPORT: u8 = 0x09;
/// SET_IDLE (class-specific, host-to-device)
pub const HID_REQ_SET_IDLE: u8 = 0x0A;
/// SET_PROTOCOL (class-specific, host-to-device)
pub const HID_REQ_SET_PROTOCOL: u8 = 0x0B;

/// Boot protocol (simplified 8-byte reports)
pub const HID_PROTOCOL_BOOT_VALUE: u16 = 0x0000;
/// Report protocol (full HID report descriptor)
pub const HID_PROTOCOL_REPORT_VALUE: u16 = 0x0001;

// ============================================================================
// USB Device Descriptor (USB 2.0, Table 9-8) — 18 bytes
// ============================================================================

/// Parsed USB device descriptor.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct UsbDeviceDescriptor {
    pub b_length: u8,            // 18
    pub b_descriptor_type: u8,   // DESC_TYPE_DEVICE (0x01)
    pub bcd_usb: u16,            // USB spec version (BCD, e.g., 0x0200 = USB 2.0)
    pub b_device_class: u8,      // Class code (0x00 = per-interface)
    pub b_device_sub_class: u8,
    pub b_device_protocol: u8,
    pub b_max_packet_size0: u8,  // Max packet size for EP0 (8, 16, 32, or 64)
    pub id_vendor: u16,          // Vendor ID
    pub id_product: u16,         // Product ID
    pub bcd_device: u16,         // Device release number (BCD)
    pub i_manufacturer: u8,      // String descriptor index
    pub i_product: u8,           // String descriptor index
    pub i_serial_number: u8,     // String descriptor index
    pub b_num_configurations: u8,
}

impl UsbDeviceDescriptor {
    /// Parse from a raw byte buffer (must be >= 18 bytes).
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 18 || data[0] < 18 || data[1] != DESC_TYPE_DEVICE {
            return None;
        }
        Some(Self {
            b_length: data[0],
            b_descriptor_type: data[1],
            bcd_usb: u16::from_le_bytes([data[2], data[3]]),
            b_device_class: data[4],
            b_device_sub_class: data[5],
            b_device_protocol: data[6],
            b_max_packet_size0: data[7],
            id_vendor: u16::from_le_bytes([data[8], data[9]]),
            id_product: u16::from_le_bytes([data[10], data[11]]),
            bcd_device: u16::from_le_bytes([data[12], data[13]]),
            i_manufacturer: data[14],
            i_product: data[15],
            i_serial_number: data[16],
            b_num_configurations: data[17],
        })
    }
}

// ============================================================================
// USB Configuration Descriptor (USB 2.0, Table 9-10) — 9 bytes header
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct UsbConfigDescriptor {
    pub b_length: u8,               // 9
    pub b_descriptor_type: u8,      // DESC_TYPE_CONFIGURATION (0x02)
    pub w_total_length: u16,        // Total length including all subordinate descriptors
    pub b_num_interfaces: u8,
    pub b_configuration_value: u8,  // Value to use in SET_CONFIGURATION
    pub i_configuration: u8,        // String descriptor index
    pub bm_attributes: u8,          // D7: reserved (1), D6: self-powered, D5: remote wakeup
    pub b_max_power: u8,            // Max current in 2mA units
}

impl UsbConfigDescriptor {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 9 || data[1] != DESC_TYPE_CONFIGURATION {
            return None;
        }
        Some(Self {
            b_length: data[0],
            b_descriptor_type: data[1],
            w_total_length: u16::from_le_bytes([data[2], data[3]]),
            b_num_interfaces: data[4],
            b_configuration_value: data[5],
            i_configuration: data[6],
            bm_attributes: data[7],
            b_max_power: data[8],
        })
    }
}

// ============================================================================
// USB Interface Descriptor (USB 2.0, Table 9-12) — 9 bytes
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct UsbInterfaceDescriptor {
    pub b_length: u8,               // 9
    pub b_descriptor_type: u8,      // DESC_TYPE_INTERFACE (0x04)
    pub b_interface_number: u8,
    pub b_alternate_setting: u8,
    pub b_num_endpoints: u8,
    pub b_interface_class: u8,      // USB_CLASS_HID = 0x03
    pub b_interface_sub_class: u8,  // HID_SUBCLASS_BOOT = 0x01
    pub b_interface_protocol: u8,   // HID_PROTOCOL_KEYBOARD = 0x01
    pub i_interface: u8,            // String descriptor index
}

impl UsbInterfaceDescriptor {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 9 || data[1] != DESC_TYPE_INTERFACE {
            return None;
        }
        Some(Self {
            b_length: data[0],
            b_descriptor_type: data[1],
            b_interface_number: data[2],
            b_alternate_setting: data[3],
            b_num_endpoints: data[4],
            b_interface_class: data[5],
            b_interface_sub_class: data[6],
            b_interface_protocol: data[7],
            i_interface: data[8],
        })
    }

    /// Check if this interface is a HID boot keyboard.
    pub fn is_hid_boot_keyboard(&self) -> bool {
        self.b_interface_class == USB_CLASS_HID
            && self.b_interface_sub_class == HID_SUBCLASS_BOOT
            && self.b_interface_protocol == HID_PROTOCOL_KEYBOARD
    }

    /// Check if this interface is a HID boot mouse.
    pub fn is_hid_boot_mouse(&self) -> bool {
        self.b_interface_class == USB_CLASS_HID
            && self.b_interface_sub_class == HID_SUBCLASS_BOOT
            && self.b_interface_protocol == HID_PROTOCOL_MOUSE
    }

    /// Check if this interface is any HID device.
    pub fn is_hid(&self) -> bool {
        self.b_interface_class == USB_CLASS_HID
    }
}

// ============================================================================
// USB Endpoint Descriptor (USB 2.0, Table 9-13) — 7 bytes
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct UsbEndpointDescriptor {
    pub b_length: u8,              // 7
    pub b_descriptor_type: u8,     // DESC_TYPE_ENDPOINT (0x05)
    pub b_endpoint_address: u8,    // [3:0] endpoint number, [7] direction (1=IN)
    pub bm_attributes: u8,         // [1:0] transfer type
    pub w_max_packet_size: u16,    // Max packet size
    pub b_interval: u8,            // Polling interval (ms for low/full speed)
}

/// Endpoint transfer type (bm_attributes[1:0])
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointType {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
}

impl UsbEndpointDescriptor {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 7 || data[1] != DESC_TYPE_ENDPOINT {
            return None;
        }
        Some(Self {
            b_length: data[0],
            b_descriptor_type: data[1],
            b_endpoint_address: data[2],
            bm_attributes: data[3],
            w_max_packet_size: u16::from_le_bytes([data[4], data[5]]),
            b_interval: data[6],
        })
    }

    /// Endpoint number (0-15).
    pub fn endpoint_number(&self) -> u8 {
        self.b_endpoint_address & 0x0F
    }

    /// Direction: true = IN (device-to-host), false = OUT (host-to-device).
    pub fn is_in(&self) -> bool {
        (self.b_endpoint_address & 0x80) != 0
    }

    /// Transfer type.
    pub fn transfer_type(&self) -> EndpointType {
        match self.bm_attributes & 0x03 {
            0 => EndpointType::Control,
            1 => EndpointType::Isochronous,
            2 => EndpointType::Bulk,
            3 => EndpointType::Interrupt,
            _ => unreachable!(),
        }
    }

    /// Check if this is an Interrupt IN endpoint (used by HID keyboards).
    pub fn is_interrupt_in(&self) -> bool {
        self.is_in() && self.transfer_type() == EndpointType::Interrupt
    }
}

// ============================================================================
// HID Descriptor (HID 1.11, Section 6.2.1) — 6+ bytes
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub struct HidDescriptor {
    pub b_length: u8,              // >= 6
    pub b_descriptor_type: u8,     // DESC_TYPE_HID (0x21)
    pub bcd_hid: u16,              // HID spec version (BCD, e.g., 0x0111 = 1.11)
    pub b_country_code: u8,        // Country code (0 = not localized)
    pub b_num_descriptors: u8,     // Number of class descriptors (at least 1)
    /// First subordinate descriptor: type + length
    pub report_desc_type: u8,      // Usually DESC_TYPE_HID_REPORT (0x22)
    pub report_desc_length: u16,   // Length of report descriptor
}

impl HidDescriptor {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 9 || data[1] != DESC_TYPE_HID {
            return None;
        }
        Some(Self {
            b_length: data[0],
            b_descriptor_type: data[1],
            bcd_hid: u16::from_le_bytes([data[2], data[3]]),
            b_country_code: data[4],
            b_num_descriptors: data[5],
            report_desc_type: data[6],
            report_desc_length: u16::from_le_bytes([data[7], data[8]]),
        })
    }
}

// ============================================================================
// Configuration Descriptor Walker
// ============================================================================

/// Result of scanning a configuration descriptor for HID boot keyboards.
#[derive(Debug)]
pub struct HidBootKeyboardInfo {
    /// Interface number to use in SET_PROTOCOL
    pub interface_number: u8,
    /// Interrupt IN endpoint address
    pub interrupt_in_ep: u8,
    /// Max packet size for the interrupt endpoint
    pub max_packet_size: u16,
    /// Polling interval in milliseconds
    pub poll_interval_ms: u8,
    /// Configuration value for SET_CONFIGURATION
    pub config_value: u8,
}

/// Walk a full configuration descriptor (header + all subordinate descriptors)
/// looking for a HID boot keyboard interface with an Interrupt IN endpoint.
///
/// `data` must contain the full wTotalLength bytes from GET_DESCRIPTOR(Configuration).
pub fn find_hid_boot_keyboard(data: &[u8]) -> Option<HidBootKeyboardInfo> {
    if data.len() < 9 {
        return None;
    }

    let config = UsbConfigDescriptor::parse(data)?;
    let total_len = config.w_total_length as usize;
    if data.len() < total_len {
        return None;
    }

    let mut offset = config.b_length as usize;
    let mut found_keyboard_iface: Option<UsbInterfaceDescriptor> = None;

    while offset + 2 <= total_len {
        let desc_len = data[offset] as usize;
        let desc_type = data[offset + 1];

        // Sanity: descriptor must be at least 2 bytes and fit
        if desc_len < 2 || offset + desc_len > total_len {
            break;
        }

        match desc_type {
            DESC_TYPE_INTERFACE => {
                if let Some(iface) = UsbInterfaceDescriptor::parse(&data[offset..]) {
                    if iface.is_hid_boot_keyboard() {
                        found_keyboard_iface = Some(iface);
                    } else {
                        // New interface that isn't a keyboard — reset search
                        found_keyboard_iface = None;
                    }
                }
            }

            DESC_TYPE_ENDPOINT if found_keyboard_iface.is_some() => {
                if let Some(ep) = UsbEndpointDescriptor::parse(&data[offset..]) {
                    if ep.is_interrupt_in() {
                        let iface = found_keyboard_iface.unwrap();
                        return Some(HidBootKeyboardInfo {
                            interface_number: iface.b_interface_number,
                            interrupt_in_ep: ep.b_endpoint_address,
                            max_packet_size: ep.w_max_packet_size,
                            poll_interval_ms: ep.b_interval,
                            config_value: config.b_configuration_value,
                        });
                    }
                }
            }

            _ => {} // Skip HID descriptors and other types
        }

        offset += desc_len;
    }

    None
}
