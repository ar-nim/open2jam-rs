//! OJN (O2Jam Note) chart file parser.
//!
//! Parses the 300-byte header and measure blocks from `.ojn` binary files.
//! Builds a velocity tree mapping measure+position to absolute time in milliseconds.

use std::collections::HashMap;
use std::path::Path;

use encoding_rs::EUC_KR;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const OJN_SIGNATURE: u32 = 0x006E_6A6F; // "ojn\0" little-endian
const HEADER_SIZE: usize = 300;
pub const CHART_PADDING_MS: f64 = 0.0;
const MEASURE_SIZE_FRACTION: f64 = 0.8; // 80% of viewport

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Chart difficulty levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Easy = 0,
    Normal = 1,
    Hard = 2,
}

/// Channel types in an OJN chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    TimeSignature,
    BpmChange,
    Note(u8),     // lanes 1-7
    AutoPlay(u8), // channels 9-15
}

impl Channel {
    pub fn from_number(n: u16) -> Self {
        match n {
            0 => Channel::TimeSignature,
            1 => Channel::BpmChange,
            2..=8 => Channel::Note((n - 1) as u8), // 1-based lane
            other => Channel::AutoPlay(other as u8),
        }
    }

    /// Returns the lane index (0-based) for note channels, None for non-note channels.
    pub fn lane_index(&self) -> Option<usize> {
        match self {
            Channel::Note(lane) => Some((*lane - 1) as usize),
            _ => None,
        }
    }
}

/// Note type flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteType {
    Tap,
    Hold,    // HEAD of a long note
    Release, // TAIL of a long note
}

/// A parsed note event from the chart.
#[derive(Debug, Clone)]
pub struct NoteEvent {
    pub time_ms: f64,
    pub channel: Channel,
    pub sample_id: Option<u32>,
    pub volume: f32,
    pub pan: f32, // -1.0 (left) to +1.0 (right)
    pub note_type: NoteType,
    pub measure: u32,
    pub position: f64, // 0.0 to <1.0 within the measure
    /// For long note HEADs: the time_ms of the matching TAIL event (after pairing pass).
    /// For TAILs: None.
    pub end_time_ms: Option<f64>,
}

impl NoteEvent {
    /// Returns true if this is a playable note (not BGM/AUTO_PLAY).
    pub fn is_note(&self) -> bool {
        matches!(self.channel, Channel::Note(_))
    }

    /// Returns true if this is the HEAD of a long note.
    pub fn is_long_note(&self) -> bool {
        matches!(self.note_type, NoteType::Hold)
    }

    /// Returns true if this is the TAIL (release) of a long note.
    pub fn is_release(&self) -> bool {
        matches!(self.note_type, NoteType::Release)
    }

    /// For long note HEADs, returns the paired TAIL time_ms.
    /// For TAILs and tap notes, returns None.
    pub fn end_time_ms(&self) -> Option<f64> {
        self.end_time_ms
    }
}

/// A BPM change event.
#[derive(Debug, Clone)]
pub struct BpmChangeEvent {
    pub time_ms: f64,
    pub bpm: f64,
    pub measure: u32,
    pub position: f64,
}

/// A measure marker event (for visual bar rendering).
#[derive(Debug, Clone)]
pub struct MeasureEvent {
    pub time_ms: f64,
    pub measure: u32,
}

/// All timed events for a chart.
#[derive(Debug, Clone)]
pub enum TimedEvent {
    Note(NoteEvent),
    BpmChange(BpmChangeEvent),
    Measure(MeasureEvent),
}

/// OJN header metadata.
#[derive(Debug, Clone)]
pub struct OjnHeader {
    pub song_id: u32,
    pub encode_version: f32,
    pub genre: u32,
    pub bpm: f32,
    pub level_easy: u16,
    pub level_normal: u16,
    pub level_hard: u16,
    pub event_count_easy: u32,
    pub event_count_normal: u32,
    pub event_count_hard: u32,
    pub note_count_easy: u32,
    pub note_count_normal: u32,
    pub note_count_hard: u32,
    pub measure_count_easy: u32,
    pub measure_count_normal: u32,
    pub measure_count_hard: u32,
    pub title: String,
    pub artist: String,
    pub noter: String,
    pub ojm_filename: String,
    pub bmp_size: u32,
    pub cover_size: u32,
    pub duration_easy: u32,
    pub duration_normal: u32,
    pub duration_hard: u32,
    pub note_offset_easy: u32,
    pub note_offset_normal: u32,
    pub note_offset_hard: u32,
    pub cover_offset: u32,
}

