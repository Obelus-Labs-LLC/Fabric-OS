//! Audio mixer and PCM buffer.
//!
//! Software audio subsystem with multi-source mixing, per-source
//! volume control, and ring-buffered PCM sample storage.
//! No hardware output in Phase 18 — mix_output writes to a
//! caller-provided slice. Future phases add AC'97/HDA DMA.

#![allow(dead_code)]

use spin::Mutex;
use crate::serial_println;

/// Maximum concurrent audio sources.
pub const MAX_SOURCES: usize = 8;

/// PCM ring buffer capacity in samples.
const PCM_BUFFER_SIZE: usize = 4096;

/// Audio format descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u8,
    pub bits_per_sample: u8,
}

impl AudioFormat {
    /// Standard CD quality: 44100 Hz, mono, 16-bit.
    pub const CD_MONO: AudioFormat = AudioFormat {
        sample_rate: 44100,
        channels: 1,
        bits_per_sample: 16,
    };

    /// Standard CD quality: 44100 Hz, stereo, 16-bit.
    pub const CD_STEREO: AudioFormat = AudioFormat {
        sample_rate: 44100,
        channels: 2,
        bits_per_sample: 16,
    };
}

/// Ring buffer for i16 PCM samples.
pub struct PcmBuffer {
    data: [i16; PCM_BUFFER_SIZE],
    read_pos: usize,
    write_pos: usize,
    len: usize,
}

impl PcmBuffer {
    /// Create an empty PCM buffer.
    pub const fn new() -> Self {
        PcmBuffer {
            data: [0i16; PCM_BUFFER_SIZE],
            read_pos: 0,
            write_pos: 0,
            len: 0,
        }
    }

    /// Number of samples available to read.
    pub fn available(&self) -> usize {
        self.len
    }

    /// Remaining capacity for writing.
    pub fn free_space(&self) -> usize {
        PCM_BUFFER_SIZE - self.len
    }

    /// Write samples into the buffer. Returns count actually written.
    pub fn write_samples(&mut self, samples: &[i16]) -> usize {
        let to_write = samples.len().min(self.free_space());
        for i in 0..to_write {
            self.data[self.write_pos] = samples[i];
            self.write_pos = (self.write_pos + 1) % PCM_BUFFER_SIZE;
        }
        self.len += to_write;
        to_write
    }

    /// Read samples from the buffer. Returns count actually read.
    pub fn read_samples(&mut self, out: &mut [i16]) -> usize {
        let to_read = out.len().min(self.len);
        for i in 0..to_read {
            out[i] = self.data[self.read_pos];
            self.read_pos = (self.read_pos + 1) % PCM_BUFFER_SIZE;
        }
        self.len -= to_read;
        to_read
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
        self.len = 0;
    }
}

/// A single audio source feeding into the mixer.
pub struct AudioSource {
    pub id: u32,
    pub format: AudioFormat,
    pub volume: u8,   // 0-255 (255 = full volume)
    pub buffer: PcmBuffer,
    pub active: bool,
}

/// Multi-source audio mixer.
pub struct AudioMixer {
    sources: [Option<AudioSource>; MAX_SOURCES],
    pub master_volume: u8,
    next_id: u32,
}

impl AudioMixer {
    /// Create a new mixer with no sources.
    pub const fn new() -> Self {
        const NONE: Option<AudioSource> = None;
        AudioMixer {
            sources: [NONE; MAX_SOURCES],
            master_volume: 255,
            next_id: 1,
        }
    }

    /// Add a new audio source. Returns source ID or None if full.
    pub fn add_source(&mut self, format: AudioFormat) -> Option<u32> {
        for slot in self.sources.iter_mut() {
            if slot.is_none() {
                let id = self.next_id;
                self.next_id += 1;
                *slot = Some(AudioSource {
                    id,
                    format,
                    volume: 255,
                    buffer: PcmBuffer::new(),
                    active: true,
                });
                return Some(id);
            }
        }
        None
    }

    /// Remove an audio source by ID.
    pub fn remove_source(&mut self, id: u32) -> bool {
        for slot in self.sources.iter_mut() {
            if let Some(ref src) = slot {
                if src.id == id {
                    *slot = None;
                    return true;
                }
            }
        }
        false
    }

    /// Write samples to a specific source. Returns count written.
    pub fn write_to_source(&mut self, id: u32, samples: &[i16]) -> usize {
        for slot in self.sources.iter_mut() {
            if let Some(ref mut src) = slot {
                if src.id == id {
                    return src.buffer.write_samples(samples);
                }
            }
        }
        0
    }

    /// Set volume for a specific source (0-255).
    pub fn set_source_volume(&mut self, id: u32, volume: u8) -> bool {
        for slot in self.sources.iter_mut() {
            if let Some(ref mut src) = slot {
                if src.id == id {
                    src.volume = volume;
                    return true;
                }
            }
        }
        false
    }

    /// Get the number of active sources.
    pub fn source_count(&self) -> usize {
        self.sources.iter().filter(|s| s.is_some()).count()
    }

    /// Mix all active sources into the output buffer.
    /// Reads from each source, applies per-source volume, sums,
    /// applies master volume, and clamps to i16 range.
    pub fn mix_output(&mut self, out: &mut [i16]) {
        // Zero the output
        for sample in out.iter_mut() {
            *sample = 0;
        }

        // Temporary buffer for reading from each source
        let mut tmp = [0i16; 256];
        let chunk = out.len().min(256);

        for slot in self.sources.iter_mut() {
            if let Some(ref mut src) = slot {
                if !src.active || src.buffer.available() == 0 {
                    continue;
                }
                let count = src.buffer.read_samples(&mut tmp[..chunk]);
                for i in 0..count {
                    // Per-source volume scaling (integer math, no FPU)
                    let scaled = (tmp[i] as i32 * src.volume as i32) / 255;
                    // Accumulate into output (i32 to avoid overflow)
                    let sum = out[i] as i32 + scaled;
                    out[i] = sum.max(i16::MIN as i32).min(i16::MAX as i32) as i16;
                }
            }
        }

        // Apply master volume
        if self.master_volume < 255 {
            for sample in out.iter_mut() {
                let scaled = (*sample as i32 * self.master_volume as i32) / 255;
                *sample = scaled as i16;
            }
        }
    }
}

/// Global audio mixer.
pub static AUDIO_MIXER: Mutex<AudioMixer> = Mutex::new(AudioMixer::new());

/// Initialize the audio subsystem.
pub fn init() {
    serial_println!("[AUDIO] Software PCM mixer initialized ({} source slots)", MAX_SOURCES);
}
