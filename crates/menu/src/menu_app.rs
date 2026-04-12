//! Menu application using eframe (handles wgpu+winit automatically).

use anyhow::Result;
use open2jam_rs_core::Config;
use crate::ojn_scanner::{OjnScanner, SongEntry};
use crate::panels::modifiers::ui_modifiers;
use crate::panels::display_config::ui_display_config;
use crate::panels::key_bind_editor::ui_key_bind_editor;

/// Tab index for the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum MenuTab {
    #[default]
    MusicSelect = 0,
    Configuration = 1,
    Advanced = 2,
}

/// The main menu application, implementing eframe::App.
pub struct MenuApp {
    config: Config,
    songs: Vec<SongEntry>,
    selected_song: Option<usize>,
    selected_difficulty: usize,
    search_query: String,
    scan_dirs: Vec<String>,
    active_tab: MenuTab,
    /// Whether to save config on next update (debounced)
    config_dirty: bool,
    /// Last time config was saved (for debounce)
    last_save_time: std::time::Instant,
}

impl MenuApp {
    pub fn new() -> Result<Self> {
        let config_path = Config::default_path();
        let config = Config::load(&config_path)
            .unwrap_or_else(|e| {
                log::info!("No config found at {:?}, using defaults: {}", config_path, e);
                Config::default()
            });
        Ok(Self {
            config,
            songs: Vec::new(),
            selected_song: None,
            selected_difficulty: 0,
            search_query: String::new(),
            scan_dirs: Vec::new(),
            active_tab: MenuTab::MusicSelect,
            config_dirty: false,
            last_save_time: std::time::Instant::now(),
        })
    }

    /// Save config to disk if dirty (debounced at 500ms).
    fn maybe_save_config(&mut self) {
        if !self.config_dirty {
            return;
        }
        let elapsed = self.last_save_time.elapsed();
        if elapsed.as_millis() < 500 {
            return; // Debounce: wait 500ms
        }
        let config_path = Config::default_path();
        if let Err(e) = self.config.save(&config_path) {
            log::warn!("Failed to save config to {:?}: {}", config_path, e);
        } else {
            log::info!("Config saved to {:?}", config_path);
        }
        self.config_dirty = false;
        self.last_save_time = std::time::Instant::now();
    }

    fn mark_dirty(&mut self) {
        self.config_dirty = true;
        self.last_save_time = std::time::Instant::now();
    }
}

impl eframe::App for MenuApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Auto-save config periodically
        self.maybe_save_config();

        // Tab bar
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, MenuTab::MusicSelect, "Music Select");
                ui.selectable_value(&mut self.active_tab, MenuTab::Configuration, "Configuration");
                ui.selectable_value(&mut self.active_tab, MenuTab::Advanced, "Advanced");
            });
        });

        // Bottom bar with PLAY button (always visible)
        egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let can_play = self.active_tab == MenuTab::MusicSelect && self.selected_song.is_some();
                let play_btn = ui.add_enabled(can_play, egui::Button::new("▶ PLAY !!!"));
                if play_btn.clicked() {
                    self.play_selected_song();
                }
                ui.separator();
                ui.checkbox(&mut self.config.game_options.autoplay, "Autoplay");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("💾 Save Config").clicked() {
                        self.config_dirty = true;
                        self.last_save_time = std::time::Instant::now() - std::time::Duration::from_millis(600);
                        self.maybe_save_config();
                    }
                });
            });
        });

        // Main content area
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.active_tab {
                MenuTab::MusicSelect => self.ui_music_select(ui),
                MenuTab::Configuration => self.ui_configuration(ui),
                MenuTab::Advanced => self.ui_advanced(ui),
            }
        });
    }
}

impl MenuApp {
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

