//! Game streaming protocol — wire format serialization.
//!
//! Pure encode/decode functions for stream headers. No network I/O.
//! All multi-byte fields use little-endian byte order (native x86).

#![allow(dead_code)]

/// Stream message type identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StreamMsgType {
    VideoFrame = 1,
    AudioChunk = 2,
    InputState = 3,
    Control = 4,
}

impl StreamMsgType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(StreamMsgType::VideoFrame),
            2 => Some(StreamMsgType::AudioChunk),
            3 => Some(StreamMsgType::InputState),
            4 => Some(StreamMsgType::Control),
            _ => None,
        }
    }
}

/// Base stream header (13 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamHeader {
    pub msg_type: u8,
    pub sequence: u32,
    pub timestamp_ms: u32,
    pub payload_len: u32,
}

/// Header size in bytes.
pub const STREAM_HEADER_SIZE: usize = 13;

/// Encode a stream header to bytes.
pub fn encode_header(h: &StreamHeader) -> [u8; STREAM_HEADER_SIZE] {
    let mut buf = [0u8; STREAM_HEADER_SIZE];
    buf[0] = h.msg_type;
    buf[1..5].copy_from_slice(&h.sequence.to_le_bytes());
    buf[5..9].copy_from_slice(&h.timestamp_ms.to_le_bytes());
    buf[9..13].copy_from_slice(&h.payload_len.to_le_bytes());
    buf
}

/// Decode a stream header from bytes.
pub fn decode_header(data: &[u8]) -> Option<StreamHeader> {
    if data.len() < STREAM_HEADER_SIZE {
        return None;
    }
    Some(StreamHeader {
        msg_type: data[0],
        sequence: u32::from_le_bytes([data[1], data[2], data[3], data[4]]),
        timestamp_ms: u32::from_le_bytes([data[5], data[6], data[7], data[8]]),
        payload_len: u32::from_le_bytes([data[9], data[10], data[11], data[12]]),
    })
}

/// Video frame header (6 bytes, follows StreamHeader).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoHeader {
    pub width: u16,
    pub height: u16,
    pub format: u8,
    pub flags: u8,
}

pub const VIDEO_HEADER_SIZE: usize = 6;

pub fn encode_video_header(h: &VideoHeader) -> [u8; VIDEO_HEADER_SIZE] {
    let mut buf = [0u8; VIDEO_HEADER_SIZE];
    buf[0..2].copy_from_slice(&h.width.to_le_bytes());
    buf[2..4].copy_from_slice(&h.height.to_le_bytes());
    buf[4] = h.format;
    buf[5] = h.flags;
    buf
}

pub fn decode_video_header(data: &[u8]) -> Option<VideoHeader> {
    if data.len() < VIDEO_HEADER_SIZE {
        return None;
    }
    Some(VideoHeader {
        width: u16::from_le_bytes([data[0], data[1]]),
        height: u16::from_le_bytes([data[2], data[3]]),
        format: data[4],
        flags: data[5],
    })
}

/// Audio chunk header (8 bytes, follows StreamHeader).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioHeader {
    pub sample_rate: u32,
    pub channels: u8,
    pub format: u8,
    pub sample_count: u16,
}

pub const AUDIO_HEADER_SIZE: usize = 8;

pub fn encode_audio_header(h: &AudioHeader) -> [u8; AUDIO_HEADER_SIZE] {
    let mut buf = [0u8; AUDIO_HEADER_SIZE];
    buf[0..4].copy_from_slice(&h.sample_rate.to_le_bytes());
    buf[4] = h.channels;
    buf[5] = h.format;
    buf[6..8].copy_from_slice(&h.sample_count.to_le_bytes());
    buf
}

pub fn decode_audio_header(data: &[u8]) -> Option<AudioHeader> {
    if data.len() < AUDIO_HEADER_SIZE {
        return None;
    }
    Some(AudioHeader {
        sample_rate: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        channels: data[4],
        format: data[5],
        sample_count: u16::from_le_bytes([data[6], data[7]]),
    })
}

/// Input state header (18 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputHeader {
    pub gamepad_buttons: u16,
    pub axes: [i16; 6],   // left_x, left_y, right_x, right_y, lt, rt
    pub keyboard_state: u32,
}

pub const INPUT_HEADER_SIZE: usize = 18;

pub fn encode_input(h: &InputHeader) -> [u8; INPUT_HEADER_SIZE] {
    let mut buf = [0u8; INPUT_HEADER_SIZE];
    buf[0..2].copy_from_slice(&h.gamepad_buttons.to_le_bytes());
    for i in 0..6 {
        let offset = 2 + i * 2;
        buf[offset..offset + 2].copy_from_slice(&h.axes[i].to_le_bytes());
    }
    buf[14..18].copy_from_slice(&h.keyboard_state.to_le_bytes());
    buf
}

pub fn decode_input(data: &[u8]) -> Option<InputHeader> {
    if data.len() < INPUT_HEADER_SIZE {
        return None;
    }
    let mut axes = [0i16; 6];
    for i in 0..6 {
        let offset = 2 + i * 2;
        axes[i] = i16::from_le_bytes([data[offset], data[offset + 1]]);
    }
    Some(InputHeader {
        gamepad_buttons: u16::from_le_bytes([data[0], data[1]]),
        axes,
        keyboard_state: u32::from_le_bytes([data[14], data[15], data[16], data[17]]),
    })
}
