//! Keyboard key bindings for 4–8 key layouts.

use serde::{Deserialize, Serialize};

/// A single key map: physical key name (e.g., "KeyA", "Space") → logical action.
/// Stored as raw key name strings for simplicity; winit NamedKey/PhysicalKey at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMap {
    /// The physical key name (winit `NamedKey` or `Key::Character`).
    pub key: String,
}

/// Key bindings for a specific layout (K4, K5, K6, K7, K8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardLayout {
    /// Lane 1–8 note keys.
    #[serde(default)]
    pub lanes: Vec<KeyMap>,
}

impl Default for KeyboardLayout {
    fn default() -> Self {
        // Default K7: S D F Space J K L
        Self {
            lanes: vec![
                KeyMap { key: "KeyS".into() },
                KeyMap { key: "KeyD".into() },
                KeyMap { key: "KeyF".into() },
                KeyMap {
                    key: "Space".into(),
                },
                KeyMap { key: "KeyJ".into() },
                KeyMap { key: "KeyK".into() },
                KeyMap { key: "KeyL".into() },
            ],
        }
    }
}

/// All keyboard configurations, keyed by layout name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBindings {
    #[serde(default)]
    pub k4: KeyboardLayout,
    #[serde(default = "default_k5")]
    pub k5: KeyboardLayout,
    #[serde(default = "default_k6")]
    pub k6: KeyboardLayout,
    #[serde(default)]
    pub k7: KeyboardLayout,
    #[serde(default = "default_k8")]
    pub k8: KeyboardLayout,
}

fn default_k5() -> KeyboardLayout {
    KeyboardLayout {
        lanes: vec![
            KeyMap { key: "KeyD".into() },
            KeyMap {
                key: "Space".into(),
            },
            KeyMap { key: "KeyK".into() },
            KeyMap { key: "KeyL".into() },
            KeyMap {
                key: "Semicolon".into(),
            },
        ],
    }
}

fn default_k6() -> KeyboardLayout {
    KeyboardLayout {
        lanes: vec![
            KeyMap { key: "KeyD".into() },
            KeyMap { key: "KeyF".into() },
            KeyMap {
                key: "Space".into(),
            },
            KeyMap { key: "KeyJ".into() },
            KeyMap { key: "KeyK".into() },
            KeyMap { key: "KeyL".into() },
        ],
    }
}

fn default_k8() -> KeyboardLayout {
    KeyboardLayout {
        lanes: vec![
            KeyMap { key: "KeyA".into() },
            KeyMap { key: "KeyS".into() },
            KeyMap { key: "KeyD".into() },
            KeyMap { key: "KeyF".into() },
            KeyMap {
                key: "Space".into(),
            },
            KeyMap { key: "KeyJ".into() },
            KeyMap { key: "KeyK".into() },
            KeyMap { key: "KeyL".into() },
        ],
    }
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            k4: KeyboardLayout::default(),
            k5: default_k5(),
            k6: default_k6(),
            k7: KeyboardLayout::default(),
            k8: default_k8(),
        }
    }
}
