//! STRESS Phase 18 Gate — Gaming & Media Tests
//!
//! 10 tests verifying audio PCM buffers, mixer, gamepad state,
//! keyboard mapping, stream protocol, codecs, and client lifecycle.

#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::format;
use crate::ocrb::StressResult;
use crate::gaming::audio::{AudioMixer, AudioFormat, PcmBuffer};
use crate::gaming::gamepad::{GamepadTable, GamepadState, ButtonFlags, AxisState};
use crate::gaming::codec::{VideoFrame, PixelFormat, rle_encode, rle_decode};
use crate::gaming::stream::{
    StreamHeader, StreamMsgType, VideoHeader, AudioHeader, InputHeader,
    encode_header, decode_header,
    encode_video_header, decode_video_header,
    encode_input, decode_input,
};
use crate::gaming::client::{StreamClient, StreamConfig, StreamClientState, StreamClientTable, MAX_CLIENTS};

pub fn run_all_tests() -> Vec<StressResult> {
    let mut results = Vec::new();
    results.push(test_pcm_buffer());
    results.push(test_mixer_two_sources());
    results.push(test_mixer_volume());
    results.push(test_gamepad_buttons());
    results.push(test_gamepad_keyboard());
    results.push(test_gamepad_table());
    results.push(test_stream_header());
    results.push(test_codec_rgb_frame());
    results.push(test_codec_rle());
    results.push(test_client_lifecycle());
    results
}

/// Test 1: Audio PCM buffer write/read roundtrip (w:10)
fn test_pcm_buffer() -> StressResult {
    let mut buf = PcmBuffer::new();

    // Write 256 sequential samples
    let samples: Vec<i16> = (0..256).map(|i| i as i16).collect();
    let written = buf.write_samples(&samples);
    let write_ok = written == 256;
    let avail_ok = buf.available() == 256;

    // Read them back
    let mut out = vec![0i16; 256];
    let read = buf.read_samples(&mut out);
    let read_ok = read == 256;

    // Verify data matches
    let mut data_ok = true;
    for i in 0..256 {
        if out[i] != i as i16 {
            data_ok = false;
            break;
        }
    }

    // Buffer should be empty now
    let empty_ok = buf.available() == 0;

    let passed = write_ok && avail_ok && read_ok && data_ok && empty_ok;
    StressResult {
        test_name: "Audio PCM buffer write/read",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("write={} avail={} read={} data={} empty={}",
            write_ok, avail_ok, read_ok, data_ok, empty_ok),
    }
}

/// Test 2: Audio mixer: two sources sum (w:10)
fn test_mixer_two_sources() -> StressResult {
    let mut mixer = AudioMixer::new();

    let id0 = mixer.add_source(AudioFormat::CD_MONO);
    let id1 = mixer.add_source(AudioFormat::CD_MONO);

    let ids_ok = id0.is_some() && id1.is_some();
    let id0 = id0.unwrap_or(0);
    let id1 = id1.unwrap_or(0);

    // Write constant samples to each source
    let samples0 = vec![100i16; 64];
    let samples1 = vec![200i16; 64];
    mixer.write_to_source(id0, &samples0);
    mixer.write_to_source(id1, &samples1);

    // Mix output
    let mut out = vec![0i16; 64];
    mixer.mix_output(&mut out);

    // Each sample should be 100 + 200 = 300 (both at full volume 255)
    let mut sum_ok = true;
    for i in 0..64 {
        if out[i] != 300 {
            sum_ok = false;
            break;
        }
    }

    let count_ok = mixer.source_count() == 2;

    let passed = ids_ok && sum_ok && count_ok;
    StressResult {
        test_name: "Audio mixer: two sources",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("ids={} sum={} count={} sample0={}",
            ids_ok, sum_ok, count_ok, out[0]),
    }
}

