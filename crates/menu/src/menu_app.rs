//! Menu application using eframe (handles wgpu+winit automatically).
//!
//! Uses SQLite as a song metadata cache. On startup, songs are loaded from the
//! database on a background thread. Scanning a library directory runs in a
//! separate thread and reports progress via an `mpsc` channel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;

use anyhow::Result;
use open2jam_rs_core::game_options::{ChannelMod, VisibilityMod};
use open2jam_rs_core::Config;
use open2jam_rs_ojn::parse_metadata_bytes;
use sha2::{Digest, Sha256};

use crate::db::{self, CachedChart, ChartScanEntry, LibraryEntry};
use crate::panels::display_config::ui_display_config;
use crate::panels::key_bind_editor::{handle_key_capture, ui_key_bind_editor, KeyCaptureState};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How often to report scan progress (number of files between reports).
const SCAN_PROGRESS_INTERVAL: usize = 100;

/// OJN genre codes mapped to human-readable names.
const OJN_GENRE_NAMES: [&str; 11] = [
    "Ballad",      // 0
    "Rock",        // 1
    "Dance",       // 2
    "Techno",      // 3
    "Hip-hop",     // 4
    "Soul/R&B",    // 5
    "Jazz",        // 6
    "Funk",        // 7
    "Classical",   // 8
    "Traditional", // 9
    "Etc",         // 10
];

/// Convert an OJN genre ID to a display name.
fn genre_name(genre_id: u32) -> &'static str {
    OJN_GENRE_NAMES
        .get(genre_id as usize)
        .copied()
        .unwrap_or("Etc")
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// Messages sent from background threads to the egui main thread.
#[allow(dead_code)]
enum AppMessage {
    /// Initial song load completed.
    SongsLoaded(Vec<SongEntry>),
    /// Libraries loaded from the database.
    LibrariesLoaded(Vec<LibraryEntry>),
    /// Database pool is ready.
    PoolReady(sqlx::SqlitePool),
    /// Scan progress update.
    ScanProgress { scanned: usize },
    /// Scan completed with results.
    ScanComplete(Vec<SongEntry>),
    /// Cover image extracted on background thread (width, height, RGBA pixels).
    CoverLoaded {
        song_index: usize,
        cover: Option<(usize, usize, Vec<u8>)>,
    },
    /// An error occurred during scan or load.
    Error(String),
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Metadata for a single chart (one difficulty of one song).
#[derive(Debug, Clone)]
pub struct ChartEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub noter: String,
    pub genre: u32,
    pub bpm: f32,
    pub duration_sec: f32,
    pub note_counts: [u32; 3],
    pub levels: [u16; 3],
    pub keys: u8,
}

/// A song group: one logical song with multiple difficulties.
#[derive(Clone)]
pub struct SongEntry {
    pub title: String,
    pub artist: String,
    pub noter: String,
    pub genre: u32,
    pub bpm: f32,
    pub keys: u8,
    /// One ChartEntry per difficulty/arrangement that has content.
    pub charts: Vec<ChartEntry>,
    pub cover: Option<egui::TextureHandle>,
    /// Level for each difficulty: [Easy, Normal, Hard].
    pub levels: [u16; 3],
    /// Duration in seconds for each difficulty: [Easy, Normal, Hard].
    pub durations_sec: [f32; 3],
    /// Note counts for each difficulty: [Easy, Normal, Hard].
    pub note_counts: [u32; 3],
    /// Relative path to the .ojn file (for lazy cover extraction).
    pub relative_path: String,
    /// Library root path (combined with relative_path for cover extraction).
    pub library_root: String,
}

/// Tab index for the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MenuTab {
    #[default]
    MusicSelect = 0,
    Configuration = 1,
    Advanced = 2,
}

/// Sort column for the song list table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
enum SongSortColumn {
    #[default]
    Name,
    Artist,
    Level,
    Bpm,
    Genre,
    Duration,
}

/// A column in the song list table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SongColumn {
    Name,
    Artist,
    Level,
    Bpm,
    Duration,
    Genre,
}

impl SongColumn {
    fn header_label(&self) -> &'static str {
        match self {
            SongColumn::Name => "Name",
            SongColumn::Artist => "Artist",
            SongColumn::Level => "Level",
            SongColumn::Bpm => "BPM",
            SongColumn::Duration => "Length",
            SongColumn::Genre => "Genre",
        }
    }

    fn to_sort_column(&self) -> Option<SongSortColumn> {
        match self {
            SongColumn::Name => Some(SongSortColumn::Name),
            SongColumn::Artist => Some(SongSortColumn::Artist),
            SongColumn::Level => Some(SongSortColumn::Level),
            SongColumn::Bpm => Some(SongSortColumn::Bpm),
            SongColumn::Duration => Some(SongSortColumn::Duration),
            SongColumn::Genre => Some(SongSortColumn::Genre),
        }
    }
}

// ---------------------------------------------------------------------------
// MenuApp
// ---------------------------------------------------------------------------

/// The main menu application, implementing eframe::App.
pub struct MenuApp {
    config: Config,
    songs: Vec<SongEntry>,
    selected_song: Option<usize>,
    selected_difficulty: usize,
    search_query: String,

    // Library system
    db_pool: Option<sqlx::SqlitePool>,
    libraries: Vec<LibraryEntry>,
    selected_library: Option<usize>,

    // Async state
    loading: bool,
    scan_in_progress: bool,
    scan_progress_count: usize,
    scan_error: Option<String>,

    // Channel receiver (checked every frame)
    msg_rx: mpsc::Receiver<AppMessage>,
    // Channel sender (cloned into background threads)
    msg_tx: mpsc::Sender<AppMessage>,

    // Cover extraction: avoid spawning duplicate threads for the same song
    last_cover_requested_idx: Option<usize>,

    /// If set, only show songs with this genre ID.
    genre_filter: Option<u32>,

    active_tab: MenuTab,
    config_dirty: bool,
    last_save_time: Instant,
    sort_column: SongSortColumn,
    sort_ascending: bool,

    // Column visibility: defaults to Name, Level, BPM
    visible_columns: [bool; 6], // indexed by SongColumn discriminant

    // Cached sorted/filtered song list — rebuilt only when inputs change
    cached_sorted: Vec<usize>,
    cached_sort_col: SongSortColumn,
    cached_sort_asc: bool,
    cached_search_query: String,
    cached_genre_filter: Option<u32>,
    cached_song_count: usize,
    cached_difficulty: usize,

