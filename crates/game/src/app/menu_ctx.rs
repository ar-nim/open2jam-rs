pub struct MenuCtx {
    pub menu_app: Option<crate::menu::menu_app::MenuApp>,
    pub egui_ctx: egui::Context,
    pub egui_winit: Option<egui_winit::State>,
    pub egui_renderer: Option<egui_wgpu::Renderer>,
}

impl MenuCtx {
    pub fn new() -> anyhow::Result<Self> {
        let menu_app = Some(crate::menu::menu_app::MenuApp::new()?);
        Ok(Self {
            menu_app,
            egui_ctx: egui::Context::default(),
            egui_winit: None,
            egui_renderer: None,
        })
    }

    pub fn init_egui(
        &mut self,
        window: &winit::window::Window,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
    ) {
        self.egui_winit = Some(egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            None,
            None,
            None,
        ));
        log::info!("egui-winit state initialised");

        crate::menu::fonts::configure_fonts(&self.egui_ctx);

        self.egui_renderer = Some(egui_wgpu::Renderer::new(
            device,
            config.format,
            egui_wgpu::RendererOptions::default(),
        ));
        log::info!("egui-wgpu renderer initialised (wgpu 29)");
    }

    pub fn cleanup(&mut self) {
        self.egui_renderer.take();
        self.egui_winit.take();
        self.egui_ctx = egui::Context::default();
    }

    pub fn take_menu_app(&mut self) -> Option<crate::menu::menu_app::MenuApp> {
        self.menu_app.take()
    }
}
