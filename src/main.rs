//! open2jam-rs — O2Jam rhythm game port in Rust.
//!
//! Preview mode: Loads a chart and plays it with auto-play enabled.
//!
//! # Run
//!
//! ```bash
//! cargo run -- <path-to-ojn-file>
//! ```

#![warn(missing_docs)]

pub mod audio;
pub mod engine;
pub mod game_state;
pub mod gameplay;
pub mod parsing;
pub mod render;
pub mod resources;
pub mod skin;
pub mod test_harness;

use std::path::PathBuf;

use anyhow::Result;
use engine::App;
use log::info;

fn main() -> Result<()> {
    env_logger::init();
    info!("Starting open2jam-rs preview mode");

    // Parse command line args: <path-to-ojn-file> [--autoplay]
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut ojn_path: Option<PathBuf> = None;
    let mut auto_play = false; // Off by default — manual input mode

    for arg in &args {
        if arg == "--autoplay" {
            auto_play = true;
        } else if ojn_path.is_none() {
            ojn_path = Some(PathBuf::from(arg));
        }
    }

    if let Some(path) = &ojn_path {
        info!("Will load chart from: {}", path.display());
        if auto_play {
            info!("Auto-play mode enabled (--autoplay flag)");
        } else {
            info!("Manual input mode (S D F Space J K L)");
        }
    } else {
        eprintln!("Open2Jam Preview Mode");
        eprintln!("====================");
        eprintln!();
        eprintln!("Usage: cargo run -- <path-to-ojn-file> [--autoplay]");
        eprintln!();
        eprintln!("Examples:");
        eprintln!("  cargo run -- /path/to/song.ojn              # Manual input mode");
        eprintln!("  cargo run -- /path/to/song.ojn --autoplay   # Auto-play mode");
        eprintln!();
        eprintln!("Default keys:");
        eprintln!("  Lane 1-3: S D F");
        eprintln!("  Lane 4:   Space");
        eprintln!("  Lane 5-7: J K L");
        eprintln!();
        eprintln!("Requirements:");
        eprintln!("  - .ojn file (chart)");
        eprintln!("  - .ojm file (audio) with matching name in same directory");
    }

    // Create and run the application
    let app = App::new(ojn_path, auto_play)?;
    app.run()?;

    info!("Shutting down cleanly");
    Ok(())
}