/// Test 3: Audio mixer: volume scaling (w:10)
fn test_mixer_volume() -> StressResult {
    let mut mixer = AudioMixer::new();

    let id = mixer.add_source(AudioFormat::CD_MONO).unwrap_or(0);
    mixer.set_source_volume(id, 128); // ~50%

    let samples = vec![1000i16; 64];
    mixer.write_to_source(id, &samples);

    let mut out = vec![0i16; 64];
    mixer.mix_output(&mut out);

    // Expected: 1000 * 128 / 255 = 501 (integer truncation)
    let expected = (1000i32 * 128) / 255;
    let val = out[0] as i32;
    let scale_ok = (val - expected).abs() <= 1; // allow +-1 for rounding

    // Negative values should also scale correctly
    let neg_samples = vec![-1000i16; 64];
    let id2 = mixer.add_source(AudioFormat::CD_MONO).unwrap_or(0);
    mixer.set_source_volume(id2, 128);
    mixer.write_to_source(id2, &neg_samples);

    // Remove first source so we only get source 2
    mixer.remove_source(id);
    let mut out2 = vec![0i16; 64];
    mixer.mix_output(&mut out2);

    let neg_expected = (-1000i32 * 128) / 255;
    let neg_ok = (out2[0] as i32 - neg_expected).abs() <= 1;

    let passed = scale_ok && neg_ok;
    StressResult {
        test_name: "Audio mixer: volume scaling",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("val={} expected={} neg={} neg_exp={}",
            val, expected, out2[0], neg_expected),
    }
}

/// Test 4: Gamepad state buttons (w:10)
fn test_gamepad_buttons() -> StressResult {
    let mut pad = GamepadState::new();
    pad.connected = true;

    // Press A, B, START
    pad.press(ButtonFlags::A);
    pad.press(ButtonFlags::B);
    pad.press(ButtonFlags::START);

    let set_ok = pad.buttons == (ButtonFlags::A | ButtonFlags::B | ButtonFlags::START);
    let a_ok = pad.is_pressed(ButtonFlags::A);
    let b_ok = pad.is_pressed(ButtonFlags::B);
    let start_ok = pad.is_pressed(ButtonFlags::START);
    let x_not = !pad.is_pressed(ButtonFlags::X);

    // Release B
    pad.release(ButtonFlags::B);
    let release_ok = !pad.is_pressed(ButtonFlags::B);
    let a_still = pad.is_pressed(ButtonFlags::A);

    // Clear all
    pad.buttons = 0;
    let clear_ok = pad.buttons == 0;

    let passed = set_ok && a_ok && b_ok && start_ok && x_not && release_ok && a_still && clear_ok;
    StressResult {
        test_name: "Gamepad state buttons",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("set=0x{:04x} release={} clear={}",
            ButtonFlags::A | ButtonFlags::B | ButtonFlags::START,
            release_ok, clear_ok),
    }
}

/// Test 5: Gamepad keyboard mapping (w:10)
fn test_gamepad_keyboard() -> StressResult {
    let mut table = GamepadTable::new();
    table.connect(0);

    // Press W (0x11) -> left_y should go negative
    table.update_from_keyboard(0x11, true);
    let w_ok = match table.get(0) {
        Some(p) => p.axes.left_y == -32767,
        None => false,
    };

    // Release W -> left_y should return to 0
    table.update_from_keyboard(0x11, false);
    let w_release = match table.get(0) {
        Some(p) => p.axes.left_y == 0,
        None => false,
    };

    // Press Space (0x39) -> A button
    table.update_from_keyboard(0x39, true);
    let space_ok = match table.get(0) {
        Some(p) => p.is_pressed(ButtonFlags::A),
        None => false,
    };

    // Press D (0x20) -> left_x positive
    table.update_from_keyboard(0x20, true);
    let d_ok = match table.get(0) {
        Some(p) => p.axes.left_x == 32767,
        None => false,
    };

    // Release Space -> A cleared
    table.update_from_keyboard(0x39, false);
    let space_release = match table.get(0) {
        Some(p) => !p.is_pressed(ButtonFlags::A),
        None => false,
    };

    let passed = w_ok && w_release && space_ok && d_ok && space_release;
    StressResult {
        test_name: "Gamepad keyboard mapping",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("w={} w_rel={} space={} d={} sp_rel={}",
            w_ok, w_release, space_ok, d_ok, space_release),
    }
}

