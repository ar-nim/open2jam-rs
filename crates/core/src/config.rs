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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.last_opened_library_id.is_none());
        assert_eq!(
            config.visible_columns,
            [true, false, true, false, true, false]
        );
    }

    #[test]
    fn test_config_load_save_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let mut config = Config::default();
        config.last_opened_library_id = Some(42);

        // Save and reload
        config.save(&config_path).unwrap();
        let loaded = Config::load(&config_path).unwrap();

        assert_eq!(loaded.last_opened_library_id, Some(42));
    }

    #[test]
    fn test_config_load_returns_error_for_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("nonexistent.json");

        // Should return an error (not default config)
        let result = Config::load(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_load_returns_error_for_corrupt_data() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("corrupt.json");

        // Write corrupt JSON
        let mut file = std::fs::File::create(&config_path).unwrap();
        file.write_all(b"not valid json {").unwrap();

        let result = Config::load(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_save_creates_parent_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir
            .path()
            .join("nested")
            .join("dir")
            .join("config.json");

        let config = Config::default();
        config.save(&config_path).unwrap();

        assert!(config_path.exists());
    }

    #[test]
    fn test_game_options_default() {
        use crate::game_options::*;

        let opts = GameOptions::default();
        assert_eq!(opts.speed_multiplier, 1.0);
        assert_eq!(opts.speed_type, SpeedType::HiSpeed);
        assert_eq!(opts.visibility_modifier, VisibilityMod::None);
        assert_eq!(opts.channel_modifier, ChannelMod::None);
        assert_eq!(opts.key_volume, 1.0);
        assert_eq!(opts.bgm_volume, 1.0);
        assert!(!opts.autoplay);
        assert!(opts.autosound);
        assert_eq!(opts.difficulty, Difficulty::Normal);
        assert_eq!(opts.display_width, 1280);
        assert_eq!(opts.display_height, 720);
        assert_eq!(opts.buffer_size, 128);
        assert!(!opts.haste_mode);
    }

    #[test]
    fn test_speed_type_display() {
        use crate::game_options::SpeedType;
        assert_eq!(SpeedType::HiSpeed.to_string(), "Hi-Speed");
        assert_eq!(SpeedType::XSpeed.to_string(), "xR-Speed");
        assert_eq!(SpeedType::WSpeed.to_string(), "W-Speed");
        assert_eq!(SpeedType::RegulSpeed.to_string(), "Regul-Speed");
    }

    #[test]
    fn test_channel_mod_display() {
        use crate::game_options::ChannelMod;
        assert_eq!(ChannelMod::None.to_string(), "None");
        assert_eq!(ChannelMod::Random.to_string(), "Random");
        assert_eq!(ChannelMod::Panic.to_string(), "Panic");
        assert_eq!(ChannelMod::Mirror.to_string(), "Mirror");
    }

    #[test]
    fn test_visibility_mod_display() {
        use crate::game_options::VisibilityMod;
        assert_eq!(VisibilityMod::None.to_string(), "None");
        assert_eq!(VisibilityMod::Hidden.to_string(), "Hidden");
        assert_eq!(VisibilityMod::Sudden.to_string(), "Sudden");
        assert_eq!(VisibilityMod::Dark.to_string(), "Dark");
    }

    #[test]
    fn test_vsync_mode_display() {
        use crate::game_options::VSyncMode;
        assert_eq!(VSyncMode::On.to_string(), "On");
        assert_eq!(VSyncMode::Fast.to_string(), "Fast");
        assert_eq!(VSyncMode::Off.to_string(), "Off");
    }

    #[test]
    fn test_fps_limiter_display() {
        use crate::game_options::FpsLimiter;
        assert_eq!(FpsLimiter::X1.to_string(), "1x Refresh Rate");
        assert_eq!(FpsLimiter::X2.to_string(), "2x");
        assert_eq!(FpsLimiter::Unlimited.to_string(), "Unlimited");
    }

    #[test]
    fn test_ui_theme_display() {
        use crate::game_options::UiTheme;
        assert_eq!(UiTheme::Automatic.to_string(), "Automatic");
        assert_eq!(UiTheme::Light.to_string(), "Light");
        assert_eq!(UiTheme::Dark.to_string(), "Dark");
    }

    #[test]
    fn test_game_options_serialization() {
        let opts = GameOptions::default();
        let json = serde_json::to_string(&opts).unwrap();
        let restored: GameOptions = serde_json::from_str(&json).unwrap();

        assert_eq!(opts.speed_multiplier, restored.speed_multiplier);
        assert_eq!(opts.speed_type, restored.speed_type);
        assert_eq!(opts.difficulty, restored.difficulty);
    }

    #[test]
    fn test_key_bindings_default() {
        use crate::key_bindings::*;
        
        let bindings = KeyBindings::default();
        
        // K7 should have 7 lanes (default)
        assert_eq!(bindings.k7.lanes.len(), 7);
        assert_eq!(bindings.k7.lanes[0].key, "KeyS");
        assert_eq!(bindings.k7.lanes[3].key, "Space");
        assert_eq!(bindings.k7.lanes[6].key, "KeyL");
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(config.visible_columns, restored.visible_columns);
    }
}
