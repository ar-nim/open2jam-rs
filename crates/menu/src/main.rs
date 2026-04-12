//! Menu GUI for open2jam-rs — egui + winit + wgpu.
//!
//! This is a standalone binary. Run with: `cargo run -p open2jam-rs-menu`

use anyhow::Result;

mod app;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("open2jam-rs-menu starting");

    let event_loop = winit::event_loop::EventLoop::new()?;
    let app = app::MenuApp::new()?;
    app.run(event_loop)
}