    // Key capture state for the keyboard configuration editor
    key_capture_state: KeyCaptureState,

    // Monitor resolution info (populated lazily on first config tab render)
    monitor_native_resolution: Option<(u32, u32)>,
}

impl MenuApp {
    pub fn new() -> Result<Self> {
        let config_path = Config::default_path();
        let config = Config::load(&config_path).unwrap_or_else(|e| {
            log::info!(
                "No config found at {:?}, using defaults: {}",
                config_path,
                e
            );
            Config::default()
        });

        let (tx, rx) = mpsc::channel();

        // Open DB pool and load songs on a background thread.
        // This keeps new() fast (<1ms) — egui shows "Loading..." on first frame.
        let load_tx = tx.clone();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    load_tx
                        .send(AppMessage::Error(format!("tokio runtime: {e}")))
                        .ok();
                    return;
                }
            };

            let db_path = Config::default_path()
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("songcache.db");

            match rt.block_on(db::open_pool(&db_path)) {
                Ok(pool) => {
                    // Clone pool for the UI thread (it's Arc-backed)
                    let ui_pool = pool.clone();
                    load_tx.send(AppMessage::PoolReady(pool)).ok();

                    // Load libraries
                    match rt.block_on(db::get_all_libraries(&ui_pool)) {
                        Ok(libs) => {
                            load_tx.send(AppMessage::LibrariesLoaded(libs)).ok();
                        }
                        Err(e) => {
                            load_tx
                                .send(AppMessage::Error(format!("load libraries: {e}")))
                                .ok();
                        }
                    }

                    // Load songs for last-opened library.
                    // If no library was previously opened, pick the most
                    // recently added one (highest id).
                    let libs = rt
                        .block_on(db::get_all_libraries(&ui_pool))
                        .unwrap_or_default();
                    let lib_id = config
                        .last_opened_library_id
                        .or_else(|| libs.last().map(|l| l.id as u64));

                    if let Some(id) = lib_id {
                        let lib_root = libs
                            .iter()
                            .find(|l| l.id == id as i64)
                            .map(|l| l.root_path.as_str())
                            .unwrap_or("")
                            .to_string();
                        match rt.block_on(db::get_charts_for_library(&ui_pool, id as i64)) {
                            Ok(charts) => {
                                let entries = group_charts_into_songs(&charts, &lib_root);
                                load_tx.send(AppMessage::SongsLoaded(entries)).ok();
                            }
                            Err(e) => {
                                load_tx
                                    .send(AppMessage::Error(format!("load charts: {e}")))
                                    .ok();
                            }
                        }
                    } else {
                        load_tx.send(AppMessage::SongsLoaded(Vec::new())).ok();
                    }

                    // Drop pool in background thread (no need to keep it alive here)
                    drop(ui_pool);
                }
                Err(e) => {
                    load_tx
                        .send(AppMessage::Error(format!("DB open failed: {e}")))
                        .ok();
                }
            }
        });

        Ok(Self {
            config,
            songs: Vec::new(),
            selected_song: None,
            selected_difficulty: 0,
            search_query: String::new(),
            db_pool: None,
            libraries: Vec::new(),
            selected_library: None,
            loading: true,
            scan_in_progress: false,
            scan_progress_count: 0,
            scan_error: None,
            msg_rx: rx,
            msg_tx: tx,
            last_cover_requested_idx: None,
            active_tab: MenuTab::MusicSelect,
            config_dirty: false,
            last_save_time: Instant::now(),
            sort_column: SongSortColumn::default(),
            sort_ascending: true,
            // Default visible columns: Name, Level, BPM
            visible_columns: [true, false, true, true, false, false],
            genre_filter: None,
            cached_sorted: Vec::new(),
            cached_sort_col: SongSortColumn::default(),
            cached_sort_asc: true,
            cached_search_query: String::new(),
            cached_genre_filter: None,
            cached_song_count: 0,
            cached_difficulty: 0,
            key_capture_state: KeyCaptureState::Idle,
            monitor_native_resolution: None,
        })
    }

    fn maybe_save_config(&mut self) {
        if !self.config_dirty {
            return;
        }
        if self.last_save_time.elapsed().as_millis() < 500 {
            return;
        }
        let config_path = Config::default_path();
        if let Err(e) = self.config.save(&config_path) {
            log::warn!("Failed to save config to {:?}: {}", config_path, e);
        } else {
            log::info!("Config saved to {:?}", config_path);
        }
        self.config_dirty = false;
        self.last_save_time = Instant::now();
    }

    fn mark_dirty(&mut self) {
        self.config_dirty = true;
        self.last_save_time = Instant::now();
    }

    /// Populate monitor resolution info from egui viewport data.
    /// Uses monitor_size (logical pixels) × native_pixels_per_point to get native resolution.
    fn ensure_monitor_info(&mut self, ctx: &egui::Context) {
        if self.monitor_native_resolution.is_some() {
            return;
        }
        ctx.input(|i| {
            let vp = i.viewport();
            if let Some(monitor_size) = vp.monitor_size {
                let ppp = vp.native_pixels_per_point.unwrap_or(1.0);
                let native_w = (monitor_size.x * ppp) as u32;
                let native_h = (monitor_size.y * ppp) as u32;
                self.monitor_native_resolution = Some((native_w, native_h));
                log::info!(
                    "Monitor native resolution: {}x{} (logical {}x{} @ {:.1}x)",
                    native_w,
                    native_h,
                    monitor_size.x,
                    monitor_size.y,
                    ppp
                );
            }
        });
    }

    /// Rebuild sorted index cache only when inputs changed. Returns cached indices.
    fn update_sorted_cache(&mut self) -> &[usize] {
        let needs_rebuild = self.cached_sort_col != self.sort_column
            || self.cached_sort_asc != self.sort_ascending
            || self.cached_search_query != self.search_query
            || self.cached_genre_filter != self.genre_filter
            || self.cached_song_count != self.songs.len()
            || self.cached_difficulty != self.selected_difficulty;

        if needs_rebuild {
            let diff_idx = self.selected_difficulty.min(2);
            let query_lower = self.search_query.to_lowercase();
            let mut filtered: Vec<usize> = self
                .songs
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    if let Some(genre_id) = self.genre_filter {
                        if s.genre != genre_id {
                            return false;
                        }
                    }
                    if self.search_query.is_empty() {
                        return true;
                    }
                    s.title.to_lowercase().contains(&query_lower)
                        || s.artist.to_lowercase().contains(&query_lower)
                })
                .map(|(i, _)| i)
                .collect();

            let col = self.sort_column;
            let asc = self.sort_ascending;
            filtered.sort_by(|&a, &b| {
                let sa = &self.songs[a];
                let sb = &self.songs[b];
                let ord = match col {
                    SongSortColumn::Name => sa.title.to_lowercase().cmp(&sb.title.to_lowercase()),
                    SongSortColumn::Artist => {
                        sa.artist.to_lowercase().cmp(&sb.artist.to_lowercase())
                    }
                    SongSortColumn::Level => sa.levels[diff_idx].cmp(&sb.levels[diff_idx]),
                    SongSortColumn::Bpm => sa
                        .bpm
                        .partial_cmp(&sb.bpm)
                        .unwrap_or(std::cmp::Ordering::Equal),
                    SongSortColumn::Genre => sa.genre.cmp(&sb.genre),
                    SongSortColumn::Duration => sa.durations_sec[diff_idx]
                        .partial_cmp(&sb.durations_sec[diff_idx])
                        .unwrap_or(std::cmp::Ordering::Equal),
                };
                if asc {
                    ord
                } else {
                    ord.reverse()
                }
            });

            self.cached_sorted = filtered;
            self.cached_sort_col = self.sort_column;
            self.cached_sort_asc = self.sort_ascending;
            self.cached_search_query = self.search_query.clone();
            self.cached_genre_filter = self.genre_filter;
            self.cached_song_count = self.songs.len();
            self.cached_difficulty = self.selected_difficulty;
        }

        &self.cached_sorted
    }

    fn play_selected_song(&self) {
        if let Some(idx) = self.selected_song {
            if let Some(song) = self.songs.get(idx) {
                if let Some(chart) = song.charts.get(self.selected_difficulty) {
                    log::info!("PLAY: {}", chart.path.display());
                    spawn_game(&chart.path, &self.config);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Scan flow
    // ------------------------------------------------------------------

    fn start_scan(&mut self, library_id: i64, root_path: String) {
        if self.scan_in_progress {
            return;
        }

        let Some(ref pool) = self.db_pool else {
            self.scan_error = Some("Database not yet initialised".to_string());
            return;
        };
        let pool = pool.clone();

        let tx = self.msg_tx.clone();
        self.scan_in_progress = true;
        self.scan_progress_count = 0;
        self.scan_error = None;

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tx.send(AppMessage::Error(format!("tokio runtime: {e}")))
                        .ok();
                    return;
                }
            };

            // Step 1: clear old cache
            if let Err(e) = rt.block_on(db::clear_cache(&pool, library_id)) {
                tx.send(AppMessage::Error(format!("clear cache: {e}"))).ok();
                return;
            }

            // Step 2: walk directory, collect scan entries
            let entries = walk_directory_for_ojn(&root_path, &tx);

            // Step 3: bulk insert
            if let Err(e) = rt.block_on(db::bulk_insert_charts(&pool, library_id, &entries)) {
                tx.send(AppMessage::Error(format!("bulk insert: {e}"))).ok();
                return;
            }

            // Step 4: update scan time
            if let Err(e) = rt.block_on(db::update_scan_time(&pool, library_id)) {
                tx.send(AppMessage::Error(format!("update scan time: {e}")))
                    .ok();
                return;
            }

            // Step 5: reload and send results
            match rt.block_on(db::get_charts_for_library(&pool, library_id)) {
                Ok(charts) => {
                    let songs = group_charts_into_songs(&charts, &root_path);
                    tx.send(AppMessage::ScanComplete(songs)).ok();
                }
                Err(e) => {
                    tx.send(AppMessage::Error(format!("reload charts: {e}")))
                        .ok();
                }
            }
        });
    }

    fn add_library_and_scan(&mut self, root_path: String) {
        let Some(ref pool) = self.db_pool else {
            return;
        };
        let pool = pool.clone();
        let tx = self.msg_tx.clone();

        self.scan_in_progress = true;
        self.scan_progress_count = 0;
        self.scan_error = None;

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tx.send(AppMessage::Error(format!("tokio runtime: {e}")))
                        .ok();
                    return;
                }
            };

            match rt.block_on(db::add_library(&pool, &root_path)) {
                Ok(lib) => {
                    let lib_id = lib.id;
                    // Reload libraries list
                    if let Ok(libs) = rt.block_on(db::get_all_libraries(&pool)) {
                        tx.send(AppMessage::LibrariesLoaded(libs)).ok();
                    }
                    // Clear old cache
                    if let Err(e) = rt.block_on(db::clear_cache(&pool, lib_id)) {
                        tx.send(AppMessage::Error(format!("clear cache: {e}"))).ok();
                        return;
                    }
                    // Walk directory and insert
                    let entries = walk_directory_for_ojn(&root_path, &tx);
                    if let Err(e) = rt.block_on(db::bulk_insert_charts(&pool, lib_id, &entries)) {
                        tx.send(AppMessage::Error(format!("bulk insert: {e}"))).ok();
                        return;
                    }
                    rt.block_on(db::update_scan_time(&pool, lib_id)).ok();
                    // Reload and send results
                    if let Ok(charts) = rt.block_on(db::get_charts_for_library(&pool, lib_id)) {
                        let songs = group_charts_into_songs(&charts, &root_path);
                        tx.send(AppMessage::ScanComplete(songs)).ok();
                    }
                }
                Err(e) => {
                    tx.send(AppMessage::Error(format!("add library: {e}"))).ok();
                }
            }
        });
    }

    fn remove_library(&mut self, library_id: i64) {
        let Some(ref pool) = self.db_pool else {
            return;
        };
        let pool = pool.clone();
        let tx = self.msg_tx.clone();

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tx.send(AppMessage::Error(format!("tokio runtime: {e}")))
                        .ok();
                    return;
                }
            };

            if let Err(e) = rt.block_on(db::delete_library(&pool, library_id)) {
                tx.send(AppMessage::Error(format!("delete library: {e}")))
                    .ok();
                return;
            }

            if let Ok(libs) = rt.block_on(db::get_all_libraries(&pool)) {
                tx.send(AppMessage::LibrariesLoaded(libs)).ok();
            }
            tx.send(AppMessage::SongsLoaded(Vec::new())).ok();
        });
    }

    #[allow(dead_code)]
    fn load_library_songs(&mut self, library_id: i64) {
        let Some(ref pool) = self.db_pool else {
            return;
        };
        let pool = pool.clone();
        let tx = self.msg_tx.clone();

        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tx.send(AppMessage::Error(format!("tokio runtime: {e}")))
                        .ok();
                    return;
                }
            };

            match rt.block_on(db::get_charts_for_library(&pool, library_id)) {
                Ok(charts) => {
                    // Look up the library root path
                    let lib_root = if let Ok(libs) = rt.block_on(db::get_all_libraries(&pool)) {
                        libs.iter()
                            .find(|l| l.id == library_id)
                            .map(|l| l.root_path.as_str())
                            .unwrap_or("")
                            .to_string()
                    } else {
                        String::new()
                    };
                    let songs = group_charts_into_songs(&charts, &lib_root);
                    tx.send(AppMessage::SongsLoaded(songs)).ok();
                }
                Err(e) => {
                    tx.send(AppMessage::Error(format!("load charts: {e}"))).ok();
                }
            }
        });
    }

    fn rescan_library(&mut self) {
        if let Some(idx) = self.selected_library {
            if let Some(lib) = self.libraries.get(idx) {
                self.start_scan(lib.id, lib.root_path.clone());
            }
        }
    }
}

