//! Gaming & Media subsystem.
//!
//! Phase 18: Audio mixer, virtual gamepad, video frame codecs,
//! game streaming protocol, and stream client. All software-only
//! (no hardware audio/USB required). Future phases add AC'97/HDA
//! and USB HID drivers.

#![allow(dead_code)]

pub mod audio;
pub mod gamepad;
pub mod codec;
pub mod stream;
pub mod client;

use crate::serial_println;

/// Capabilities provided by the gaming subsystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamingCapability {
    AudioMixer,
    GamepadInput,
    StreamClient,
    MediaCodec,
}

/// Current gaming subsystem mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GamingMode {
    Idle,
    Streaming,
    LocalPlayback,
}

/// Initialize the gaming subsystem.
pub fn init() {
    audio::init();
    gamepad::init();

    serial_println!("[GAMING] Audio mixer: {} sources, software PCM", audio::MAX_SOURCES);
    serial_println!("[GAMING] Gamepad table: {} slots, keyboard-mapped", gamepad::MAX_GAMEPADS);
    serial_println!("[GAMING] Codecs: PCM S16LE, RGB888/RGBA8888, RLE compression");
    serial_println!("[GAMING] Stream protocol: video/audio/input, 13-byte base header");
    serial_println!("[GAMING] Stream client: {} concurrent sessions", client::MAX_CLIENTS);
}
