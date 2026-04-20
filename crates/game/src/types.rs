//! Shared data types extracted from engine.rs.
//!
//! Contains pure data definitions: enums, newtypes, and structs with no
//! internal state machine logic. This module intentionally contains no
//! business logic — only types that are shared across the application.
//!
//! # Types
//!
//! - [`LaneIndex`] — newtype for validated lane indices (0–6)
//! - [`AppMode`] — menu vs. game engine mode
//! - [`RenderMetrics`] — spatial layout for game rendering
//! - [`LoadingMessage`] — async loading result passed via mpsc
//! - [`FrameLimiter`] — hybrid spin-sleep FPS limiter

use mint::Vector2;

// ---------------------------------------------------------------------------
// LaneIndex newtype
// ---------------------------------------------------------------------------

/// A validated lane index for the 7-note-lane gameplay.
///
/// `LaneIndex` wraps a `u8` and only accepts values 0–6.
/// This makes invalid lane indices unrepresentable at the type level,
/// preventing lane index errors from propagating through the codebase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneIndex(u8);

impl LaneIndex {
    /// The maximum valid lane index (6, for 7 lanes: 0–6).
    pub const MAX: u8 = 6;

    /// Create a `LaneIndex` if the value is within the valid range.
    ///
    /// # Errors
    ///
    /// Returns `None` if `val > 6`.
    #[must_use]
    pub fn new(val: u8) -> Option<Self> {
        if val <= Self::MAX {
            Some(Self(val))
        } else {
            None
        }
    }

    /// Convert to `usize` for array/index lookups.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

// ---------------------------------------------------------------------------
// AppMode
// ---------------------------------------------------------------------------

/// The current application mode: menu GUI or game engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    /// Menu GUI: song library, configuration panels.
    Menu,
    /// Game engine: chart playback, note rendering, audio.
    Playing,
}

// ---------------------------------------------------------------------------
// RenderMetrics
// ---------------------------------------------------------------------------

/// Spatial metrics for game rendering.
///
/// Bundled into a single struct to avoid parameter bloat and keep the
/// [`render_game`](crate::render_game::render_game) signature stable
/// as new layout fields are added.
#[derive(Debug, Clone, Copy)]
pub struct RenderMetrics {
    /// Scale factor for skin sprites [x, y].
    pub scale: Vector2<f32>,
    /// Pixel offset for the rendering area [x, y].
    pub offset: Vector2<f32>,
    /// Y position of the judgment line in skin pixels.
    pub judgment_line_y: f32,
}

