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

    // Get OJN file path from command line args
    let ojn_path = std::env::args()
        .nth(1)
        .map(PathBuf::from);

    if let Some(path) = &ojn_path {
        info!("Will load chart from: {}", path.display());
    } else {
        eprintln!("Open2Jam Preview Mode");
        eprintln!("====================");
        eprintln!();
        eprintln!("Usage: cargo run -- <path-to-ojn-file>");
        eprintln!();
        eprintln!("Example:");
        eprintln!("  cargo run -- /path/to/song.ojn");
        eprintln!();
        eprintln!("Requirements:");
        eprintln!("  - .ojn file (chart)");
        eprintln!("  - .ojm file (audio) with matching name in same directory");
    }

    // Create and run the application
    let app = App::new(ojn_path)?;
    app.run()?;

    info!("Shutting down cleanly");
    Ok(())
}
