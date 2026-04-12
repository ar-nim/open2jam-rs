//! Top-level menu application state.

use anyhow::Result;
use winit::event_loop::EventLoop;
use winit::window::Window;

/// The main menu application.
pub struct MenuApp {
    config: Config,
}

use open2jam_rs_core::Config;

impl MenuApp {
    pub fn new() -> Result<Self> {
        let config_path = Config::default_path();
        let config = Config::load(&config_path)
            .unwrap_or_else(|e| {
                log::info!("No config found at {:?}, using defaults: {}", config_path, e);
                Config::default()
            });

        Ok(Self { config })
    }

    /// Run the menu event loop.
    pub fn run(mut self, event_loop: EventLoop<()>) -> Result<()> {
        let mut app = MenuRunner {
            config: self.config,
            egui_ctx: egui::Context::default(),
            window: None,
            integration: None,
        };
        event_loop.run_app(&mut app)?;
        Ok(())
    }
}

/// Event loop handler.
struct MenuRunner {
    config: Config,
    egui_ctx: egui::Context,
    window: Option<Window>,
    integration: Option<egui_winit::State>,
}

impl winit::application::ApplicationHandler for MenuRunner {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = winit::window::Window::default_attributes()
            .with_title("open2jam-rs — Music Select")
            .with_inner_size(winit::dpi::LogicalSize::new(928.0, 730.0));

        let window = event_loop.create_window(attrs).unwrap();
        let integration = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            None,
            None,
            None,
        );

        log::info!("Menu window created");
        self.window = Some(window);
        self.integration = Some(integration);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        use winit::event::WindowEvent;

        // Feed events to egui-winit
        let Some(integration) = &mut self.integration else { return };
        let Some(window) = &self.window else { return };

        let response = integration.on_window_event(window, &event);
        if response.consumed {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if self.window.is_some() && self.integration.is_some() {
                    self.render_window();
                }
            }
            _ => {}
        }
    }
}

impl MenuRunner {
    fn render_window(&mut self) {
        let raw_input = {
            let window = self.window.as_ref().unwrap();
            let integration = self.integration.as_mut().unwrap();
            integration.take_egui_input(window)
        };
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            ui_panel(ctx, &mut self.config);
        });
        {
            let window = self.window.as_ref().unwrap();
            let integration = self.integration.as_mut().unwrap();
            integration.handle_platform_output(window, full_output.platform_output);
        }
    }
}

fn ui_panel(ctx: &egui::Context, config: &mut Config) {
    egui::TopBottomPanel::top("menu_tabs").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut config.game_options.autoplay, false, "Music Select");
            ui.selectable_value(&mut config.game_options.autoplay, true, "Configuration");
            ui.button("Advanced");
        })
    });

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("open2jam-rs Menu");
        ui.label("Select a chart directory to begin.");
        ui.separator();
        ui.label(&format!("Difficulty: {:?}", config.game_options.difficulty));
        ui.label(&format!("Speed: {:.1} ({:?})", config.game_options.speed_multiplier, config.game_options.speed_type));
    });
}
