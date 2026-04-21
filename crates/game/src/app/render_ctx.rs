use crate::gpu::GpuResources;

pub struct RenderCtx {
    pub window: winit::window::Window,
    pub surface: Option<wgpu::Surface<'static>>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub gpu: Option<GpuResources>,
    pub skin_scale: (f32, f32),
}

impl RenderCtx {
    pub fn shutdown(&mut self) {
        self.gpu.take();
        self.surface.take();
        log::info!("RenderCtx shutdown complete.");
    }

    pub fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.config.width = size.width;
            self.config.height = size.height;
            if let Some(ref surface) = self.surface {
                surface.configure(&self.device, &self.config);
            }
            if let Some(ref mut gpu) = self.gpu {
                gpu.textured_renderer
                    .resize(&self.device, &self.queue, size.width, size.height);
            }
        }
    }
}
