//! Top-level menu application state.

use anyhow::Result;
use winit::event_loop::EventLoop;
use winit::window::Window;

use open2jam_rs_core::Config;
use crate::ojn_scanner::{OjnScanner, SongEntry};

/// The main menu application.
pub struct MenuApp {
    config: Config,
}

impl MenuApp {
    pub fn new() -> Result<Self> {
        let config_path = Config::default_path();
        let config = Config::load(&config_path)
            .unwrap_or_else(|e| {
                log::info!("No config found at {:?}, using defaults: {}", config_path, e);
                Config::default()
            });

        Ok(Self { config })
    }

    pub fn run(mut self, event_loop: EventLoop<()>) -> Result<()> {
        let mut app = MenuRunner {
            config: self.config,
            egui_ctx: egui::Context::default(),
            window: None,
            integration: None,
            songs: Vec::new(),
            selected_song: None,
            selected_difficulty: 0,
            search_query: String::new(),
            scan_dirs: Vec::new(),
            scanning: false,
        };
        event_loop.run_app(&mut app)?;
        Ok(())
    }
}

struct MenuRunner {
    config: Config,
    egui_ctx: egui::Context,
    window: Option<Window>,
    integration: Option<egui_winit::State>,
    /// All scanned songs
    songs: Vec<SongEntry>,
    /// Currently selected song index
    selected_song: Option<usize>,
    /// Selected difficulty index within the song (0-based into charts vec)
    selected_difficulty: usize,
    /// Search filter text
    search_query: String,
    /// Scanned library directories
    scan_dirs: Vec<String>,
    /// Whether a scan is in progress
    scanning: bool,
}

impl winit::application::ApplicationHandler for MenuRunner {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = winit::window::Window::default_attributes()
            .with_title("open2jam-rs — Music Select")
            .with_inner_size(winit::dpi::LogicalSize::new(928.0, 730.0));

        let window = event_loop.create_window(attrs).unwrap();
        let integration = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            None,
            None,
            None,
        );

        log::info!("Menu window created");
        self.window = Some(window);
        self.integration = Some(integration);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        use winit::event::WindowEvent;

        let Some(integration) = &mut self.integration else { return };
        let Some(window) = &self.window else { return };

        let response = integration.on_window_event(window, &event);
        if response.consumed {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if self.window.is_some() && self.integration.is_some() {
                    self.render_window();
                }
            }
            _ => {}
        }
    }
}

impl MenuRunner {
    fn render_window(&mut self) {
        let raw_input = {
            let window = self.window.as_ref().unwrap();
            let integration = self.integration.as_mut().unwrap();
            integration.take_egui_input(window)
        };
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            ui_menu(ctx, &mut self.config, &mut self.songs, &mut self.selected_song,
                &mut self.selected_difficulty, &mut self.search_query,
                &mut self.scan_dirs, &mut self.scanning);
        });
        {
            let window = self.window.as_ref().unwrap();
            let integration = self.integration.as_mut().unwrap();
            integration.handle_platform_output(window, full_output.platform_output);
        }
    }
}

fn ui_menu(
    ctx: &egui::Context,
    config: &mut Config,
    songs: &mut Vec<SongEntry>,
    selected_song: &mut Option<usize>,
    selected_difficulty: &mut usize,
    search_query: &mut String,
    scan_dirs: &mut Vec<String>,
    scanning: &mut bool,
) {
    egui::TopBottomPanel::bottom("bottom_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            let can_play = selected_song.is_some();
            let play_btn = ui.add_enabled(can_play, egui::Button::new("▶ PLAY !!!"));
            if play_btn.clicked() {
                if let Some(idx) = *selected_song {
                    if let Some(song) = songs.get(idx) {
                        if let Some(chart) = song.charts.get(*selected_difficulty) {
                            log::info!("PLAY: {}", chart.path.display());
                            // Phase 1: spawn game binary
                            let _ = std::process::Command::new("open2jam-rs")
                                .arg(&chart.path)
                                .arg(if config.game_options.autoplay { "--autoplay" } else { "" })
                                .spawn();
                        }
                    }
                }
            }
            ui.checkbox(&mut config.game_options.autoplay, "Autoplay");
        });
    });

    egui::SidePanel::left("song_list").resizable(true).default_width(300.0).show(ctx, |ui| {
        ui.vertical(|ui| {
            // Library management bar
            ui.horizontal(|ui| {
                if ui.button("Choose dir").clicked() {
                    // TODO: native file dialog
                    *scanning = true;
                }
                if ui.button("Scan").clicked() && !scan_dirs.is_empty() {
                    scan_directories(songs, scan_dirs);
                }
            });

            ui.separator();
            ui.label(&format!("{} songs", songs.len()));

            // Search
            ui.text_edit_singleline(search_query);

            ui.separator();

            // Song list table
            egui::ScrollArea::vertical().show(ui, |ui| {
                let filtered: Vec<(usize, &SongEntry)> = songs.iter().enumerate()
                    .filter(|(_, s)| {
                        search_query.is_empty()
                            || s.title.to_lowercase().contains(&search_query.to_lowercase())
                            || s.artist.to_lowercase().contains(&search_query.to_lowercase())
                    })
                    .collect();

                for (orig_idx, song) in filtered {
                    let level_str = if song.max_level > 0 {
                        song.max_level.to_string()
                    } else {
                        "-".into()
                    };
                    let label = format!("{}  Lv{}  {}", song.title, level_str, song.genre);
                    let selected = *selected_song == Some(orig_idx);
                    if ui.selectable_label(selected, &label).clicked() {
                        *selected_song = Some(orig_idx);
                        *selected_difficulty = 0;
                    }
                }
            });
        });
    });

    egui::CentralPanel::default().show(ctx, |ui| {
        if let Some(idx) = *selected_song {
            if let Some(song) = songs.get(idx) {
                ui_song_info(ui, config, song, selected_difficulty);
            }
        } else {
            ui.heading("Select a song");
            ui.label("Choose a directory and scan for OJN charts.");
        }
    });
}

fn ui_song_info(
    ui: &mut egui::Ui,
    config: &mut Config,
    song: &SongEntry,
    selected_difficulty: &mut usize,
) {
    ui.horizontal(|ui| {
        // Cover art placeholder
        ui.vertical(|ui| {
            if song.cover.is_some() {
                ui.label("[Cover art]");
            } else {
                ui.label("[No cover]");
            }
        });

        // Song metadata
        ui.vertical(|ui| {
            ui.heading(&song.title);
            ui.label(format!("Artist: {}", song.artist));
            ui.label(format!("BPM: {:.1}", song.bpm));
            ui.label(format!("Keys: {}", song.keys));
            let dur = song.duration_sec;
            ui.label(format!("Duration: {}:{:02}", dur as u32 / 60, dur as u32 % 60));
        });
    });

    ui.separator();

    // Difficulty selection
    ui.label("Difficulty:");
    for (i, chart) in song.charts.iter().enumerate() {
        let label = format!(
            "{}  (Notes: {}, Level: {})",
            ["Easy", "Normal", "Hard"][i.min(2)],
            chart.note_counts[i],
            chart.levels[i],
        );
        if ui.selectable_value(selected_difficulty, i, &label).clicked() {
            // update game_options difficulty
            config.game_options.difficulty = match i {
                0 => open2jam_rs_core::Difficulty::Easy,
                1 => open2jam_rs_core::Difficulty::Normal,
                _ => open2jam_rs_core::Difficulty::Hard,
            };
        }
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
