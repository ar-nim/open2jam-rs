mod frame;
mod game_ctx;
mod menu_ctx;
mod render_ctx;
mod wgpu_init;
mod window;

pub use frame::FrameLimiter;
pub use game_ctx::GameCtx;
pub use menu_ctx::MenuCtx;
pub use render_ctx::RenderCtx;

use std::collections::HashMap;
use std::time::Instant;

use anyhow::Result;
use log::info;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::Key;
use winit::window::{Window, WindowId};

use crate::input;
use crate::types::{AppMode, LaneIndex, RenderMetrics};

pub struct App {
    ojn_path: Option<std::path::PathBuf>,
    event_loop: Option<EventLoop<()>>,
    render_ctx: Option<RenderCtx>,
    audio_mgr: Option<crate::audio::manager::AudioManager>,
    mode: AppMode,
    menu_ctx: Option<MenuCtx>,
    game_ctx: Option<GameCtx>,
    config: open2jam_rs_core::Config,
    last_frame_time: Option<Instant>,
    auto_play: bool,
    display_width: u32,
    display_height: u32,
    display_fullscreen: bool,
    scroll_speed: f64,
    difficulty: open2jam_rs_core::Difficulty,
    vsync_mode: open2jam_rs_core::game_options::VSyncMode,
    fps_limiter: open2jam_rs_core::game_options::FpsLimiter,
    key_to_lane: HashMap<String, LaneIndex>,
    frame_limiter: Option<frame::FrameLimiter>,
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

        let menu_ctx = if mode == AppMode::Menu {
            Some(MenuCtx::new()?)
        } else {
            None
        };

        let game_ctx = if mode == AppMode::Playing {
            Some(GameCtx::new())
        } else {
            None
        };

