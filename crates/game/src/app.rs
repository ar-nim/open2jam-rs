use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use anyhow::Result;
use log::info;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::Key;
use winit::window::Window;

use crate::audio::AudioManager;
use crate::game_state::GameState;
use crate::gpu::{self, RenderState};
use crate::menu::menu_app::MenuApp;
use crate::types::{AppMode, FrameLimiter, LaneIndex, LoadingMessage, RenderMetrics};
use crate::{assets, input};

pub struct App {
    ojn_path: Option<std::path::PathBuf>,
    event_loop: Option<EventLoop<()>>,
    render: Option<RenderState>,
    audio: Option<AudioManager>,
    mode: AppMode,
    menu_app: Option<MenuApp>,
    game_state: Option<GameState>,
    config: open2jam_rs_core::Config,
    egui_ctx: egui::Context,
    egui_winit: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    last_frame_time: Option<Instant>,
    game_start_instant: Option<Instant>,
    start_load_game_state: bool,
    loading_state: Option<LoadingState>,
    hybrid_clock_prev: Option<f64>,
    hybrid_clock_prev_delta: Option<f64>,
    hybrid_clock_frame_count: u64,
    auto_play: bool,
    display_width: u32,
    display_height: u32,
    display_fullscreen: bool,
    scroll_speed: f64,
    difficulty: open2jam_rs_core::Difficulty,
    vsync_mode: open2jam_rs_core::game_options::VSyncMode,
    fps_limiter: open2jam_rs_core::game_options::FpsLimiter,
    key_to_lane: HashMap<String, LaneIndex>,
    frame_limiter: Option<FrameLimiter>,
}

struct LoadingState {
    receiver: mpsc::Receiver<LoadingMessage>,
    _thread: thread::JoinHandle<()>,
}

impl App {
    pub fn new(
        ojn_path: Option<std::path::PathBuf>,
        auto_play: bool,
        config: &open2jam_rs_core::Config,
    ) -> Result<Self> {
        let opts = &config.game_options;

        let scroll_speed = if opts.speed_type == open2jam_rs_core::game_options::SpeedType::HiSpeed
        {
            1.0 * opts.speed_multiplier as f64
        } else {
            1.0
        };
        info!(
            "Scroll speed: {:.1} (speed_type={:?}, multiplier={:.1})",
            scroll_speed, opts.speed_type, opts.speed_multiplier
        );

        let mode = if ojn_path.is_some() {
            AppMode::Playing
        } else {
            AppMode::Menu
        };

        let menu_app = if mode == AppMode::Menu {
            Some(MenuApp::new()?)
        } else {
            None
        };

        Ok(Self {
            ojn_path,
            event_loop: None,
            render: None,
            audio: None,
            mode,
            menu_app,
            game_state: None,
            config: config.clone(),
            egui_ctx: egui::Context::default(),
            egui_winit: None,
            egui_renderer: None,
            last_frame_time: None,
            game_start_instant: None,
            start_load_game_state: false,
            loading_state: None,
            hybrid_clock_prev: None,
            hybrid_clock_prev_delta: None,
            hybrid_clock_frame_count: 0,
            auto_play,
            display_width: opts.display_width,
            display_height: opts.display_height,
            display_fullscreen: opts.display_fullscreen,
            scroll_speed,
            difficulty: opts.difficulty,
            vsync_mode: opts.vsync_mode,
            fps_limiter: opts.fps_limiter,
            key_to_lane: input::build_key_mapping(&config.key_bindings.k7.lanes),
            frame_limiter: None,
        })
    }

    pub fn run(mut self) -> Result<()> {
        info!("Initialising winit event loop...");
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::Poll);
        self.event_loop = Some(event_loop);

        info!("Initialising audio backend (oddio + cpal)...");
        let audio_mgr = AudioManager::new();
        if audio_mgr.is_active() {
            info!("Audio manager active.");
        } else {
            info!("Audio manager failed to initialise (running headless).");
        }
        self.audio = Some(audio_mgr);

        let event_loop = self.event_loop.take().unwrap();
        event_loop.run_app(&mut self)?;

        self.cleanup();

