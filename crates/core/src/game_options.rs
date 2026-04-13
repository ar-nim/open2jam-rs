//! Game options: speed, visibility, channel modifiers, display settings.

use serde::{Deserialize, Serialize};

/// Scroll speed type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpeedType {
    HiSpeed,
    XSpeed,
    WSpeed,
    RegulSpeed,
}

impl Default for SpeedType {
    fn default() -> Self {
        Self::HiSpeed
    }
}

impl std::fmt::Display for SpeedType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HiSpeed => write!(f, "Hi-Speed"),
            Self::XSpeed => write!(f, "xR-Speed"),
            Self::WSpeed => write!(f, "W-Speed"),
            Self::RegulSpeed => write!(f, "Regul-Speed"),
        }
    }
}

/// Lane channel modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelMod {
    None,
    Random,
    Panic,
    Mirror,
}

impl Default for ChannelMod {
    fn default() -> Self {
        Self::None
    }
}

impl std::fmt::Display for ChannelMod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Random => write!(f, "Random"),
            Self::Panic => write!(f, "Panic"),
            Self::Mirror => write!(f, "Mirror"),
        }
    }
}

/// Note visibility modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisibilityMod {
    None,
    Hidden,
    Sudden,
    Dark,
}

impl Default for VisibilityMod {
    fn default() -> Self {
        Self::None
    }
}

impl std::fmt::Display for VisibilityMod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Hidden => write!(f, "Hidden"),
            Self::Sudden => write!(f, "Sudden"),
            Self::Dark => write!(f, "Dark"),
        }
    }
}

/// FPS limiter setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FpsLimiter {
    Unlimited,
    X1,
    X2,
    X4,
    X8,
}

impl Default for FpsLimiter {
    fn default() -> Self {
        Self::X1
    }
}

impl std::fmt::Display for FpsLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unlimited => write!(f, "Unlimited"),
            Self::X1 => write!(f, "1x Refresh Rate"),
            Self::X2 => write!(f, "2x"),
            Self::X4 => write!(f, "4x"),
            Self::X8 => write!(f, "8x"),
        }
    }
}

/// UI theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiTheme {
    Automatic,
    Light,
    Dark,
}

impl Default for UiTheme {
    fn default() -> Self {
        Self::Automatic
    }
}

impl std::fmt::Display for UiTheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Automatic => write!(f, "Automatic"),
            Self::Light => write!(f, "Light"),
            Self::Dark => write!(f, "Dark"),
        }
    }
}

/// Difficulty selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

impl Default for Difficulty {
    fn default() -> Self {
        Self::Normal
    }
}

/// Complete game options, mirroring Java GameOptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameOptions {
    /// Scroll speed multiplier (0.5–10.0 for Hi-Speed).
    #[serde(default = "default_speed")]
    pub speed_multiplier: f32,

    /// Speed type (Hi-Speed, xR-Speed, etc.).
    #[serde(default)]
    pub speed_type: SpeedType,

    /// Visibility modifier (None, Hidden, Sudden, Dark).
    #[serde(default)]
    pub visibility_modifier: VisibilityMod,

    /// Channel modifier (None, Mirror, Shuffle, Random).
    #[serde(default)]
    pub channel_modifier: ChannelMod,

    /// Keysound volume (0.0–1.0).
    #[serde(default = "default_one")]
    pub key_volume: f32,

    /// BGM volume (0.0–1.0).
    #[serde(default = "default_one")]
    pub bgm_volume: f32,

    /// Master volume (0.0–1.0).
    #[serde(default = "default_one")]
    pub master_volume: f32,

    /// Auto-play all notes as COOL.
    #[serde(default)]
    pub autoplay: bool,

    /// Auto-play BGM sounds (keysounds only on player input otherwise).
    #[serde(default = "default_true")]
    pub autosound: bool,

    /// Use timed judgment like Bemani games (timing-based rather than position-based).
    #[serde(default)]
    pub timed_judgment: bool,

    /// Selected difficulty (Easy/Normal/Hard).
    #[serde(default)]
    pub difficulty: Difficulty,

    /// Run game in fullscreen.
    #[serde(default)]
    pub display_fullscreen: bool,

    /// Enable VSync.
    #[serde(default = "default_true")]
    pub display_vsync: bool,

    /// FPS limiter.
    #[serde(default)]
    pub fps_limiter: FpsLimiter,

    /// Window width.
    #[serde(default = "default_width")]
    pub display_width: u32,

    /// Window height.
    #[serde(default = "default_height")]
    pub display_height: u32,

    /// Display refresh rate in Hz (0 = use monitor default).
    #[serde(default)]
    pub display_refresh_rate: u32,

    /// Audio buffer size (1–4096).
    #[serde(default = "default_buffer")]
    pub buffer_size: u32,

    /// Display lag offset (ms).
    #[serde(default)]
    pub display_lag: f32,

    /// Audio latency offset (ms).
    #[serde(default)]
    pub audio_latency: f32,

    /// GUI theme.
    #[serde(default)]
    pub ui_theme: UiTheme,

    /// Menu window fullscreen state (independent of game fullscreen).
    #[serde(default)]
    pub menu_fullscreen: bool,

    /// Haste Mode: advanced timing mode.
    #[serde(default)]
    pub haste_mode: bool,

    /// Normalize Speed: sub-option of Haste Mode.
    #[serde(default = "default_true")]
    pub haste_mode_normalize_speed: bool,
}

fn default_speed() -> f32 {
    1.0
}
fn default_one() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}
fn default_width() -> u32 {
    1280
}
fn default_height() -> u32 {
    720
}
fn default_buffer() -> u32 {
    128
}

impl Default for GameOptions {
    fn default() -> Self {
        Self {
            speed_multiplier: 1.0,
            speed_type: SpeedType::HiSpeed,
            visibility_modifier: VisibilityMod::None,
            channel_modifier: ChannelMod::None,
            key_volume: 1.0,
            bgm_volume: 1.0,
            master_volume: 1.0,
            autoplay: false,
            autosound: true,
            timed_judgment: false,
            difficulty: Difficulty::Normal,
            display_fullscreen: false,
            display_vsync: true,
            fps_limiter: FpsLimiter::X1,
            display_width: 1280,
            display_height: 720,
            display_refresh_rate: 0,
            buffer_size: 128,
            display_lag: 0.0,
            audio_latency: 0.0,
            ui_theme: UiTheme::Automatic,
            menu_fullscreen: false,
            haste_mode: false,
            haste_mode_normalize_speed: true,
        }
    }
}
