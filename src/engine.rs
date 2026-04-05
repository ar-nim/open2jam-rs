//! Frame orchestrator — winit event loop, wgpu device, oddio mixer.
//!
//! MILESTONE 0: Scaffolding with window, GPU, and audio backend.

use std::path::PathBuf;

use anyhow::Result;
use log::{info, warn};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use wgpu::CurrentSurfaceTexture;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::Window;

use crate::audio::AudioManager;

struct RenderState {
    window: Window,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
}

pub struct App {
    ojn_path: Option<PathBuf>,
    event_loop: Option<EventLoop<()>>,
    render: Option<RenderState>,
    audio: Option<AudioManager>,
}

impl App {
    pub fn new(ojn_path: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            ojn_path,
            event_loop: None,
            render: None,
            audio: None,
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

        info!("wgpu surface configured: {}x{}", config.width, config.height);
        self.render = Some(RenderState {
            window,
            surface,
            device,
            queue,
            config,
        });
    }

    fn render_frame(&mut self) {
        let Some(render) = &mut self.render else { return };

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

        let mut encoder = render
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render encoder"),
            });

        {
            let _rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear pass"),
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
        }

        render.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();
    }
}
