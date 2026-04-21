use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::gpu::GpuResources;

pub fn init_wgpu(
    window: winit::window::Window,
    vsync_mode: open2jam_rs_core::game_options::VSyncMode,
) -> (
    winit::window::Window,
    wgpu::Instance,
    wgpu::Surface<'static>,
    wgpu::Adapter,
    wgpu::Device,
    wgpu::Queue,
    wgpu::SurfaceConfiguration,
) {
    log::info!("Initialising wgpu...");
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

    let present_mode = match vsync_mode {
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

    (window, instance, surface, adapter, device, queue, config)
}

pub fn build_gpu_resources(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    config: &wgpu::SurfaceConfiguration,
    skin_dir: std::path::PathBuf,
    ojn_path: Option<&std::path::Path>,
) -> (GpuResources, (f32, f32)) {
    crate::assets::build_gpu_resources(device, queue, config, skin_dir, ojn_path)
}
