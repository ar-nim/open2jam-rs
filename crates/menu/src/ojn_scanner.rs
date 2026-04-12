//! OJN chart scanner — extracts metadata from OJN file headers for the song list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Metadata for a single chart (one difficulty of one song).
#[derive(Debug, Clone)]
pub struct ChartEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub genre: String,
    /// Base BPM.
    pub bpm: f32,
    /// Duration in seconds (from OJN header).
    pub duration_sec: f32,
    /// Note counts per difficulty: [easy, normal, hard].
    pub note_counts: [u32; 3],
    /// Level (difficulty rating) per difficulty: [easy, normal, hard].
    pub levels: [u16; 3],
    /// Number of keys (4–8).
    pub keys: u8,
    /// Cover image JPEG bytes (if extracted), or None.
    pub cover: Option<Vec<u8>>,
}

/// A song group: one logical song with multiple difficulties.
#[derive(Debug, Clone)]
pub struct SongEntry {
    pub title: String,
    pub artist: String,
    pub genre: String,
    pub bpm: f32,
    pub duration_sec: f32,
    pub keys: u8,
    /// Charts available: one per difficulty that exists.
    pub charts: Vec<ChartEntry>,
    /// Cover from the first chart that has one.
    pub cover: Option<Vec<u8>>,
    /// Maximum level across all difficulties.
    pub max_level: u16,
}

/// Scans a directory tree for OJN files and extracts metadata.
pub struct OjnScanner {
    chart_paths: Vec<PathBuf>,
}

impl OjnScanner {
    pub fn new() -> Self {
        Self {
            chart_paths: Vec::new(),
        }
    }

    /// Add a root directory to scan recursively.
    pub fn add_directory(&mut self, dir: &Path) -> std::io::Result<()> {
        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if let Some(ext) = entry.path().extension() {
                if ext.eq_ignore_ascii_case("ojn") {
                    self.chart_paths.push(entry.path().to_path_buf());
                }
            }
        }
        Ok(())
    }

    /// Scan all added directories and return grouped songs.
    pub fn scan(&self) -> Vec<SongEntry> {
        let mut charts: Vec<ChartEntry> = Vec::new();

        for path in &self.chart_paths {
            if let Ok(entry) = parse_ojn_header(path) {
                charts.push(entry);
            } else {
                log::warn!("Failed to parse OJN header: {}", path.display());
            }
        }

        // Group charts by song (title + artist + keys)
        let mut groups: HashMap<String, Vec<ChartEntry>> = HashMap::new();
        for chart in charts {
            let key = format!("{}|{}|{}", chart.title, chart.artist, chart.keys);
            groups.entry(key).or_default().push(chart);
        }

        groups
            .into_values()
            .map(|mut charts| {
                charts.sort_by_key(|c| {
                    // Sort by first non-zero level
                    c.levels.iter().find(|&&l| l > 0).copied().unwrap_or(0)
                });
                let first = charts.first().cloned().unwrap_or_else(|| ChartEntry {
                    path: PathBuf::new(),
                    title: String::new(),
                    artist: String::new(),
                    genre: String::new(),
                    bpm: 0.0,
                    duration_sec: 0.0,
                    note_counts: [0; 3],
                    levels: [0; 3],
                    keys: 0,
                    cover: None,
                });

                let cover = charts.iter().find_map(|c| c.cover.clone());
                let max_level = charts.iter().flat_map(|c| c.levels.iter()).copied().max().unwrap_or(0);

                SongEntry {
                    title: first.title,
                    artist: first.artist,
                    genre: first.genre,
                    bpm: first.bpm,
                    duration_sec: first.duration_sec,
                    keys: first.keys,
                    charts,
                    cover,
                    max_level,
                }
            })
            .collect()
    }
}

/// Parse an OJN file header without reading note events.
fn parse_ojn_header(path: &Path) -> std::io::Result<ChartEntry> {
    let data = std::fs::read(path)?;

    if data.len() < 300 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "OJN file too small",
        ));
    }

    // Check signature: "O2JN" = 0x4E4A324F (little-endian)
    let sig = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if sig != 0x4E4A324F {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid OJN signature",
        ));
    }

    let _version = f32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let _song_id = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let genre_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
    let bpm = f32::from_le_bytes([data[16], data[17], data[18], data[19]]);
    let level_easy = u16::from_le_bytes([data[20], data[21]]);
    let level_normal = u16::from_le_bytes([data[22], data[23]]);
    let level_hard = u16::from_le_bytes([data[24], data[25]]);
    let _event_count_easy = u32::from_le_bytes([data[26], data[27], data[28], data[29]]);
    let _event_count_normal = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);
    let _event_count_hard = u32::from_le_bytes([data[34], data[35], data[36], data[37]]);
    let note_count_easy = u32::from_le_bytes([data[38], data[39], data[40], data[41]]);
    let note_count_normal = u32::from_le_bytes([data[42], data[43], data[44], data[45]]);
    let note_count_hard = u32::from_le_bytes([data[46], data[47], data[48], data[49]]);
    let _measure_count_easy = u32::from_le_bytes([data[50], data[51], data[52], data[53]]);
    let _measure_count_normal = u32::from_le_bytes([data[54], data[55], data[56], data[57]]);
    let _measure_count_hard = u32::from_le_bytes([data[58], data[59], data[60], data[61]]);

    // Strings are null-terminated, max 32 bytes each
    let title = read_string(&data, 62, 32);
    let artist = read_string(&data, 94, 32);
    let _noter = read_string(&data, 126, 32);
    let _ojm_filename = read_string(&data, 158, 32);

    let cover_size = u32::from_le_bytes([data[190], data[191], data[192], data[193]]);
    let _duration_easy = u32::from_le_bytes([data[194], data[195], data[196], data[197]]);
    let _duration_normal = u32::from_le_bytes([data[198], data[199], data[200], data[201]]);
    let duration_hard = u32::from_le_bytes([data[202], data[203], data[204], data[205]]);
    let cover_offset = u32::from_le_bytes([data[266], data[267], data[268], data[269]]);

    // Extract cover image if present
    let cover = if cover_size > 0 && (cover_offset as usize + cover_size as usize) <= data.len() {
        Some(data[cover_offset as usize..(cover_offset + cover_size) as usize].to_vec())
    } else {
        None
    };

    // Determine number of keys by looking at channel usage in the file
    // For now, default to 7 (most common)
    let keys: u8 = 7;

    Ok(ChartEntry {
        path: path.to_path_buf(),
        title,
        artist,
        genre: genre_to_string(genre_id),
        bpm,
        duration_sec: (duration_hard as f32) / 1000.0,
        note_counts: [note_count_easy, note_count_normal, note_count_hard],
        levels: [level_easy, level_normal, level_hard],
        keys,
        cover,
    })
}

/// Read a null-terminated string from a fixed-width field.
fn read_string(data: &[u8], offset: usize, max_len: usize) -> String {
    let end = offset + max_len;
    if end > data.len() {
        return String::new();
    }
    let slice = &data[offset..end];
    let null_pos = slice.iter().position(|&b| b == 0).unwrap_or(max_len);
    // O2Jam uses EUC-KR encoding; fall back to lossy UTF-8 for now
    String::from_utf8_lossy(&slice[..null_pos]).into_owned()
}

/// Map genre ID to a human-readable name.
fn genre_to_string(id: u32) -> String {
    id.to_string()
}
