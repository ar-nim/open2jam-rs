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
use crate::parsing::xml::{parse_file as parse_skin_xml, Resources as SkinResources};
use crate::render::atlas::SkinAtlas;
use crate::render::textured_renderer::TexturedRenderer;

const SCROLL_SPEED: f64 = 1.0;
const AUTO_PLAY: bool = true;

struct RenderState {
    window: Window,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    textured_renderer: TexturedRenderer,
    atlas: Option<SkinAtlas>,
    skin: Option<SkinResources>,
    /// Scale factor from skin coordinates to screen coordinates.
    skin_scale: (f32, f32),
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
                        render.textured_renderer.resize(
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

        let mut textured_renderer = TexturedRenderer::new(&device, &queue, &config);

        // Try to load the skin XML and build atlas
        let skin_dir = std::path::Path::new("/home/arnim/projects/o2jam/open2jam-modern/src/resources");
        let (atlas, skin, skin_scale) = Self::load_skin(&device, &queue, skin_dir);

        // Set atlas on textured renderer
        if let Some(ref atlas) = atlas {
            textured_renderer.set_atlas(&device, atlas);
        }

        info!("wgpu surface configured: {}x{}", config.width, config.height);
        self.render = Some(RenderState {
            window,
            surface,
            device,
            queue,
            config,
            textured_renderer,
            atlas,
            skin,
            skin_scale,
        });
    }

    /// Load skin XML, parse frames, build texture atlas.
    fn load_skin(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        skin_dir: &std::path::Path,
    ) -> (Option<SkinAtlas>, Option<SkinResources>, (f32, f32)) {
        let xml_path = skin_dir.join("resources.xml");
        if !xml_path.exists() {
            warn!("Skin XML not found at {}", xml_path.display());
            return (None, None, (1.0, 1.0));
        }

        let skin_resources = match parse_skin_xml(&xml_path) {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to parse skin XML: {e:?}");
                return (None, None, (1.0, 1.0));
            }
        };

        let skin_def = match skin_resources.get_skin("o2jam") {
            Some(s) => s.clone(),
            None => {
                warn!("Skin 'o2jam' not found in resources.xml");
                return (None, None, (1.0, 1.0));
            }
        };

        // Collect all frame definitions: (id, file, x, y, w, h)
        let frames: Vec<(String, String, u32, u32, u32, u32)> = skin_def
            .frames
            .iter()
            .map(|f| {
                (
                    f.id.clone(),
                    f.file.to_string_lossy().to_string(),
                    f.x,
                    f.y,
                    f.w,
                    f.h,
                )
            })
            .collect();

        // Build atlas
        let skin_dir_owned = skin_dir.to_path_buf();
        let atlas = SkinAtlas::from_frames(device, queue, &frames, |file: &str| {
            let path = skin_dir_owned.join(file);
            match image::open(&path) {
                Ok(img) => Some(img.into_rgba8()),
                Err(e) => {
                    warn!("Failed to load skin image {}: {e}", path.display());
                    None
                }
            }
        });

        let skin_width = skin_def.width as f32;
        let skin_height = skin_def.height as f32;
        let skin_scale = (1.0, 1.0); // Will be recalculated per-frame based on viewport

        info!(
            "Skin loaded: {}x{}, {} frames, atlas {}x{}",
            skin_width,
            skin_height,
            frames.len(),
            atlas.as_ref().map(|a| a.width).unwrap_or(0),
            atlas.as_ref().map(|a| a.height).unwrap_or(0),
        );

