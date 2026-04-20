//! open2jam-rs — O2Jam rhythm game port in Rust.
//!
//! Unified single binary: menu and game share the same winit window
//! and wgpu device. Everything runs on the main thread (macOS compliant).
//!
//! # Run
//!
//! ```bash
//! cargo run                        # opens menu
//! cargo run -- <path-to-ojn-file>  # opens game directly
//! cargo run -- <path.ojn> --autoplay
//! ```

#![warn(missing_docs)]

use open2jam_rs::app::App;
use open2jam_rs::core;
use open2jam_rs::parsing;

use std::path::PathBuf;

use anyhow::Result;
use log::{info, warn};
use open2jam_rs_core::Config;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    info!("open2jam-rs starting (unified single binary)");

    let config = Config::load(&Config::default_path()).unwrap_or_else(|e| {
        warn!(
            "Failed to load config from {:?}: {}, using defaults",
            Config::default_path(),
            e
        );
        Config::default()
    });
    info!(
        "Config: {}x{}, fullscreen={}, vsync={:?}, difficulty={:?}, speed={:.1}x",
        config.game_options.display_width,
        config.game_options.display_height,
        config.game_options.display_fullscreen,
        config.game_options.vsync_mode,
        config.game_options.difficulty,
        config.game_options.speed_multiplier,
    );

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut ojn_path: Option<PathBuf> = None;
    let mut auto_play = config.game_options.autoplay;

    for arg in &args {
        if arg == "--autoplay" {
            auto_play = true;
        } else if ojn_path.is_none() {
            ojn_path = Some(PathBuf::from(arg));
        }
    }

    if let Some(path) = &ojn_path {
        info!("Launching game directly: {}", path.display());
        if auto_play {
            info!("Auto-play mode enabled");
        }
    } else {
        info!("Launching menu (no chart specified on command line)");
    }

    let app = App::new(ojn_path, auto_play, &config)?;
    app.run()?;

    info!("Shutting down cleanly");
    Ok(())
}