impl eframe::App for MenuApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.maybe_save_config();
        self.ensure_monitor_info(ctx);

        // Drain messages from background threads (non-blocking)
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                AppMessage::SongsLoaded(entries) => {
                    self.songs = entries;
                    self.loading = false;
                    ctx.request_repaint();
                }
                AppMessage::LibrariesLoaded(libs) => {
                    let was_empty = self.libraries.is_empty();
                    self.libraries = libs;
                    // Auto-select the newest library when adding
                    if was_empty && !self.libraries.is_empty() {
                        self.selected_library = Some(self.libraries.len() - 1);
                    } else if self.scan_in_progress && !self.libraries.is_empty() {
                        // During add-library flow, select the last (newest) library
                        self.selected_library = Some(self.libraries.len() - 1);
                    }
                    ctx.request_repaint();
                }
                AppMessage::PoolReady(pool) => {
                    self.db_pool = Some(pool);
                    ctx.request_repaint();
                }
                AppMessage::ScanProgress { scanned } => {
                    self.scan_progress_count = scanned;
                    ctx.request_repaint();
                }
                AppMessage::ScanComplete(entries) => {
                    self.songs = entries;
                    self.scan_in_progress = false;
                    // Clear cover request tracker since songs list changed
                    self.last_cover_requested_idx = None;
                    // Reload library list to update last_scan
                    if let Some(ref pool) = self.db_pool {
                        let pool = pool.clone();
                        let tx = self.msg_tx.clone();
                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build();
                            if let Ok(rt) = rt {
                                if let Ok(libs) = rt.block_on(db::get_all_libraries(&pool)) {
                                    tx.send(AppMessage::LibrariesLoaded(libs)).ok();
                                }
                            }
                        });
                    }
                    ctx.request_repaint();
                }
                AppMessage::CoverLoaded { song_index, cover } => {
                    if let (Some((w, h, pixels)), Some(song)) =
                        (cover, self.songs.get_mut(song_index))
                    {
                        song.cover = Some(ctx.load_texture(
                            "song_cover",
                            egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels),
                            egui::TextureOptions::LINEAR,
                        ));
                        ctx.request_repaint();
                    }
                }
                AppMessage::Error(err) => {
                    self.scan_error = Some(err.clone());
                    self.loading = false;
                    self.scan_in_progress = false;
                    log::error!("Background error: {err}");
                    ctx.request_repaint();
                }
            }
        }

        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, MenuTab::MusicSelect, "Music Select");
                ui.selectable_value(
                    &mut self.active_tab,
                    MenuTab::Configuration,
                    "Configuration",
                );
                ui.selectable_value(&mut self.active_tab, MenuTab::Advanced, "Advanced");
            });
        });

        egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.separator();
                ui.checkbox(&mut self.config.game_options.autoplay, "Autoplay");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("💾 Save Config").clicked() {
                        self.config_dirty = true;
                        self.last_save_time =
                            Instant::now() - std::time::Duration::from_millis(600);
                        self.maybe_save_config();
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.active_tab {
            MenuTab::MusicSelect => self.ui_music_select(ui),
            MenuTab::Configuration => self.ui_configuration(ui, ctx),
            MenuTab::Advanced => self.ui_advanced(ui),
        });

        // Process keyboard events for key capture (only on Configuration tab)
        if self.active_tab == MenuTab::Configuration
            && matches!(self.key_capture_state, KeyCaptureState::Listening(_))
        {
            ctx.input(|i| {
                for event in &i.events {
                    if let egui::Event::Key {
                        key, pressed: true, ..
                    } = event
                    {
                        let key_text = key.name();
                        if !key_text.is_empty() {
                            let captured = handle_key_capture(
                                key_text,
                                &mut self.key_capture_state,
                                &mut self.config,
                            );
                            if captured {
                                self.mark_dirty();
                            }
                        }
                    }
                }
            });
        }
    }
}

