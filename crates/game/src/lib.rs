//! Open2Jam rhythm game — unified binary crate.
//!
//! Both the menu GUI and the game engine are compiled from this single crate.
//! They share the same winit window and wgpu device, running on the main thread
//! (macOS compliant).
//!
//! Single binary: `open2jam-rs`
//! - No CLI args → launches menu
//! - `<path.ojn>` → launches game directly

pub mod app;
pub mod assets;
pub mod audio;
pub mod game_state;
pub mod gameplay;
pub mod gpu;
pub mod input;
pub mod render;
pub mod render_game;
pub mod skin;
pub mod types;

pub use open2jam_rs_core as core;
pub use open2jam_rs_menu as menu;
pub use open2jam_rs_parsers as parsing;
