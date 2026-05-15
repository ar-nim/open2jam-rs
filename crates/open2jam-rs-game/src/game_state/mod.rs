//! Game state: struct definition and note counting.

pub mod animations;
pub mod audio;
pub mod cleanup;
pub mod effects;
pub mod entities;
pub mod judge;
pub mod load;
pub mod spawn;
pub mod stats;
pub mod update;

use self::animations::{ComboCounterState, PendingJudgment};
use self::effects::{LongFlareEffect, NoteClickEffect};
use self::entities::{ActiveLongNote, ActiveNote};
use self::stats::GameStats;

use crate::audio::trigger::AudioTriggerSystem;
use crate::gameplay::clock::Clock;
use crate::gameplay::modifiers::Modifiers;
use crate::gameplay::timing_data::TimingData;
use crate::skin::prefab::NotePrefabs;
use open2jam_rs_parsers::Chart;

/// Total number of notes in the chart (for scoring).
pub fn count_playable_notes(chart: &Chart) -> u32 {
    chart
        .events
        .iter()
        .filter(|e| matches!(e, open2jam_rs_parsers::TimedEvent::Note(n) if n.is_note()))
        .count() as u32
}

/// The main game state.
pub struct GameState {
    pub clock: Clock,
    pub audio_triggers: AudioTriggerSystem,
    pub sound_cache: crate::audio::cache::SoundCache,
    pub chart: Chart,
    pub note_prefabs: NotePrefabs,
    /// Active tap notes on screen
    pub active_notes: Vec<ActiveNote>,
    /// Active long notes on screen
    pub active_long_notes: Vec<ActiveLongNote>,
    /// Iterator index into chart events
    pub next_event_idx: usize,
    /// Scroll speed multiplier
    pub scroll_speed: f64,
    /// BPM-aware timing data (velocity tree) for scroll calculation.
    pub timing: TimingData,
    /// Whether we're in auto-play mode
    pub auto_play: bool,
    /// Difficulty level (affects HP curve)
    pub difficulty: open2jam_rs_core::Difficulty,
    /// Which lanes currently have keys held down (for pressed note visual)
    pub pressed_lanes: [bool; 7],
    /// Spawn lead time in milliseconds
    pub spawn_lead_time_ms: f64,
    /// Game statistics
    pub stats: GameStats,
    /// Pending judgment results to visualize
    pub pending_judgments: Vec<PendingJudgment>,
    /// Combo counter with wobble animation
    pub combo_counter: ComboCounterState,
    /// Jam counter visibility timer (ms remaining, 0 = hidden)
    pub jam_counter_visible_ms: f64,
    /// Combo title visibility timer (ms remaining, 0 = hidden)
    pub combo_title_visible_ms: f64,
    /// Startup delay: time before gameplay begins (2000ms for lifebar fill animation)
    pub startup_delay_ms: f64,
    /// Whether the game is in rendering mode (false during startup delay)
    pub is_rendering: bool,
    /// Audio stream needs to be started (set true when startup delay completes)
    pub startup_audio_pending: bool,
    /// Life percentage during startup animation (0.0 to 1.0)
    pub startup_life_percent: f32,
    /// Song duration in milliseconds (from OJN header, used for progress bar display)
    pub song_duration_ms: f64,
    /// Calculated time (ms) when the song ends based on chart content.
    /// Includes a 1-measure buffer after the last event.
    pub end_time_ms: f64,
    /// Whether the song/game has ended
    pub is_song_ended: bool,
    /// Last absolute game time (for computing internal delta in update())
    pub last_absolute_ms: u64,
    /// Active note click effects (EFFECT_CLICK, triggered on Cool/Good for tap notes)
    pub note_click_effects: Vec<NoteClickEffect>,
    /// Active long note flare effects (EFFECT_LONGFLARE, triggered on Cool/Good for long notes)
    pub long_flare_effects: Vec<LongFlareEffect>,
    /// EFFECT_CLICK sprite ID from skin XML
    pub effect_click_sprite: Option<String>,
    /// EFFECT_CLICK animation duration (frame_count * frame_speed_ms)
    pub effect_click_duration_ms: f64,
    /// EFFECT_LONGFLARE sprite ID from skin XML
    pub effect_longflare_sprite: Option<String>,
    /// EFFECT_LONGFLARE animation duration (frame_count * frame_speed_ms)
    pub effect_longflare_duration_ms: f64,
    /// EFFECT_LONGFLARE Y position from skin XML (default 460)
    pub effect_longflare_y: i32,
    /// Index of the next BGM note to schedule (into chart.events)
    pub next_bgm_event_idx: usize,
    /// Lookahead window in milliseconds (how far ahead to schedule BGM notes)
    pub bgm_lookahead_ms: f64,
    /// Previous frame's combo count (for detecting combo increases across frames).
    pub prev_frame_combo: u32,
    /// Lane arrangement modifier (Mirror, Random, Panic)
    pub modifiers: Modifiers,
}