impl MenuApp {
    fn ui_music_select(&mut self, ui: &mut egui::Ui) {
        // Split the screen in half: left = song list, right = song info
        let col_width = ui.available_width() / 2.0;
        ui.columns(2, |cols| {
            // ── Left: Song List ──
            cols[0].vertical(|ui| {
                ui.label(egui::RichText::new("Select Music").strong().heading());
                ui.separator();
                ui.add_space(10.0);

                // Library selector
                ui.horizontal(|ui| {
                    ui.label("Library:");
                    let lib_names: Vec<&str> =
                        self.libraries.iter().map(|l| l.name.as_str()).collect();
                    let current_sel = self.selected_library.map(|i| i as i32).unwrap_or(-1);
                    let mut new_sel = current_sel;
                    egui::ComboBox::from_id_salt("library_select")
                        .selected_text(if new_sel >= 0 {
                            lib_names.get(new_sel as usize).unwrap_or(&"")
                        } else {
                            "(none)"
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut new_sel, -1, "(none)");
                            for (i, name) in lib_names.iter().enumerate() {
                                ui.selectable_value(&mut new_sel, i as i32, *name);
                            }
                        });
                    if new_sel != current_sel {
                        self.selected_library = if new_sel >= 0 {
                            Some(new_sel as usize)
                        } else {
                            None
                        };
                        if let Some(idx) = self.selected_library {
                            if let Some(lib) = self.libraries.get(idx) {
                                self.config.last_opened_library_id = Some(lib.id as u64);
                                self.mark_dirty();
                            }
                        }
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button("📁 Add Library").clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            if let Some(path_str) = path.to_str() {
                                self.add_library_and_scan(path_str.to_string());
                            }
                        }
                    }
                    if ui.button("🗑 Remove").clicked() {
                        if let Some(idx) = self.selected_library {
                            if let Some(lib) = self.libraries.get(idx) {
                                let lib_id = lib.id;
                                self.remove_library(lib_id);
                            }
                        }
                    }
                    if ui.button("🔄 Rescan").clicked() && self.selected_library.is_some() {
                        self.rescan_library();
                    }
                });

                if self.loading {
                    ui.spinner();
                    ui.label("Loading song library...");
                } else if let Some(idx) = self.selected_library {
                    if let Some(lib) = self.libraries.get(idx) {
                        if let Some(last) = lib.last_scan {
                            let ago = format_timestamp_age(last);
                            ui.label(format!("📚 {} — last scan: {}", lib.name, ago));
                        } else {
                            ui.label(format!("📚 {} — never scanned", lib.name));
                        }
                    }
                } else {
                    ui.label("No library selected");
                }

                if self.scan_in_progress {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!(
                            "Scanning... {} files processed",
                            self.scan_progress_count
                        ));
                    });
                }

