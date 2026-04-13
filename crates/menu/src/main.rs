//! Menu GUI for open2jam-rs — eframe (egui + winit + wgpu).
//!
//! Run with: `cargo run -p open2jam-rs-menu`

use anyhow::Result;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("open2jam-rs-menu starting");

    let app = open2jam_rs_menu::menu_app::MenuApp::new()?;
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("open2jam-rs — Music Select")
            .with_inner_size([928.0, 730.0]),
        ..Default::default()
    };
    eframe::run_native(
        "open2jam-rs — Music Select",
        native_options,
        Box::new(
            |_cc| -> std::result::Result<
                Box<dyn eframe::App>,
                Box<dyn std::error::Error + Send + Sync>,
            > { Ok(Box::new(app)) },
        ),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {:?}", e))?;
    Ok(())
}
