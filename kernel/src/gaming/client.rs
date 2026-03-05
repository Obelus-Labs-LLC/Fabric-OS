//! Game stream client — receives video frames and audio, sends input.
//!
//! Phase 18: No actual network connections. process_video_frame and
//! process_audio_chunk operate on raw byte slices passed by callers.
//! Future phases integrate with the TCP/TLS network stack.

#![allow(dead_code)]

use super::stream::{VideoHeader, AudioHeader, InputHeader, encode_input};
use super::codec::{VideoFrame, PixelFormat};
use super::gamepad::GamepadState;

/// Maximum concurrent stream clients.
pub const MAX_CLIENTS: usize = 4;

/// Stream client connection state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamClientState {
    Disconnected,
    Connected,
    Streaming,
    Paused,
}

/// Stream client configuration.
#[derive(Clone, Copy, Debug)]
pub struct StreamConfig {
    pub server_addr: [u8; 4],
    pub port: u16,
    pub target_fps: u8,
    pub audio_enabled: bool,
}

/// Stream performance statistics.
#[derive(Clone, Copy, Debug, Default)]
pub struct StreamStats {
    pub frames_received: u32,
    pub frames_dropped: u32,
    pub audio_underruns: u32,
    pub latency_ms: u32,
}

/// A single game stream client session.
pub struct StreamClient {
    pub config: StreamConfig,
    pub state: StreamClientState,
    pub window_id: Option<u32>,
    pub audio_source: Option<u32>,
    pub frame_count: u32,
    pub stats: StreamStats,
    /// Last decoded frame dimensions for verification.
    pub last_frame_width: u16,
    pub last_frame_height: u16,
}

impl StreamClient {
    /// Create a new disconnected stream client.
    pub fn create(config: StreamConfig) -> Self {
        StreamClient {
            config,
            state: StreamClientState::Disconnected,
            window_id: None,
            audio_source: None,
            frame_count: 0,
            stats: StreamStats::default(),
            last_frame_width: 0,
            last_frame_height: 0,
        }
    }

    /// Process an incoming video frame.
    /// In Phase 18, this just validates the data and updates stats.
    /// Future phases blit to a WM window surface.
    pub fn process_video_frame(&mut self, header: &VideoHeader, data: &[u8]) {
        let bpp = match header.format {
            0 => 3, // RGB888
            1 => 4, // RGBA8888
            2 => 3, // BGR888
            _ => 3,
        };
        let expected_size = (header.width as usize) * (header.height as usize) * bpp;

        if data.len() >= expected_size {
            self.stats.frames_received += 1;
            self.frame_count += 1;
            self.last_frame_width = header.width;
            self.last_frame_height = header.height;
            self.state = StreamClientState::Streaming;
        } else {
            self.stats.frames_dropped += 1;
        }
    }

    /// Process an incoming audio chunk.
    /// In Phase 18, validates and counts. Future phases write to audio mixer.
    pub fn process_audio_chunk(&mut self, header: &AudioHeader, data: &[u8]) {
        let expected_bytes = (header.sample_count as usize) * 2 * (header.channels as usize);
        if data.len() >= expected_bytes {
            // Would write to audio mixer source here
            self.state = StreamClientState::Streaming;
        } else {
            self.stats.audio_underruns += 1;
        }
    }

    /// Encode current input state for sending to stream server.
    pub fn send_input(&self, gamepad: &GamepadState, keyboard_state: u32) -> [u8; 18] {
        let header = InputHeader {
            gamepad_buttons: gamepad.buttons,
            axes: [
                gamepad.axes.left_x,
                gamepad.axes.left_y,
                gamepad.axes.right_x,
                gamepad.axes.right_y,
                gamepad.axes.left_trigger as i16,
                gamepad.axes.right_trigger as i16,
            ],
            keyboard_state,
        };
        encode_input(&header)
    }

    /// Destroy the client session.
    pub fn destroy(&mut self) {
        self.state = StreamClientState::Disconnected;
        self.window_id = None;
        self.audio_source = None;
        self.frame_count = 0;
        self.stats = StreamStats::default();
        self.last_frame_width = 0;
        self.last_frame_height = 0;
    }
}

/// Table of stream clients.
pub struct StreamClientTable {
    clients: [Option<StreamClient>; MAX_CLIENTS],
}

impl StreamClientTable {
    pub const fn new() -> Self {
        const NONE: Option<StreamClient> = None;
        StreamClientTable {
            clients: [NONE; MAX_CLIENTS],
        }
    }

    /// Create a new client in the first available slot.
    pub fn create(&mut self, config: StreamConfig) -> Option<usize> {
        for (i, slot) in self.clients.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(StreamClient::create(config));
                return Some(i);
            }
        }
        None
    }

    /// Get client by slot (immutable).
    pub fn get(&self, slot: usize) -> Option<&StreamClient> {
        if slot >= MAX_CLIENTS {
            return None;
        }
        self.clients[slot].as_ref()
    }

    /// Get client by slot (mutable).
    pub fn get_mut(&mut self, slot: usize) -> Option<&mut StreamClient> {
        if slot >= MAX_CLIENTS {
            return None;
        }
        self.clients[slot].as_mut()
    }

    /// Destroy a client by slot.
    pub fn destroy(&mut self, slot: usize) -> bool {
        if slot >= MAX_CLIENTS {
            return false;
        }
        if self.clients[slot].is_some() {
            self.clients[slot] = None;
            true
        } else {
            false
        }
    }

    /// Count active clients.
    pub fn count(&self) -> usize {
        self.clients.iter().filter(|c| c.is_some()).count()
    }
}