                if let Some(ref err) = self.scan_error {
                    ui.label(egui::RichText::new(format!("⚠ {err}")).color(egui::Color32::RED));
                }

                ui.label(format!("🎵 {} songs", self.songs.len()));
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    ui.text_edit_singleline(&mut self.search_query);
                    if !self.search_query.is_empty() && ui.small_button("✖").clicked() {
                        self.search_query.clear();
                    }
                });

                // Genre filter dropdown
                let genre_label = self
                    .genre_filter
                    .map(|id| genre_name(id))
                    .unwrap_or("All genres");
                ui.horizontal(|ui| {
                    ui.label("Genre:");
                    egui::ComboBox::from_id_salt("genre_filter")
                        .selected_text(genre_label)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(self.genre_filter.is_none(), "All genres")
                                .clicked()
                            {
                                self.genre_filter = None;
                            }
                            for gid in 0..=10u32 {
                                let name = genre_name(gid);
                                if ui
                                    .selectable_label(self.genre_filter == Some(gid), name)
                                    .clicked()
                                {
                                    self.genre_filter = Some(gid);
                                }
                            }
                        });
                    if self.genre_filter.is_some() && ui.small_button("✖").clicked() {
                        self.genre_filter = None;
                    }
                });
                ui.separator();

                // Column visibility toggle
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Columns:").size(10.0));
                    let all_cols = [
                        SongColumn::Name,
                        SongColumn::Artist,
                        SongColumn::Level,
                        SongColumn::Bpm,
                        SongColumn::Duration,
                        SongColumn::Genre,
                    ];
                    for col in &all_cols {
                        let idx = *col as usize;
                        let vis = &mut self.visible_columns[idx];
                        if ui.selectable_label(*vis, col.header_label()).clicked() {
                            *vis = !*vis;
                            if !self.visible_columns.iter().any(|v| *v) {
                                self.visible_columns[idx] = true;
                            }
                        }
                    }
                });
                ui.separator();

                // ── Song list table (sticky header + scrollable body) ──
                {
                    // Update cache (only rebuilds when inputs change), then copy for use in closure
                    let sorted_indices: Vec<usize> = self.update_sorted_cache().to_vec();
                    let mut sel = self.selected_song;
                    let mut sd = self.selected_difficulty;
                    let di = self.selected_difficulty.min(2);

                    let all_cols = [
                        SongColumn::Name,
                        SongColumn::Artist,
                        SongColumn::Level,
                        SongColumn::Bpm,
                        SongColumn::Duration,
                        SongColumn::Genre,
                    ];
                    let vc = self.visible_columns;
                    let visible: Vec<_> =
                        all_cols.into_iter().filter(|c| vc[*c as usize]).collect();

                    let cur_col = self.sort_column;
                    let cur_asc = self.sort_ascending;
                    let mut clicked_sort: Option<SongSortColumn> = None;

                    // Build column list for TableBuilder — first col gets remainder, rest are initial
                    let mut builder = egui_extras::TableBuilder::new(ui)
                        .id_salt("song_table")
                        .striped(true);
                    for (i, _col) in visible.iter().enumerate() {
                        if i == 0 {
                            builder = builder.column(egui_extras::Column::remainder());
                        } else {
                            builder =
                                builder.column(egui_extras::Column::initial(80.0).resizable(true));
                        }
                    }

                    // Header row — .header() returns Table (not TableBuilder)
                    let table = builder.header(20.0, |mut header| {
                        for col in &visible {
                            header.col(|ui| {
                                if let Some(sort_col) = col.to_sort_column() {
                                    let is_active = cur_col == sort_col;
                                    let arrow = if is_active {
                                        if cur_asc {
                                            " ^"
                                        } else {
                                            " v"
                                        }
                                    } else {
                                        ""
                                    };
                                    if ui
                                        .selectable_label(
                                            is_active,
                                            format!("{}{}", col.header_label(), arrow),
                                        )
                                        .clicked()
                                    {
                                        clicked_sort = Some(sort_col);
                                    }
                                } else {
                                    ui.label(col.header_label());
                                }
                            });
                        }
                    });

                    // Body rows — .rows() virtualizes: only visible rows are rendered
                    table.body(|body| {
                        body.rows(18.0, sorted_indices.len(), |mut row| {
                            let orig_idx = sorted_indices[row.index()];
                            let is_sel = sel == Some(orig_idx);
                            row.set_selected(is_sel);
                            for col in &visible {
                                row.col(|ui| match col {
                                    SongColumn::Name => {
                                        if ui
                                            .selectable_label(is_sel, &self.songs[orig_idx].title)
                                            .clicked()
                                        {
                                            sel = Some(orig_idx);
                                            sd = 0;
                                        }
                                    }
                                    SongColumn::Artist => {
                                        if ui
                                            .selectable_label(is_sel, &self.songs[orig_idx].artist)
                                            .clicked()
                                        {
                                            sel = Some(orig_idx);
                                            sd = 0;
                                        }
                                    }
                                    SongColumn::Level => {
                                        let text = self.songs[orig_idx].levels[di].to_string();
                                        if ui.selectable_label(is_sel, &text).clicked() {
                                            sel = Some(orig_idx);
                                            sd = 0;
                                        }
                                    }
                                    SongColumn::Bpm => {
                                        ui.label(format!("{:.1}", self.songs[orig_idx].bpm));
                                    }
                                    SongColumn::Duration => {
                                        let d = self.songs[orig_idx].durations_sec[di];
                                        ui.label(format!("{}:{:02}", d as u32 / 60, d as u32 % 60));
                                    }
                                    SongColumn::Genre => {
                                        if self.songs[orig_idx].genre == 0 {
                                            ui.label("");
                                        } else {
                                            ui.label(genre_name(self.songs[orig_idx].genre));
                                        }
                                    }
                                });
                            }
                        });
                    });

                    // Apply sort click
                    if let Some(sc) = clicked_sort {
                        if cur_col == sc {
                            self.sort_ascending = !cur_asc;
                        } else {
                            self.sort_column = sc;
                            self.sort_ascending = true;
                        }
                    }
                    self.selected_song = sel;
                    self.selected_difficulty = sd;
                }

                // Lazy cover extraction on background thread when selection changes
                if let Some(idx) = self.selected_song {
                    if self.last_cover_requested_idx == Some(idx) {
                        // Already requested extraction for this song
                    } else if let Some(song) = self.songs.get(idx) {
                        if song.cover.is_none()
                            && !song.library_root.is_empty()
                            && !song.relative_path.is_empty()
                        {
                            self.last_cover_requested_idx = Some(idx);
                            let root = song.library_root.clone();
                            let rel = song.relative_path.clone();
                            let tx = self.msg_tx.clone();
                            std::thread::spawn(move || {
                                let cover = std::fs::read(Path::new(&root).join(&rel))
                                    .ok()
                                    .and_then(|data| open2jam_rs_ojn::decode_bmp_thumbnail(&data));
                                tx.send(AppMessage::CoverLoaded {
                                    song_index: idx,
                                    cover,
                                })
                                .ok();
                            });
                        }
                    }
                }
            });

            // ── Right: Song Info + Options ──
            cols[1].vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("song_info_scroll")
                    .show(ui, |ui| {
                        ui.set_max_width(col_width - 16.0);

                        // Compute selected song data here (inside the right column)
                        // to avoid borrow conflicts with the Grid in the left column.
                        let selected_song_data: Option<SongEntry> = match self.selected_song {
                            Some(idx) => self.songs.get(idx).cloned(),
                            None => None,
                        };

                        // ── Song Info ──
                        ui.label(egui::RichText::new("Song Info").strong().heading());
                        ui.separator();
                        ui.add_space(10.0);
                        if let Some(song) = &selected_song_data {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.group(|ui| {
                                        if let Some(ref texture) = song.cover {
                                            ui.image(egui::load::SizedTexture::new(
                                                texture,
                                                egui::vec2(100.0, 100.0),
                                            ));
                                        } else {
                                            ui.label("[No cover]");
                                            ui.allocate_space(egui::vec2(100.0, 100.0));
                                        }
                                    });
                                });
                                ui.vertical(|ui| {
                                    ui.heading(&song.title);
                                    ui.label(format!("Artist: {}", song.artist));
                                    ui.label(format!("Notecharter: {}", song.noter));
                                    ui.label(format!("BPM: {:.1}", song.bpm));
                                    let dur = song.durations_sec[self.selected_difficulty.min(2)];
                                    ui.label(format!(
                                        "Length: {}:{:02}",
                                        dur as u32 / 60,
                                        dur as u32 % 60
                                    ));
                                    if song.genre != 0 {
                                        ui.label(format!("Genre: {}", genre_name(song.genre)));
                                    }
                                });
                            });
                        } else {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.group(|ui| {
                                        ui.label("[No cover]");
                                        ui.allocate_space(egui::vec2(100.0, 100.0));
                                    });
                                });
                                ui.vertical(|ui| {
                                    ui.heading("No song selected");
                                    ui.label("Artist: ");
                                    ui.label("Notecharter: ");
                                    ui.label("BPM: -");
                                    ui.label("Length: -:--");
                                    ui.label("");
                                });
                            });
                        }
                        ui.add_space(10.0);
                        // ── Difficulty ──
                        ui.label(egui::RichText::new("Difficulty").strong().heading());
                        ui.separator();
                        if let Some(song) = &selected_song_data {
                            let dn = ["Easy", "Normal", "Hard"];
                            egui::Grid::new("diff_grid").striped(true).show(ui, |ui| {
                                for i in 0..3usize {
                                    let level = song.levels[i];
                                    let notes = song.note_counts[i];
                                    if level == 0 && notes == 0 {
                                        continue;
                                    }
                                    let is_selected = self.selected_difficulty == i;
                                    if ui
                                        .selectable_label(
                                            is_selected,
                                            format!("{} [{}]", dn[i], level),
                                        )
                                        .clicked()
                                    {
                                        self.selected_difficulty = i;
                                        self.config.game_options.difficulty = match i {
                                            0 => open2jam_rs_core::Difficulty::Easy,
                                            1 => open2jam_rs_core::Difficulty::Normal,
                                            _ => open2jam_rs_core::Difficulty::Hard,
                                        };
                                        self.mark_dirty();
                                    }
                                    ui.label(format!("Total Notes: {}", notes));
                                    ui.end_row();
                                }
                            });
                        } else {
                            egui::Grid::new("diff_grid").striped(true).show(ui, |ui| {
                                for diff in ["Easy", "Normal", "Hard"] {
                                    let _ = ui.selectable_label(false, format!("{} [-]", diff));
                                    ui.label("Total Notes: -");
                                    ui.end_row();
                                }
                            });
                        }

                        // ── Game Options ──
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new("Game Options").strong().heading());
                        ui.separator();

                        // Speed
                        ui.label(egui::RichText::new("Speed").strong().size(14.0));
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            if ui.button("−").clicked() {
                                self.config.game_options.speed_multiplier =
                                    (self.config.game_options.speed_multiplier - 0.5).max(0.5);
                                self.mark_dirty();
                            }
                            ui.add(
                                egui::DragValue::new(
                                    &mut self.config.game_options.speed_multiplier,
                                )
                                .speed(0.5)
                                .clamp_range(0.5..=10.0)
                                .custom_formatter(|n, _| format!("{:.1}", n))
                                .custom_parser(|s| s.parse::<f64>().ok()),
                            );
                            if ui.button("+").clicked() {
                                self.config.game_options.speed_multiplier =
                                    (self.config.game_options.speed_multiplier + 0.5).min(10.0);
                                self.mark_dirty();
                            }
                        });

                        ui.add_space(5.0);

                        // Arrangement Mods
                        ui.label(egui::RichText::new("Arrangement Mods").strong().size(14.0));
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            let mut clicked = false;
                            for mod_val in [
                                ChannelMod::None,
                                ChannelMod::Random,
                                ChannelMod::Panic,
                                ChannelMod::Mirror,
                            ] {
                                let is_selected =
                                    self.config.game_options.channel_modifier == mod_val;
                                if ui
                                    .selectable_label(is_selected, mod_val.to_string())
                                    .clicked()
                                {
                                    self.config.game_options.channel_modifier = mod_val;
                                    clicked = true;
                                }
                            }
                            if clicked {
                                self.mark_dirty();
                            }
                        });
                        ui.add_space(5.0);

                        // Visibility Mods
                        ui.label(egui::RichText::new("Visibility Mods").strong().size(14.0));
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            let mut clicked = false;
                            for mod_val in [
                                VisibilityMod::None,
                                VisibilityMod::Hidden,
                                VisibilityMod::Sudden,
                                VisibilityMod::Dark,
                            ] {
                                let is_selected =
                                    self.config.game_options.visibility_modifier == mod_val;
                                if ui
                                    .selectable_label(is_selected, mod_val.to_string())
                                    .clicked()
                                {
                                    self.config.game_options.visibility_modifier = mod_val;
                                    clicked = true;
                                }
                            }
                            if clicked {
                                self.mark_dirty();
                            }
                        });
                        ui.add_space(10.0);
                        // Play Button — fills full width, larger text, centered
                        let can_play =
                            self.active_tab == MenuTab::MusicSelect && self.selected_song.is_some();
                        let mut btn_rect = ui.available_rect_before_wrap();
                        btn_rect.max.y = btn_rect.min.y + 40.0;
                        let btn_id = ui.make_persistent_id("start_btn");
                        let btn_response = ui.interact(btn_rect, btn_id, egui::Sense::click());
                        let is_hovered = btn_response.hovered() && can_play;
                        let fill_color = if can_play {
                            if is_hovered {
                                egui::Color32::from_rgb(100, 150, 255)
                            } else {
                                egui::Color32::BLUE
                            }
                        } else {
                            ui.style().visuals.widgets.inactive.bg_fill
                        };
                        let text_color = if can_play {
                            egui::Color32::WHITE
                        } else {
                            ui.style().visuals.text_color()
                        };
                        ui.painter().rect_filled(btn_rect, 4.0, fill_color);
                        ui.painter().text(
                            btn_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "▶ START GAME",
                            egui::TextStyle::Button.resolve(ui.style()),
                            text_color,
                        );
                        if btn_response.clicked() && can_play {
                            self.play_selected_song();
                        }
                    });
            });
        });
    }

    fn ui_configuration(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Ensure monitor info is populated before rendering display config
        self.ensure_monitor_info(ctx);
        let native_res = self.monitor_native_resolution;

        egui::ScrollArea::vertical()
            .id_salt("config_scroll")
            .show(ui, |ui| {
                ui.group(|ui| {
                    ui_key_bind_editor(ui, &mut self.config, &mut self.key_capture_state);
                });
                self.mark_dirty();
                ui.separator();
                ui.group(|ui| {
                    ui_display_config(ui, &mut self.config.game_options, native_res);
                });
                self.mark_dirty();
                ui.separator();
                ui.group(|ui| {
                    ui.label(egui::RichText::new("GUI Settings").strong());
                    ui.horizontal(|ui| {
                        ui.label("Theme:");
                        egui::ComboBox::from_id_salt("ui_theme")
                            .selected_text(self.config.game_options.ui_theme.to_string())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.config.game_options.ui_theme,
                                    open2jam_rs_core::UiTheme::Automatic,
                                    "Automatic",
                                );
                                ui.selectable_value(
                                    &mut self.config.game_options.ui_theme,
                                    open2jam_rs_core::UiTheme::Light,
                                    "Light",
                                );
                                ui.selectable_value(
                                    &mut self.config.game_options.ui_theme,
                                    open2jam_rs_core::UiTheme::Dark,
                                    "Dark",
                                );
                            });
                    });
                });
                self.mark_dirty();
            });
    }

    fn ui_advanced(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("advanced_scroll")
            .show(ui, |ui| {
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Advanced Options").strong());
                    ui.checkbox(&mut self.config.game_options.haste_mode, "Haste Mode");
                    ui.add_enabled(self.config.game_options.haste_mode, |ui: &mut egui::Ui| {
                        ui.checkbox(
                            &mut self.config.game_options.haste_mode_normalize_speed,
                            "Normalize Speed",
                        )
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Buffer Size:");
                        ui.add(
                            egui::DragValue::new(&mut self.config.game_options.buffer_size)
                                .clamp_range(1..=4096),
                        );
                        ui.label("(1–4096 samples)");
                    });
                });
                self.mark_dirty();
            });
    }
}