impl Default for RenderMetrics {
    fn default() -> Self {
        Self {
            scale: Vector2 { x: 1.0, y: 1.0 },
            offset: Vector2 { x: 0.0, y: 0.0 },
            judgment_line_y: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// LoadingMessage
// ---------------------------------------------------------------------------

/// A message sent from a background loading thread to the main thread.
#[allow(clippy::large_enum_variant)]
pub enum LoadingMessage {
    /// Game state loaded successfully or failed.
    GameLoaded(anyhow::Result<crate::game_state::GameState>),
    /// Skin/atlas loaded successfully or failed.
    SkinLoaded(Result<SkinLoadOutput, anyhow::Error>),
}

/// The output of skin/atlas loading.
pub struct SkinLoadOutput {
    pub atlas: Option<crate::render::atlas::SkinAtlas>,
    pub resources: Option<open2jam_rs_parsers::xml::Resources>,
    pub scale: (f32, f32),
}

// ---------------------------------------------------------------------------
// FrameLimiter
// ---------------------------------------------------------------------------

/// Hybrid spin-sleep frame limiter — same approach as open2jam-modern Java.
///
/// Sleep in 1ms increments, stop 1ms early, then spin-wait for nanosecond
/// precision. Uses absolute target time to prevent drift.
pub struct FrameLimiter {
    target_frame_duration_ns: u64,
    next_frame_deadline: std::time::Instant,
}

impl FrameLimiter {
    /// Create a new frame limiter targeting `target_fps`.
    #[must_use]
    pub fn new(target_fps: f64) -> Self {
        let target_frame_duration_ns = (1_000_000_000.0 / target_fps) as u64;
        Self {
            target_frame_duration_ns,
            next_frame_deadline: std::time::Instant::now()
                + std::time::Duration::from_nanos(target_frame_duration_ns),
        }
    }

    /// The target duration of one frame in nanoseconds.
    #[must_use]
    pub fn target_frame_duration_ns(&self) -> u64 {
        self.target_frame_duration_ns
    }

    /// Wait until the next frame boundary using hybrid sleep + spin-wait.
    pub fn wait(&mut self) {
        let target_ns = self.next_frame_deadline.elapsed().as_nanos() as i64;
        if target_ns >= 0 {
            self.next_frame_deadline = std::time::Instant::now()
                + std::time::Duration::from_nanos(self.target_frame_duration_ns);
            return;
        }
        let time_remaining_ns = (-target_ns) as u64;
        let sleep_until_ns = time_remaining_ns.saturating_sub(1_000_000);
        if sleep_until_ns > 0 {
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_nanos(sleep_until_ns);
            while std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        while std::time::Instant::now() < self.next_frame_deadline {
            std::hint::spin_loop();
        }
        self.next_frame_deadline += std::time::Duration::from_nanos(self.target_frame_duration_ns);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    mod lane_index {
        use super::*;

        #[test]
        fn new_with_valid_value_should_return_some() {
            for val in 0..=6u8 {
                let lane = LaneIndex::new(val);
                assert!(lane.is_some(), "LaneIndex::new({}) should succeed", val);
                assert_eq!(lane.unwrap().0, val);
            }
        }

        #[test]
        fn new_with_invalid_value_should_return_none() {
            assert!(LaneIndex::new(7).is_none());
            assert!(LaneIndex::new(255).is_none());
        }

        #[test]
        fn index_returns_usize() {
            assert_eq!(LaneIndex::new(3).unwrap().index(), 3);
            assert_eq!(LaneIndex::new(0).unwrap().index(), 0);
        }

        #[test]
        fn max_value_is_six() {
            assert_eq!(LaneIndex::MAX, 6);
        }

        #[test]
        fn lane_index_eq_and_ne() {
            let a = LaneIndex::new(2).unwrap();
            let b = LaneIndex::new(2).unwrap();
            let c = LaneIndex::new(4).unwrap();
            assert_eq!(a, b);
            assert_ne!(a, c);
        }
    }

    mod app_mode {
        use super::*;

        #[test]
        fn app_mode_has_menu_and_playing() {
            assert_eq!(AppMode::Menu, AppMode::Menu);
            assert_eq!(AppMode::Playing, AppMode::Playing);
            assert_ne!(AppMode::Menu, AppMode::Playing);
        }

        #[test]
        fn app_mode_debug() {
            assert!(format!("{:?}", AppMode::Menu).contains("Menu"));
            assert!(format!("{:?}", AppMode::Playing).contains("Playing"));
        }
    }

    mod render_metrics {
        use super::*;

        #[test]
        fn default_values() {
            let m = RenderMetrics::default();
            assert_eq!(m.scale.x, 1.0);
            assert_eq!(m.scale.y, 1.0);
            assert_eq!(m.offset.x, 0.0);
            assert_eq!(m.offset.y, 0.0);
            assert_eq!(m.judgment_line_y, 0.0);
        }

        #[test]
        fn custom_values() {
            let m = RenderMetrics {
                scale: Vector2 { x: 2.0, y: 3.0 },
                offset: Vector2 { x: 10.0, y: 20.0 },
                judgment_line_y: 480.0,
            };
            assert_eq!(m.scale.x, 2.0);
            assert_eq!(m.scale.y, 3.0);
            assert_eq!(m.offset.x, 10.0);
            assert_eq!(m.offset.y, 20.0);
            assert_eq!(m.judgment_line_y, 480.0);
        }
    }

    mod frame_limiter {
        use super::*;

        #[test]
        fn target_frame_duration_ns_at_60fps() {
            let limiter = FrameLimiter::new(60.0);
            assert_eq!(limiter.target_frame_duration_ns(), 16_666_666);
        }

        #[test]
        fn target_frame_duration_ns_at_120fps() {
            let limiter = FrameLimiter::new(120.0);
            assert_eq!(limiter.target_frame_duration_ns(), 8_333_333);
        }

        #[test]
        fn target_frame_duration_ns_at_144fps() {
            let limiter = FrameLimiter::new(144.0);
            assert_eq!(limiter.target_frame_duration_ns(), 6_944_444);
        }
    }
}