/// A complete parsed chart.
#[derive(Debug, Clone)]
pub struct Chart {
    pub header: OjnHeader,
    pub events: Vec<TimedEvent>,
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum OjnError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid signature: expected 0x{OJN_SIGNATURE:08X}, got 0x{0:08X}")]
    InvalidSignature(u32),
    #[error("truncated file: expected at least {expected} bytes, got {actual}")]
    Truncated { expected: usize, actual: usize },
    #[error("note data offset out of bounds: {offset} (file size: {size})")]
    NoteOffsetOutOfBounds { offset: u32, size: usize },
    #[error("no cover image in OJN file")]
    NoCoverImage,
}

/// Extract the cover JPEG image from OJN file bytes.
/// Returns the raw JPEG bytes if a cover image exists.
pub fn extract_cover_image(data: &[u8]) -> Result<Vec<u8>, OjnError> {
    let header = parse_header(data)?;
    if header.cover_offset == 0 || header.cover_size == 0 {
        return Err(OjnError::NoCoverImage);
    }
    let offset = header.cover_offset as usize;
    let size = header.cover_size as usize;
    if offset + size > data.len() {
        return Err(OjnError::Truncated {
            expected: offset + size,
            actual: data.len(),
        });
    }
    Ok(data[offset..offset + size].to_vec())
}

/// Extract the BMP thumbnail (typically 8×8) from OJN file bytes.
/// The BMP data starts right after the cover JPEG image.
/// Returns raw BMP bytes if a thumbnail exists.
pub fn extract_bmp_thumbnail(data: &[u8]) -> Result<Vec<u8>, OjnError> {
    let header = parse_header(data)?;
    if header.cover_offset == 0 || header.bmp_size == 0 {
        return Err(OjnError::NoCoverImage);
    }
    let offset = (header.cover_offset + header.cover_size) as usize;
    let size = header.bmp_size as usize;
    if offset + size > data.len() {
        return Err(OjnError::Truncated {
            expected: offset + size,
            actual: data.len(),
        });
    }
    Ok(data[offset..offset + size].to_vec())
}

/// Extract and decode the BMP thumbnail as RGBA pixels, ready for GPU upload.
/// Returns `(width, height, rgba_bytes)` or `None` if no thumbnail exists.
pub fn decode_bmp_thumbnail(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let bmp_bytes = extract_bmp_thumbnail(data).ok()?;
    decode_bmp(&bmp_bytes)
}

/// Decode raw BMP bytes into RGBA pixel data.
/// Supports both 24-bit BMP (no alpha) and 32-bit BMP (RGBA).
fn decode_bmp(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if data.len() < 54 {
        return None;
    }
    // BMP magic: 'B' 'M'
    if data[0] != b'B' || data[1] != b'M' {
        return None;
    }

    let data_offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
    let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]) as usize;
    let height_abs =
        (i32::from_le_bytes([data[22], data[23], data[24], data[25]])).unsigned_abs() as usize;
    let bpp = u16::from_le_bytes([data[28], data[29]]) as u32;

    if width == 0 || height_abs == 0 || data_offset >= data.len() {
        return None;
    }

    let pixel_data = &data[data_offset..];

    // BMP rows are bottom-to-top and padded to 4-byte boundaries.
    // Each row size = ceil(width * bpp / 8) rounded up to 4 bytes.
    let bytes_per_pixel = (bpp + 7) / 8;
    let row_stride = (width * bytes_per_pixel as usize + 3) & !3;

    if pixel_data.len() < row_stride * height_abs {
        return None;
    }

    let mut rgba = Vec::with_capacity(width * height_abs * 4);

    // Iterate rows bottom-to-top (BMP storage order), convert to RGBA
    for row in (0..height_abs).rev() {
        let row_start = row * row_stride;
        for col in 0..width {
            let px = row_start + col * bytes_per_pixel as usize;
            if px + 3 <= pixel_data.len() {
                match bpp {
                    24 => {
                        // BGR
                        rgba.push(pixel_data[px + 2]); // R
                        rgba.push(pixel_data[px + 1]); // G
                        rgba.push(pixel_data[px]); // B
                        rgba.push(255); // A
                    }
                    32 => {
                        // BGRA
                        rgba.push(pixel_data[px + 2]); // R
                        rgba.push(pixel_data[px + 1]); // G
                        rgba.push(pixel_data[px]); // B
                        rgba.push(pixel_data[px + 3]); // A
                    }
                    _ => return None,
                }
            }
        }
    }

    Some((width, height_abs, rgba))
}

