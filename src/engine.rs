//! Frame orchestrator — winit event loop, wgpu device, oddio mixer, game loop.

use std::time::Instant;

use anyhow::Result;
use log::{info, warn};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use wgpu::CurrentSurfaceTexture;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::Window;

use crate::audio::AudioManager;
use crate::game_state::GameState;
use crate::gameplay::scroll::note_y_position;
use crate::render::pipeline::SpriteRenderer;

const SCROLL_SPEED: f64 = 1.0;
const AUTO_PLAY: bool = true;
const NOTE_WIDTH: f32 = 60.0;
const NOTE_HEIGHT: f32 = 25.0;

/// Lane colors palette — each lane gets a distinct color.
const LANE_COLORS: [[f32; 4]; 7] = [
    [0.2, 0.5, 1.0, 1.0],  // Lane 1: Blue
    [0.3, 0.8, 0.3, 1.0],  // Lane 2: Green
    [1.0, 0.6, 0.2, 1.0],  // Lane 3: Orange
    [1.0, 0.3, 0.3, 1.0],  // Lane 4: Red
    [1.0, 0.8, 0.0, 1.0],  // Lane 5: Yellow
    [0.6, 0.3, 1.0, 1.0],  // Lane 6: Purple
    [0.2, 1.0, 0.9, 1.0],  // Lane 7: Cyan
];

struct RenderState {
    window: Window,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    sprite_renderer: SpriteRenderer,
}

pub struct App {
    ojn_path: Option<std::path::PathBuf>,
    event_loop: Option<EventLoop<()>>,
    render: Option<RenderState>,
    audio: Option<AudioManager>,
    game_state: Option<GameState>,
    last_frame_time: Option<Instant>,
}

