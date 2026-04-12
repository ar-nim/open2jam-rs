//! Top-level menu application state — egui + winit + wgpu.

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
        // Initialise wgpu first (before window creation) so the surface is ready.
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions::default(),
        )).ok_or_else(|| anyhow::anyhow!("No GPU adapter found"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor::default(),
        ))?;

        let mut app = MenuRunner {
            config: self.config,
            instance,
            window: None,
            surface: None,
            surface_config: None,
            device,
            queue,
            renderer: None,
            egui_ctx: egui::Context::default(),
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
    instance: wgpu::Instance,
    window: Option<Window>,
    surface: Option<wgpu::Surface<'static>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Option<egui_wgpu::Renderer>,
    egui_ctx: egui::Context,
    integration: Option<egui_winit::State>,
    songs: Vec<SongEntry>,
    selected_song: Option<usize>,
    selected_difficulty: usize,
    search_query: String,
    scan_dirs: Vec<String>,
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
        let size = window.inner_size();

        // Create wgpu surface for this window
        let surface = unsafe {
            self.instance.create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::from_window(&window).unwrap()
            )
        }.unwrap();

        let caps = surface.get_capabilities(&self.device.adapter_info().ok().as_ref().map(|_| &self.device).unwrap_or_else(|| unreachable!()));
        // Simplified: just pick the first format
        let caps2 = surface.get_capabilities(unsafe { std::mem::transmute::<_, &wgpu::Adapter>(std::ptr::null()) });
        
        let format = wgpu::TextureFormat::Bgra8UnormSrgb; // Safe default for Wayland

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&self.device, &config);

        // Create egui-wgpu renderer
        let renderer = egui_wgpu::Renderer::new(&self.device, config.format, None, 1);

        let integration = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            None,
            None,
            None,
        );

        log::info!("Menu window created ({}x{})", size.width, size.height);
        self.window = Some(window);
        self.surface = Some(surface);
        self.surface_config = Some(config);
        self.renderer = Some(renderer);
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
                self.render_window();
            }
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    if let Some(surface) = &self.surface {
                        if let Some(config) = &mut self.surface_config {
                            config.width = size.width;
                            config.height = size.height;
                            surface.configure(&self.device, config);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl MenuRunner {
    fn render_window(&mut self) {
        let Some(integration) = &mut self.integration else { return };
        let Some(window) = &self.window else { return };
        let Some(surface) = &self.surface else { return };
        let Some(renderer) = &mut self.renderer else { return };
        let Some(config) = &self.surface_config else { return };

        let raw_input = integration.take_egui_input(window);
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            ui_menu(ctx, &mut self.config, &mut self.songs, &mut self.selected_song,
                &mut self.selected_difficulty, &mut self.search_query,
                &mut self.scan_dirs, &mut self.scanning);
        });

        integration.handle_platform_output(window, &self.egui_ctx, full_output.platform_output);

        // Render with wgpu
        let frame = match surface.get_current_texture() {
            Ok(f) => f,
            Err(_) => return,
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        // Tessellate egui output
        let clipped_primitives = self.egui_ctx.tessellate(full_output.shapes, 1.0);
        renderer.render(
            &mut encoder,
            &view,
            &clipped_primitives,
            &self.egui_ctx,
            &full_output.textures_delta,
        );

        self.queue.submit(Some(encoder.finish()));
        frame.present();
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
                            if let Ok(exe) = std::env::current_exe() {
                                let game_bin = exe.with_file_name("open2jam-rs");
                                let project_root = exe.parent()
                                    .and_then(|p| p.parent())
                                    .map(|p| p.to_path_buf());

                                log::info!("=== PLAY DEBUG ===");
                                log::info!("Game binary: {} (exists: {})", game_bin.display(), game_bin.exists());
                                log::info!("Chart path: {}", chart.path.display());
                                log::info!("Project root: {:?}", project_root);

                                let mut cmd = std::process::Command::new(&game_bin);
                                cmd.arg(&chart.path);
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
                    }
                }
            }
            ui.checkbox(&mut config.game_options.autoplay, "Autoplay");
        });
    });

    egui::SidePanel::left("song_list").resizable(true).default_width(300.0).show(ctx, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if ui.button("Choose dir").clicked() {
                    *scanning = true;
                }
                if ui.button("Scan").clicked() && !scan_dirs.is_empty() {
                    scan_directories(songs, scan_dirs);
                }
            });
            ui.separator();
            ui.label(&format!("{} songs", songs.len()));
            ui.text_edit_singleline(search_query);
            ui.separator();

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
        ui.vertical(|ui| {
            if song.cover.is_some() {
                ui.label("[Cover art]");
            } else {
                ui.label("[No cover]");
            }
        });
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
    ui.label("Difficulty:");
    for (i, chart) in song.charts.iter().enumerate() {
        let label = format!(
            "{}  (Notes: {}, Level: {})",
            ["Easy", "Normal", "Hard"][i.min(2)],
            chart.note_counts[i],
            chart.levels[i],
        );
        if ui.selectable_value(selected_difficulty, i, &label).clicked() {
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
