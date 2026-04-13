//! Configuration persistence — mirrors Java config.json.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::game_options::GameOptions;
use crate::key_bindings::KeyBindings;

/// Default visible columns: Name, Level, Duration.
fn default_visible_columns() -> [bool; 6] {
    [true, false, true, false, true, false]
}

/// Top-level configuration, persisted to `config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub key_bindings: KeyBindings,
    #[serde(default)]
    pub game_options: GameOptions,
    /// Last opened library (SQLite database ID).
    pub last_opened_library_id: Option<u64>,
    /// Visible columns in the song table: [Name, Artist, Level, Bpm, Duration, Genre].
    #[serde(default = "default_visible_columns")]
    pub visible_columns: [bool; 6],
}

impl Default for Config {
    fn default() -> Self {
        Self {
            key_bindings: KeyBindings::default(),
            game_options: GameOptions::default(),
            last_opened_library_id: None,
            visible_columns: default_visible_columns(),
        }
    }
}

impl Config {
    /// Load config from a JSON file. Returns default config if file not found.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let data = std::fs::read_to_string(path)?;
        serde_json::from_str(&data)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Save config to a JSON file. Creates parent directories if needed.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, data)
    }

    /// Get the default config path: `~/.config/open2jam-rs/config.json`
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("open2jam-rs")
            .join("config.json")
    }
}
