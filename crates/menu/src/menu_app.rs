//! Menu application using eframe (handles wgpu+winit automatically).

use anyhow::Result;
use open2jam_rs_core::Config;
use open2jam_rs_core::game_options::{ChannelMod, VisibilityMod};
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

/// Sort column for the song list table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SongSortColumn {
    #[default]
    Name,
    Level,
    Bpm,
    Genre,
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
    config_dirty: bool,
    last_save_time: std::time::Instant,
    sort_column: SongSortColumn,
    sort_ascending: bool,
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
            sort_column: SongSortColumn::default(),
            sort_ascending: true,
        })
    }

    fn maybe_save_config(&mut self) {
        if !self.config_dirty { return; }
        if self.last_save_time.elapsed().as_millis() < 500 { return; }
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

    fn sorted_songs(&self) -> Vec<(usize, &SongEntry)> {
        let mut filtered: Vec<(usize, &SongEntry)> = self.songs.iter().enumerate()
            .filter(|(_, s)| {
                self.search_query.is_empty()
                    || s.title.to_lowercase().contains(&self.search_query.to_lowercase())
                    || s.artist.to_lowercase().contains(&self.search_query.to_lowercase())
            })
            .collect();
        let col = self.sort_column;
        let asc = self.sort_ascending;
        filtered.sort_by(|a, b| {
            let ord = match col {
                SongSortColumn::Name => a.1.title.to_lowercase().cmp(&b.1.title.to_lowercase()),
                SongSortColumn::Level => a.1.max_level.cmp(&b.1.max_level),
                SongSortColumn::Bpm => a.1.bpm.partial_cmp(&b.1.bpm).unwrap_or(std::cmp::Ordering::Equal),
                SongSortColumn::Genre => a.1.genre.cmp(&b.1.genre),
            };
            if asc { ord } else { ord.reverse() }
        });
        filtered
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
}

impl eframe::App for MenuApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.maybe_save_config();

        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, MenuTab::MusicSelect, "Music Select");
                ui.selectable_value(&mut self.active_tab, MenuTab::Configuration, "Configuration");
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
                        self.last_save_time = std::time::Instant::now() - std::time::Duration::from_millis(600);
                        self.maybe_save_config();
                    }
                });
            });
        });

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
    fn ui_music_select(&mut self, ui: &mut egui::Ui) {
        let selected_song_data = self.selected_song.and_then(|idx| self.songs.get(idx).cloned());

        // Split the screen in half: left = song list, right = song info
        let col_width = ui.available_width() / 2.0;
        ui.columns(2, |cols| {
            // ── Left: Song List ──
            cols[0].vertical(|ui| {
                ui.label(egui::RichText::new("Select Music").strong().heading());
                ui.separator();
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("📁 Choose dir").clicked() { log::info!("Choose directory clicked"); }
                    if ui.button("🔄 Scan").clicked() && !self.scan_dirs.is_empty() {
                        scan_directories(&mut self.songs, &self.scan_dirs);
                    }
                });
                ui.label(&format!("� {} songs", self.songs.len()));
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    ui.text_edit_singleline(&mut self.search_query);
                    if !self.search_query.is_empty() && ui.small_button("✖").clicked() { self.search_query.clear(); }
                });
                ui.separator();

                ui.horizontal(|ui| {
                    let mut col_btn = |ui: &mut egui::Ui, col: SongSortColumn, label: &str| {
                        let is_active = self.sort_column == col;
                        let arrow = if is_active {
                            if self.sort_ascending { " ▲" } else { " ▼" }
                        } else { "" };
                        if ui.selectable_label(is_active, format!("{}{}", label, arrow)).clicked() {
                            if self.sort_column == col { self.sort_ascending = !self.sort_ascending; }
                            else { self.sort_column = col; self.sort_ascending = true; }
                        }
                    };
                    col_btn(ui, SongSortColumn::Name, "Name");
                    ui.separator();
                    col_btn(ui, SongSortColumn::Level, "Level");
                    ui.separator();
                    col_btn(ui, SongSortColumn::Bpm, "BPM");
                });
                ui.separator();

                egui::ScrollArea::vertical().id_salt("song_list_scroll").show(ui, |ui| {
                    ui.set_max_width(col_width - 16.0);
                    let sorted = self.sorted_songs();
                    let mut sel = self.selected_song;
                    let mut sd = self.selected_difficulty;
                    egui::Grid::new("song_grid").striped(true).show(ui, |ui| {
                        for (orig_idx, song) in sorted {
                            if ui.selectable_label(sel == Some(orig_idx), &song.title).clicked() {
                                sel = Some(orig_idx); sd = 0;
                            }
                            ui.label(song.max_level.to_string());
                            ui.label(format!("{:.1}", song.bpm));
                            ui.end_row();
                        }
                    });
                    self.selected_song = sel;
                    self.selected_difficulty = sd;
                });
            });

            // ── Right: Song Info + Options ──
            cols[1].vertical(|ui| {
                egui::ScrollArea::vertical().id_salt("song_info_scroll").show(ui, |ui| {
                    ui.set_max_width(col_width - 16.0);

                    // ── Song Info ──
                    ui.label(egui::RichText::new("Song Info").strong().heading());
                    ui.separator();
                    ui.add_space(10.0);
                    if let Some(song) = &selected_song_data {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| { ui.group(|ui| {
                                ui.label(if song.cover.is_some() { "🖼 Cover" } else { "[No cover]" });
                                ui.allocate_space(egui::vec2(100.0, 100.0));
                            });});
                            ui.vertical(|ui| {
                                ui.heading(&song.title);
                                ui.label(format!("Artist: {}", song.artist));
                                ui.label(format!("Notecharter: "));
                                ui.label(format!("BPM: {:.1}", song.bpm));
                                let dur = song.duration_sec;
                                ui.label(format!("⏱ {}:{:02}", dur as u32 / 60, dur as u32 % 60));
                                if !song.genre.is_empty() && song.genre != "0" {
                                    ui.label(format!("Genre: {}", song.genre));
                                }
                            });
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| { ui.group(|ui| {
                                ui.label("[No cover]");
                                ui.allocate_space(egui::vec2(100.0, 100.0));
                            });});
                            ui.vertical(|ui| {
                                ui.heading("No song selected");
                                ui.label("Artist: ");
                                ui.label("Notecharter: ");
                                ui.label("BPM: -");
                                ui.label("⏱ -:--");
                                ui.label("");
                            });
                        });
                    }
                     ui.add_space(10.0);  
                    // ── Difficulty ──
                    ui.label(egui::RichText::new("Difficulty").strong().heading());
                    ui.separator();
                    if let Some(song) = &selected_song_data {
                        for (i, chart) in song.charts.iter().enumerate() {
                            if chart.note_counts[i] == 0 && chart.levels[i] == 0 { continue; }
                            let dn = ["Easy", "Normal", "Hard"][i.min(2)];
                            let is_selected = self.selected_difficulty == i;
                            let lb = format!("{} [{}] | Total Notes: [{}]", dn, chart.levels[i], chart.note_counts[i]);
                            if ui.selectable_label(is_selected, lb).clicked() {
                                self.selected_difficulty = i;
                                self.config.game_options.difficulty = match i {
                                    0 => open2jam_rs_core::Difficulty::Easy,
                                    1 => open2jam_rs_core::Difficulty::Normal,
                                    _ => open2jam_rs_core::Difficulty::Hard,
                                };
                                self.mark_dirty();
                            }
                        }
                    } else {
                        ui.selectable_label(false, "Easy [-] | Total Notes: [-]");
                        ui.selectable_label(false, "Normal [-] | Total Notes: [-]");
                        ui.selectable_label(false, "Hard [-] | Total Notes: [-]");
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
                            self.config.game_options.speed_multiplier = (self.config.game_options.speed_multiplier - 0.5).max(0.5);
                            self.mark_dirty();
                        }
                        ui.add(egui::DragValue::new(&mut self.config.game_options.speed_multiplier)
                            .speed(0.5)
                            .clamp_range(0.5..=10.0)
                            .custom_formatter(|n, _| format!("{:.1}", n))
                            .custom_parser(|s| s.parse::<f64>().ok()));
                        if ui.button("+").clicked() {
                            self.config.game_options.speed_multiplier = (self.config.game_options.speed_multiplier + 0.5).min(10.0);
                            self.mark_dirty();
                        }
                    });

                    ui.add_space(5.0);
                    
                    // Arrangement Mods
                    ui.label(egui::RichText::new("Arrangement Mods").strong().size(14.0));
                    ui.add_space(5.0);
                    ui.horizontal(|ui| {
                        let mut clicked = false;
                        for mod_val in [ChannelMod::None, ChannelMod::Random, ChannelMod::Panic, ChannelMod::Mirror] {
                            let is_selected = self.config.game_options.channel_modifier == mod_val;
                            if ui.selectable_label(is_selected, mod_val.to_string()).clicked() {
                                self.config.game_options.channel_modifier = mod_val;
                                clicked = true;
                            }
                        }
                        if clicked { self.mark_dirty(); }
                    });
                    ui.add_space(5.0);

                    // Visibility Mods
                    ui.label(egui::RichText::new("Visibility Mods").strong().size(14.0));
                    ui.add_space(5.0);
                    ui.horizontal(|ui| {
                        let mut clicked = false;
                        for mod_val in [VisibilityMod::None, VisibilityMod::Hidden, VisibilityMod::Sudden, VisibilityMod::Dark] {
                            let is_selected = self.config.game_options.visibility_modifier == mod_val;
                            if ui.selectable_label(is_selected, mod_val.to_string()).clicked() {
                                self.config.game_options.visibility_modifier = mod_val;
                                clicked = true;
                            }
                        }
                        if clicked { self.mark_dirty(); }
                    });
                    ui.add_space(10.0);
                    // Play Button — fills full width, larger text, centered
                    let can_play = self.active_tab == MenuTab::MusicSelect && self.selected_song.is_some();
                    let mut btn_rect = ui.available_rect_before_wrap();
                    btn_rect.max.y = btn_rect.min.y + 40.0;
                    let btn_id = ui.make_persistent_id("start_btn");
                    let btn_response = ui.interact(btn_rect, btn_id, egui::Sense::click());
                    let is_hovered = btn_response.hovered() && can_play;
                    let fill_color = if can_play {
                        if is_hovered { egui::Color32::from_rgb(100, 150, 255) } else { egui::Color32::BLUE }
                    } else {
                        ui.style().visuals.widgets.inactive.bg_fill
                    };
                    let text_color = if can_play { egui::Color32::WHITE } else { ui.style().visuals.text_color() };
                    ui.painter().rect_filled(btn_rect, 4.0, fill_color);
                    ui.painter().text(
                        btn_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "▶ START GAME",
                        egui::TextStyle::Button.resolve(ui.style()),
                        text_color,
                    );
                    if btn_response.clicked() && can_play { self.play_selected_song(); }
                });
            });
        });
    }

    fn ui_configuration(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("config_scroll").show(ui, |ui| {
            ui.group(|ui| { ui_key_bind_editor(ui, &mut self.config); });
            self.mark_dirty();
            ui.separator();
            ui.group(|ui| { ui_display_config(ui, &mut self.config.game_options); });
            self.mark_dirty();
            ui.separator();
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
        egui::ScrollArea::vertical().id_salt("advanced_scroll").show(ui, |ui| {
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
        if config.game_options.autoplay { cmd.arg("--autoplay"); }
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());
        if let Some(ref dir) = project_root { cmd.current_dir(dir); }
        #[cfg(unix)] {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        match cmd.spawn() {
            Ok(child) => log::info!("Game spawned: PID={}", child.id()),
            Err(e) => log::error!("Failed to spawn game: {} (binary: {})", e, game_bin.display()),
        }
    }
}