impl App {
    pub fn new(ojn_path: Option<std::path::PathBuf>) -> Result<Self> {
        Ok(Self {
            ojn_path,
            event_loop: None,
            render: None,
            audio: None,
            game_state: None,
            last_frame_time: None,
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
            warn!("Audio manager failed to initialise.");
        }
        self.audio = Some(audio_mgr);

        let event_loop = self.event_loop.take().unwrap();
        event_loop.run_app(&mut self)?;

        info!("App shutting down.");
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.render.is_none() {
            info!("Creating window and initialising wgpu...");
            let attrs = winit::window::WindowAttributes::default()
                .with_title("open2jam-rs")
                .with_inner_size(winit::dpi::LogicalSize::new(1000, 750))
                .with_visible(true)
                .with_resizable(true);
            let window = event_loop.create_window(attrs).unwrap();
            self.init_wgpu(window);

            // Load the game state after rendering is ready
            if let Some(path) = &self.ojn_path {
                info!("Loading game state from: {}", path.display());
                match GameState::load(path, SCROLL_SPEED, AUTO_PLAY) {
                    Ok(mut gs) => {
                        info!(
                            "Game loaded: {} ({:.1}ms spawn lead)",
                            gs.chart.header.title, gs.spawn_lead_time_ms
                        );
                        gs.clock.start();
                        self.game_state = Some(gs);
                    }
                    Err(e) => {
                        warn!("Failed to load game state: {e:?}");
                    }
                }
            } else {
                warn!("No OJN file specified — running in demo mode (no notes)");
            }

            self.last_frame_time = Some(Instant::now());
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(render) = &self.render {
            render.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Close requested, exiting...");
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                use winit::event::ElementState;
                use winit::keyboard::{Key, NamedKey};
                if event.state == ElementState::Pressed {
                    if let Key::Named(NamedKey::Escape) = &event.logical_key {
                        info!("Escape pressed, exiting...");
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(render) = &mut self.render {
                    if size.width > 0 && size.height > 0 {
                        render.config.width = size.width;
                        render.config.height = size.height;
                        render.surface.configure(&render.device, &render.config);
                        render.sprite_renderer.resize(
                            &render.device,
                            &render.queue,
                            size.width,
                            size.height,
                        );
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.render_frame();
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

        // SAFETY: window is stored alongside surface in RenderState,
        // so handles remain valid for the surface lifetime.
        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(raw_display_handle),
                    raw_window_handle,
                })
                .expect("Failed to create surface")
        };

        let adapter = pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            },
        ))
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

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let sprite_renderer = SpriteRenderer::new(&device, &queue, &config);

        info!("wgpu surface configured: {}x{}", config.width, config.height);
        self.render = Some(RenderState {
            window,
            surface,
            device,
            queue,
            config,
            sprite_renderer,
        });
    }

    fn render_frame(&mut self) {
        let Some(render) = &mut self.render else { return };

        // 1. Calculate delta time (rounded to prevent cumulative drift — Bug 11 fix)
        let now = Instant::now();
        let delta_ms = if let Some(last) = self.last_frame_time {
            let delta = now.duration_since(last);
            (delta.as_micros() as f64 / 1000.0).round() as u64
        } else {
            16 // first frame: assume ~60fps
        };
        self.last_frame_time = Some(now);

        // 2. Advance game state
        if let Some(gs) = &mut self.game_state {
            gs.update(delta_ms);
            gs.spawn_notes();
            gs.cleanup_notes();

            // Process audio triggers
            if let Some(audio_mgr) = &mut self.audio {
                gs.process_audio(audio_mgr);
            }
        }

        // 3. Acquire surface texture
        let surface_texture = match render.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(st) => st,
            CurrentSurfaceTexture::Suboptimal(st) => {
                warn!("Surface suboptimal.");
                st
            }
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => return,
            CurrentSurfaceTexture::Outdated => {
                warn!("Surface outdated, reconfiguring...");
                render.surface.configure(&render.device, &render.config);
                return;
            }
            CurrentSurfaceTexture::Lost | CurrentSurfaceTexture::Validation => return,
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 4. Begin sprite rendering batch
        render.sprite_renderer.begin(&render.queue);

        // 5. Draw judgment line
        let (config_width, config_height, game_state_ref) = (
            render.config.width,
            render.config.height,
            &self.game_state,
        );

        if let Some(gs) = game_state_ref {
            let jly = gs.note_prefabs.judgment_line_y as f32;
            let vw = config_width as f32;
            let color = [0.8, 0.8, 0.8, 0.6];
            render.sprite_renderer.draw_quad(
                0.0, jly - 2.0, vw, 4.0, color,
            );
        } else {
            // Demo mode: draw a judgment line at 80% of height
            let jly = config_height as f32 * 0.8;
            let vw = config_width as f32;
            let color = [0.8, 0.8, 0.8, 0.6];
            render.sprite_renderer.draw_quad(
                0.0, jly - 2.0, vw, 4.0, color,
            );
        }

        // 6. Draw active notes
        if let Some(gs) = &self.game_state {
            let render_time = gs.clock.render_time();
            let bpm = gs.clock.bpm() as f64;
            let viewport_height = config_height as f64;
            let judgment_line_y = gs.note_prefabs.judgment_line_y as f64;

            for note in &gs.active_notes {
                let y = note_y_position(
                    render_time,
                    note.target_time_ms,
                    bpm,
                    judgment_line_y,
                    viewport_height,
                    gs.scroll_speed,
                );

                // Get lane X position
                let lane_x = gs.note_prefabs.lanes[note.lane].x as f32;
                let note_w = NOTE_WIDTH;
                let note_h = NOTE_HEIGHT;
                let x = lane_x - note_w / 2.0;
                let y = y as f32 - note_h / 2.0;

                let color = LANE_COLORS[note.lane];
                render.sprite_renderer.draw_quad(x, y, note_w, note_h, color);
            }
        }

        // 7. Flush render pass
        render.sprite_renderer.end(&view, &render.queue, &render.device);

        // 8. Present
        surface_texture.present();
    }
}
