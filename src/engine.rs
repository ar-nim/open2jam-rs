//! Frame orchestrator — winit event loop, wgpu device, oddio mixer, game loop.

use std::time::Instant;

use anyhow::Result;
use log::{info, warn};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
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

/// GPU resources that must be dropped before the device.
/// This wrapper allows explicit drop ordering to prevent segfaults.
struct GpuResources {
    textured_renderer: TexturedRenderer,
    atlas: Option<SkinAtlas>,
    skin: Option<SkinResources>,
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
            info!("Audio manager failed to initialise (running headless).");
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
                
                // Get skin resources for note prefab loading
                let skin_res = self.render.as_ref()
                    .and_then(|r| r.gpu.as_ref())
                    .and_then(|g| g.skin.as_ref());
                
                match GameState::load(path, SCROLL_SPEED, AUTO_PLAY, skin_res) {
                    Ok(mut gs) => {
                        info!(
                            "Game loaded: {} ({:.1}ms spawn lead)",
                            gs.chart.header.title, gs.spawn_lead_time_ms
                        );
                        gs.clock.start();
                        self.game_state = Some(gs);
                    }
                    Err(e) => {
                        info!("Failed to load game state: {e:?}");
                    }
                }
            } else {
                info!("No OJN file specified — running in demo mode (no notes)");
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
                info!("Close requested, cleaning up...");
                if let Some(render) = self.render.as_mut() {
                    render.shutdown();
                }
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                use winit::event::ElementState;
                use winit::keyboard::{Key, NamedKey};
                if event.state == ElementState::Pressed {
                    if let Key::Named(NamedKey::Escape) = &event.logical_key {
                        info!("Escape pressed, exiting...");
                        if let Some(render) = self.render.as_mut() {
                            render.shutdown();
                        }
                        event_loop.exit();
                    }
                }
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

        if let Some(ref atlas) = atlas {
            textured_renderer.set_atlas(&device, atlas);
        }

        let gpu = GpuResources {
            textured_renderer,
            atlas,
            skin,
        };

        info!("wgpu surface configured: {}x{}", config.width, config.height);
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
        let mut frame_entries: Vec<(String, String, u32, u32, u32, u32)> = Vec::new();
        for (sprite_id, sprite_def) in &resources.sprites {
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

        info!("Skin has {} sprite frames to pack into atlas", frame_entries.len());

        let skin_dir_owned = skin_dir.to_path_buf();
        let atlas = SkinAtlas::from_frames(device, queue, &frame_entries, |file: &str| {
            let path = skin_dir_owned.join(file);
            if !path.exists() {
                info!("Skin image not found: {}", path.display());
            }
            match image::open(&path) {
                Ok(img) => Some(img.into_rgba8()),
                Err(e) => {
                    info!("Failed to load skin image {}: {e}", path.display());
                    None
                }
            }
        });

        if let Some(ref a) = atlas {
            info!(
                "Atlas built: {} frames in {}x{} texture",
                a.frames.len(),
                a.width,
                a.height
            );
            for key in &["head_note_white", "head_note_blue", "head_note_yellow", "judgmentarea", "note_bg", "measure_mark"] {
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

    fn render_frame(&mut self) {
        let Some(render) = &mut self.render else {
            warn!("render_frame: render state is None");
            return;
        };

        // Log first time we enter render_frame
        static FIRST_FRAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !FIRST_FRAME.load(std::sync::atomic::Ordering::Relaxed) {
            FIRST_FRAME.store(true, std::sync::atomic::Ordering::Relaxed);
            info!("=== First render_frame call ===");
            info!("gpu state: atlas={}, skin={}",
                render.gpu.as_ref().map(|g| g.atlas.is_some()).unwrap_or(false),
                render.gpu.as_ref().map(|g| g.skin.is_some()).unwrap_or(false));
        }

        // 1. Calculate delta time (rounded to prevent cumulative drift — Bug 11 fix)
        let now = Instant::now();
        let delta_ms = if let Some(last) = self.last_frame_time {
            let delta = now.duration_since(last);
            (delta.as_micros() as f64 / 1000.0).round() as u64
        } else {
            16
        };
        self.last_frame_time = Some(now);

        // 2. Advance game state
        if let Some(gs) = &mut self.game_state {
            gs.update(delta_ms);
            gs.spawn_notes();
            gs.cleanup_notes();

            if let Some(audio_mgr) = &mut self.audio {
                gs.process_audio(audio_mgr);
            }
        }

        // 3. Acquire surface texture
        let Some(ref surface) = render.surface else { return };
        let surface_texture = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(st) => st,
            wgpu::CurrentSurfaceTexture::Suboptimal(st) => {
                st
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return,
            wgpu::CurrentSurfaceTexture::Outdated => {
                surface.configure(&render.device, &render.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Validation => return,
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 4. Calculate skin scale and letterbox offset
        // Scale the 800x600 skin to fit the window while maintaining aspect ratio
        let (config_width, config_height) = (render.config.width as f32, render.config.height as f32);
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
                    const STATIC_SPRITES: &[&str] = &[
                        "bga10",
                        "note_bg",
                        "dashboard",
                        "lifebar_bg",
                        "timebar",
                        "judgmentarea",
                        "lifebar",
                        "pill",
                        "jam_bar",
                        "static_keyboard",
                    ];

                    static FRAME_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                    let frame_num = FRAME_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if frame_num == 0 {
                        info!("=== Static render pass starting ===");
                        info!("Static sprites whitelist: {:?}", STATIC_SPRITES);
                    }

                    let mut draw_count = 0;
                    let mut skip_count = 0;
                    for entity in &skin.entities {
                        let sprite_id = match &entity.sprite {
                            Some(s) => s,
                            None => { skip_count += 1; continue; }
                        };
                        let first_sprite = sprite_id.split(',').next().unwrap_or(sprite_id).trim();
                        if !STATIC_SPRITES.contains(&first_sprite) {
                            skip_count += 1;
                            continue;
                        }
                        if let Some(atlas_frame) = atlas.get_frame(first_sprite) {
                            draw_count += 1;

                            if frame_num == 0 {
                                let frame_w = atlas_frame.width as f32 * skin_scale_x;
                                let frame_h = atlas_frame.height as f32 * skin_scale_y;
                                let frame_x = offset_x + entity.x as f32 * skin_scale_x;
                                let frame_y = offset_y + entity.y as f32 * skin_scale_y;
                                info!("  [RENDER] sprite={} skin({},{}) -> screen({:.1},{:.1}) size {:.1}x{:.1}",
                                    first_sprite, entity.x, entity.y, frame_x, frame_y, frame_w, frame_h);
                            }

                            let frame_w = atlas_frame.width as f32 * skin_scale_x;
                            let frame_h = atlas_frame.height as f32 * skin_scale_y;
                            let frame_x = offset_x + entity.x as f32 * skin_scale_x;
                            let frame_y = offset_y + entity.y as f32 * skin_scale_y;

                            gpu.textured_renderer.draw_textured_quad(
                                frame_x, frame_y, frame_w, frame_h,
                                atlas_frame.uv, [1.0, 1.0, 1.0, 1.0],
                            );
                        } else {
                            if frame_num == 0 {
                                info!("  [MISSING] sprite={} not found in atlas", first_sprite);
                            }
                        }
                    }
                    if frame_num == 0 {
                        info!("Static pass: {} drawn, {} skipped (no sprite), {} skipped (not in whitelist)",
                            draw_count, skip_count, skin.entities.len() - draw_count - skip_count);
                    }

                    // Draw measure mark once at the judgment line
                    if let Some(measure_frame) = atlas.get_frame("measure_mark") {
                        let mw = measure_frame.width as f32 * skin_scale_x;
                        let mh = measure_frame.height as f32 * skin_scale_y;
                        let jly = offset_y + skin_judgment_line_y * skin_scale_y;
                        let mx = offset_x + (skin_w - measure_frame.width as f32) / 2.0 * skin_scale_x;
                        gpu.textured_renderer.draw_textured_quad(
                            mx, jly - mh / 2.0, mw, mh, measure_frame.uv, [1.0, 1.0, 1.0, 0.5],
                        );
                    }
                }
            }
        }

        // 7. Draw notes dynamically from game state
        if let (Some(ref mut gpu), Some(gs)) = (&mut render.gpu, &self.game_state) {
            let render_time = gs.clock.render_time();
            let bpm = gs.clock.bpm() as f64;
            let viewport_height = skin_h as f64;
            let judgment_line_y = skin_judgment_line_y as f64;

            if let (Some(atlas), Some(_skin_res)) = (&gpu.atlas, &gpu.skin) {
                for note in &gs.active_notes {
                    let y = note_y_position(
                        render_time,
                        note.target_time_ms,
                        bpm,
                        judgment_line_y,
                        viewport_height,
                        gs.scroll_speed,
                    );

                    let lane_prefab = &gs.note_prefabs.lanes[note.lane];
                    let lane_x = offset_x + lane_prefab.x as f32 * skin_scale_x;

                    // Use the sprite ID from the skin XML prefab, fallback to lane-based default
                    let head_frame_name = lane_prefab.sprite_id.as_deref().unwrap_or_else(|| {
                        match note.lane {
                            0 | 1 | 2 => "head_note_white",
                            3 => "head_note_blue",
                            _ => "head_note_yellow",
                        }
                    });

                    if let Some(head_frame) = atlas.get_frame(head_frame_name) {
                        let note_w = head_frame.width as f32 * skin_scale_x;
                        let note_h = head_frame.height as f32 * skin_scale_y;
                        let x = lane_x; // Left edge aligned with receptor (entity.x is left edge in skin XML)
                        let y = offset_y + y as f32 * skin_scale_y - note_h / 2.0;
                        gpu.textured_renderer.draw_textured_quad(
                            x, y, note_w, note_h, head_frame.uv, [1.0, 1.0, 1.0, 1.0],
                        );
                    }
                }
            }
        }

        // 8. Flush render pass
        if let Some(ref mut gpu) = render.gpu {
            gpu.textured_renderer.end(&view, &render.queue, &render.device);
        }

        // 9. Present
        surface_texture.present();
    }
}