    fn ui_music_select(&mut self, ui: &mut egui::Ui) {
        // Left panel: Song Info + Modifiers
        let selected_song_data = self.selected_song.and_then(|idx| self.songs.get(idx).cloned());

        egui::SidePanel::left("song_info").resizable(true).default_width(300.0).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if let Some(song) = selected_song_data {
                    // ── Cover + Metadata ──
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.group(|ui| {
                                ui.label(if song.cover.is_some() { "🖼 Cover" } else { "[No cover]" });
                                ui.allocate_space(egui::vec2(120.0, 120.0));
                            });
                        });
                        ui.vertical(|ui| {
                            ui.heading(&song.title);
                            ui.label(format!("🎵 {}", song.artist));
                            ui.label(format!("BPM: {:.1}  |  Keys: {}", song.bpm, song.keys));
                            let dur = song.duration_sec;
                            ui.label(format!("⏱ {}:{:02}", dur as u32 / 60, dur as u32 % 60));
                            if !song.genre.is_empty() && song.genre != "0" {
                                ui.label(format!("Genre: {}", song.genre));
                            }
                        });
                    });

                    ui.separator();

                    // ── Difficulty Selection ──
                    ui.label(egui::RichText::new("Difficulty").strong());
                    for (i, chart) in song.charts.iter().enumerate() {
                        if chart.note_counts[i] == 0 && chart.levels[i] == 0 {
                            continue; // Skip non-existent difficulty
                        }
                        let diff_name = ["Easy", "Normal", "Hard"][i.min(2)];
                        let label = format!("{}  Lv:{}  Notes:{}", diff_name, chart.levels[i], chart.note_counts[i]);
                        let selected = self.selected_difficulty == i;
                        if ui.selectable_label(selected, &label).clicked() {
                            self.selected_difficulty = i;
                            self.config.game_options.difficulty = match i {
                                0 => open2jam_rs_core::Difficulty::Easy,
                                1 => open2jam_rs_core::Difficulty::Normal,
                                _ => open2jam_rs_core::Difficulty::Hard,
                            };
                            self.mark_dirty();
                        }
                    }

                    ui.separator();

                    // ── Modifiers ──
                    ui_modifiers(ui, &mut self.config.game_options);
                    self.mark_dirty();
                } else {
                    ui.heading("No song selected");
                    ui.label("Scan a directory and select a song from the list.");
                }
            });
        });

        // Right panel: Song List
        egui::SidePanel::right("song_list").resizable(true).default_width(350.0).show_inside(ui, |ui| {
            ui.vertical(|ui| {
                // Library management bar
                ui.horizontal(|ui| {
                    if ui.button("📁 Choose dir").clicked() {
                        log::info!("Choose directory clicked");
                        // TODO: native file dialog
                    }
                    if ui.button("🔄 Scan").clicked() && !self.scan_dirs.is_empty() {
                        scan_directories(&mut self.songs, &self.scan_dirs);
                    }
                });
                ui.separator();
                ui.label(&format!("🎼 {} songs", self.songs.len()));

                // Search
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    ui.text_edit_singleline(&mut self.search_query);
                    if !self.search_query.is_empty() && ui.small_button("✖").clicked() {
                        self.search_query.clear();
                    }
                });
                ui.separator();

                // Song list table
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("song_grid").striped(true).show(ui, |ui| {
                        ui.label(egui::RichText::new("Name").strong());
                        ui.label(egui::RichText::new("Lv").strong());
                        ui.label(egui::RichText::new("Genre").strong());
                        ui.end_row();

                        let filtered: Vec<(usize, &SongEntry)> = self.songs.iter().enumerate()
                            .filter(|(_, s)| {
                                self.search_query.is_empty()
                                    || s.title.to_lowercase().contains(&self.search_query.to_lowercase())
                                    || s.artist.to_lowercase().contains(&self.search_query.to_lowercase())
                            })
                            .collect();

                        for (orig_idx, song) in filtered {
                            let level_str = if song.max_level > 0 {
                                song.max_level.to_string()
                            } else {
                                "-".into()
                            };
                            let selected = self.selected_song == Some(orig_idx);
                            let response = ui.selectable_label(selected, &song.title);
                            if response.clicked() {
                                self.selected_song = Some(orig_idx);
                                self.selected_difficulty = 0;
                            }
                            ui.label(level_str);
                            ui.label(if song.genre.is_empty() || song.genre == "0" { "-" } else { &song.genre });
                            ui.end_row();
                        }
                    });
                });
            });
        });
    }

    fn ui_configuration(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            // ── Key Bindings ──
            ui.group(|ui| {
                ui_key_bind_editor(ui, &mut self.config);
            });
            self.mark_dirty();

            ui.separator();

            // ── Display Configuration ──
            ui.group(|ui| {
                ui_display_config(ui, &mut self.config.game_options);
            });
            self.mark_dirty();

            ui.separator();

            // ── GUI Settings ──
            ui.group(|ui| {
                ui.label(egui::RichText::new("GUI Settings").strong());
                ui.horizontal(|ui| {
                    ui.label("Theme:");
                    egui::ComboBox::from_id_salt("ui_theme")
                        .selected_text(self.config.game_options.ui_theme.to_string())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.config.game_options.ui_theme, open2jam_rs_core::UiTheme::Automatic, "Automatic");
                            ui.selectable_value(&mut self.config.game_options.ui_theme, open2jam_rs_core::UiTheme::Light, "Light");
                            ui.selectable_value(&mut self.config.game_options.ui_theme, open2jam_rs_core::UiTheme::Dark, "Dark");
                        });
                });
            });
            self.mark_dirty();
        });
    }

    fn ui_advanced(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.group(|ui| {
                ui.label(egui::RichText::new("Advanced Options").strong());

                ui.checkbox(&mut self.config.game_options.haste_mode, "Haste Mode");
                ui.add_enabled(self.config.game_options.haste_mode, |ui: &mut egui::Ui| {
                    ui.checkbox(&mut self.config.game_options.haste_mode_normalize_speed, "Normalize Speed")
                });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Buffer Size:");
                    ui.add(egui::DragValue::new(&mut self.config.game_options.buffer_size).clamp_range(1..=4096));
                    ui.label("(1–4096 samples)");
                });
            });
            self.mark_dirty();
        });
    }
}

fn scan_directories(songs: &mut Vec<SongEntry>, dirs: &[String]) {
    let mut scanner = OjnScanner::new();
    for dir in dirs {
        if let Err(e) = scanner.add_directory(std::path::Path::new(dir)) {
            log::warn!("Failed to scan {}: {}", dir, e);
        }
    }
    *songs = scanner.scan();
    log::info!("Scanned {} songs from {} directories", songs.len(), dirs.len());
}

fn spawn_game(chart_path: &std::path::Path, config: &Config) {
    if let Ok(exe) = std::env::current_exe() {
        let game_bin = exe.with_file_name("open2jam-rs");
        let project_root = exe.parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf());

        log::info!("Game binary: {} (exists: {})", game_bin.display(), game_bin.exists());
        log::info!("Project root: {:?}", project_root);

        let mut cmd = std::process::Command::new(&game_bin);
        cmd.arg(chart_path);
        if config.game_options.autoplay {
            cmd.arg("--autoplay");
        }
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());
        if let Some(ref dir) = project_root {
            cmd.current_dir(dir);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        match cmd.spawn() {
            Ok(child) => log::info!("Game spawned: PID={}", child.id()),
            Err(e) => log::error!("Failed to spawn game: {} (binary: {})", e, game_bin.display()),
        }
    }
}
