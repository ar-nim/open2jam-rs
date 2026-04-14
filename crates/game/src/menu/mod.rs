//! Menu GUI module — song library, selection table, configuration panels.
//!
//! This module was formerly the `open2jam-rs-menu` crate. It has been merged
//! into the game crate as part of binary unification (Phase 1).
//!
//! The menu still uses its own entry point (`main_menu.rs`) and runs via
//! `eframe::run_native()` — structurally separate from the game binary
//! (`main.rs`) but compiled from the same crate.

pub mod db;
pub mod menu_app;
pub mod panels;