        Ok(Self {
            ojn_path,
            event_loop: None,
            render_ctx: None,
            audio_mgr: None,
            mode,
            menu_ctx,
            game_ctx,
            config: config.clone(),
            last_frame_time: None,
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
        let audio_mgr = crate::audio::manager::AudioManager::new();
        if audio_mgr.is_active() {
            info!("Audio manager active.");
        } else {
            info!("Audio manager failed to initialise (running headless).");
        }
        self.audio_mgr = Some(audio_mgr);

        let event_loop = self.event_loop.take().unwrap();
        event_loop.run_app(&mut self)?;

        self.cleanup();

        info!("App shutting down.");
        Ok(())
    }

    fn cleanup(&mut self) {
        if let Some(ref mut menu_ctx) = self.menu_ctx {
            menu_ctx.cleanup();
        }
        if let Some(ref mut render_ctx) = self.render_ctx {
            render_ctx.shutdown();
        }
        if let Some(ref mut game_ctx) = self.game_ctx {
            game_ctx.cleanup();
        }
        self.game_ctx.take();
        self.audio_mgr.take();
        self.menu_ctx.take();
        self.render_ctx.take();
        info!("All resources cleaned up in correct order.");
    }

    fn setup_frame_limiter(&mut self) {
        let monitor = self
            .render_ctx
            .as_ref()
            .and_then(|r| r.window.current_monitor());
        self.frame_limiter =
            frame::setup_frame_limiter(self.vsync_mode, self.fps_limiter, monitor.as_ref());
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        self.on_resumed(el)
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {}

    fn window_event(&mut self, el: &ActiveEventLoop, wid: WindowId, ev: WindowEvent) {
        self.on_window_event(el, wid, ev)
    }
}

impl App {
    pub fn on_resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.render_ctx.is_some() {
            return;
        }

        let inner_size = window::compute_inner_size(
            self.mode,
            self.display_width,
            self.display_height,
            event_loop.primary_monitor().as_ref(),
        );

        info!(
            "Creating window ({}x{}, fullscreen={}) and initialising wgpu...",
            inner_size.width, inner_size.height, self.display_fullscreen
        );

        let window_attrs =
            window::build_window_attributes(inner_size, self.mode, self.display_fullscreen);

        let window = event_loop.create_window(window_attrs).unwrap();
        self.init_wgpu(window);

        if self.ojn_path.is_none() {
            info!("No OJN file specified — running in demo mode (no notes)");
        }

        self.last_frame_time = Some(Instant::now());

        if let Some(ref render_ctx) = self.render_ctx {
            render_ctx.window.request_redraw();
        }
    }

    fn init_wgpu(&mut self, window: Window) {
        let (window, _, surface, _, device, queue, config) =
            wgpu_init::init_wgpu(window, self.vsync_mode);

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

        let (gpu, skin_scale) = wgpu_init::build_gpu_resources(
            &device,
            &queue,
            &config,
            skin_dir,
            self.ojn_path.as_deref(),
        );

        if let Some(ref mut menu_ctx) = self.menu_ctx {
            menu_ctx.init_egui(&window, &device, &queue, &config);
        }

        self.setup_frame_limiter();

        self.render_ctx = Some(RenderCtx {
            window,
            surface: Some(surface),
            device,
            queue,
            config,
            gpu: Some(gpu),
            skin_scale,
        });
    }

    pub fn on_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Close requested, cleaning up...");
                self.cleanup();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(ref mut render_ctx) = self.render_ctx {
                    render_ctx.resize(size);
                }
            }
            WindowEvent::KeyboardInput {
                event: ref key_event,
                ..
            } => {
                if self.mode == AppMode::Menu {
                    if let (Some(render_ctx), Some(menu_ctx)) =
                        (&self.render_ctx, &mut self.menu_ctx)
                    {
                        if let (Some(egui_winit), render_ctx) =
                            (menu_ctx.egui_winit.as_mut(), render_ctx)
                        {
                            let response = egui_winit.on_window_event(&render_ctx.window, &event);
                            if response.repaint {
                                render_ctx.window.request_redraw();
                            }
                            if response.consumed {
                                return;
                            }
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

                    let Some(game_ctx) = &mut self.game_ctx else {
                        return;
                    };
                    let Some(gs) = &mut game_ctx.game_state else {
                        return;
                    };

                    let os_timestamp = std::time::Instant::now();
                    match key_event.state {
                        ElementState::Pressed => {
                            if !gs.pressed_lanes[lane.index()] {
                                if let Some(ref mut audio_mgr) = self.audio_mgr {
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
                if let Some(ref mut game_ctx) = self.game_ctx {
                    if let Some(gs) = game_ctx.poll_loading() {
                        game_ctx.game_state = Some(gs);
                    }
                }

                if self.game_ctx.is_none()
                    && self.ojn_path.is_some()
                    && self
                        .game_ctx
                        .as_ref()
                        .map_or(true, |gc| gc.loading_state.is_none())
                {
                    if let Some(ref mut game_ctx) = self.game_ctx {
                        game_ctx.set_start_load(true);
                    }
                }

                if let Some(ref mut game_ctx) = self.game_ctx {
                    if game_ctx.start_load_game_state && game_ctx.loading_state.is_none() {
                        if let Some(ref ojn_path) = self.ojn_path {
                            let skin_res = self
                                .render_ctx
                                .as_ref()
                                .and_then(|r| r.gpu.as_ref())
                                .and_then(|g| g.skin.clone());
                            game_ctx.start_loading(
                                ojn_path.clone(),
                                self.scroll_speed,
                                self.auto_play,
                                self.difficulty,
                                skin_res,
                            );
                        }
                    }
                }

                let song_ended = self.render_frame();
                if song_ended {
                    info!("Song ended, exiting game loop");
                    self.cleanup();
                    event_loop.exit();
                    return;
                }
                if let Some(ref render_ctx) = self.render_ctx {
                    render_ctx.window.request_redraw();
                }
                if let Some(ref mut limiter) = self.frame_limiter {
                    limiter.wait();
                }
            }
            WindowEvent::MouseInput { .. }
            | WindowEvent::CursorMoved { .. }
            | WindowEvent::MouseWheel { .. } => {
                if self.mode == AppMode::Menu {
                    if let (Some(render_ctx), Some(menu_ctx)) =
                        (&self.render_ctx, &mut self.menu_ctx)
                    {
                        if let Some(egui_winit) = menu_ctx.egui_winit.as_mut() {
                            let response = egui_winit.on_window_event(&render_ctx.window, &event);
                            if response.repaint {
                                render_ctx.window.request_redraw();
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn render_frame(&mut self) -> bool {
        use std::sync::atomic::Ordering;

        let Some(ref mut render_ctx) = self.render_ctx else {
            log::warn!("render_frame: render_ctx is None");
            return false;
        };

        if self.mode == AppMode::Menu {
            if let Some(ref mut menu_ctx) = self.menu_ctx {
                if let Some(ref mut menu) = menu_ctx.menu_app {
                    let raw_input = menu_ctx
                        .egui_winit
                        .as_mut()
                        .unwrap()
                        .take_egui_input(&render_ctx.window);

                    let full_output = menu_ctx.egui_ctx.run_ui(raw_input, |ui| {
                        menu.ui(ui);
                    });

                    let pixels_per_point = menu_ctx.egui_ctx.pixels_per_point();
                    let clipped_primitives = menu_ctx
                        .egui_ctx
                        .tessellate(full_output.shapes, pixels_per_point);

                    if let Some(surface) = &render_ctx.surface {
                        let surface_texture = match surface.get_current_texture() {
                            wgpu::CurrentSurfaceTexture::Success(st) => st,
                            wgpu::CurrentSurfaceTexture::Suboptimal(st) => st,
                            _ => return false,
                        };

                        let view = surface_texture
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());

                        let mut encoder = render_ctx.device.create_command_encoder(
                            &wgpu::CommandEncoderDescriptor {
                                label: Some("egui_encoder"),
                            },
                        );

                        let screen_descriptor = egui_wgpu::ScreenDescriptor {
                            size_in_pixels: [render_ctx.config.width, render_ctx.config.height],
                            pixels_per_point,
                        };

                        if let Some(ref mut renderer) = menu_ctx.egui_renderer {
                            renderer.update_buffers(
                                &render_ctx.device,
                                &render_ctx.queue,
                                &mut encoder,
                                &clipped_primitives,
                                &screen_descriptor,
                            );

                            for (id, image_delta) in &full_output.textures_delta.set {
                                renderer.update_texture(
                                    &render_ctx.device,
                                    &render_ctx.queue,
                                    *id,
                                    image_delta,
                                );
                            }
                        }

                        if let Some(ref mut renderer) = menu_ctx.egui_renderer {
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

                        if let Some(ref mut renderer) = menu_ctx.egui_renderer {
                            for id in &full_output.textures_delta.free {
                                renderer.free_texture(id);
                            }
                        }

                        render_ctx.queue.submit(Some(encoder.finish()));
                        surface_texture.present();
                    }
                }
            }
            return false;
        }

        let now = Instant::now();
        self.last_frame_time = Some(now);

        if let Some(ref mut game_ctx) = self.game_ctx {
            if game_ctx.game_state.is_none()
                && game_ctx.loading_state.is_none()
                && self.ojn_path.is_some()
            {
                game_ctx.start_load_game_state = true;
            }

            if game_ctx.game_start_instant.is_none() {
                game_ctx.game_start_instant = Some(now);
                game_ctx.hybrid_clock_prev = None;
                game_ctx.hybrid_clock_prev_delta = None;
                game_ctx.hybrid_clock_frame_count = 0;
            }

            let elapsed_ms = game_ctx
                .game_start_instant
                .map(|t| now.duration_since(t).as_millis() as u64)
                .unwrap_or(0);

            if let Some(ref mut gs) = game_ctx.game_state {
                let prev_jam_combo = gs.stats.jam_combo;
                let prev_max_combo = gs.stats.max_combo;
                gs.update(elapsed_ms);

                if gs.is_rendering {
                    if gs.startup_audio_pending {
                        gs.startup_audio_pending = false;
                        if let Some(ref mut audio_mgr) = self.audio_mgr {
                            audio_mgr.play();
                        }
                    }

                    gs.spawn_notes();
                    if let Some(ref mut audio_mgr) = self.audio_mgr {
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

                    if let Some(ref mut audio_mgr) = self.audio_mgr {
                        let base = Instant::now();
                        let (now_ms, delta_ms, monotonic) = audio_mgr.validate_hybrid_clock(
                            base,
                            10.0,
                            &mut game_ctx.hybrid_clock_prev,
                            &mut game_ctx.hybrid_clock_prev_delta,
                            &mut game_ctx.hybrid_clock_frame_count,
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
        }

        let Some(ref surface) = render_ctx.surface else {
            return false;
        };
        let surface_texture = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(st) => st,
            wgpu::CurrentSurfaceTexture::Suboptimal(st) => st,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return false
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                surface.configure(&render_ctx.device, &render_ctx.config);
                return false;
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Validation => {
                return false
            }
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        if self
            .game_ctx
            .as_ref()
            .map_or(true, |gc| gc.game_state.is_none())
        {
            if let Some(ref gpu) = render_ctx.gpu {
                if let (Some(pipeline), Some(bind_group)) =
                    (&gpu.cover_pipeline, &gpu.cover_bind_group)
                {
                    let mut encoder =
                        render_ctx
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
                    render_ctx.queue.submit(Some(encoder.finish()));
                    surface_texture.present();
                    return false;
                }
            }
        }

        let config_width = render_ctx.config.width as f32;
        let config_height = render_ctx.config.height as f32;
        let skin_w = 800.0f32;
        let skin_h = 600.0f32;
        let scale = (config_width / skin_w).min(config_height / skin_h);
        let offset_x = (config_width - skin_w * scale) / 2.0;
        let offset_y = (config_height - skin_h * scale) / 2.0;

        let skin_judgment_line_y: f32 = if let Some(ref gpu) = render_ctx.gpu {
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

        if let Some(ref mut gpu) = render_ctx.gpu {
            gpu.textured_renderer.begin();
        }

        if let Some(ref mut gpu) = render_ctx.gpu {
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

        let audio_mgr = self.audio_mgr.as_mut();

        let game_ended = if let (Some(ref mut gpu), Some(ref mut game_ctx)) =
            (&mut render_ctx.gpu, &mut self.game_ctx)
        {
            if let Some(ref mut gs) = game_ctx.game_state {
                crate::render_game::render_game(
                    gpu,
                    gs,
                    render_ctx.config.width,
                    render_ctx.config.height,
                    &metrics,
                    audio_mgr,
                    game_ctx.game_start_instant,
                    &mut game_ctx.hybrid_clock_prev,
                    &mut game_ctx.hybrid_clock_prev_delta,
                    &mut game_ctx.hybrid_clock_frame_count,
                )
            } else {
                false
            }
        } else {
            false
        };

        if let Some(ref mut gpu) = render_ctx.gpu {
            gpu.textured_renderer
                .end(&view, &render_ctx.queue, &render_ctx.device);
        }

        surface_texture.present();

        game_ended
            || self.game_ctx.as_ref().map_or(false, |gc| {
                gc.game_state
                    .as_ref()
                    .map_or(false, |gs| gs.is_song_ended())
            })
    }
}
