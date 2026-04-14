//! Frame orchestrator — unified winit event loop, wgpu device, egui + game.
//!
//! Both the menu GUI and the game engine run in a single winit event loop
//! on the main thread (macOS compliant). State transitions between menu and
//! game happen in-process without spawning a new process.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use anyhow::Result;
use log::{info, warn};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::Key;
use winit::window::Window;

use crate::audio::AudioManager;
use crate::game_state::GameState;
use crate::gameplay::scroll::note_y_position_bpm_aware;
use crate::menu::menu_app::MenuApp;
use crate::parsing::xml::{parse_file as parse_skin_xml, Resources as SkinResources};
use crate::parsing::TimedEvent;
use crate::render::atlas::SkinAtlas;
use crate::render::hud::{render_hud_with_atlas, HudLayout};
use crate::render::textured_renderer::{BlendMode, TexturedRenderer};
use open2jam_rs_core::game_options::SpeedType;
use open2jam_rs_core::Config;

/// Application mode: either showing the menu or playing a game.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AppMode {
    /// Menu GUI: song library, configuration panels.
    Menu,
    /// Game engine: chart playback, note rendering, audio.
    Playing,
}

/// Base scroll speed — multiplied by user-configured speed multiplier.
const BASE_SCROLL_SPEED: f64 = 1.0;

/// Build a key name → lane index mapping from the config's K7 layout.
fn build_key_mapping(keys: &[open2jam_rs_core::key_bindings::KeyMap]) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for (lane, key_map) in keys.iter().enumerate() {
        map.insert(key_map.key.clone(), lane);
    }
    info!("Key bindings loaded: {:?}", map);
    map
}

/// Convert a character from `Key::Character` to the config storage format.
///
/// Config stores keys as winit PhysicalKey names:
///   - Letters: "KeyA".."KeyZ" (always uppercase)
///   - Digits: "Digit0".."Digit9"
///   - Punctuation: "Comma", "Period", "Semicolon", "Quote", "Slash", etc.
///   - Special: "Space"
fn config_key_for_character(c: &str) -> &str {
    // Single ASCII letter → "KeyX"
    if c.len() == 1 {
        let ch = c.chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            // We need to return a &'static str, so use a match
            return match ch.to_ascii_uppercase() {
                'A' => "KeyA",
                'B' => "KeyB",
                'C' => "KeyC",
                'D' => "KeyD",
                'E' => "KeyE",
                'F' => "KeyF",
                'G' => "KeyG",
                'H' => "KeyH",
                'I' => "KeyI",
                'J' => "KeyJ",
                'K' => "KeyK",
                'L' => "KeyL",
                'M' => "KeyM",
                'N' => "KeyN",
                'O' => "KeyO",
                'P' => "KeyP",
                'Q' => "KeyQ",
                'R' => "KeyR",
                'S' => "KeyS",
                'T' => "KeyT",
                'U' => "KeyU",
                'V' => "KeyV",
                'W' => "KeyW",
                'X' => "KeyX",
                'Y' => "KeyY",
                'Z' => "KeyZ",
                _ => c,
            };
        }
        if ch.is_ascii_digit() {
            return match ch {
                '0' => "Digit0",
                '1' => "Digit1",
                '2' => "Digit2",
                '3' => "Digit3",
                '4' => "Digit4",
                '5' => "Digit5",
                '6' => "Digit6",
                '7' => "Digit7",
                '8' => "Digit8",
                '9' => "Digit9",
                _ => c,
            };
        }
        // Punctuation characters
        return match ch {
            ',' => "Comma",
            '.' => "Period",
            ';' => "Semicolon",
            '\'' => "Quote",
            '/' => "Slash",
            '\\' => "Backslash",
            '[' => "BracketLeft",
            ']' => "BracketRight",
            '-' => "Minus",
            '=' => "Equal",
            '`' => "Backquote",
            ' ' => "Space",
            _ => c,
        };
    }
    c
}

/// GPU resources that must be dropped before the device.
/// This wrapper allows explicit drop ordering to prevent segfaults.
struct GpuResources {
    textured_renderer: TexturedRenderer,
    atlas: Option<SkinAtlas>,
    skin: Option<SkinResources>,
    cover_texture: Option<wgpu::Texture>,
    cover_bind_group: Option<wgpu::BindGroup>,
    cover_pipeline: Option<wgpu::RenderPipeline>,
    cover_sampler: Option<wgpu::Sampler>,
}

/// Message sent from background loading thread to main thread.
enum LoadingMessage {
    /// Game state loaded successfully
    GameLoaded(Result<GameState>),
}

/// Background loading state
struct LoadingState {
    receiver: mpsc::Receiver<LoadingMessage>,
    _thread: thread::JoinHandle<()>,
}

struct RenderState {
    window: Window,
    surface: Option<wgpu::Surface<'static>>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    gpu: Option<GpuResources>,
    skin_scale: (f32, f32),
}

impl RenderState {
    /// Clean up GPU resources in the correct order to prevent segfaults.
    fn shutdown(&mut self) {
        self.gpu.take();
        self.surface.take();
        info!("RenderState shutdown complete.");
    }
}

pub struct App {
    ojn_path: Option<std::path::PathBuf>,
    event_loop: Option<EventLoop<()>>,
    render: Option<RenderState>,
    audio: Option<AudioManager>,
    /// Current mode — determines what to render and where to route input.
    mode: AppMode,
    /// Menu GUI (active when mode == Menu).
    menu_app: Option<MenuApp>,
    /// Game engine state (active when mode == Playing).
    game_state: Option<GameState>,
    /// Config, shared between menu and game.
    config: Config,
    /// egui context for menu rendering.
    egui_ctx: egui::Context,
    /// egui-winit integration for input handling.
    egui_winit: Option<egui_winit::State>,
    /// egui-wgpu renderer. Created in resumed() alongside wgpu device.
    egui_renderer: Option<egui_wgpu::Renderer>,
    last_frame_time: Option<Instant>,
    /// When the gameplay started (for absolute time, no delta accumulation).
    game_start_instant: Option<Instant>,
    /// Whether to start loading the game state on next frame.
    start_load_game_state: bool,
    /// Background loading state (Some while loading is in progress)
    loading_state: Option<LoadingState>,
    /// Hybrid clock validation state (tracks previous frame time for monotonicity checks).
    hybrid_clock_prev: Option<f64>,
    /// Running average of hybrid clock delta (for jitter detection).
    hybrid_clock_prev_delta: Option<f64>,
    /// Frame counter for clock validation warmup.
    hybrid_clock_frame_count: u64,
    /// Whether auto-play mode is enabled (false = manual input mode).
    auto_play: bool,
    /// Config-driven game settings
    display_width: u32,
    display_height: u32,
    display_fullscreen: bool,
    scroll_speed: f64,
    difficulty: open2jam_rs_core::Difficulty,
    vsync_mode: open2jam_rs_core::game_options::VSyncMode,
    fps_limiter: open2jam_rs_core::game_options::FpsLimiter,
    /// Key name → lane index mapping (built from config)
    key_to_lane: HashMap<String, usize>,
    /// Hybrid spin-sleep frame limiter state.
    frame_limiter: Option<FrameLimiter>,
}

