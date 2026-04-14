//! Open2Jam rhythm game — unified binary crate.
//!
//! Contains both the game engine and the menu GUI, compiled from the same crate.
//! Two binaries are produced:
//! - `open2jam-rs` — game engine (via `main.rs`)
//! - `open2jam-rs-menu` — menu GUI (via `main_menu.rs`)

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
pub mod menu {
    pub mod db;
    pub mod menu_app;
    pub mod panels;
}
