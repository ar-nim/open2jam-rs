//! Sound cache resource mapping sample IDs to pre-decoded `oddio::Frames`.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use log::{info, warn};
use oddio::Frames;

use crate::parsing::ojm::{self, SampleEntry, SampleMap};

// ---------------------------------------------------------------------------
// Sound cache
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct SoundCache {
    sounds: HashMap<u32, Arc<Frames<[f32; 2]>>>,
    source_path: String,
    loaded: bool,
}

use std::sync::Arc;

impl Default for SoundCache {
    fn default() -> Self {
        Self {
            sounds: HashMap::new(),
            source_path: String::new(),
            loaded: false,
        }
    }
}

impl SoundCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn get_sound(&self, sample_id: u32) -> Option<&Arc<Frames<[f32; 2]>>> {
        self.sounds.get(&sample_id)
    }

    pub fn len(&self) -> usize {
        self.sounds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sounds.is_empty()
    }

    pub fn populate_from_sample_map(&mut self, sample_map: SampleMap, source_path: &str) {
        self.source_path = source_path.to_string();
        let total = sample_map.len();
        let mut decoded = 0;
        let mut failed = 0;

        for (id, entry) in sample_map {
            match sample_entry_to_frames(&entry) {
                Ok(frames) => {
                    self.sounds.insert(id, frames);
                    decoded += 1;
                }
                Err(e) => {
                    warn!("Failed to decode sample {} ({}): {}", id, entry.name, e);
                    failed += 1;
                }
            }
        }

        self.loaded = true;
        info!(
            "SoundCache: {}/{} decoded ({} skipped) from {}",
            decoded, total, failed, source_path
        );
    }
}

// ---------------------------------------------------------------------------
// Decoders
// ---------------------------------------------------------------------------

/// Decode OGG Vorbis using lewton (tolerant of CRC errors).
fn decode_ogg(data: &[u8]) -> Result<Arc<Frames<[f32; 2]>>, DecodeError> {
    use lewton::inside_ogg::OggStreamReader;

    let cursor = Cursor::new(data.to_vec());
    let mut reader =
        OggStreamReader::new(cursor).map_err(|e| DecodeError::Lewton(format!("lewton open: {}", e)))?;

    let sample_rate = reader.ident_hdr.audio_sample_rate as u32;
    let channels = reader.ident_hdr.audio_channels as usize;

    let mut planar: Vec<Vec<i16>> = Vec::new();

    while let Ok(Some(packet)) = reader.read_dec_packet() {
        while planar.len() < packet.len() {
            planar.push(Vec::new());
        }
        for (ch_idx, ch_data) in packet.iter().enumerate() {
            planar[ch_idx].extend_from_slice(ch_data);
        }
    }

    if planar.is_empty() || planar[0].is_empty() {
        return Err(DecodeError::NoSamples);
    }

    let samples_per_ch = planar[0].len();
    let mut interleaved: Vec<[f32; 2]> = Vec::with_capacity(samples_per_ch);

    if channels <= 1 {
        for i in 0..samples_per_ch {
            let sample = planar[0][i] as f32 / 32767.0;
            interleaved.push([sample, sample]);
        }
    } else {
        let right = if planar.len() > 1 {
            &planar[1]
        } else {
            &planar[0]
        };
        for i in 0..samples_per_ch {
            let left = planar[0][i] as f32 / 32767.0;
            let right = if i < right.len() {
                right[i] as f32 / 32767.0
            } else {
                left
            };
            interleaved.push([left, right]);
        }
    }

    if interleaved.is_empty() {
        return Err(DecodeError::NoSamples);
    }

    Ok(Frames::from_slice(sample_rate, &interleaved))
}

/// Decode WAV using hound (pure Rust, no external deps).
fn decode_wav(data: &[u8]) -> Result<Arc<Frames<[f32; 2]>>, DecodeError> {
    let cursor = Cursor::new(data.to_vec());
    let reader = hound::WavReader::new(cursor)
        .map_err(|e| DecodeError::Hound(format!("hound open: {}", e)))?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as usize;

    let mut interleaved: Vec<[f32; 2]> = Vec::new();

    match spec.sample_format {
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let samples: Vec<i32> = reader.into_samples::<i32>().filter_map(|s| s.ok()).collect();
            let max_val = (1i64 << (bits - 1)) as f32;
            for chunk in samples.chunks(channels) {
                let left = if chunk.len() > 0 {
                    chunk[0] as f32 / max_val
                } else {
                    0.0
                };
                let right = if chunk.len() > 1 {
                    chunk[1] as f32 / max_val
                } else {
                    left
                };
                interleaved.push([left, right]);
            }
        }
        hound::SampleFormat::Float => {
            let samples: Vec<f32> = reader.into_samples::<f32>().filter_map(|s| s.ok()).collect();
            for chunk in samples.chunks(channels) {
                let left = if chunk.len() > 0 { chunk[0] } else { 0.0 };
                let right = if chunk.len() > 1 { chunk[1] } else { left };
                interleaved.push([left, right]);
            }
        }
    }

    if interleaved.is_empty() {
        return Err(DecodeError::NoSamples);
    }

    Ok(Frames::from_slice(sample_rate, &interleaved))
}

/// Decode a [`SampleEntry`] into stereo frames.
fn sample_entry_to_frames(entry: &SampleEntry) -> Result<Arc<Frames<[f32; 2]>>, DecodeError> {
    if entry.data.starts_with(b"OggS") || entry.extension == "ogg" {
        decode_ogg(&entry.data)
    } else if entry.data.starts_with(b"RIFF") || entry.extension == "wav" {
        decode_wav(&entry.data)
    } else {
        Err(DecodeError::UnknownFormat)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("lewton error: {0}")]
    Lewton(String),
    #[error("hound error: {0}")]
    Hound(String),
    #[error("no audio samples decoded")]
    NoSamples,
    #[error("unknown audio format")]
    UnknownFormat,
}
