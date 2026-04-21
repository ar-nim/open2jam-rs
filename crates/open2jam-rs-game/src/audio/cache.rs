//! Sound cache resource mapping sample IDs to pre-decoded `oddio::Frames`.
//!
//! Optimized for O(1) lookup using a flat array indexed by sample_id.
//! OJN sample IDs are typically sequential (0-999 for WAV, 1000+ for OGG).

use std::io::Cursor;
use std::sync::Arc;

use log::{info, warn};
use oddio::Frames;

use open2jam_rs_parsers::ojm::{SampleEntry, SampleMap};

// ---------------------------------------------------------------------------
// Sound cache with flat array for O(1) lookup
// ---------------------------------------------------------------------------

/// Maximum sample ID we expect to encounter.
/// WAV samples: 0-999, OGG samples: 1000+
const MAX_SAMPLE_ID: usize = 4096;

#[derive(Debug)]
pub struct SoundCache {
    /// Flat array indexed by sample_id for O(1) lookup.
    /// None = not loaded, Some = loaded sample.
    sounds: Vec<Option<Arc<Frames<[f32; 2]>>>>,
    source_path: String,
    loaded: bool,
    /// Count of actually loaded samples (for len())
    loaded_count: usize,
}

impl Default for SoundCache {
    fn default() -> Self {
        Self {
            sounds: Vec::with_capacity(MAX_SAMPLE_ID),
            source_path: String::new(),
            loaded: false,
            loaded_count: 0,
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

    /// Get a sound by sample_id.
    /// O(1) lookup - no hashing, no pointer chasing.
    #[inline]
    pub fn get_sound(&self, sample_id: u32) -> Option<&Arc<Frames<[f32; 2]>>> {
        let idx = sample_id as usize;
        if idx < self.sounds.len() {
            self.sounds[idx].as_ref()
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.loaded_count
    }

    pub fn is_empty(&self) -> bool {
        self.loaded_count == 0
    }

    pub fn populate_from_sample_map(&mut self, sample_map: SampleMap, source_path: &str) {
        self.source_path = source_path.to_string();

        // Find the maximum sample_id to size our array
        let max_id = sample_map.keys().max().copied().unwrap_or(0) as usize;
        let size = (max_id + 1).max(MAX_SAMPLE_ID);

        // Pre-allocate with None slots
        self.sounds = vec![None; size];
        self.loaded_count = 0;

        let total = sample_map.len();
        let mut decoded = 0;
        let mut failed = 0;

        for (id, entry) in sample_map {
            let idx = id as usize;

            // Expand array if needed (unlikely but safe)
            if idx >= self.sounds.len() {
                self.sounds.resize(idx + 1, None);
            }

            match sample_entry_to_frames(&entry) {
                Ok(frames) => {
                    self.sounds[idx] = Some(frames);
                    self.loaded_count += 1;
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
            "SoundCache: {}/{} decoded ({} skipped) from {} (array size: {})",
            decoded,
            total,
            failed,
            source_path,
            self.sounds.len()
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
    let mut reader = OggStreamReader::new(cursor)
        .map_err(|e| DecodeError::Lewton(format!("lewton open: {}", e)))?;

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
            let samples: Vec<i32> = reader
                .into_samples::<i32>()
                .filter_map(|s| s.ok())
                .collect();
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
            let samples: Vec<f32> = reader
                .into_samples::<f32>()
                .filter_map(|s| s.ok())
                .collect();
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
