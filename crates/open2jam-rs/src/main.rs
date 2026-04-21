//! open2jam-rs — O2Jam rhythm game port in Rust.
//!
//! Thin binary wrapper that orchestrates the game and menu libraries.

use open2jam_rs_core::Config;
use open2jam_rs_game::app::App;

use std::path::PathBuf;

use anyhow::Result;
use log::{info, warn};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    info!("open2jam-rs starting");

    let config = Config::load(&Config::default_path()).unwrap_or_else(|e| {
        warn!(
            "Failed to load config from {:?}: {}, using defaults",
            Config::default_path(),
            e
        );
        Config::default()
    });

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
        info!("Launching menu (no chart specified)");
    }

    let app = App::new(ojn_path, auto_play, &config)?;
    app.run()?;

    info!("Shutting down cleanly");
    Ok(())
}
