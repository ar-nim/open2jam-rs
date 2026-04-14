//! Open2Jam rhythm game — unified binary crate.
//!
//! Both the menu GUI and the game engine are compiled from this single crate.
//! They share the same winit window and wgpu device, running on the main thread
//! (macOS compliant).
//!
//! Single binary: `open2jam-rs`
//! - No CLI args → launches menu
//! - `<path.ojn>` → launches game directly

pub mod audio;
pub mod engine;
pub mod game_state;
pub mod gameplay;
pub mod parsing;
pub mod render;
pub mod resources;
pub mod skin;
pub mod test_harness;

// Menu GUI module (formerly the open2jam-rs-menu crate)
pub(crate) mod menu {
    pub mod db;
    pub mod menu_app;
    pub mod panels;
}
