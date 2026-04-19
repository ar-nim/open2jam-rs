//! Keyboard key bindings for the 7-note K7 layout.

use serde::{Deserialize, Serialize};

/// A single key map: physical key name (e.g., "KeyA", "Space") → logical action.
/// Stored as raw key name strings for simplicity; winit NamedKey/PhysicalKey at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMap {
    /// The physical key name (winit `NamedKey` or `Key::Character`).
    pub key: String,
}

/// Key bindings for the K7 layout (7 lanes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBindings {
    /// Lane 1–7 note keys.
    #[serde(default)]
    pub k7: KeyboardLayout,
}

impl Default for KeyBindings {
    fn default() -> Self {
        // Default K7: S D F Space J K L
        Self {
            k7: KeyboardLayout {
                lanes: vec![
                    KeyMap { key: "KeyS".into() },
                    KeyMap { key: "KeyD".into() },
                    KeyMap { key: "KeyF".into() },
                    KeyMap { key: "Space".into() },
                    KeyMap { key: "KeyJ".into() },
                    KeyMap { key: "KeyK".into() },
                    KeyMap { key: "KeyL".into() },
                ],
            },
        }
    }
}

/// Key bindings for a specific layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardLayout {
    /// Lane note keys.
    #[serde(default)]
    pub lanes: Vec<KeyMap>,
}

impl Default for KeyboardLayout {
    fn default() -> Self {
        Self {
            lanes: vec![
                KeyMap { key: "KeyS".into() },
                KeyMap { key: "KeyD".into() },
                KeyMap { key: "KeyF".into() },
                KeyMap { key: "Space".into() },
                KeyMap { key: "KeyJ".into() },
                KeyMap { key: "KeyK".into() },
                KeyMap { key: "KeyL".into() },
            ],
        }
    }
}