// ---------------------------------------------------------------------------
// Decoding helpers
// ---------------------------------------------------------------------------

fn decode_c_string(bytes: &[u8]) -> String {
    let trimmed = bytes.split(|&b| b == 0).next().unwrap_or(b"");
    if trimmed.is_empty() {
        return String::new();
    }
    // Try EUC-KR first (Korean charts)
    let (decoded, _encoding, had_errors) = EUC_KR.decode(trimmed);
    if !had_errors {
        let s = decoded.into_owned();
        if !s.is_empty() {
            return s;
        }
    }
    // Fallback to UTF-8
    String::from_utf8_lossy(trimmed).into_owned()
}

fn decode_volume_pan(volume_pan: u8) -> (f32, f32) {
    let high_nibble = (volume_pan >> 4) & 0x0F;
    let low_nibble = volume_pan & 0x0F;

    // Volume: 1-15 normal, 0 = MAX (full volume = 1.0)
    let mut volume = high_nibble as f32 / 15.0;
    if volume == 0.0 {
        volume = 1.0;
    }

    // Pan: 0=center, 1=far left, 8=center, 15=far right
    let mut pan = low_nibble as i32;
    if pan == 0 {
        pan = 8;
    }
    let pan = (pan - 8) as f32 / 7.0; // -1.0 to +1.0

    (volume, pan)
}

fn decode_note_type_byte(type_byte: u8, value: u16) -> (u16, NoteType) {
    let mut adjusted_value = value;
    let type_value = type_byte % 4;

    // Check for long note tail (type_value > 3 before mod would mean bits 0-2 > 3)
    // Actually: if (type_byte % 8) > 3, value += 1000
    if (type_byte % 8) > 3 {
        adjusted_value += 1000;
    }

    let note_type = match type_value {
        0 => NoteType::Tap,
        1 => NoteType::Tap, // "W Normal" — treat as tap
        2 => NoteType::Hold,
        3 => NoteType::Release,
        _ => unreachable!(),
    };

    (adjusted_value, note_type)
}

// ---------------------------------------------------------------------------
// Public API — metadata only (fast scan)
// ---------------------------------------------------------------------------

/// Parse only the OJN header (metadata) without reading note events.
///
/// This is for song list scanning — much faster than full parsing
/// since it skips all measure blocks.
pub fn parse_metadata(path: impl AsRef<Path>) -> Result<OjnHeader, OjnError> {
    let data = std::fs::read(path)?;
    parse_metadata_bytes(&data)
}

/// Parse only the OJN header from raw bytes.
pub fn parse_metadata_bytes(data: &[u8]) -> Result<OjnHeader, OjnError> {
    if data.len() < HEADER_SIZE {
        return Err(OjnError::Truncated {
            expected: HEADER_SIZE,
            actual: data.len(),
        });
    }
    let signature = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if signature != OJN_SIGNATURE {
        return Err(OjnError::InvalidSignature(signature));
    }
    parse_header(data)
}

// ---------------------------------------------------------------------------
// Public API — full chart parsing
// ---------------------------------------------------------------------------

/// Parse an OJN file from disk.
pub fn parse_file(path: impl AsRef<Path>) -> Result<Chart, OjnError> {
    let data = std::fs::read(path)?;
    parse_bytes(&data)
}

