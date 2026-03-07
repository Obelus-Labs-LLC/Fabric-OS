//! USB HID (Human Interface Device) Class Driver
//!
//! Phase 21c: Boot protocol keyboard support.
//!
//! Modules:
//! - `hid_desc`: USB and HID descriptor parsing (device, config, interface, endpoint, HID)
//! - `hid_boot`: Boot protocol enumeration state machine (Setup Packet generation)
//! - `hid_input`: 8-byte boot report decoder (key press/release events, ASCII translation)

#![allow(dead_code)]

pub mod hid_desc;
pub mod hid_boot;
pub mod hid_input;

pub use hid_desc::{
    // Descriptor types
    UsbDeviceDescriptor, UsbConfigDescriptor, UsbInterfaceDescriptor,
    UsbEndpointDescriptor, HidDescriptor, EndpointType,
    // HID class constants
    USB_CLASS_HID, HID_SUBCLASS_BOOT,
    HID_PROTOCOL_KEYBOARD, HID_PROTOCOL_MOUSE,
    // HID class requests
    HID_REQ_GET_REPORT, HID_REQ_SET_REPORT,
    HID_REQ_GET_IDLE, HID_REQ_SET_IDLE,
    HID_REQ_GET_PROTOCOL, HID_REQ_SET_PROTOCOL,
    // Scanner
    HidBootKeyboardInfo, find_hid_boot_keyboard,
};

pub use hid_boot::{
    SetupPacket, HidBootState, HidBootDriver,
};

pub use hid_input::{
    // Modifier bits
    MOD_LEFT_CTRL, MOD_LEFT_SHIFT, MOD_LEFT_ALT, MOD_LEFT_GUI,
    MOD_RIGHT_CTRL, MOD_RIGHT_SHIFT, MOD_RIGHT_ALT, MOD_RIGHT_GUI,
    // Key events
    KeyEvent, BootKeyboardParser,
    // Translation
    usage_to_ascii,
    // LEDs
    LED_NUM_LOCK, LED_CAPS_LOCK, LED_SCROLL_LOCK,
    build_led_report,
};