// ---------------------------------------------------------------------------
// Standalone functions
// ---------------------------------------------------------------------------

/// Walk a directory tree, parse OJN headers, report progress via channel.
fn walk_directory_for_ojn(root: &str, tx: &mpsc::Sender<AppMessage>) -> Vec<ChartScanEntry> {
    let mut entries = Vec::new();
    let root_path = Path::new(root);

    for entry in walkdir::WalkDir::new(root_path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let Some(ext) = entry.path().extension() else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("ojn") {
            continue;
        }

        let Ok(data) = std::fs::read(entry.path()) else {
            continue;
        };
        let Ok(header) = parse_metadata_bytes(&data) else {
            continue;
        };

        let metadata = entry.metadata().ok();
        let file_size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
        let file_modified = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let relative_path = entry
            .path()
            .strip_prefix(root_path)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .into_owned();

        let song_group_id = {
            let mut hasher = Sha256::new();
            hasher.update(relative_path.as_bytes());
            let result = hasher.finalize();
            let mut hex_str = String::with_capacity(16);
            for byte in &result[..8] {
                hex_str.push_str(&format!("{byte:02x}"));
            }
            hex_str
        };

        entries.push(ChartScanEntry {
            relative_path,
            song_group_id,
            header,
            file_size,
            file_modified,
        });

        if entries.len() % SCAN_PROGRESS_INTERVAL == 0 {
            tx.send(AppMessage::ScanProgress {
                scanned: entries.len(),
            })
            .ok();
        }
    }

    tx.send(AppMessage::ScanProgress {
        scanned: entries.len(),
    })
    .ok();
    entries
}