/// Parse an OJN file from raw bytes.
pub fn parse_bytes(data: &[u8]) -> Result<Chart, OjnError> {
    if data.len() < HEADER_SIZE {
        return Err(OjnError::Truncated {
            expected: HEADER_SIZE,
            actual: data.len(),
        });
    }

    // Verify signature
    let signature = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if signature != OJN_SIGNATURE {
        return Err(OjnError::InvalidSignature(signature));
    }

    // Parse header
    let header = parse_header(data)?;

    // For now, parse only the Hard difficulty (most common for testing)
    // Full implementation would parse all difficulties
    let events = parse_difficulty_notes(data, &header, Difficulty::Hard)?;

    Ok(Chart { header, events })
}

// ---------------------------------------------------------------------------
// Header parsing
// ---------------------------------------------------------------------------

fn parse_header(data: &[u8]) -> Result<OjnHeader, OjnError> {
    Ok(OjnHeader {
        song_id: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        // signature at [4..8] already verified
        encode_version: f32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        genre: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        bpm: f32::from_le_bytes([data[16], data[17], data[18], data[19]]),
        level_easy: u16::from_le_bytes([data[20], data[21]]),
        level_normal: u16::from_le_bytes([data[22], data[23]]),
        level_hard: u16::from_le_bytes([data[24], data[25]]),
        event_count_easy: u32::from_le_bytes([data[28], data[29], data[30], data[31]]),
        event_count_normal: u32::from_le_bytes([data[32], data[33], data[34], data[35]]),
        event_count_hard: u32::from_le_bytes([data[36], data[37], data[38], data[39]]),
        note_count_easy: u32::from_le_bytes([data[40], data[41], data[42], data[43]]),
        note_count_normal: u32::from_le_bytes([data[44], data[45], data[46], data[47]]),
        note_count_hard: u32::from_le_bytes([data[48], data[49], data[50], data[51]]),
        measure_count_easy: u32::from_le_bytes([data[52], data[53], data[54], data[55]]),
        measure_count_normal: u32::from_le_bytes([data[56], data[57], data[58], data[59]]),
        measure_count_hard: u32::from_le_bytes([data[60], data[61], data[62], data[63]]),
        // [64..76] package counts (unused for now)
        // [76..80] old encode version, old song ID
        // [80..100] old genre
        bmp_size: u32::from_le_bytes([data[100], data[101], data[102], data[103]]),
        // [104..108] old file version
        title: decode_c_string(&data[108..172]),
        artist: decode_c_string(&data[172..204]),
        noter: decode_c_string(&data[204..236]),
        ojm_filename: decode_c_string(&data[236..268]),
        cover_size: u32::from_le_bytes([data[268], data[269], data[270], data[271]]),
        duration_easy: u32::from_le_bytes([data[272], data[273], data[274], data[275]]),
        duration_normal: u32::from_le_bytes([data[276], data[277], data[278], data[279]]),
        duration_hard: u32::from_le_bytes([data[280], data[281], data[282], data[283]]),
        note_offset_easy: u32::from_le_bytes([data[284], data[285], data[286], data[287]]),
        note_offset_normal: u32::from_le_bytes([data[288], data[289], data[290], data[291]]),
        note_offset_hard: u32::from_le_bytes([data[292], data[293], data[294], data[295]]),
        cover_offset: u32::from_le_bytes([data[296], data[297], data[298], data[299]]),
    })
}

// ---------------------------------------------------------------------------
// Note data parsing
// ---------------------------------------------------------------------------