        info!("App shutting down.");
        Ok(())
    }

    fn cleanup(&mut self) {
        self.egui_renderer.take();
        self.egui_winit.take();
        self.egui_ctx = egui::Context::default();
        if let Some(render) = &mut self.render {
            render.gpu.take();
        }
        if let Some(render) = &mut self.render {
            render.shutdown();
        }
        self.game_state.take();
        self.audio.take();
        self.menu_app.take();
        self.render.take();
        info!("All resources cleaned up in correct order.");
    }

    fn setup_frame_limiter(&mut self) {
        use open2jam_rs_core::game_options::{FpsLimiter, VSyncMode};

        if self.vsync_mode == VSyncMode::On {
            return;
        }

        if self.fps_limiter == FpsLimiter::Unlimited {
            return;
        }

        let base_hz = self
            .render
            .as_ref()
            .and_then(|r| r.window.current_monitor())
            .and_then(|monitor| {
                let modes: Vec<_> = monitor.video_modes().collect();
                modes
                    .into_iter()
                    .max_by_key(|vm| vm.refresh_rate_millihertz())
            })
            .map(|vm| vm.refresh_rate_millihertz() as f64 / 1000.0)
            .unwrap_or(60.0);

        let multiplier = match self.fps_limiter {
            FpsLimiter::X1 => 1.0,
            FpsLimiter::X2 => 2.0,
            FpsLimiter::X4 => 4.0,
            FpsLimiter::X8 => 8.0,
            FpsLimiter::Unlimited => 1.0,
        };

        let target_fps = base_hz * multiplier;
        self.frame_limiter = Some(FrameLimiter::new(target_fps));
        info!(
            "Frame limiter: {:.0} Hz × {:.0} = {:.0} fps",
            base_hz, multiplier, target_fps
        );
    }
}