/// Test 6: Gamepad table 4-player (w:10)
fn test_gamepad_table() -> StressResult {
    let mut table = GamepadTable::new();

    // Connect all 4 slots
    let mut all_ok = true;
    for i in 0..4 {
        if table.connect(i).is_none() {
            all_ok = false;
        }
    }
    let count_4 = table.count() == 4;

    // Slot 4 should fail (out of bounds)
    let oob_ok = table.connect(4).is_none();

    // Disconnect slot 1
    let disc_ok = table.disconnect(1);
    let count_3 = table.count() == 3;

    // Slot 1 should be None
    let slot1_gone = table.get(1).is_none();

    // Slots 0, 2, 3 should still be connected
    let others_ok = table.get(0).is_some() && table.get(2).is_some() && table.get(3).is_some();

    // Reconnect slot 1
    let reconn = table.connect(1).is_some();
    let count_4_again = table.count() == 4;

    let passed = all_ok && count_4 && oob_ok && disc_ok && count_3 && slot1_gone && others_ok && reconn && count_4_again;
    StressResult {
        test_name: "Gamepad table 4-player",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("all={} c4={} disc={} c3={} reconn={} c4b={}",
            all_ok, count_4, disc_ok, count_3, reconn, count_4_again),
    }
}

/// Test 7: Stream header encode/decode roundtrip (w:10)
fn test_stream_header() -> StressResult {
    // Base header
    let hdr = StreamHeader {
        msg_type: StreamMsgType::VideoFrame as u8,
        sequence: 42,
        timestamp_ms: 123456,
        payload_len: 9600,
    };
    let encoded = encode_header(&hdr);
    let decoded = decode_header(&encoded);

    let base_ok = match decoded {
        Some(h) => h.msg_type == 1 && h.sequence == 42 && h.timestamp_ms == 123456 && h.payload_len == 9600,
        None => false,
    };

    // Video header
    let vh = VideoHeader {
        width: 1920,
        height: 1080,
        format: 0,
        flags: 0x01,
    };
    let v_enc = encode_video_header(&vh);
    let v_dec = decode_video_header(&v_enc);

    let video_ok = match v_dec {
        Some(h) => h.width == 1920 && h.height == 1080 && h.format == 0 && h.flags == 0x01,
        None => false,
    };

    // Input header
    let ih = InputHeader {
        gamepad_buttons: ButtonFlags::A | ButtonFlags::START,
        axes: [-32767, 0, 100, -100, 255, 0],
        keyboard_state: 0xDEAD,
    };
    let i_enc = encode_input(&ih);
    let i_dec = decode_input(&i_enc);

    let input_ok = match i_dec {
        Some(h) => h.gamepad_buttons == ih.gamepad_buttons
            && h.axes == ih.axes
            && h.keyboard_state == 0xDEAD,
        None => false,
    };

    // Short buffer should return None
    let short_ok = decode_header(&[0u8; 5]).is_none();

    let passed = base_ok && video_ok && input_ok && short_ok;
    StressResult {
        test_name: "Stream header encode/decode",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("base={} video={} input={} short={}",
            base_ok, video_ok, input_ok, short_ok),
    }
}

/// Test 8: Codec: RGB frame pixel access (w:10)
fn test_codec_rgb_frame() -> StressResult {
    let mut frame = VideoFrame::new(64, 64, PixelFormat::Rgb888);

    // Verify size
    let size_ok = frame.data.len() == 64 * 64 * 3;

    // Set pixel at (10, 20) to red
    frame.set_pixel(10, 20, 255, 0, 128);
    let (r, g, b) = frame.pixel_at(10, 20);
    let pixel_ok = r == 255 && g == 0 && b == 128;

    // Set pixel at (0, 0) to green
    frame.set_pixel(0, 0, 0, 255, 0);
    let (r2, g2, b2) = frame.pixel_at(0, 0);
    let green_ok = r2 == 0 && g2 == 255 && b2 == 0;

    // Out of bounds should return (0,0,0)
    let (r3, g3, b3) = frame.pixel_at(64, 64);
    let oob_ok = r3 == 0 && g3 == 0 && b3 == 0;

    // Surface packed format
    let packed = frame.to_surface_packed(10, 20);
    let packed_ok = packed == ((255u32 << 16) | (0u32 << 8) | 128u32);

    // RGBA format
    let mut frame_rgba = VideoFrame::new(8, 8, PixelFormat::Rgba8888);
    let rgba_size = frame_rgba.data.len() == 8 * 8 * 4;
    frame_rgba.set_pixel(1, 1, 100, 200, 50);
    let (rr, rg, rb) = frame_rgba.pixel_at(1, 1);
    let rgba_ok = rr == 100 && rg == 200 && rb == 50;

    let passed = size_ok && pixel_ok && green_ok && oob_ok && packed_ok && rgba_size && rgba_ok;
    StressResult {
        test_name: "Codec: RGB frame pixel access",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("size={} pixel={} green={} oob={} packed={} rgba={}",
            size_ok, pixel_ok, green_ok, oob_ok, packed_ok, rgba_ok),
    }
}