fn parse_difficulty_notes(
    data: &[u8],
    header: &OjnHeader,
    difficulty: Difficulty,
) -> Result<Vec<TimedEvent>, OjnError> {
    let note_offset = match difficulty {
        Difficulty::Easy => header.note_offset_easy,
        Difficulty::Normal => header.note_offset_normal,
        Difficulty::Hard => header.note_offset_hard,
    };

    let cover_offset = header.cover_offset as usize;

    if note_offset as usize > data.len() {
        return Err(OjnError::NoteOffsetOutOfBounds {
            offset: note_offset,
            size: data.len(),
        });
    }

    // Parse all measure blocks from this difficulty's section
    let mut offset = note_offset as usize;
    let mut measure_blocks: Vec<(u32, u16, u16, Vec<[u8; 4]>)> = Vec::new();

    while offset + 8 <= cover_offset {
        // Check if we've run past this difficulty's data (heuristic: next section or cover)
        let next_offsets = [
            header.note_offset_easy,
            header.note_offset_normal,
            header.note_offset_hard,
        ];
        let section_end = next_offsets
            .iter()
            .filter(|&&o| o > note_offset)
            .copied()
            .min()
            .unwrap_or(header.cover_offset) as usize;

        if offset >= section_end {
            break;
        }

        let measure_num = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        let channel_num = u16::from_le_bytes([data[offset + 4], data[offset + 5]]);
        let events_count = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
        offset += 8;

        // Sanity check
        if events_count == 0 || offset + (events_count as usize) * 4 > section_end {
            // Could be padding or end of section
            break;
        }

        let mut events = Vec::with_capacity(events_count as usize);
        for _ in 0..events_count {
            let event_bytes = [
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ];
            events.push(event_bytes);
            offset += 4;
        }

        measure_blocks.push((measure_num, channel_num, events_count, events));
    }

    // Build the velocity tree and timed events
    build_timed_events(&measure_blocks, header.bpm as f64)
}