impl App {
    pub fn on_resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.render.is_none() {
            let inner_size = if self.mode == AppMode::Menu {
                if let Some(monitor) = event_loop.primary_monitor() {
                    let size = monitor.size();
                    let scale = monitor.scale_factor();
                    winit::dpi::LogicalSize::new(
                        (size.width as f64 * 0.65) / scale as f64,
                        (size.height as f64 * 0.65) / scale as f64,
                    )
                } else {
                    winit::dpi::LogicalSize::new(1280.0, 720.0)
                }
            } else {
                winit::dpi::LogicalSize::new(self.display_width as f64, self.display_height as f64)
            };

            info!(
                "Creating window ({}x{}, fullscreen={}) and initialising wgpu...",
                inner_size.width, inner_size.height, self.display_fullscreen
            );

            let mut attrs = winit::window::WindowAttributes::default()
                .with_title("open2jam-rs")
                .with_visible(true)
                .with_resizable(true)
                .with_inner_size(inner_size);

            if self.mode == AppMode::Playing && self.display_fullscreen {
                attrs = attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
            }

            let window = event_loop.create_window(attrs).unwrap();
            self.init_wgpu(window);

            if let Some(render) = &self.render {
                self.egui_winit = Some(egui_winit::State::new(
                    self.egui_ctx.clone(),
                    egui::ViewportId::ROOT,
                    &render.window,
                    None,
                    None,
                    None,
                ));
                info!("egui-winit state initialised");
            }

            crate::menu::fonts::configure_fonts(&self.egui_ctx);

            self.setup_frame_limiter();

            if self.ojn_path.is_none() {
                info!("No OJN file specified — running in demo mode (no notes)");
            }

            self.last_frame_time = Some(Instant::now());

            if let Some(render) = &self.render {
                render.window.request_redraw();
            }
        }
    }

    pub fn on_about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}

    pub fn on_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Close requested, cleaning up...");
                self.cleanup();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(render) = &mut self.render {
                    if size.width > 0 && size.height > 0 {
                        render.config.width = size.width;
                        render.config.height = size.height;
                        if let Some(ref surface) = render.surface {
                            surface.configure(&render.device, &render.config);
                        }
                        if let Some(ref mut gpu) = render.gpu {
                            gpu.textured_renderer.resize(
                                &render.device,
                                &render.queue,
                                size.width,
                                size.height,
                            );
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event: ref key_event,
                ..
            } => {
                if self.mode == AppMode::Menu {
                    if let (Some(render), Some(egui_winit)) = (&self.render, &mut self.egui_winit) {
                        let response = egui_winit.on_window_event(&render.window, &event);
                        if response.repaint {
                            render.window.request_redraw();
                        }
                        if response.consumed {
                            return;
                        }
                    }
                }

                if self.mode != AppMode::Playing {
                    return;
                }

                use winit::event::ElementState;
                use winit::keyboard::NamedKey;

                let lane = match &key_event.logical_key {
                    Key::Character(c) => {
                        let lookup = input::config_key_for_character(c);
                        self.key_to_lane.get(lookup).copied()
                    }
                    Key::Named(named) => {
                        let name = match named {
                            NamedKey::Space => "Space",
                            NamedKey::Enter => "Enter",
                            NamedKey::Escape => "Escape",
                            NamedKey::Tab => "Tab",
                            NamedKey::Backspace => "Backspace",
                            NamedKey::Delete => "Delete",
                            NamedKey::Insert => "Insert",
                            NamedKey::ArrowUp => "ArrowUp",
                            NamedKey::ArrowDown => "ArrowDown",
                            NamedKey::ArrowLeft => "ArrowLeft",
                            NamedKey::ArrowRight => "ArrowRight",
                            NamedKey::Home => "Home",
                            NamedKey::End => "End",
                            NamedKey::PageUp => "PageUp",
                            NamedKey::PageDown => "PageDown",
                            NamedKey::Shift => "Shift",
                            NamedKey::Control => "Control",
                            NamedKey::Alt => "Alt",
                            NamedKey::F1 => "F1",
                            NamedKey::F2 => "F2",
                            NamedKey::F3 => "F3",
                            NamedKey::F4 => "F4",
                            NamedKey::F5 => "F5",
                            NamedKey::F6 => "F6",
                            NamedKey::F7 => "F7",
                            NamedKey::F8 => "F8",
                            NamedKey::F9 => "F9",
                            NamedKey::F10 => "F10",
                            NamedKey::F11 => "F11",
                            NamedKey::F12 => "F12",
                            _ => "Unknown",
                        };
                        self.key_to_lane.get(name).copied()
                    }
                    _ => None,
                };

                if let Some(lane) = lane {
                    if self.auto_play {
                        return;
                    }

                    let Some(gs) = &mut self.game_state else {
                        return;
                    };

                    let os_timestamp = std::time::Instant::now();
                    match key_event.state {
                        ElementState::Pressed => {
                            if !gs.pressed_lanes[lane.index()] {
                                if let Some(audio_mgr) = &mut self.audio {
                                    gs.handle_key_press(lane.index(), os_timestamp, audio_mgr);
                                }
                            }
                        }
                        ElementState::Released => {
                            gs.handle_key_release(lane.index(), os_timestamp);
                        }
                    }
                } else if key_event.state == ElementState::Pressed {
                    if let Key::Named(NamedKey::Escape) = &key_event.logical_key {
                        info!("Escape pressed, exiting...");
                        self.cleanup();
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(ref loading) = self.loading_state {
                    if let Ok(msg) = loading.receiver.try_recv() {
                        if let LoadingMessage::GameLoaded(result) = msg {
                            match result {
                                Ok(gs) => {
                                    info!(
                                        "Game loaded: {} ({:.1}ms spawn lead)",
                                        gs.chart.header.title, gs.spawn_lead_time_ms
                                    );
                                    self.game_state = Some(gs);
                                }
                                Err(e) => {
                                    info!("Failed to load game state: {e:?}");
                                }
                            }
                            self.loading_state.take();
                            self.last_frame_time = Some(Instant::now());
                        }
                    }
                }

                if self.start_load_game_state && self.loading_state.is_none() {
                    self.start_load_game_state = false;
                    if let Some(path) = self.ojn_path.clone() {
                        info!(
                            "Starting background game state load from: {}",
                            path.display()
                        );
                        let skin_res = self
                            .render
                            .as_ref()
                            .and_then(|r| r.gpu.as_ref())
                            .and_then(|g| g.skin.clone());
                        let (tx, rx) = mpsc::channel();
                        let scroll_speed = self.scroll_speed;
                        let auto_play = self.auto_play;
                        let difficulty = self.difficulty;
                        let thread_handle = thread::spawn(move || {
                            let result = GameState::load(
                                &path,
                                scroll_speed,
                                auto_play,
                                difficulty,
                                skin_res.as_ref(),
                            );
                            let _ = tx.send(LoadingMessage::GameLoaded(result));
                        });
                        self.loading_state = Some(LoadingState {
                            receiver: rx,
                            _thread: thread_handle,
                        });
                    }
                }

                let song_ended = self.render_frame();
                if song_ended {
                    info!("Song ended, exiting game loop");
                    self.cleanup();
                    event_loop.exit();
                    return;
                }
                if let Some(render) = &self.render {
                    render.window.request_redraw();
                }
                if let Some(ref mut limiter) = self.frame_limiter {
                    limiter.wait();
                }
            }
            WindowEvent::MouseInput { .. }
            | WindowEvent::CursorMoved { .. }
            | WindowEvent::MouseWheel { .. } => {
                if self.mode == AppMode::Menu {
                    if let (Some(render), Some(egui_winit)) = (&self.render, &mut self.egui_winit) {
                        let response = egui_winit.on_window_event(&render.window, &event);
                        if response.repaint {
                            render.window.request_redraw();
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl App {
    fn init_wgpu(&mut self, window: Window) {
        let (window, _, surface, _, device, queue, config) =
            gpu::init_wgpu(window, self.vsync_mode);

        let root_dir = if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            std::path::PathBuf::from(manifest_dir)
                .parent()
                .and_then(|p| p.parent())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("."))
        } else {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|parent| parent.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."))
        };
        let skin_dir = root_dir.join("assets").join("skins").join("default");

        let (gpu, skin_scale) = assets::build_gpu_resources(
            &device,
            &queue,
            &config,
            skin_dir,
            self.ojn_path.as_deref(),
        );

        self.egui_renderer = Some(egui_wgpu::Renderer::new(
            &device,
            config.format,
            egui_wgpu::RendererOptions::default(),
        ));
        info!("egui-wgpu renderer initialised (wgpu 29)");

        self.render = Some(RenderState {
            window,
            surface: Some(surface),
            device,
            queue,
            config,
            gpu: Some(gpu),
            skin_scale,
        });
    }

    fn render_frame(&mut self) -> bool {
        use std::sync::atomic::Ordering;

        let Some(render) = &mut self.render else {
            log::warn!("render_frame: render state is None");
            return false;
        };

        if self.mode == AppMode::Menu {
            if let Some(menu) = &mut self.menu_app {
                let raw_input = self
                    .egui_winit
                    .as_mut()
                    .unwrap()
                    .take_egui_input(&render.window);

                let full_output = self.egui_ctx.run_ui(raw_input, |ui| {
                    menu.ui(ui);
                });

                let pixels_per_point = self.egui_ctx.pixels_per_point();
                let clipped_primitives = self
                    .egui_ctx
                    .tessellate(full_output.shapes, pixels_per_point);

                if let Some(surface) = &render.surface {
                    let surface_texture = match surface.get_current_texture() {
                        wgpu::CurrentSurfaceTexture::Success(st) => st,
                        wgpu::CurrentSurfaceTexture::Suboptimal(st) => st,
                        _ => return false,
                    };

                    let view = surface_texture
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());

                    let mut encoder =
                        render
                            .device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("egui_encoder"),
                            });

                    let screen_descriptor = egui_wgpu::ScreenDescriptor {
                        size_in_pixels: [render.config.width, render.config.height],
                        pixels_per_point,
                    };

                    if let Some(renderer) = &mut self.egui_renderer {
                        renderer.update_buffers(
                            &render.device,
                            &render.queue,
                            &mut encoder,
                            &clipped_primitives,
                            &screen_descriptor,
                        );

                        for (id, image_delta) in &full_output.textures_delta.set {
                            renderer.update_texture(
                                &render.device,
                                &render.queue,
                                *id,
                                image_delta,
                            );
                        }
                    }

                    if let Some(renderer) = &mut self.egui_renderer {
                        let mut render_pass =
                            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("egui_pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &view,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color {
                                            r: 0.1,
                                            g: 0.1,
                                            b: 0.15,
                                            a: 1.0,
                                        }),
                                        store: wgpu::StoreOp::Store,
                                    },
                                    depth_slice: None,
                                })],
                                ..Default::default()
                            });
                        let render_pass: &mut wgpu::RenderPass<'static> =
                            unsafe { std::mem::transmute(&mut render_pass) };
                        renderer.render(render_pass, &clipped_primitives, &screen_descriptor);
                    }

                    if let Some(renderer) = &mut self.egui_renderer {
                        for id in &full_output.textures_delta.free {
                            renderer.free_texture(id);
                        }
                    }

                    render.queue.submit(Some(encoder.finish()));
                    surface_texture.present();
                }
            }
            return false;
        }

        if self.game_state.is_none() && self.loading_state.is_none() && self.ojn_path.is_some() {
            self.start_load_game_state = true;
        }

        let now = Instant::now();
        self.last_frame_time = Some(now);

        if let Some(gs) = &mut self.game_state {
            if self.game_start_instant.is_none() {
                self.game_start_instant = Some(now);
                self.hybrid_clock_prev = None;
                self.hybrid_clock_prev_delta = None;
                self.hybrid_clock_frame_count = 0;
            }
            let elapsed_ms = self
                .game_start_instant
                .map(|t| now.duration_since(t).as_millis() as u64)
                .unwrap_or(0);

            let prev_jam_combo = gs.stats.jam_combo;
            let prev_max_combo = gs.stats.max_combo;
            gs.update(elapsed_ms);

            if gs.is_rendering {
                if gs.startup_audio_pending {
                    gs.startup_audio_pending = false;
                    if let Some(audio_mgr) = &mut self.audio {
                        audio_mgr.play();
                    }
                }

                gs.spawn_notes();
                if let Some(audio_mgr) = &mut self.audio {
                    gs.process_audio(audio_mgr);
                    gs.process_judgments(audio_mgr);
                }
                gs.cleanup_notes();
                gs.cleanup_effects();

                if gs.stats.combo > gs.prev_frame_combo {
                    gs.combo_counter.increment();
                    if gs.prev_frame_combo == 0 {
                        gs.show_combo_title();
                    }
                } else if gs.stats.combo == 0 && gs.prev_frame_combo > 0 {
                    gs.combo_counter.reset();
                }

                if gs.stats.jam_combo > prev_jam_combo {
                    gs.show_jam_counter();
                }

                if gs.stats.max_combo > prev_max_combo {
                    gs.show_combo_title();
                }

                gs.prev_frame_combo = gs.stats.combo;

                if let Some(audio_mgr) = &mut self.audio {
                    let base = Instant::now();
                    let (now_ms, delta_ms, monotonic) = audio_mgr.validate_hybrid_clock(
                        base,
                        10.0,
                        &mut self.hybrid_clock_prev,
                        &mut self.hybrid_clock_prev_delta,
                        &mut self.hybrid_clock_frame_count,
                    );

                    static FRAME_COUNTER: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(0);
                    let fc = FRAME_COUNTER.fetch_add(1, Ordering::Relaxed);

                    if fc % 60 == 0 {
                        log::info!(
                            "Hybrid clock: time={:.1}ms delta={:.3}ms monotonic={} samples={}",
                            now_ms,
                            delta_ms,
                            monotonic,
                            audio_mgr.state().samples_played.load(Ordering::Relaxed)
                        );
                    }

                    static CPU_FRAME_COUNTER: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(0);
                    let cpu_fc = CPU_FRAME_COUNTER.fetch_add(1, Ordering::Relaxed);
                    if cpu_fc % 600 == 0 {
                        let (avg, max, budget, pct) = audio_mgr.callback_cpu_usage();
                        let bar = "█".repeat((pct / 2.0).max(0.5) as usize);
                        log::info!(
                            "Audio CPU: avg={}µs max={}µs budget={}µs [{:5.1}%] {}",
                            avg,
                            max,
                            budget,
                            pct,
                            bar
                        );
                    }
                }
            }
        }

        let Some(ref surface) = render.surface else {
            return false;
        };
        let surface_texture = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(st) => st,
            wgpu::CurrentSurfaceTexture::Suboptimal(st) => st,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return false
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                surface.configure(&render.device, &render.config);
                return false;
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Validation => {
                return false
            }
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        if self.game_state.is_none() {
            if let Some(ref gpu) = render.gpu {
                if let (Some(pipeline), Some(bind_group)) =
                    (&gpu.cover_pipeline, &gpu.cover_bind_group)
                {
                    let mut encoder =
                        render
                            .device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("cover_encoder"),
                            });
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("cover_pass"),
                            multiview_mask: None,
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                depth_slice: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, bind_group, &[]);
                        pass.draw(0..4, 0..1);
                    }
                    render.queue.submit(Some(encoder.finish()));
                    surface_texture.present();
                    return false;
                }
            }
        }

        let config_width = render.config.width as f32;
        let config_height = render.config.height as f32;
        let skin_w = 800.0f32;
        let skin_h = 600.0f32;
        let scale = (config_width / skin_w).min(config_height / skin_h);
        let offset_x = (config_width - skin_w * scale) / 2.0;
        let offset_y = (config_height - skin_h * scale) / 2.0;

        let skin_judgment_line_y: f32 = if let Some(ref gpu) = render.gpu {
            if let Some(ref skin_res) = gpu.skin {
                if let Some(s) = skin_res.get_skin("o2jam") {
                    s.judgment_line_y as f32
                } else {
                    480.0
                }
            } else {
                480.0
            }
        } else {
            480.0
        };

        if let Some(ref mut gpu) = render.gpu {
            gpu.textured_renderer.begin();
        }

        if let Some(ref mut gpu) = render.gpu {
            if let (Some(atlas), Some(skin_res)) = (&gpu.atlas, &gpu.skin) {
                if let Some(skin) = skin_res.get_skin("o2jam") {
                    const STATIC_SPRITES: &[&str] = &[
                        "bga10",
                        "note_bg",
                        "dashboard",
                        "lifebar_bg",
                        "judgmentarea",
                        "lifebar",
                    ];

                    for entity in &skin.entities {
                        let sprite_id = match &entity.sprite {
                            Some(s) => s,
                            None => continue,
                        };
                        let first_sprite = sprite_id.split(',').next().unwrap_or(sprite_id).trim();
                        if !STATIC_SPRITES.contains(&first_sprite) {
                            continue;
                        }
                        if let Some(atlas_frame) = atlas.get_frame(first_sprite) {
                            let frame_w = atlas_frame.width as f32 * scale;
                            let frame_h = atlas_frame.height as f32 * scale;
                            let frame_x = offset_x + entity.x as f32 * scale;
                            let frame_y = offset_y + entity.y as f32 * scale;

                            gpu.textured_renderer.draw_textured_quad(
                                frame_x,
                                frame_y,
                                frame_w,
                                frame_h,
                                atlas_frame.uv,
                                [1.0, 1.0, 1.0, 1.0],
                            );
                        }
                    }
                }
            }
        }

        let metrics = RenderMetrics {
            scale: mint::Vector2 { x: scale, y: scale },
            offset: mint::Vector2 {
                x: offset_x,
                y: offset_y,
            },
            judgment_line_y: skin_judgment_line_y,
        };

        let game_ended = {
            let gs = self.game_state.as_ref();
            if let (Some(ref mut gpu), Some(_)) = (&mut render.gpu, gs) {
                crate::render_game::render_game(
                    gpu,
                    gs.unwrap(),
                    render.config.width,
                    render.config.height,
                    &metrics,
                    &mut self.audio,
                    self.game_start_instant,
                    &mut self.hybrid_clock_prev,
                    &mut self.hybrid_clock_prev_delta,
                    &mut self.hybrid_clock_frame_count,
                )
            } else {
                false
            }
        };

        if let Some(ref mut gpu) = render.gpu {
            gpu.textured_renderer
                .end(&view, &render.queue, &render.device);
        }

        surface_texture.present();

        game_ended
            || self
                .game_state
                .as_ref()
                .map_or(false, |gs| gs.is_song_ended())
    }
}