/// Hybrid spin-sleep frame limiter (same approach as open2jam-modern Java).
///
/// Sleep in 1ms increments, stop 1ms early, then spin-wait for nanosecond
/// precision. Uses absolute target time to prevent drift.
struct FrameLimiter {
    target_frame_duration_ns: u64,
    next_frame_deadline: std::time::Instant,
}

impl FrameLimiter {
    fn new(target_fps: f64) -> Self {
        let target_frame_duration_ns = (1_000_000_000.0 / target_fps) as u64;
        Self {
            target_frame_duration_ns,
            next_frame_deadline: std::time::Instant::now()
                + std::time::Duration::from_nanos(target_frame_duration_ns),
        }
    }

    /// Wait until the next frame boundary using hybrid sleep + spin-wait.
    fn wait(&mut self) {
        let target_ns = self.next_frame_deadline.elapsed().as_nanos() as i64;
        if target_ns >= 0 {
            // Already past deadline — reset to now + duration (no wait)
            self.next_frame_deadline =
                std::time::Instant::now() + std::time::Duration::from_nanos(self.target_frame_duration_ns);
            return;
        }
        // time_remaining_ns = -target_ns (positive)
        let time_remaining_ns = (-target_ns) as u64;

        // Phase 1: Sleep in 1ms increments, stopping 1ms early
        let sleep_until_ns = time_remaining_ns.saturating_sub(1_000_000);
        if sleep_until_ns > 0 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_nanos(sleep_until_ns);
            while std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }

        // Phase 2: Spin-wait for final <1ms (nanosecond precision)
        while std::time::Instant::now() < self.next_frame_deadline {
            std::hint::spin_loop();
        }

        // Phase 3: Advance deadline (accumulates to prevent drift)
        self.next_frame_deadline += std::time::Duration::from_nanos(self.target_frame_duration_ns);
    }
}