/// Test 9: Codec: RLE roundtrip (w:10)
fn test_codec_rle() -> StressResult {
    // Create data: 100x 0xAA, 50x 0xBB, 100x 0xCC
    let mut data = Vec::new();
    for _ in 0..100 { data.push(0xAA); }
    for _ in 0..50 { data.push(0xBB); }
    for _ in 0..100 { data.push(0xCC); }

    let original_len = data.len(); // 250

    let encoded = rle_encode(&data);
    let compressed = encoded.len() < original_len;

    let decoded = rle_decode(&encoded);
    let length_ok = decoded.len() == original_len;

    let mut match_ok = true;
    for i in 0..original_len {
        if decoded[i] != data[i] {
            match_ok = false;
            break;
        }
    }

    // Single-byte runs should also work
    let single = vec![1u8, 2, 3, 4, 5];
    let s_enc = rle_encode(&single);
    let s_dec = rle_decode(&s_enc);
    let single_ok = s_dec == single;

    // Empty input
    let empty = rle_encode(&[]);
    let empty_ok = empty.is_empty();

    let passed = compressed && length_ok && match_ok && single_ok && empty_ok;
    StressResult {
        test_name: "Codec: RLE roundtrip",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("compressed={}/{} match={} single={} empty={}",
            encoded.len(), original_len, match_ok, single_ok, empty_ok),
    }
}

/// Test 10: Stream client lifecycle (w:10)
fn test_client_lifecycle() -> StressResult {
    let config = StreamConfig {
        server_addr: [10, 0, 2, 2],
        port: 9000,
        target_fps: 30,
        audio_enabled: true,
    };

    let mut client = StreamClient::create(config);
    let disc_ok = client.state == StreamClientState::Disconnected;

    // Create a 64x64 RGB888 frame worth of data
    let frame_data = vec![0xFFu8; 64 * 64 * 3]; // white pixels
    let vh = VideoHeader {
        width: 64,
        height: 64,
        format: 0, // RGB888
        flags: 0,
    };

    client.process_video_frame(&vh, &frame_data);
    let recv_ok = client.stats.frames_received == 1;
    let stream_ok = client.state == StreamClientState::Streaming;
    let dim_ok = client.last_frame_width == 64 && client.last_frame_height == 64;

    // Process with short data — should increment dropped
    client.process_video_frame(&vh, &[0u8; 10]);
    let drop_ok = client.stats.frames_dropped == 1;

    // Send input
    let pad = GamepadState {
        buttons: ButtonFlags::A | ButtonFlags::B,
        axes: AxisState::new(),
        connected: true,
    };
    let input_bytes = client.send_input(&pad, 0x42);
    let input_ok = input_bytes.len() == 18;

    // Client table
    let mut table = StreamClientTable::new();
    let slot = table.create(config);
    let table_ok = slot.is_some();
    let count_1 = table.count() == 1;

    // Destroy
    client.destroy();
    let destroyed_ok = client.state == StreamClientState::Disconnected
        && client.stats.frames_received == 0;

    let passed = disc_ok && recv_ok && stream_ok && dim_ok && drop_ok
        && input_ok && table_ok && count_1 && destroyed_ok;
    StressResult {
        test_name: "Stream client lifecycle",
        passed,
        score: if passed { 100 } else { 0 },
        weight: 10,
        details: format!("disc={} recv={} stream={} drop={} input={} table={} destroy={}",
            disc_ok, recv_ok, stream_ok, drop_ok, input_ok, table_ok, destroyed_ok),
    }
}