/// Group cached chart rows into display-ready SongEntry structs.
///
/// Each `CachedChart` row contains all 3 difficulty levels
/// (level_easy/normal/hard, note_count_easy/normal/hard, duration_easy/normal/hard).
/// This function emits per-difficulty `levels` and `durations_sec` arrays on
/// the `SongEntry` so the UI can show the selected difficulty's metadata.
pub fn group_charts_into_songs(charts: &[CachedChart], library_root: &str) -> Vec<SongEntry> {
    let mut groups: HashMap<String, Vec<&CachedChart>> = HashMap::new();
    for chart in charts {
        groups
            .entry(chart.song_group_id.clone())
            .or_default()
            .push(chart);
    }

    groups
        .into_values()
        .map(|mut charts_in_group| {
            charts_in_group.sort_by_key(|c| {
                [c.level_easy, c.level_normal, c.level_hard]
                    .into_iter()
                    .find(|&l| l > 0)
                    .unwrap_or(0)
            });

            let first = charts_in_group
                .first()
                .copied()
                .expect("group has at least one chart");

            // Per-difficulty level: take the max across all charts in the group
            let levels: [u16; 3] = [
                charts_in_group
                    .iter()
                    .map(|c| c.level_easy as u16)
                    .max()
                    .unwrap_or(0),
                charts_in_group
                    .iter()
                    .map(|c| c.level_normal as u16)
                    .max()
                    .unwrap_or(0),
                charts_in_group
                    .iter()
                    .map(|c| c.level_hard as u16)
                    .max()
                    .unwrap_or(0),
            ];

            // Per-difficulty duration: take the first non-zero value (already in seconds)
            let durations_sec: [f32; 3] = [
                charts_in_group
                    .iter()
                    .find(|c| c.duration_easy > 0)
                    .map(|c| c.duration_easy as f32)
                    .unwrap_or(0.0),
                charts_in_group
                    .iter()
                    .find(|c| c.duration_normal > 0)
                    .map(|c| c.duration_normal as f32)
                    .unwrap_or(0.0),
                charts_in_group
                    .iter()
                    .find(|c| c.duration_hard > 0)
                    .map(|c| c.duration_hard as f32)
                    .unwrap_or(0.0),
            ];

            // Per-difficulty note counts: take the max across all charts
            let note_counts: [u32; 3] = [
                charts_in_group
                    .iter()
                    .map(|c| c.note_count_easy as u32)
                    .max()
                    .unwrap_or(0),
                charts_in_group
                    .iter()
                    .map(|c| c.note_count_normal as u32)
                    .max()
                    .unwrap_or(0),
                charts_in_group
                    .iter()
                    .map(|c| c.note_count_hard as u32)
                    .max()
                    .unwrap_or(0),
            ];

            // Expand each CachedChart row into up to 3 ChartEntries
            // (one per difficulty level that has content)
            let mut chart_entries: Vec<ChartEntry> = Vec::new();
            for c in &charts_in_group {
                let level_arr = [c.level_easy, c.level_normal, c.level_hard];
                let notes_arr = [c.note_count_easy, c.note_count_normal, c.note_count_hard];
                let dur_arr = [c.duration_easy, c.duration_normal, c.duration_hard];
                let diff_names = ["Easy", "Normal", "Hard"];

                for (i, &level) in level_arr.iter().enumerate() {
                    let notes = notes_arr[i] as u32;
                    if level == 0 && notes == 0 {
                        continue; // skip empty difficulty
                    }
                    let diff_label = diff_names[i];
                    chart_entries.push(ChartEntry {
                        path: PathBuf::new(),
                        title: format!("{} [{}]", c.title, diff_label),
                        artist: c.artist.clone(),
                        noter: c.noter.clone(),
                        genre: c.genre as u32,
                        bpm: c.bpm as f32,
                        duration_sec: dur_arr[i] as f32,
                        note_counts: [notes, 0, 0],
                        levels: [level as u16, 0, 0],
                        keys: c.keys as u8,
                    });
                }
            }

            SongEntry {
                title: first.title.clone(),
                artist: first.artist.clone(),
                noter: first.noter.clone(),
                genre: first.genre as u32,
                bpm: first.bpm as f32,
                keys: first.keys as u8,
                charts: chart_entries,
                cover: None,
                levels,
                durations_sec,
                note_counts,
                relative_path: first.relative_path.clone(),
                library_root: library_root.to_string(),
            }
        })
        .collect()
}

fn spawn_game(chart_path: &std::path::Path, config: &Config) {
    if let Ok(exe) = std::env::current_exe() {
        let game_bin = exe.with_file_name("open2jam-rs");
        let project_root = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent());

        let mut cmd = std::process::Command::new(&game_bin);
        cmd.arg(chart_path);
        if config.game_options.autoplay {
            cmd.arg("--autoplay");
        }

        if let Some(root) = project_root {
            cmd.current_dir(root);
        }

        match cmd.spawn() {
            Ok(_) => log::info!("Launched game for: {}", chart_path.display()),
            Err(e) => log::error!("Failed to launch game: {}", e),
        }
    }
}

/// Format a unix epoch timestamp as a human-readable "X ago" string.
fn format_timestamp_age(ts: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let diff = (now - ts).max(0) as u64;

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else {
        format!("{}w ago", diff / 604800)
    }
}
