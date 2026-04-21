//! Core application orchestrator types.
//!
//! Contains shared data types used by both menu and game crates:
//! - [`Transition`] — signal types for state changes
//! - [`AppMode`] — menu vs game engine mode
//!
//! No concrete App struct lives here — that stays in the game crate to avoid
//! circular dependencies (game → core → menu/game → ...).

use std::path::PathBuf;

/// Signals a requested state transition from the menu or game engine.
///
/// The App acts on these signals in its event loop. States themselves
/// never construct another state — they only signal intent.
#[derive(Debug, Clone)]
#[must_use]
#[non_exhaustive]
pub enum Transition {
    /// No transition requested.
    None,
    /// Menu requests: load game with this OJN path.
    LoadGame(PathBuf),
    /// Game requests: return to menu.
    ReturnToMenu,
    /// Quit the application.
    Quit,
    /// State cannot continue — e.g., WGPU device lost.
    Error(String),
}

/// The current application mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// Menu GUI: song library, configuration panels.
    Menu,
    /// Game engine: chart playback, note rendering, audio.
    Game,
}