fn build_timed_events(
    measure_blocks: &[(u32, u16, u16, Vec<[u8; 4]>)],
    base_bpm: f64,
) -> Result<Vec<TimedEvent>, OjnError> {
    let mut events: Vec<TimedEvent> = Vec::new();

    // Group blocks by measure number
    let mut measures: HashMap<u32, Vec<(u16, u16, Vec<[u8; 4]>)>> = HashMap::new();
    for &(measure, channel, events_count, ref event_data) in measure_blocks {
        measures
            .entry(measure)
            .or_default()
            .push((channel, events_count, event_data.clone()));
    }

    // Sort measure numbers
    let mut sorted_measures: Vec<u32> = measures.keys().copied().collect();
    sorted_measures.sort();

    // Java-style incremental timing:
    // BEATS_PER_MSEC = 4 * 60 * 1000 = 240000
    // timer advances incrementally: delta_time = 240000 * delta_position / current_bpm
    const BEATS_PER_MSEC: f64 = 240000.0;

    let mut current_bpm = base_bpm;
    let mut frac_measure: f64 = 1.0; // fraction of full measure (set by TIME_SIGNATURE)
    let mut measure_pointer: f64 = 0.0; // current position within the measure
    let mut time_ms: f64 = CHART_PADDING_MS;
    let mut last_measure_num: u32 = 0;

    for &measure_num in sorted_measures.iter() {
        // Advance across skipped measure boundaries
        if measure_num > last_measure_num {
            for _ in 0..(measure_num - last_measure_num) {
                // Advance from current pointer to end of this measure
                time_ms += BEATS_PER_MSEC * (frac_measure - measure_pointer) / current_bpm;
                // Add measure marker
                events.push(TimedEvent::Measure(MeasureEvent {
                    time_ms,
                    measure: last_measure_num,
                }));
                // Reset for next measure
                frac_measure = 1.0;
                measure_pointer = 0.0;
                last_measure_num += 1;
            }
            if measure_num > last_measure_num {
                last_measure_num = measure_num;
            }
        }

        // Add measure marker for current measure (before processing events)
        if !events
            .iter()
            .any(|e| matches!(e, TimedEvent::Measure(m) if m.measure == measure_num))
        {
            events.push(TimedEvent::Measure(MeasureEvent {
                time_ms,
                measure: measure_num,
            }));
        }

        // Collect all events in this measure with their positions, then sort
        let mut sorted_events: Vec<(f64, Channel, [u8; 4])> = Vec::new();
        for &(channel_num, events_count, ref event_data) in &measures[&measure_num] {
            let channel = Channel::from_number(channel_num);
            for (i, event_bytes) in event_data.iter().enumerate() {
                let position = i as f64 / events_count as f64;
                sorted_events.push((position, channel, *event_bytes));
            }
        }
        // Sort by position within the measure
        sorted_events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        // Process events sequentially, advancing timer incrementally
        for (position, channel, event_bytes) in sorted_events {
            // Advance timer from current measure_pointer to this event's position
            let delta_position = position - measure_pointer;
            if delta_position > 0.0 {
                time_ms += BEATS_PER_MSEC * delta_position / current_bpm;
                measure_pointer = position;
            }

            match channel {
                Channel::TimeSignature => {
                    let value = f32::from_le_bytes(event_bytes) as f64;
                    if value > 0.0 {
                        frac_measure = value;
                    }
                }
                Channel::BpmChange => {
                    let bpm = f32::from_le_bytes(event_bytes) as f64;
                    if bpm > 0.0 {
                        current_bpm = bpm;
                    }
                    events.push(TimedEvent::BpmChange(BpmChangeEvent {
                        time_ms,
                        bpm: current_bpm,
                        measure: measure_num,
                        position,
                    }));
                }
                _ => {
                    // Note event
                    let raw_value = u16::from_le_bytes([event_bytes[0], event_bytes[1]]);
                    if raw_value == 0 {
                        continue; // Skip empty events
                    }

                    let volume_pan = event_bytes[2];
                    let type_byte = event_bytes[3];
                    let (adjusted_value, note_type) = decode_note_type_byte(type_byte, raw_value);
                    let (volume, pan) = decode_volume_pan(volume_pan);
                    let sample_id = if adjusted_value > 0 {
                        Some((adjusted_value - 1) as u32)
                    } else {
                        None
                    };

                    let note_event = NoteEvent {
                        time_ms,
                        channel,
                        sample_id,
                        volume,
                        pan,
                        note_type,
                        measure: measure_num,
                        position,
                        end_time_ms: None,
                    };
                    events.push(TimedEvent::Note(note_event));
                }
            }
        }
    }

    // Sort all events by time
    events.sort_by(|a, b| {
        let time_a = match a {
            TimedEvent::Note(n) => n.time_ms,
            TimedEvent::BpmChange(b) => b.time_ms,
            TimedEvent::Measure(m) => m.time_ms,
        };
        let time_b = match b {
            TimedEvent::Note(n) => n.time_ms,
            TimedEvent::BpmChange(b) => b.time_ms,
            TimedEvent::Measure(m) => m.time_ms,
        };
        time_a
            .partial_cmp(&time_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Pair long note HOLD/RELEASE events (second pass)
    pair_long_notes(&mut events);

    Ok(events)
}

/// Pair HOLD and RELEASE events per channel to populate end_time_ms for long note heads.
fn pair_long_notes(events: &mut [TimedEvent]) {
    // Track open long notes per note channel (lanes 1-7, index 0-6)
    // Stores the event index of the HOLD
    let mut ln_buffer: [Option<usize>; 7] = [None; 7];

    // First pass: collect all pairs (hold_idx, release_time)
    let mut pairs_to_update: Vec<(usize, f64)> = Vec::new();

    for (idx, event) in events.iter().enumerate() {
        if let TimedEvent::Note(note) = event {
            if let Some(lane) = note.channel.lane_index() {
                if lane < 7 {
                    if note.note_type == NoteType::Hold {
                        ln_buffer[lane] = Some(idx);
                    } else if note.note_type == NoteType::Release {
                        if let Some(hold_idx) = ln_buffer[lane].take() {
                            // Found matching pair: store for later update
                            pairs_to_update.push((hold_idx, note.time_ms));
                        }
                        // If no open long note, orphaned release — ignore
                    }
                }
            }
        }
    }

    // Second pass: apply updates (separate borrow)
    for (hold_idx, release_time) in pairs_to_update {
        if let TimedEvent::Note(ref mut hold_note) = events[hold_idx] {
            hold_note.end_time_ms = Some(release_time);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ojn_header() {
        let chart = parse_file("test_assets/o2ma100.ojn").expect("Failed to parse OJN file");
        let h = &chart.header;

        assert_eq!(h.song_id, 100);
        assert!(
            (h.bpm - 130.0).abs() < 1.0,
            "BPM should be ~130, got {}",
            h.bpm
        );
        assert_eq!(h.title, "Bach Alive");
        assert_eq!(h.artist, "Beautiful Day");
        assert_eq!(h.noter, "HWAN");
        assert!(h.ojm_filename.starts_with("o2ma100.ojm"));
        assert!(h.note_offset_hard > 0);
    }

    #[test]
    fn test_parse_ojn_signature() {
        let result = parse_bytes(&[0u8; HEADER_SIZE]);
        assert!(matches!(result, Err(OjnError::InvalidSignature(_))));
    }

    #[test]
    fn test_parse_ojn_truncated() {
        let result = parse_bytes(&[0u8; 100]);
        assert!(matches!(result, Err(OjnError::Truncated { .. })));
    }

    #[test]
    fn test_volume_pan_decoding() {
        // Center pan, full volume
        let (vol, pan) = decode_volume_pan(0x80); // high=8, low=0 -> pan=8 (center)
        assert!((vol - 8.0 / 15.0).abs() < 0.01);
        assert!(pan.abs() < 0.01); // center

        // Full volume (high nibble = 0)
        let (vol, _) = decode_volume_pan(0x08);
        assert!((vol - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_note_type_decoding() {
        // Tap note
        let (val, note_type) = decode_note_type_byte(0, 5);
        assert_eq!(val, 5);
        assert_eq!(note_type, NoteType::Tap);

        // Hold (long note head)
        let (_val, note_type) = decode_note_type_byte(2, 5);
        assert_eq!(note_type, NoteType::Hold);

        // Release (long note tail)
        let (_val, note_type) = decode_note_type_byte(3, 5);
        assert_eq!(note_type, NoteType::Release);
    }

    #[test]
    fn test_channel_from_number() {
        assert_eq!(Channel::from_number(0), Channel::TimeSignature);
        assert_eq!(Channel::from_number(1), Channel::BpmChange);
        assert_eq!(Channel::from_number(2), Channel::Note(1));
        assert_eq!(Channel::from_number(8), Channel::Note(7));
        assert!(matches!(Channel::from_number(9), Channel::AutoPlay(_)));
    }

    #[test]
    fn test_chart_has_events() {
        let chart = parse_file("test_assets/o2ma100.ojn").expect("Failed to parse OJN");
        let note_count = chart
            .events
            .iter()
            .filter(|e| matches!(e, TimedEvent::Note(_)))
            .count();
        assert!(
            note_count > 0,
            "Chart should have note events, got {}",
            note_count
        );

        // Print first 30 events to check ordering
        println!("First 30 events (sorted by time):");
        for (i, event) in chart.events.iter().enumerate().take(30) {
            match event {
                TimedEvent::Note(n) => {
                    println!(
                        "  [{}] Note at {:.1}ms, lane {:?}, type: {:?}, end_time: {:?}",
                        i, n.time_ms, n.channel, n.note_type, n.end_time_ms
                    );
                }
                TimedEvent::BpmChange(b) => {
                    println!("  [{}] BPM at {:.1}ms, bpm={}", i, b.time_ms, b.bpm);
                }
                TimedEvent::Measure(m) => {
                    println!("  [{}] Measure {} at {:.1}ms", i, m.measure, m.time_ms);
                }
            }
        }

        // Check unique measure numbers
        let measure_nums: Vec<u32> = chart
            .events
            .iter()
            .filter_map(|e| match e {
                TimedEvent::Measure(m) => Some(m.measure),
                _ => None,
            })
            .collect();
        println!("\nMeasure numbers in events: {:?}", measure_nums);

        // Check long notes
        let long_note_heads: Vec<_> = chart
            .events
            .iter()
            .filter_map(|e| match e {
                TimedEvent::Note(n) if n.note_type == NoteType::Hold => Some(n),
                _ => None,
            })
            .collect();

        println!("\nLong note heads: {}", long_note_heads.len());
        for ln in &long_note_heads {
            println!(
                "  HOLD at {:.1}ms, lane {:?}, end_time_ms: {:?}",
                ln.time_ms, ln.channel, ln.end_time_ms
            );
        }
    }

    #[test]
    fn test_chart_has_measure_markers() {
        let chart = parse_file("test_assets/o2ma100.ojn").expect("Failed to parse OJN");
        let measure_count = chart
            .events
            .iter()
            .filter(|e| matches!(e, TimedEvent::Measure(_)))
            .count();
        assert!(measure_count > 0, "Chart should have measure markers");
    }
}