impl App {
    pub fn new(
        ojn_path: Option<std::path::PathBuf>,
        auto_play: bool,
        config: &Config,
    ) -> Result<Self> {
        let opts = &config.game_options;

        // Compute scroll speed from config
        let scroll_speed = if opts.speed_type == SpeedType::HiSpeed {
            BASE_SCROLL_SPEED * opts.speed_multiplier as f64
        } else {
            BASE_SCROLL_SPEED
        };
        info!(
            "Scroll speed: {:.1} (speed_type={:?}, multiplier={:.1})",
            scroll_speed, opts.speed_type, opts.speed_multiplier
        );

        // Determine starting mode
        let mode = if ojn_path.is_some() {
            AppMode::Playing
        } else {
            AppMode::Menu
        };

        // Create menu app if starting in menu mode
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
            key_to_lane: build_key_mapping(&config.key_bindings.k7.lanes),
            frame_limiter: None,
        })
    }

    pub fn run(mut self) -> Result<()> {
        info!("Initialising winit event loop...");
        let event_loop = EventLoop::new()?;
        // Poll: continuously process events. VSync present() or the frame
        // limiter already throttle the frame rate — we need the event loop
        // to deliver RedrawRequested events immediately, not after a delay.
        // ControlFlow::Wait caused RedrawRequested delivery to be delayed on
        // some platforms, resulting in sluggish FPS.
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

        // Explicit cleanup in correct order before App is dropped
        self.cleanup();

        info!("App shutting down.");
        Ok(())
    }

    /// Explicit resource teardown in correct dependency order.
    /// This prevents double-panics during Drop when wgpu resources are invalid.
    fn cleanup(&mut self) {
        // 1. Drop egui renderer first (uses wgpu device/queue)
        self.egui_renderer.take();
        // 2. Drop egui_winit state (may hold clipboard/OS resources)
        self.egui_winit.take();
        // 3. Drop egui context (may hold font textures)
        self.egui_ctx = egui::Context::default();
        // 4. Drop game GPU resources (textures, atlases, pipelines)
        if let Some(render) = &mut self.render {
            render.gpu.take();
        }
        // 5. Shutdown render state (drops surface — child of device)
        if let Some(render) = &mut self.render {
            render.shutdown();
        }
        // 6. Drop game state (may hold audio triggers, sound cache)
        self.game_state.take();
        // 7. Drop audio manager (stops cpal stream)
        self.audio.take();
        // 8. Drop menu app (SQLite pool, background threads)
        self.menu_app.take();
        // 9. Drop render state (drops device/queue — parent of everything)
        self.render.take();
        info!("All resources cleaned up in correct order.");
    }

    /// Set up the hybrid spin-sleep frame limiter based on monitor refresh rate
    /// and the configured FPS limiter multiplier.
    fn setup_frame_limiter(&mut self) {
        use open2jam_rs_core::game_options::{FpsLimiter, VSyncMode};

        // Skip if VSync is On (wgpu handles pacing)
        if self.vsync_mode == VSyncMode::On {
            return;
        }

        // Skip if Unlimited
        if self.fps_limiter == FpsLimiter::Unlimited {
            return;
        }

        // Get the refresh rate from the window's current monitor
        let base_hz = self
            .render
            .as_ref()
            .and_then(|r| r.window.current_monitor())
            .and_then(|monitor| {
                // Get the video mode with the highest refresh rate (likely the native one)
                let modes: Vec<_> = monitor.video_modes().collect();
                modes
                    .into_iter()
                    .max_by_key(|vm| vm.refresh_rate_millihertz())
            })
            .map(|vm| vm.refresh_rate_millihertz() as f64 / 1000.0)
            .unwrap_or(60.0); // Fallback to 60 Hz

        let multiplier = match self.fps_limiter {
            FpsLimiter::X1 => 1.0,
            FpsLimiter::X2 => 2.0,
            FpsLimiter::X4 => 4.0,
            FpsLimiter::X8 => 8.0,
            FpsLimiter::Unlimited => 1.0, // unreachable, handled above
        };

        let target_fps = base_hz * multiplier;
        self.frame_limiter = Some(FrameLimiter::new(target_fps));
        info!(
            "Frame limiter: {:.0} Hz × {:.0} = {:.0} fps",
            base_hz, multiplier, target_fps
        );
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.render.is_none() {
            info!(
                "Creating window ({}x{}, fullscreen={}) and initialising wgpu...",
                self.display_width, self.display_height, self.display_fullscreen
            );

            let mut attrs = winit::window::WindowAttributes::default()
                .with_title("open2jam-rs")
                .with_inner_size(winit::dpi::LogicalSize::new(
                    self.display_width as f64,
                    self.display_height as f64,
                ))
                .with_visible(true)
                .with_resizable(true);

            if self.display_fullscreen {
                attrs = attrs.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
            }

            let window = event_loop.create_window(attrs).unwrap();
            self.init_wgpu(window);

            // Set up egui-winit integration for input handling
            if let Some(render) = &self.render {
                self.egui_winit = Some(egui_winit::State::new(
                    self.egui_ctx.clone(),
                    egui::ViewportId::ROOT,
                    &render.window,
                    None, // native_pixels_per_point
                    None, // theme
                    None, // max_texture_side
                ));
                info!("egui-winit state initialised");
            }

            // Configure Noto Sans CJK as the sole font (covers Latin + CJK)
            configure_fonts(&self.egui_ctx);

            // Set up frame limiter based on monitor refresh rate
            self.setup_frame_limiter();

            if self.ojn_path.is_none() {
                info!("No OJN file specified — running in demo mode (no notes)");
            }

            self.last_frame_time = Some(Instant::now());

            // CRITICAL: Request the first redraw. Without this, ControlFlow::Wait
            // will never wake up and no frames will be rendered.
            if let Some(render) = &self.render {
                render.window.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // No-op: we request redraws explicitly when needed, not on every wake.
        // The frame limiter runs AFTER render_frame() completes.
    }

    fn window_event(
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
                return;
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
            WindowEvent::KeyboardInput { event: ref key_event, .. } => {
                // Route to egui first in Menu mode
                if self.mode == AppMode::Menu {
                    if let (Some(render), Some(egui_winit)) =
                        (&self.render, &mut self.egui_winit)
                    {
                        let response = egui_winit.on_window_event(&render.window, &event);
                        if response.repaint {
                            render.window.request_redraw();
                        }
                        if response.consumed {
                            return;
                        }
                    }
                }

                // Process lane key input for game mode
                if self.mode != AppMode::Playing {
                    return;
                }

                use winit::event::ElementState;
                use winit::keyboard::NamedKey;

                let lane = match &key_event.logical_key {
                    Key::Character(c) => {
                        let lookup = config_key_for_character(c);
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
                    if let Some(gs) = &mut self.game_state {
                        let os_timestamp = std::time::Instant::now();
                        match key_event.state {
                            ElementState::Pressed => {
                                if !gs.pressed_lanes[lane] {
                                    if let Some(audio_mgr) = &mut self.audio {
                                        gs.handle_key_press(lane, os_timestamp, audio_mgr);
                                    }
                                }
                            }
                            ElementState::Released => {
                                gs.handle_key_release(lane, os_timestamp);
                            }
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
                // Check for messages from background loading thread
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

                // Start background loading if needed
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
                // Request next frame redraw
                if let Some(render) = &self.render {
                    render.window.request_redraw();
                }
                // Frame limiter AFTER rendering (controls FPS)
                if let Some(ref mut limiter) = self.frame_limiter {
                    limiter.wait();
                }
            }
            WindowEvent::MouseInput { .. }
            | WindowEvent::CursorMoved { .. }
            | WindowEvent::MouseWheel { .. } => {
                // Route mouse events to egui in Menu mode
                if self.mode == AppMode::Menu {
                    if let (Some(render), Some(egui_winit)) =
                        (&self.render, &mut self.egui_winit)
                    {
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
        info!("Initialising wgpu...");
        let instance = wgpu::Instance::default();

        let raw_display_handle = window.display_handle().unwrap().as_raw();
        let raw_window_handle = window.window_handle().unwrap().as_raw();

        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(raw_display_handle),
                    raw_window_handle,
                })
                .expect("Failed to create surface")
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .expect("Failed to find adapter");

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .expect("Failed to create device");

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let present_mode = match self.vsync_mode {
            open2jam_rs_core::game_options::VSyncMode::On => wgpu::PresentMode::AutoVsync,
            open2jam_rs_core::game_options::VSyncMode::Fast => wgpu::PresentMode::Mailbox,
            open2jam_rs_core::game_options::VSyncMode::Off => wgpu::PresentMode::AutoNoVsync,
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let mut textured_renderer = TexturedRenderer::new(&device, &queue, &config);

        // Try to load the skin XML and build atlas
        let skin_dir = std::path::Path::new("resources");
        let (atlas, skin, skin_scale) = Self::load_skin(&device, &queue, skin_dir);

        if let Some(ref atlas) = atlas {
            textured_renderer.set_atlas(&device, atlas);
        }

        // Load OJN file to extract cover image (before game state loads it)
        let (cover_texture, cover_bind_group, cover_pipeline, cover_sampler) =
            if let Some(ref path) = self.ojn_path {
                Self::load_cover_from_ojn(&device, &queue, &config, path)
            } else {
                (None, None, None, None)
            };

        let gpu = GpuResources {
            textured_renderer,
            atlas,
            skin,
            cover_texture,
            cover_bind_group,
            cover_pipeline,
            cover_sampler,
        };

        info!(
            "wgpu surface configured: {}x{}",
            config.width, config.height
        );
        self.render = Some(RenderState {
            window,
            surface: Some(surface),
            device,
            queue,
            config,
            gpu: Some(gpu),
            skin_scale,
        });

        // Create egui-wgpu renderer (shares the same wgpu device/queue)
        if let Some(render) = &self.render {
            self.egui_renderer = Some(egui_wgpu::Renderer::new(
                &render.device,
                render.config.format,
                egui_wgpu::RendererOptions::default(),
            ));
            info!("egui-wgpu renderer initialised (wgpu 29)");
        }
    }

    /// Load skin XML, parse frames, build texture atlas.
    fn load_skin(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        skin_dir: &std::path::Path,
    ) -> (Option<SkinAtlas>, Option<SkinResources>, (f32, f32)) {
        let xml_path = skin_dir.join("resources.xml");
        if !xml_path.exists() {
            info!("Skin XML not found at {}", xml_path.display());
            return (None, None, (1.0, 1.0));
        }

        let resources = match parse_skin_xml(&xml_path) {
            Ok(r) => r,
            Err(e) => {
                info!("Failed to parse skin XML: {e:?}");
                return (None, None, (1.0, 1.0));
            }
        };

        let _skin_def = match resources.get_skin("o2jam") {
            Some(s) => s.clone(),
            None => {
                info!("Skin 'o2jam' not found in resources.xml");
                return (None, None, (1.0, 1.0));
            }
        };

        // Build atlas from global sprites (all sprite definitions in the XML)
        // Collect frame_speed map for animated sprites
        let mut sprite_speeds: HashMap<String, u32> = HashMap::new();
        let mut frame_entries: Vec<(String, String, u32, u32, u32, u32)> = Vec::new();
        for (sprite_id, sprite_def) in &resources.sprites {
            sprite_speeds.insert(sprite_id.clone(), sprite_def.frame_speed_ms);
            for frame in &sprite_def.frames {
                frame_entries.push((
                    sprite_id.clone(),
                    frame.file.to_string_lossy().to_string(),
                    frame.x,
                    frame.y,
                    frame.w,
                    frame.h,
                ));
            }
        }

        info!(
            "Skin has {} sprite frames to pack into atlas",
            frame_entries.len()
        );

        let speed_map = sprite_speeds;
        let atlas = SkinAtlas::from_frames_with_speed(
            device,
            queue,
            &frame_entries,
            |sprite_id: &str| *speed_map.get(sprite_id).unwrap_or(&50),
            |file: &str| {
                // Paths already include the skin_dir prefix from the XML parser's base_path
                let path = Path::new(file);
                if !path.exists() {
                    info!("Skin image not found: {}", path.display());
                }
                match image::open(path) {
                    Ok(img) => Some(img.into_rgba8()),
                    Err(e) => {
                        info!("Failed to load skin image {}: {e}", path.display());
                        None
                    }
                }
            },
        );

        if let Some(ref a) = atlas {
            info!(
                "Atlas built: {} frames in {}x{} texture",
                a.frames.len(),
                a.width,
                a.height
            );
            for key in &[
                "head_note_white",
                "head_note_blue",
                "head_note_yellow",
                "judgmentarea",
                "note_bg",
                "measure_mark",
            ] {
                if let Some(f) = a.get_frame(key) {
                    info!("  [OK] {} -> uv={:?}, {}x{}", key, f.uv, f.width, f.height);
                } else {
                    info!("  [MISSING] {}", key);
                }
            }
        } else {
            info!("Atlas failed to build — using colored quad fallback");
        }

        info!("Skin loaded: 800x600");

        (atlas, Some(resources), (1.0, 1.0))
    }

    /// Load cover image from OJN file and create a fullscreen-textured quad.
    fn load_cover_from_ojn(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        ojn_path: &std::path::Path,
    ) -> (
        Option<wgpu::Texture>,
        Option<wgpu::BindGroup>,
        Option<wgpu::RenderPipeline>,
        Option<wgpu::Sampler>,
    ) {
        let data = match std::fs::read(ojn_path) {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to read OJN file for cover: {e}");
                return (None, None, None, None);
            }
        };

        let jpeg_bytes = match crate::parsing::extract_cover_image(&data) {
            Ok(b) => b,
            Err(e) => {
                warn!("No cover image in OJN: {e}");
                return (None, None, None, None);
            }
        };

        let img = match image::load_from_memory(&jpeg_bytes) {
            Ok(i) => i,
            Err(e) => {
                warn!("Failed to decode cover JPEG: {e}");
                return (None, None, None, None);
            }
        };

        let rgba = img.into_rgba8();
        let (w, h) = rgba.dimensions();
        info!("Cover image: {}x{}", w, h);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            label: Some("cover_texture"),
            view_formats: &[],
        });

        queue.write_texture(
            texture.as_image_copy(),
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * w),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cover_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cover_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("cover_shader.wgsl"));

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cover_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cover_pipeline"),
            layout: Some(&pipeline_layout),
            multiview_mask: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            cache: None,
        });

        info!("Cover texture uploaded to GPU");
        (
            Some(texture),
            Some(bind_group),
            Some(pipeline),
            Some(sampler),
        )
    }

    fn render_frame(&mut self) -> bool {
        use std::sync::atomic::Ordering;

        let Some(render) = &mut self.render else {
            warn!("render_frame: render state is None");
            return false;
        };

        // ── Menu mode: render egui ──
        if self.mode == AppMode::Menu {
            if let Some(menu) = &mut self.menu_app {
                let raw_input = self
                    .egui_winit
                    .as_mut()
                    .unwrap()
                    .take_egui_input(&render.window);

                let full_output = self.egui_ctx.run(raw_input, |ctx| {
                    menu.ui(ctx);
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
                            renderer.update_texture(&render.device, &render.queue, *id, image_delta);
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

        // ── Game mode ──
        // If game state is None and not already loading, schedule game load for next redraw
        if self.game_state.is_none() && self.loading_state.is_none() && self.ojn_path.is_some() {
            self.start_load_game_state = true;
        }

        // 1. Absolute time — no delta accumulation drift
        let now = Instant::now();
        self.last_frame_time = Some(now);

        // 2. Advance game state using absolute elapsed time
        if let Some(gs) = &mut self.game_state {
            // Record start instant once when game state becomes available
            if self.game_start_instant.is_none() {
                self.game_start_instant = Some(now);
                // Reset clock validation state for a fresh warmup period
                self.hybrid_clock_prev = None;
                self.hybrid_clock_prev_delta = None;
                self.hybrid_clock_frame_count = 0;
            }
            let elapsed_ms = self
                .game_start_instant
                .map(|t| now.duration_since(t).as_millis() as u64)
                .unwrap_or(0);

            let _prev_combo = gs.stats.combo;
            let prev_jam_combo = gs.stats.jam_combo;
            let prev_max_combo = gs.stats.max_combo;
            gs.update(elapsed_ms);

            // Only run gameplay logic after startup delay (is_rendering = true)
            if gs.is_rendering {
                // Start audio stream when startup just completed
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
                gs.cleanup_effects(); // Remove expired effects

                // Trigger combo counter animation when combo increases.
                // Uses gs.prev_frame_combo (persisted across frames) instead of
                // a local variable, because handle_key_press increments combo in
                // the winit event handler BEFORE the render frame starts.
                if gs.stats.combo > gs.prev_frame_combo {
                    gs.combo_counter.increment();
                    // Only show combo title/max combo when starting a new combo streak (combo was 0)
                    if gs.prev_frame_combo == 0 {
                        gs.show_combo_title();
                        gs.show_max_combo_counter();
                    }
                } else if gs.stats.combo == 0 && gs.prev_frame_combo > 0 {
                    gs.combo_counter.reset();
                }

                // Show jam counter when jam combo increases
                if gs.stats.jam_combo > prev_jam_combo {
                    gs.show_jam_counter();
                }

                // Show max combo when max combo increases (new high score)
                if gs.stats.max_combo > prev_max_combo {
                    gs.show_max_combo_counter();
                    gs.show_combo_title();
                }

                // Update prev_frame_combo for the next frame's comparison
                gs.prev_frame_combo = gs.stats.combo;

                if let Some(audio_mgr) = &mut self.audio {
                    // ── Step 1: Hybrid Clock Validation ──
                    let base = Instant::now();
                    let (now_ms, delta_ms, monotonic) = audio_mgr.validate_hybrid_clock(
                        base,
                        10.0, // max jitter ms (deviation from running average); 10ms is normal tolerance at 60fps
                        &mut self.hybrid_clock_prev,
                        &mut self.hybrid_clock_prev_delta,
                        &mut self.hybrid_clock_frame_count,
                    );

                    static FRAME_COUNTER: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(0);
                    let fc = FRAME_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                    if fc % 60 == 0 {
                        log::info!(
                            "Hybrid clock: time={:.1}ms delta={:.3}ms monotonic={} samples={}",
                            now_ms,
                            delta_ms,
                            monotonic,
                            audio_mgr
                                .state()
                                .samples_played
                                .load(std::sync::atomic::Ordering::Relaxed)
                        );
                    }

                    // ── Audio CPU Usage Monitor ──
                    // Log every 600 frames (~10s at 60fps)
                    static CPU_FRAME_COUNTER: std::sync::atomic::AtomicU64 =
                        std::sync::atomic::AtomicU64::new(0);
                    let cpu_fc =
                        CPU_FRAME_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

        // 3. Acquire surface texture
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

        // Draw cover background (only before game state is loaded)
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

        // 4. Calculate skin scale and letterbox offset
        // Scale the 800x600 skin to fit the window while maintaining aspect ratio
        let (config_width, config_height) =
            (render.config.width as f32, render.config.height as f32);
        let skin_w = 800.0f32;
        let skin_h = 600.0f32;
        let scale = (config_width / skin_w).min(config_height / skin_h);
        let offset_x = (config_width - skin_w * scale) / 2.0;
        let offset_y = (config_height - skin_h * scale) / 2.0;
        let (skin_scale_x, skin_scale_y) = (scale, scale);

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

        // 5. Begin textured rendering
        if let Some(ref mut gpu) = render.gpu {
            gpu.textured_renderer.begin();
        }

        // 6. Draw skin entities (background, judgment line, notes)
        if let Some(ref mut gpu) = render.gpu {
            if let (Some(atlas), Some(skin_res)) = (&gpu.atlas, &gpu.skin) {
                if let Some(skin) = skin_res.get_skin("o2jam") {
                    // Whitelist of static sprite IDs to render in the background pass.
                    // NOTE: static_keyboard is NOT here — it must be drawn AFTER notes
                    // so it occludes notes that have passed the judgment line.
                    const STATIC_SPRITES: &[&str] = &[
                        "bga10",
                        "note_bg",
                        "dashboard",
                        "lifebar_bg",
                        "judgmentarea",
                        "lifebar",
                        // "pill" is drawn by HUD based on pill_count
                        // "jam_bar" is drawn by HUD with fill clipping, not as static sprite
                        // "timebar" is drawn by HUD with progress-based fill clipping
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
                            let frame_w = atlas_frame.width as f32 * skin_scale_x;
                            let frame_h = atlas_frame.height as f32 * skin_scale_y;
                            let frame_x = offset_x + entity.x as f32 * skin_scale_x;
                            let frame_y = offset_y + entity.y as f32 * skin_scale_y;

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

        // 7. Draw notes dynamically from game state
        if let (Some(ref mut gpu), Some(gs)) = (&mut render.gpu, &self.game_state) {
            let render_time = gs.clock.render_time();
            let bpm = gs.clock.bpm() as f64;
            let judgment_line_y = skin_judgment_line_y as f64;
            // measure_basis is the value that measure_size = 0.8 × this.
            // In the original Java HiSpeed, this is hardcoded as 385 for a judgment_line of 480,
            // which is approximately 0.8 × 480 = 384. We use judgment_line_y directly for accuracy.
            let measure_basis = judgment_line_y;

            if let (Some(atlas), Some(_skin_res)) = (&gpu.atlas, &gpu.skin) {
                // Only draw chart elements (measure marks, notes, long notes) after startup delay
                // During startup: chart is invisible, not frozen
                if gs.is_rendering {
                    // Draw measure marks scrolling with the notes (animated)
                    if let Some(measure_frame) = atlas
                        .get_frame_at_time("measure_mark", render_time as f64)
                        .or_else(|| atlas.get_frame("measure_mark").copied())
                    {
                        let mw = measure_frame.width as f32 * skin_scale_x;
                        let mh = measure_frame.height as f32 * skin_scale_y;

                        // Center measure mark on the judgment area
                        // judgmentarea: x=3, w=192; measure_mark: w=188 → centered at 3 + (192-188)/2 = 5
                        let mark_skin_x: f32 = 5.0;
                        let mx = offset_x + mark_skin_x * skin_scale_x;

                        for event in &gs.chart.events {
                            if let TimedEvent::Measure(ev) = event {
                                // Skip measures that have already passed the judgment line
                                if render_time > ev.time_ms {
                                    continue;
                                }

                                let y = note_y_position_bpm_aware(
                                    render_time,
                                    ev.time_ms,
                                    &gs.timing,
                                    judgment_line_y,
                                    measure_basis,
                                    gs.scroll_speed,
                                );
                                // Only draw if within viewport (above top, below bottom skip)
                                let screen_y = offset_y + y as f32 * skin_scale_y - mh / 2.0;
                                if screen_y > -mh && screen_y < config_height + mh {
                                    gpu.textured_renderer.draw_textured_quad(
                                        mx,
                                        screen_y,
                                        mw,
                                        mh,
                                        measure_frame.uv,
                                        [1.0, 1.0, 1.0, 0.5],
                                    );
                                }
                            }
                        }
                    }

                    // Draw PRESSED_NOTE lane effects BEFORE notes (behind them).
                    // These are the lane light-up effects at y < judgment_line_y.
                    for lane in 0..7 {
                        if gs.pressed_lanes[lane] {
                            for (sprite_id, x_pos, y_pos) in
                                &gs.note_prefabs.pressed_lane_effects[lane]
                            {
                                if let Some(pressed_frame) = atlas
                                    .get_frame_at_time(sprite_id, render_time as f64)
                                    .or_else(|| atlas.get_frame(sprite_id).copied())
                                {
                                    let sprite_w = pressed_frame.width as f32 * skin_scale_x;
                                    let sprite_h = pressed_frame.height as f32 * skin_scale_y;
                                    let x = offset_x + *x_pos as f32 * skin_scale_x;
                                    let y = offset_y + *y_pos as f32 * skin_scale_y;
                                    gpu.textured_renderer.draw_textured_quad(
                                        x,
                                        y,
                                        sprite_w,
                                        sprite_h,
                                        pressed_frame.uv,
                                        [1.0, 1.0, 1.0, 0.6],
                                    );
                                }
                            }
                        }
                    }

                    // Draw tap notes (layer 5 in original)
                    for note in &gs.active_notes {
                        let y = note_y_position_bpm_aware(
                            render_time,
                            note.target_time_ms,
                            &gs.timing,
                            judgment_line_y,
                            measure_basis,
                            gs.scroll_speed,
                        );

                        let lane_prefab = &gs.note_prefabs.lanes[note.lane];
                        let lane_x = offset_x + lane_prefab.x as f32 * skin_scale_x;

                        let head_frame_name =
                            lane_prefab
                                .sprite_id
                                .as_deref()
                                .unwrap_or_else(|| match note.lane {
                                    0 | 1 | 2 => "head_note_white",
                                    3 => "head_note_blue",
                                    _ => "head_note_yellow",
                                });

                        let head_frame = atlas
                            .get_frame_at_time(head_frame_name, render_time as f64)
                            .or_else(|| atlas.get_frame(head_frame_name).copied());
                        if let Some(head_frame) = head_frame {
                            let note_w = head_frame.width as f32 * skin_scale_x;
                            let note_h = head_frame.height as f32 * skin_scale_y;
                            let x = lane_x;
                            let y = offset_y + y as f32 * skin_scale_y - note_h / 2.0;
                            gpu.textured_renderer.draw_textured_quad(
                                x,
                                y,
                                note_w,
                                note_h,
                                head_frame.uv,
                                [1.0, 1.0, 1.0, 1.0],
                            );
                        }
                    }

                    // Draw long notes (layer 5 in original, same as tap notes)
                    for long_note in &gs.active_long_notes {
                        let lane_prefab = &gs.note_prefabs.lanes[long_note.lane];
                        let lane_x = offset_x + lane_prefab.x as f32 * skin_scale_x;

                        // Calculate head and tail Y positions (BPM-aware)
                        let head_y = note_y_position_bpm_aware(
                            render_time,
                            long_note.head_time_ms,
                            &gs.timing,
                            judgment_line_y,
                            measure_basis,
                            gs.scroll_speed,
                        );

                        let tail_y = note_y_position_bpm_aware(
                            render_time,
                            long_note.tail_time_ms,
                            &gs.timing,
                            judgment_line_y,
                            measure_basis,
                            gs.scroll_speed,
                        );

                        // Determine sprite names for this lane
                        let head_frame_name = lane_prefab
                            .head_sprite
                            .as_deref()
                            .or(lane_prefab.sprite_id.as_deref())
                            .unwrap_or_else(|| match long_note.lane {
                                0 | 1 | 2 => "head_note_white",
                                3 => "head_note_blue",
                                _ => "head_note_yellow",
                            });

                        let body_frame_name = lane_prefab
                            .body_sprite
                            .as_deref()
                            .or(lane_prefab.sprite_id.as_deref())
                            .unwrap_or_else(|| match long_note.lane {
                                0 | 1 | 2 => "body_note_white",
                                3 => "body_note_blue",
                                _ => "body_note_yellow",
                            });

                        // Tail uses the same sprite as head (or can be customized)
                        let tail_frame_name = lane_prefab
                            .tail_sprite
                            .as_deref()
                            .or(lane_prefab.sprite_id.as_deref())
                            .unwrap_or(head_frame_name);

                        if let (Some(head_frame), Some(body_frame), Some(tail_frame)) = (
                            atlas.get_frame(head_frame_name),
                            atlas.get_frame(body_frame_name),
                            atlas.get_frame(tail_frame_name),
                        ) {
                            let note_w = head_frame.width as f32 * skin_scale_x;
                            let head_h = head_frame.height as f32 * skin_scale_y;
                            let tail_h = tail_frame.height as f32 * skin_scale_y;

                            // Long note rendering follows Java pattern:
                            // - head_sprite (tap note) at the bottom (end_y position, near judgment line)
                            // - body_sprite stretched between head and tail
                            // - tail_sprite at the top (tail_y position, where long note started)
                            //
                            // CLAMPING: When the head passes the judgment line (head_y > judgment_line_y),
                            // the head sprite should NOT be drawn, but the body (clipped at the judgment line)
                            // and tail should continue rendering above the judgment line.

                            let judgment_line_screen_y =
                                offset_y + judgment_line_y as f32 * skin_scale_y;
                            let head_unclamped_screen_y =
                                offset_y + head_y as f32 * skin_scale_y - head_h / 2.0;
                            let tail_screen_y =
                                offset_y + tail_y as f32 * skin_scale_y - tail_h / 2.0;

                            // Determine if head is past the judgment line (use skin coords, not scaled screen)
                            let head_past_judgment = head_y > judgment_line_y;

                            // Calculate effective body bottom: if head is past judgment line, clamp to line
                            let effective_head_y = if head_past_judgment {
                                judgment_line_y
                            } else {
                                head_y
                            };
                            let effective_head_screen_y =
                                offset_y + effective_head_y as f32 * skin_scale_y;

                            // In screen coords, body stretches from tail (higher/smaller Y) to head (lower/larger Y)
                            let body_top = tail_screen_y.min(effective_head_screen_y);
                            let body_bottom = tail_screen_y.max(effective_head_screen_y);
                            let body_pixel_height = (body_bottom - body_top).max(0.0);

                            if body_pixel_height > 0.5 {
                                let body_x = lane_x;
                                let body_y = body_top;

                                // Draw order (Java): body first, then tail, then head
                                // 1. Body (middle, stretched) — clipped at judgment line if head is past
                                gpu.textured_renderer.draw_textured_quad(
                                    body_x,
                                    body_y,
                                    note_w,
                                    body_pixel_height,
                                    body_frame.uv,
                                    [1.0, 1.0, 1.0, 1.0],
                                );

                                // 2. Tail (top cap) — only render if above judgment line
                                if !head_past_judgment || tail_y < judgment_line_y {
                                    gpu.textured_renderer.draw_textured_quad(
                                        lane_x,
                                        tail_screen_y,
                                        note_w,
                                        tail_h,
                                        tail_frame.uv,
                                        [1.0, 1.0, 1.0, 1.0],
                                    );
                                }

                                // 3. Head (bottom tap note) — only render if head is above judgment line
                                if !head_past_judgment {
                                    gpu.textured_renderer.draw_textured_quad(
                                        lane_x,
                                        head_unclamped_screen_y - head_h,
                                        note_w,
                                        head_h,
                                        head_frame.uv,
                                        [1.0, 1.0, 1.0, 1.0],
                                    );
                                }
                            }
                        }
                    }

                    // Draw static_keyboard AFTER notes so it occludes notes below the judgment line.
                    // Matches original layer order: notes (5) → static_keyboard (6) → pressed keys (8).
                    if let Some(skin_res) = &gpu.skin {
                        if let Some(skin) = skin_res.get_skin("o2jam") {
                            for entity in &skin.entities {
                                let sprite_id = match &entity.sprite {
                                    Some(s) => s,
                                    None => continue,
                                };
                                let first = sprite_id.split(',').next().unwrap_or(sprite_id).trim();
                                if first != "static_keyboard" {
                                    continue;
                                }
                                if let Some(frame) = atlas.get_frame(first) {
                                    let fw = frame.width as f32 * skin_scale_x;
                                    let fh = frame.height as f32 * skin_scale_y;
                                    let fx = offset_x + entity.x as f32 * skin_scale_x;
                                    let fy = offset_y + entity.y as f32 * skin_scale_y;
                                    gpu.textured_renderer.draw_textured_quad(
                                        fx,
                                        fy,
                                        fw,
                                        fh,
                                        frame.uv,
                                        [1.0, 1.0, 1.0, 1.0],
                                    );
                                }
                            }
                        }
                    }

                    // Draw PRESSED_NOTE overlays on top of static_keyboard (layer 8 in original).
                    for lane in 0..7 {
                        if gs.pressed_lanes[lane] {
                            for (sprite_id, x_pos, y_pos) in
                                &gs.note_prefabs.pressed_keyboard_overlays[lane]
                            {
                                if let Some(pressed_frame) = atlas
                                    .get_frame_at_time(sprite_id, render_time as f64)
                                    .or_else(|| atlas.get_frame(sprite_id).copied())
                                {
                                    let sprite_w = pressed_frame.width as f32 * skin_scale_x;
                                    let sprite_h = pressed_frame.height as f32 * skin_scale_y;
                                    let x = offset_x + *x_pos as f32 * skin_scale_x;
                                    let y = offset_y + *y_pos as f32 * skin_scale_y;
                                    gpu.textured_renderer.draw_textured_quad(
                                        x,
                                        y,
                                        sprite_w,
                                        sprite_h,
                                        pressed_frame.uv,
                                        [1.0, 1.0, 1.0, 0.6],
                                    );
                                }
                            }
                        }
                    }
                } // end if gs.is_rendering

                // Draw PRESSED_NOTE overlays during startup (before gameplay begins)
                // Draw both lane effects (behind) and keyboard overlays (on top)
                if !gs.is_rendering {
                    if let Some(atlas) = &gpu.atlas {
                        for lane in 0..7 {
                            if gs.pressed_lanes[lane] {
                                // Lane effects (y < judgment_line)
                                for (sprite_id, x_pos, y_pos) in
                                    &gs.note_prefabs.pressed_lane_effects[lane]
                                {
                                    if let Some(pressed_frame) = atlas
                                        .get_frame_at_time(sprite_id, render_time as f64)
                                        .or_else(|| atlas.get_frame(sprite_id).copied())
                                    {
                                        let sprite_w = pressed_frame.width as f32 * skin_scale_x;
                                        let sprite_h = pressed_frame.height as f32 * skin_scale_y;
                                        let x = offset_x + *x_pos as f32 * skin_scale_x;
                                        let y = offset_y + *y_pos as f32 * skin_scale_y;
                                        gpu.textured_renderer.draw_textured_quad(
                                            x,
                                            y,
                                            sprite_w,
                                            sprite_h,
                                            pressed_frame.uv,
                                            [1.0, 1.0, 1.0, 0.6],
                                        );
                                    }
                                }
                                // Keyboard key overlays
                                for (sprite_id, x_pos, y_pos) in
                                    &gs.note_prefabs.pressed_keyboard_overlays[lane]
                                {
                                    if let Some(pressed_frame) = atlas
                                        .get_frame_at_time(sprite_id, render_time as f64)
                                        .or_else(|| atlas.get_frame(sprite_id).copied())
                                    {
                                        let sprite_w = pressed_frame.width as f32 * skin_scale_x;
                                        let sprite_h = pressed_frame.height as f32 * skin_scale_y;
                                        let x = offset_x + *x_pos as f32 * skin_scale_x;
                                        let y = offset_y + *y_pos as f32 * skin_scale_y;
                                        gpu.textured_renderer.draw_textured_quad(
                                            x,
                                            y,
                                            sprite_w,
                                            sprite_h,
                                            pressed_frame.uv,
                                            [1.0, 1.0, 1.0, 0.6],
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 8. Draw note click effects
        if let (Some(ref mut gpu), Some(gs)) = (&mut render.gpu, &self.game_state) {
            if gs.is_rendering {
                if let Some(atlas) = &gpu.atlas {
                    let render_time = gs.clock.render_time() as f64;

                    // Draw EFFECT_CLICK effects (11 frames, framespeed=60 → 16.67ms per frame)
                    // Match Java: ee.setPos(ne.getX()+ne.getWidth()/2-ee.getWidth()/2, getViewport()-ee.getHeight()/2)
                    if let Some(ref click_sprite) = gs.effect_click_sprite {
                        if let Some(anim) = atlas.animations.get(click_sprite) {
                            // frame_speed_ms already converted from FPS in XML parser
                            let frame_speed_ms = anim.frame_speed_ms;
                            for effect in &gs.note_click_effects {
                                let frame_idx = effect.frame_index(
                                    render_time,
                                    frame_speed_ms,
                                    anim.frame_count,
                                );
                                let atlas_id = format!("{}_{}", click_sprite, frame_idx);
                                if let Some(frame) = atlas.get_frame(&atlas_id) {
                                    let lane_prefab = &gs.note_prefabs.lanes[effect.lane];
                                    // Match Java: ne.getX() + ne.getWidth()/2 - ee.getWidth()/2
                                    // lane_prefab.x is the left edge (ne.getX())
                                    // Get note width from the sprite atlas to center properly
                                    let note_sprite = lane_prefab
                                        .sprite_id
                                        .as_deref()
                                        .unwrap_or_else(|| match effect.lane {
                                            0 | 1 | 2 => "head_note_white",
                                            3 => "head_note_blue",
                                            _ => "head_note_yellow",
                                        });
                                    let note_width = atlas
                                        .get_frame(note_sprite)
                                        .map(|f| f.width as f32)
                                        .unwrap_or(50.0); // fallback width
                                    let effect_x = offset_x
                                        + lane_prefab.x as f32 * skin_scale_x
                                        + (note_width * skin_scale_x / 2.0)
                                        - (frame.width as f32 * skin_scale_x / 2.0);
                                    let effect_y = offset_y
                                        + skin_judgment_line_y as f32 * skin_scale_y
                                        - (frame.height as f32 * skin_scale_y / 2.0);

                                    // Alpha blending handled by renderer (textures have alpha channel)
                                    gpu.textured_renderer.draw_textured_quad(
                                        effect_x,
                                        effect_y,
                                        frame.width as f32 * skin_scale_x,
                                        frame.height as f32 * skin_scale_y,
                                        frame.uv,
                                        [1.0, 1.0, 1.0, 1.0],
                                    );
                                }
                            }
                        }
                    }

                    // Draw EFFECT_LONGFLARE effects (15 frames, framespeed=33.3 → 30.03ms per frame)
                    // Match Java: ee.setPos(ne.getX() + ne.getWidth()/2 - ee.getWidth()/2, ee.getY())
                    if let Some(ref flare_sprite) = gs.effect_longflare_sprite {
                        if let Some(anim) = atlas.animations.get(flare_sprite) {
                            // frame_speed_ms already converted from FPS in XML parser
                            let frame_speed_ms = anim.frame_speed_ms;
                            for effect in &gs.long_flare_effects {
                                let frame_idx = effect.frame_index(
                                    render_time,
                                    frame_speed_ms,
                                    anim.frame_count,
                                );
                                let atlas_id = format!("{}_{}", flare_sprite, frame_idx);
                                if let Some(frame) = atlas.get_frame(&atlas_id) {
                                    let lane_prefab = &gs.note_prefabs.lanes[effect.lane];
                                    // Match Java: ne.getX() + ne.getWidth()/2 - ee.getWidth()/2, ee.getY()
                                    let note_sprite = lane_prefab
                                        .sprite_id
                                        .as_deref()
                                        .unwrap_or_else(|| match effect.lane {
                                            0 | 1 | 2 => "head_note_white",
                                            3 => "head_note_blue",
                                            _ => "head_note_yellow",
                                        });
                                    let note_width = atlas
                                        .get_frame(note_sprite)
                                        .map(|f| f.width as f32)
                                        .unwrap_or(50.0); // fallback width
                                    let flare_x = offset_x
                                        + lane_prefab.x as f32 * skin_scale_x
                                        + (note_width * skin_scale_x / 2.0)
                                        - (frame.width as f32 * skin_scale_x / 2.0);
                                    // Use entity Y from skin XML (y="460"), top-aligned at that position
                                    let flare_y =
                                        offset_y + gs.effect_longflare_y as f32 * skin_scale_y;

                                    // EFFECT_LONGFLARE uses additive blending for glow effect (GL_SRC_ALPHA, GL_DST_ALPHA)
                                    gpu.textured_renderer.set_blend_mode(BlendMode::Additive);
                                    gpu.textured_renderer.draw_textured_quad(
                                        flare_x,
                                        flare_y,
                                        frame.width as f32 * skin_scale_x,
                                        frame.height as f32 * skin_scale_y,
                                        frame.uv,
                                        [1.0, 1.0, 1.0, 1.0],
                                    );
                                    // Reset to alpha blending for subsequent draws
                                    gpu.textured_renderer.set_blend_mode(BlendMode::Alpha);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 9. Draw HUD elements (score, combo, lifebar, judgment popups)
        // Use separate borrows to avoid conflicting borrows
        let hud_layout = HudLayout::from_skin();
        if let Some(gs) = &self.game_state {
            let render_time = gs.clock.render_time();
            if let Some(ref mut gpu) = render.gpu {
                let atlas_ref = gpu.atlas.as_ref();
                render_hud_with_atlas(
                    &mut gpu.textured_renderer,
                    atlas_ref,
                    gs,
                    &hud_layout,
                    (skin_scale_x, skin_scale_y),
                    (offset_x, offset_y),
                    render_time as f64,
                );
            }
        }

        // 9. Flush render pass
        if let Some(ref mut gpu) = render.gpu {
            gpu.textured_renderer
                .end(&view, &render.queue, &render.device);
        }

        // 10. Present
        surface_texture.present();

        // Return whether song has ended (for caller to handle exit)
        self.game_state
            .as_ref()
            .map_or(false, |gs| gs.is_song_ended())
    }
}

/// Configure egui fonts with Inter (Latin) + Noto Sans CJK (CJK fallback).
/// Keeps egui's default emoji font intact.
fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Try to load Inter as the primary Latin font
    if let Some(data) = load_font_data("Inter-Regular.ttf") {
        fonts.font_data.insert(
            "inter".to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(data)),
        );
        // Insert at the FRONT of the proportional family (highest priority)
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "inter".to_string());
        fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap()
            .insert(0, "inter".to_string());
        log::info!("Loaded Inter font");
    } else {
        log::warn!("Inter font not found — using egui default for Latin text");
    }

    // Try to load Noto Sans CJK as CJK fallback
    if let Some(data) = load_font_data("NotoSansCJKsc-Regular.otf") {
        fonts.font_data.insert(
            "noto-sans-cjk".to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(data)),
        );
        // Append at the END of both families (fallback after Latin/emoji)
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .push("noto-sans-cjk".to_string());
        fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap()
            .push("noto-sans-cjk".to_string());
        log::info!("Loaded Noto Sans CJK font");
    } else {
        log::warn!(
            "Noto Sans CJK font not found — CJK characters may not render correctly"
        );
    }

    ctx.set_fonts(fonts);
}

/// Load font file bytes from the assets directory.
fn load_font_data(filename: &str) -> Option<Vec<u8>> {
    load_font_path(filename).and_then(|p| std::fs::read(p).ok())
}

/// Find a font file in the assets directory relative to various locations.
fn load_font_path(filename: &str) -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let candidates = [
        // Relative to crate root (for `cargo run`)
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(filename),
        // Relative to current working directory
        PathBuf::from("assets").join(filename),
        // Relative to executable directory
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("assets")
            .join(filename),
    ];
    for path in &candidates {
        if path.exists() {
            return Some(path.clone());
        }
    }
    None
}