        (atlas, Some(skin_resources), skin_scale)
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
            wgpu::CurrentSurfaceTexture::Success(st) => st,
            wgpu::CurrentSurfaceTexture::Suboptimal(st) => {
                warn!("Surface suboptimal.");
                st
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            wgpu::CurrentSurfaceTexture::Outdated => {
                warn!("Surface outdated, reconfiguring...");
                render.surface.configure(&render.device, &render.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Validation => return,
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 4. Calculate skin scale
        let (config_width, config_height) = (render.config.width as f32, render.config.height as f32);
        let (skin_scale_x, skin_scale_y) = if let Some(ref skin_res) = render.skin {
            let skin = skin_res.get_skin("o2jam");
            if let Some(s) = skin {
                (config_width / s.width as f32, config_height / s.height as f32)
            } else {
                (1.0, 1.0)
            }
        } else {
            (1.0, 1.0)
        };

        // 5. Begin textured rendering
        render.textured_renderer.begin();

        // 6. Draw background (note_bg if available)
        if let (Some(atlas), Some(skin_res)) = (&render.atlas, &render.skin) {
            if let Some(skin) = skin_res.get_skin("o2jam") {
                // Draw note background
                if let Some(note_bg_frame) = atlas.get_frame("note_bg") {
                    let note_w = note_bg_frame.width as f32 * skin_scale_x;
                    let note_h = note_bg_frame.height as f32 * skin_scale_y;
                    let note_x = (config_width - note_w) / 2.0;
                    let note_y = 0.0;
                    render.textured_renderer.draw_textured_quad(
                        note_x, note_y, note_w, note_h, note_bg_frame.uv, [1.0, 1.0, 1.0, 1.0],
                    );
                }

                // Draw judgment line
                let jly = skin.judgment_line_y as f32 * skin_scale_y;
                if let Some(jl_frame) = atlas.get_frame("judgmentarea") {
                    let jl_w = jl_frame.width as f32 * skin_scale_x;
                    let jl_h = jl_frame.height as f32 * skin_scale_y;
                    let jl_x = (config_width - jl_w) / 2.0;
                    render.textured_renderer.draw_textured_quad(
                        jl_x, jly - jl_h / 2.0, jl_w, jl_h, jl_frame.uv, [1.0, 1.0, 1.0, 1.0],
                    );
                } else {
                    // Fallback: colored line
                    render.textured_renderer.draw_textured_quad(
                        0.0, jly - 2.0, config_width, 4.0, [0.0, 0.0, 0.0, 0.0], [0.8, 0.8, 0.8, 0.6],
                    );
                }

                // Draw notes
                if let Some(gs) = &self.game_state {
                    let render_time = gs.clock.render_time();
                    let bpm = gs.clock.bpm() as f64;
                    let viewport_height = config_height as f64;
                    let judgment_line_y = jly as f64;

                    for note in &gs.active_notes {
                        let y = note_y_position(
                            render_time,
                            note.target_time_ms,
                            bpm,
                            judgment_line_y,
                            viewport_height,
                            gs.scroll_speed,
                        );

                        // Get lane X from skin prefab
                        let lane_prefab = &gs.note_prefabs.lanes[note.lane];
                        let lane_x = lane_prefab.x as f32 * skin_scale_x;

                        // Pick note sprite based on lane color
                        // Lanes 1-3: white notes, lanes 4: blue, lanes 5-7: yellow
                        let head_frame_name = match note.lane {
                            0 | 1 | 2 => "head_note_white",
                            3 => "head_note_blue",
                            _ => "head_note_yellow",
                        };

                        let note_w = 28.0 * skin_scale_x;
                        let note_h = 7.0 * skin_scale_y;

                        if let Some(head_frame) = atlas.get_frame(head_frame_name) {
                            let x = lane_x - note_w / 2.0;
                            let y = y as f32 - note_h / 2.0;
                            render.textured_renderer.draw_textured_quad(
                                x, y, note_w, note_h, head_frame.uv, [1.0, 1.0, 1.0, 1.0],
                            );
                        }
                    }
                }

                // Draw measure mark
                if let Some(measure_frame) = atlas.get_frame("measure_mark") {
                    let mw = measure_frame.width as f32 * skin_scale_x;
                    let mh = measure_frame.height as f32 * skin_scale_y;
                    let mx = (config_width - mw) / 2.0;
                    render.textured_renderer.draw_textured_quad(
                        mx, jly - mh / 2.0, mw, mh, measure_frame.uv, [1.0, 1.0, 1.0, 0.5],
                    );
                }
            }
        }

        // 7. Flush render pass
        render.textured_renderer.end(&view, &render.queue, &render.device);

        // 8. Present
        surface_texture.present();
    }
}
