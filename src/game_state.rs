//! Game state machine: integrates clock, chart, audio, and note lifecycle.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use log::{info, warn};

use crate::audio::manager::AudioManager;
use crate::audio::trigger::{AudioTriggerEvent, AudioTriggerSystem};
use crate::audio::cache::SoundCache;
use crate::gameplay::judgment::{
    JudgmentType, judge_tap_note, judge_release, is_missed, cool_score_with_jam_bonus, good_score_with_jam_bonus,
};
use crate::gameplay::scroll::scroll_travel_time_ms;
use crate::gameplay::timing_data::TimingData;
use crate::parsing::ojn::{Chart, NoteType, TimedEvent};
use crate::resources::clock::Clock;
use crate::skin::prefab::NotePrefabs;
use crate::parsing::xml::Resources as SkinResources;

/// A note click effect instance (EFFECT_CLICK sprite, shown for Cool/Good).
///
/// Animation behavior: plays through all frames of the effect_click_1 sprite once,
/// then disappears. Centered on the lane at the judgment line position.
#[derive(Debug, Clone)]
pub struct NoteClickEffect {
    /// Lane index (0-6)
    pub lane: usize,
    /// Time when the effect was triggered (for animation timing)
    pub time_created_ms: f64,
    /// Total duration the effect is displayed (based on frame count * frame_speed)
    pub duration_ms: f64,
}

impl NoteClickEffect {
    pub fn new(lane: usize, time_ms: f64, duration_ms: f64) -> Self {
        Self {
            lane,
            time_created_ms: time_ms,
            duration_ms,
        }
    }

    /// Check if this effect is still active (animating).
    pub fn is_active(&self, current_time_ms: f64) -> bool {
        (current_time_ms - self.time_created_ms) < self.duration_ms
    }

    /// Get the current animation frame index.
    /// Loops through frames indefinitely (matches Java AnimatedEntity modulo behavior).
    pub fn frame_index(&self, current_time_ms: f64, frame_speed_ms: u32, frame_count: usize) -> usize {
        let elapsed = current_time_ms - self.time_created_ms;
        if frame_speed_ms == 0 || frame_count == 0 {
            return 0;
        }
        let idx = (elapsed / frame_speed_ms as f64) as usize;
        idx % frame_count  // loops, matches Java: sub_frame %= frames.size()
    }
}

/// A long note flare effect instance (EFFECT_LONGFLARE sprite, shown for Cool/Good).
///
/// Animation behavior: plays through all frames of the longflare sprite once,
/// then disappears. Centered on the lane at the judgment line position.
#[derive(Debug, Clone)]
pub struct LongFlareEffect {
    /// Lane index (0-6)
    pub lane: usize,
    /// Time when the effect was triggered (for animation timing)
    pub time_created_ms: f64,
    /// Total duration the effect is displayed (based on frame count * frame_speed)
    pub duration_ms: f64,
}

impl LongFlareEffect {
    pub fn new(lane: usize, time_ms: f64, duration_ms: f64) -> Self {
        Self {
            lane,
            time_created_ms: time_ms,
            duration_ms,
        }
    }

    /// Check if this effect is still active (animating).
    pub fn is_active(&self, current_time_ms: f64) -> bool {
        (current_time_ms - self.time_created_ms) < self.duration_ms
    }

    /// Get the current animation frame index.
    /// Loops through frames indefinitely (matches Java AnimatedEntity modulo behavior).
    pub fn frame_index(&self, current_time_ms: f64, frame_speed_ms: u32, frame_count: usize) -> usize {
        let elapsed = current_time_ms - self.time_created_ms;
        if frame_speed_ms == 0 || frame_count == 0 {
            return 0;
        }
        let idx = (elapsed / frame_speed_ms as f64) as usize;
        idx % frame_count  // loops, matches Java: sub_frame %= frames.size()
    }
}

/// Total number of notes in the chart (for scoring).
pub fn count_playable_notes(chart: &Chart) -> u32 {
    chart.events.iter().filter(|e| {
        matches!(e, TimedEvent::Note(n) if n.is_note())
    }).count() as u32
}

/// A note entity in the active game.
#[derive(Debug, Clone)]
pub struct ActiveNote {
    pub lane: usize,
    pub target_time_ms: f64,
    pub sample_id: Option<u32>,
    pub judged: bool,
    pub missed: bool,
    pub judgment_type: Option<JudgmentType>,
}

/// A long note entity in the active game.
#[derive(Debug, Clone)]
pub struct ActiveLongNote {
    pub lane: usize,
    pub head_time_ms: f64,
    pub tail_time_ms: f64,
    pub sample_id: Option<u32>,
    pub judged: bool,
    pub missed: bool,
    pub holding: bool,
    pub dead: bool,
    pub head_judgment: Option<JudgmentType>,
    pub tail_judgment: Option<JudgmentType>,
}

/// Game statistics tracking: score, combo, life, judgment counts.
#[derive(Debug, Clone)]
pub struct GameStats {
    /// Current score
    pub score: u32,
    /// Current combo counter (resets on BAD/MISS)
    pub combo: u32,
    /// Maximum combo achieved during the game
    pub max_combo: u32,
    /// Jam counter: +4 for COOL, +2 for GOOD, +0 for BAD/MISS
    pub jam_counter: u32,
    /// Jam combo: every 100 jam_counter = 1 jam combo (the multiplier)
    pub jam_combo: u32,
    /// Maximum jam combo
    pub max_jam_combo: u32,
    /// Number of COOL judgments
    pub cool_count: u32,
    /// Number of GOOD judgments
    pub good_count: u32,
    /// Number of BAD judgments
    pub bad_count: u32,
    /// Number of MISS judgments
    pub miss_count: u32,
    /// Current life/health (starts at 1000, game over at 0)
    pub life: i32,
    /// Maximum life
    pub max_life: i32,
    /// Number of pills/buffers collected (1 per 15 consecutive Cools, max 5)
    pub pill_count: u32,
    /// Total number of playable notes in the chart
    pub total_notes: u32,
    /// Consecutive Cools counter (for buffer/pill awards, resets on Good/Miss)
    pub consecutive_cools: u32,
}

impl GameStats {
    /// Create a new stats tracker with initial values.
    pub fn new(total_notes: u32, max_life: i32) -> Self {
        Self {
            score: 0,
            combo: 0,
            max_combo: 0,
            jam_counter: 0,
            jam_combo: 0,
            max_jam_combo: 0,
            cool_count: 0,
            good_count: 0,
            bad_count: 0,
            miss_count: 0,
            life: max_life,
            max_life,
            pill_count: 0,
            total_notes,
            consecutive_cools: 0,
        }
    }

    /// Record a judgment result and update all counters.
    /// Uses the new scoring system with jam combo bonuses.
    /// Score is calculated BEFORE jam_counter increments (matches C++ behavior).
    pub fn record_judgment(&mut self, judgment: JudgmentType, has_pill: bool) {
        // Check if pill converts BAD to COOL
        let effective_judgment = if has_pill && judgment == JudgmentType::Bad {
            JudgmentType::Cool
        } else {
            judgment
        };

        // Score is calculated using CURRENT jam_combo (before incrementing jam_counter)
        // This matches C++ behavior where scoring happens before jam_counter update
        let note_score: i32 = match effective_judgment {
            JudgmentType::Cool => cool_score_with_jam_bonus(self.jam_combo) as i32,
            JudgmentType::Good => good_score_with_jam_bonus(self.jam_combo) as i32,
            JudgmentType::Bad => 4,
            JudgmentType::Miss => -10, // Penalty for missing
        };
        if note_score >= 0 {
            self.score += note_score as u32;
        } else {
            let penalty = (-note_score) as u32;
            self.score = self.score.saturating_sub(penalty);
        }

        let is_first_hit = self.combo == 0 && self.cool_count == 0 && self.good_count == 0;
        match effective_judgment {
            JudgmentType::Cool => {
                self.cool_count += 1;
                if !is_first_hit {
                    self.combo += 1;
                    self.consecutive_cools += 1;
                }
                self.jam_counter += 4;
            }
            JudgmentType::Good => {
                self.good_count += 1;
                if !is_first_hit {
                    self.combo += 1;
                }
                self.consecutive_cools = 0;
                self.jam_counter += 2;
            }
            JudgmentType::Bad => {
                self.bad_count += 1;
                self.combo = 0;
                self.jam_counter = 0;
                self.jam_combo = 0; // Reset jam combo multiplier on combo break
            }
            JudgmentType::Miss => {
                self.miss_count += 1;
                self.combo = 0;
                self.jam_counter = 0;
                self.jam_combo = 0; // Reset jam combo multiplier on combo break
                self.consecutive_cools = 0;
            }
        }

        // Check threshold crossed (no division: subtract and increment)
        while self.jam_counter >= 100 {
            self.jam_counter -= 100;
            self.jam_combo += 1;
            if self.jam_combo > self.max_jam_combo {
                self.max_jam_combo = self.jam_combo;
            }
        }

        if self.combo > self.max_combo {
            self.max_combo = self.combo;
        }

        // Award buffers/pills: 1 per 15 consecutive Cools, max 5 stored
        if self.consecutive_cools > 0 && self.consecutive_cools % 15 == 0 {
            let expected_buffers = (self.consecutive_cools / 15).min(5);
            if expected_buffers > self.pill_count {
                self.pill_count = expected_buffers;
            }
        }

        // Update life (Hard difficulty)
        self.life += effective_judgment.hp_change_hard();
        self.life = self.life.clamp(0, self.max_life);

    }

    /// Check if the game is over (life reached 0).
    pub fn is_game_over(&self) -> bool {
        self.life <= 0
    }

    /// Get life as a normalized value (0.0 to 1.0).
    pub fn life_percent(&self) -> f32 {
        if self.max_life <= 0 {
            0.0
        } else {
            (self.life as f32 / self.max_life as f32).clamp(0.0, 1.0)
        }
    }
}

/// A pending judgment result to be visualized.
///
/// Animation behavior (matches Java open2jam):
/// - Pop-in: scales from 50%→100% over first 100ms
/// - Stays at full size for 3s total
/// - Disappears after 3s
#[derive(Debug, Clone)]
pub struct PendingJudgment {
    pub judgment_type: JudgmentType,
    pub lane: usize,
    /// Time when the judgment was made (for animation timing)
    pub time_created_ms: f64,
    /// Total duration the judgment is displayed (2000ms)
    pub duration_ms: f64,
    /// Pop-in duration (100ms for 50%→100% scale)
    pub pop_in_ms: f64,
}

impl PendingJudgment {
    pub fn new(judgment_type: JudgmentType, lane: usize, time_ms: f64) -> Self {
        Self {
            judgment_type,
            lane,
            time_created_ms: time_ms,
            duration_ms: 750.0,
            pop_in_ms: 100.0,
        }
    }

    /// Check if this judgment is still visible.
    pub fn is_active(&self, current_time_ms: f64) -> bool {
        (current_time_ms - self.time_created_ms) < self.duration_ms
    }

    /// Get the current scale factor (0.5→1.0 during pop-in, then 1.0).
    pub fn scale_factor(&self, current_time_ms: f64) -> f64 {
        let elapsed = current_time_ms - self.time_created_ms;
        if elapsed < self.pop_in_ms {
            // Pop-in: 50%→100% over first 100ms
            0.5 + (elapsed / self.pop_in_ms) * 0.5
        } else {
            1.0
        }
    }

    /// Get the current alpha (1.0 until near end, then fades).
    pub fn alpha(&self, current_time_ms: f64) -> f64 {
        let elapsed = current_time_ms - self.time_created_ms;
        let remaining = self.duration_ms - elapsed;
        // Fade out in last 200ms
        if remaining < 200.0 {
            (remaining / 200.0).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
}

/// Combo counter entity with wobble animation.
///
/// Animation behavior (matches Java open2jam):
/// - On increment: drops 10px, slides back up in 20ms
/// - Visible for 4s total, then hidden until next combo
#[derive(Debug, Clone)]
pub struct ComboCounterState {
    /// Current combo number
    pub number: u32,
    /// Base Y position (skin coords)
    pub base_y: f32,
    /// Current Y offset from base (for wobble)
    pub y_offset: f32,
    /// Visibility timer (counts down from 4000ms)
    pub show_time_ms: f64,
    /// Whether the counter is currently visible
    pub visible: bool,
}

impl ComboCounterState {
    pub fn new(base_y: f32) -> Self {
        Self {
            number: 0,
            base_y,
            y_offset: 0.0,
            show_time_ms: 0.0,
            visible: false,
        }
    }

    /// Increment combo and trigger wobble animation.
    pub fn increment(&mut self) {
        self.number += 1;
        self.y_offset = 10.0; // Drop 10px
        self.show_time_ms = 750.0;
        self.visible = true;
    }

    /// Reset combo to 0 and hide.
    pub fn reset(&mut self) {
        self.number = 0;
        self.show_time_ms = 0.0;
        self.visible = false;
        self.y_offset = 0.0;
    }

    /// Update animation state.
    pub fn update(&mut self, delta_ms: f64) {
        if self.show_time_ms > 0.0 {
            self.show_time_ms -= delta_ms;
            if self.show_time_ms <= 0.0 {
                self.visible = false;
            }
        }

        // Slide back up: -0.5px per ms (10px / 20ms = 0.5px/ms)
        if self.y_offset > 0.0 {
            self.y_offset -= delta_ms as f32 * 0.5;
            if self.y_offset < 0.0 {
                self.y_offset = 0.0;
            }
        }
    }

    /// Get current Y position (base + offset).
    pub fn current_y(&self) -> f32 {
        self.base_y + self.y_offset
    }
}

/// The main game state.
pub struct GameState {
    pub clock: Clock,
    pub audio_triggers: AudioTriggerSystem,
    pub sound_cache: SoundCache,
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
    /// Max combo counter visibility timer (ms remaining, 0 = hidden)
    pub max_combo_counter_visible_ms: f64,
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
}

impl GameState {
    /// Load chart, audio, and skin from file paths.
    pub fn load(
        ojn_path: impl AsRef<Path>,
        scroll_speed: f64,
        auto_play: bool,
        skin_resources: Option<&SkinResources>,
    ) -> Result<Self> {
        let ojn_path = ojn_path.as_ref();
        let dir = ojn_path.parent().context("OJN file must have a parent directory")?;

        // 1. Parse the OJN chart
        info!("Parsing chart: {}", ojn_path.display());
        let chart = crate::parsing::ojn::parse_file(ojn_path)
            .with_context(|| format!("Failed to parse OJN: {}", ojn_path.display()))?;
        info!(
            "Chart loaded: {} - {} ({} events, {} measures)",
            chart.header.title,
            chart.header.artist,
            chart.events.len(),
            chart.events.iter().filter(|e| matches!(e, TimedEvent::Measure(_))).count()
        );

        // Count playable notes
        let total_playable_notes = count_playable_notes(&chart);

        // 2. Find and parse the OJM audio file
        let ojm_filename = &chart.header.ojm_filename;
        let ojm_path = dir.join(ojm_filename);
        info!("Loading audio: {}", ojm_path.display());
        let sample_map = crate::parsing::ojm::parse_file(&ojm_path)
            .with_context(|| format!("Failed to parse OJM: {}", ojm_path.display()))?;
        info!("OJM loaded: {} samples", sample_map.len());

        // 3. Decode audio samples into the sound cache
        let mut sound_cache = SoundCache::new();
        sound_cache.populate_from_sample_map(sample_map, &ojm_path.to_string_lossy());
        info!("Sound cache: {} decoded samples", sound_cache.len());

        // 4. Build note prefabs from skin XML if available, otherwise use defaults
        // Also extract effect entity info
        let (note_prefabs, click_sprite, click_duration, longflare_sprite, longflare_duration, longflare_y) =
            if let Some(skin_res) = skin_resources {
                if let Some(skin) = skin_res.get_skin("o2jam") {
                    info!("Building note prefabs from skin XML (o2jam)");
                    let prefabs = NotePrefabs::from_skin(skin);

                    // Extract EFFECT_CLICK entity and sprite data
                    let click_entity = skin.entities.iter()
                        .find(|e| e.id.as_deref() == Some("EFFECT_CLICK"));
                    let click_sprite = click_entity.and_then(|e| e.sprite.clone());
                    let click_duration = click_sprite.as_ref()
                        .and_then(|sprite_id| skin_res.sprites.get(sprite_id))
                        .map(|s| s.frames.len() as f64 * s.frame_speed_ms as f64)
                        .unwrap_or(660.0); // fallback: 11 frames × 60ms
                    if click_sprite.is_some() {
                        info!("EFFECT_CLICK sprite: {:?}, duration: {}ms", click_sprite, click_duration);
                    }

                    // Extract EFFECT_LONGFLARE entity and sprite data
                    let longflare_entity = skin.entities.iter()
                        .find(|e| e.id.as_deref() == Some("EFFECT_LONGFLARE"));
                    let longflare_sprite = longflare_entity.and_then(|e| e.sprite.clone());
                    let longflare_y = longflare_entity.map(|e| e.y).unwrap_or(460);
                    let longflare_duration = longflare_sprite.as_ref()
                        .and_then(|sprite_id| skin_res.sprites.get(sprite_id))
                        .map(|s| s.frames.len() as f64 * s.frame_speed_ms as f64)
                        .unwrap_or(450.0); // fallback: 15 frames × 30ms
                    if longflare_sprite.is_some() {
                        info!("EFFECT_LONGFLARE sprite: {:?}, y={}, duration: {}ms", longflare_sprite, longflare_y, longflare_duration);
                    }

                    (prefabs, click_sprite, click_duration, longflare_sprite, longflare_duration, longflare_y)
                } else {
                    info!("Skin 'o2jam' not found, using default 7-lane layout");
                    let prefabs = NotePrefabs::default_7lan(1000, 750, 600);
                    (prefabs, None, 660.0, None, 450.0, 460)
                }
            } else {
                info!("No skin resources provided, using default 7-lane layout");
                let prefabs = NotePrefabs::default_7lan(1000, 750, 600);
                (prefabs, None, 660.0, None, 450.0, 460)
            };

        // 5. Calculate spawn lead time based on BPM and viewport
        // 2x travel time ensures notes appear at the very top even at low BPM / 1x speed.
        // The +500ms padding gives extra margin for the scroll offset.
        let base_bpm = chart.header.bpm as f64;
        let viewport_height = note_prefabs.skin_height as f64;
        let travel_time = scroll_travel_time_ms(base_bpm, viewport_height, scroll_speed);
        let spawn_lead_time_ms = (travel_time * 2.0) + 500.0;

        // 6. Schedule audio triggers for BGM events (auto-play mode)
        // In autoplay, we use the BGM lookahead scheduler instead of AudioTriggerSystem
        let audio_triggers = AudioTriggerSystem::new();
        let next_bgm_event_idx = 0;
        let bgm_lookahead_ms = 500.0; // 500ms lookahead
        if !auto_play {
            // Manual play: schedule BGM via lookahead scheduler (not AudioTriggerSystem)
            // AudioTriggerSystem is kept empty; BGM is scheduled via push_bgm_command
        }

        let mut clock = Clock::new();
        clock.set_bpm(chart.header.bpm);
        clock.set_chart_padding(0);

        // 7. Build the velocity tree (TimingData) from BPM change events
        let mut timing = TimingData::new();
        // Add initial BPM at time 0 (no padding — the 2s startup animation serves as the buffer)
        timing.add(0.0, chart.header.bpm as f64);
        // Add each BPM change event
        for event in &chart.events {
            if let TimedEvent::BpmChange(bpm_event) = event {
                timing.add(bpm_event.time_ms, bpm_event.bpm);
            }
        }
        timing.finish();
        info!(
            "TimingData: {} BPM change points (base BPM={:.1})",
            timing.len(),
            chart.header.bpm
        );

        // 8. Initialize game stats
        let max_life = 1000;
        let stats = GameStats::new(total_playable_notes, max_life);

        // 8. Compute song end time using the original O2Jam position-based formula.
        // end_position = ceil(max(event measure + position)) + 1
        // end_time = ((end_position - refPosition) / bpm * 240000) + refTime
        // This matches the original: GetRenderPosition() > m_endPosition check.
        let end_time_ms = {
            const TICK_SIGNATURE: f64 = 240000.0; // 60000 * 4 = ms per measure at 60 BPM for 4/4
            
            // Find max(measure + position) across ALL events (includes release/tail positions)
            let max_pos = chart.events.iter()
                .filter_map(|e| match e {
                    TimedEvent::Note(n) => Some(n.measure as f64 + n.position),
                    TimedEvent::BpmChange(b) => Some(b.measure as f64 + b.position),
                    TimedEvent::Measure(m) => Some(m.measure as f64),
                })
                .fold(0.0, f64::max);
            
            let end_position = max_pos.ceil() + 1.0; // +1 measure buffer, matching m_endPosition
            
            log::info!("Chart position debug: max_pos={:.4}, end_position={:.4}", max_pos, end_position);
            
            // Simulate the reference-point tracking the original game does.
            // We start at measure 1.0 instead of 0.0 to match the original game's timeline.
            let mut ref_position = 1.0;
            let mut ref_time_ms = 0.0;
            let mut current_bpm = chart.header.bpm as f64;
            
            // Collect and sort BPM events by position
            let mut bpm_events: Vec<(f64, f64)> = chart.events.iter()
                .filter_map(|e| match e {
                    TimedEvent::BpmChange(b) => Some((b.measure as f64 + b.position, b.bpm)),
                    _ => None,
                })
                .collect();
            bpm_events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            
            for (bpm_pos, new_bpm) in &bpm_events {
                if *bpm_pos > ref_position {
                    ref_time_ms += (*bpm_pos - ref_position) / current_bpm * TICK_SIGNATURE;
                }
                log::info!("BPM event: pos={:.4}, bpm={:.1}, ref_time={:.1}ms", bpm_pos, new_bpm, ref_time_ms);
                ref_position = *bpm_pos;
                current_bpm = *new_bpm;
            }
            
            // Add time from last ref_position to end_position
            if end_position > ref_position {
                ref_time_ms += (end_position - ref_position) / current_bpm * TICK_SIGNATURE;
            }
            
            log::info!("End calculation: ref_position={:.4}, ref_time={:.1}ms, end_pos={:.4}, final={:.1}ms",
                ref_position, ref_time_ms, end_position, ref_time_ms);
            
            ref_time_ms
        };
        info!(
            "Song end: {:.1}ms ({:.1}s) based on chart position + 1 measure",
            end_time_ms, end_time_ms / 1000.0
        );

        let song_duration_ms = chart.header.duration_hard as f64 * 1000.0;

        Ok(Self {
            clock,
            audio_triggers,
            sound_cache,
            chart,
            note_prefabs,
            active_notes: Vec::new(),
            active_long_notes: Vec::new(),
            next_event_idx: 0,
            scroll_speed,
            timing,
            auto_play,
            pressed_lanes: [false; 7],
            spawn_lead_time_ms,
            stats,
            pending_judgments: Vec::new(),
            combo_counter: ComboCounterState::new(210.0), // COMBO_COUNTER y="210"
            jam_counter_visible_ms: 0.0,
            max_combo_counter_visible_ms: 0.0,
            combo_title_visible_ms: 0.0,
            startup_delay_ms: 2000.0, // 2 second startup delay
            is_rendering: false,
            startup_audio_pending: false,
            startup_life_percent: 0.0,
            song_duration_ms,
            end_time_ms,
            is_song_ended: false,
            last_absolute_ms: 0,
            note_click_effects: Vec::new(),
            long_flare_effects: Vec::new(),
            effect_click_sprite: click_sprite,
            effect_click_duration_ms: click_duration,
            effect_longflare_sprite: longflare_sprite,
            effect_longflare_duration_ms: longflare_duration,
            effect_longflare_y: longflare_y,
            next_bgm_event_idx,
            bgm_lookahead_ms,
        })
    }

    /// Startup delay duration in milliseconds (2000ms for lifebar fill animation)
    pub const STARTUP_DELAY_MS: f64 = 2000.0;

    /// Update game state to the given absolute elapsed time.
    /// No accumulation — the clock is driven directly.
    /// Computes the internal delta from the previous call for timers and animations.
    pub fn update(&mut self, absolute_ms: u64) {
        // Compute internal delta from last absolute time (for timers/animations)
        let delta_ms = absolute_ms.saturating_sub(self.last_absolute_ms);
        self.last_absolute_ms = absolute_ms;
        let delta = delta_ms as f64;

        // Set clock directly to the absolute time
        self.clock.set_raw_time(absolute_ms);

        // Handle startup delay phase
        if !self.is_rendering {
            if self.startup_delay_ms > 0.0 {
                self.startup_delay_ms -= delta;
                // Animate lifebar from 0 to 100% over 2000ms
                self.startup_life_percent = (1.0 - self.startup_delay_ms / Self::STARTUP_DELAY_MS).min(1.0) as f32;
            }
            if self.startup_delay_ms <= 0.0 {
                self.startup_delay_ms = 0.0;
                self.is_rendering = true;
                self.startup_life_percent = 1.0;
                // Start the game clock after startup animation
                self.clock.start();
                self.startup_audio_pending = true;
                info!("Startup delay complete, gameplay begins now");
            }
        }
        // No else branch — clock is driven directly by set_raw_time(absolute_ms).
        // The old advance_game_time(delta_ms) call is removed since it would
        // double-update the clock (already set above).

        // Update visibility timers (count down)
        if self.jam_counter_visible_ms > 0.0 {
            self.jam_counter_visible_ms -= delta;
            if self.jam_counter_visible_ms < 0.0 {
                self.jam_counter_visible_ms = 0.0;
            }
        }
        if self.max_combo_counter_visible_ms > 0.0 {
            self.max_combo_counter_visible_ms -= delta;
            if self.max_combo_counter_visible_ms < 0.0 {
                self.max_combo_counter_visible_ms = 0.0;
            }
        }
        if self.combo_title_visible_ms > 0.0 {
            self.combo_title_visible_ms -= delta;
            if self.combo_title_visible_ms < 0.0 {
                self.combo_title_visible_ms = 0.0;
            }
        }

        // Update combo counter animation
        self.combo_counter.update(delta);

        // Check if song has ended (game time exceeds calculated chart end time)
        if !self.is_song_ended && self.end_time_ms > 0.0 {
            let game_time = self.clock.game_time() as f64;
            if game_time >= self.end_time_ms {
                self.is_song_ended = true;
                info!("Song ended at {:.1}ms", self.end_time_ms);
            }
        }
    }

    /// Check if the song has ended (game time exceeded song duration from OJN metadata).
    pub fn is_song_ended(&self) -> bool {
        self.is_song_ended
    }

    /// Get song duration in milliseconds (from OJN header).
    pub fn song_duration_ms(&self) -> f64 {
        self.song_duration_ms
    }

    /// Get current game time in milliseconds.
    pub fn game_time_ms(&self) -> f64 {
        self.clock.game_time() as f64
    }

    /// Get the current life percentage (startup animation or gameplay stats)
    pub fn life_percent_for_display(&self) -> f32 {
        if !self.is_rendering {
            // During startup, show animated lifebar
            self.startup_life_percent
        } else {
            // During gameplay, show actual stats
            self.stats.life_percent()
        }
    }

    /// Show jam counter for 750ms.
    pub fn show_jam_counter(&mut self) {
        self.jam_counter_visible_ms = 750.0;
    }

    /// Show max combo counter for 750ms.
    pub fn show_max_combo_counter(&mut self) {
        self.max_combo_counter_visible_ms = 750.0;
    }

    /// Show combo title for 750ms.
    pub fn show_combo_title(&mut self) {
        self.combo_title_visible_ms = 750.0;
    }

    /// Check if jam counter is visible.
    pub fn is_jam_counter_visible(&self) -> bool {
        self.jam_counter_visible_ms > 0.0
    }

    /// Check if max combo counter is visible.
    pub fn is_max_combo_counter_visible(&self) -> bool {
        self.max_combo_counter_visible_ms > 0.0
    }

    /// Check if combo title is visible.
    pub fn is_combo_title_visible(&self) -> bool {
        self.combo_title_visible_ms > 0.0
    }

    /// Spawn notes that are within the spawn window.
    pub fn spawn_notes(&mut self) {
        let render_time = self.clock.render_time();

        while self.next_event_idx < self.chart.events.len() {
            let event = &self.chart.events[self.next_event_idx];
            let target_time = match event {
                TimedEvent::Note(n) => n.time_ms,
                _ => {
                    self.next_event_idx += 1;
                    continue;
                }
            };

            if target_time > render_time as f64 + self.spawn_lead_time_ms {
                break;
            }

            if let TimedEvent::Note(note_event) = event {
                if let Some(lane) = note_event.channel.lane_index() {
                    match note_event.note_type {
                        NoteType::Tap => {
                            self.active_notes.push(ActiveNote {
                                lane,
                                target_time_ms: note_event.time_ms,
                                sample_id: note_event.sample_id,
                                judged: false,
                                missed: false,
                                judgment_type: None,
                            });
                        }
                        NoteType::Hold => {
                            let end_time = note_event.end_time_ms.unwrap_or(note_event.time_ms + 500.0);
                            self.active_long_notes.push(ActiveLongNote {
                                lane,
                                head_time_ms: note_event.time_ms,
                                tail_time_ms: end_time,
                                sample_id: note_event.sample_id,
                                judged: false,
                                missed: false,
                                holding: false,
                                dead: false,
                                head_judgment: None,
                                tail_judgment: None,
                            });
                        }
                        NoteType::Release => {
                            // Skip (already paired with HEAD during parsing)
                        }
                    }
                }
            }

            self.next_event_idx += 1;
        }
    }

    /// Remove notes that have passed the judgment line.
    pub fn cleanup_notes(&mut self) {
        let render_time = self.clock.render_time() as f64;

        self.active_notes.retain(|note| {
            note.target_time_ms >= render_time
        });
        
        self.active_long_notes.retain(|long_note| {
            render_time <= long_note.tail_time_ms
        });
    }

    /// Process audio triggers for the current game time.
    ///
    /// Scans the chart for BGM notes within the lookahead window and pushes them
    /// to the BGM signal queue via `audio_manager.push_bgm_command()`. This ensures
    /// sample-accurate timing regardless of frame rate.
    ///
    /// Returns the number of BGM notes scheduled this frame.
    ///
    /// **Important:** Only schedules notes while the audio stream is active.
    /// Commands pushed before `stream.play()` would have stale `delay_samples`
    /// values that don't account for the gap between push time and stream start.
    pub fn process_audio(&mut self, audio_manager: &mut AudioManager) -> usize {
        use crate::audio::bgm_signal::BgmCommand;
        use crate::parsing::ojn::{NoteType, TimedEvent};

        // Don't push BGM commands until the audio stream is running.
        if !audio_manager.is_active() {
            return 0;
        }

        let game_time = self.clock.game_time() as f64;
        let lookahead_end = game_time + self.bgm_lookahead_ms;
        let mut scheduled_count = 0;

        // Scan chart for BGM notes within the lookahead window
        while self.next_bgm_event_idx < self.chart.events.len() {
            let event = &self.chart.events[self.next_bgm_event_idx];

            match event {
                TimedEvent::Note(note_event) => {
                    if note_event.note_type == NoteType::Release {
                        self.next_bgm_event_idx += 1;
                        continue;
                    }

                    if note_event.time_ms < game_time {
                        self.next_bgm_event_idx += 1;
                        continue;
                    }

                    if note_event.time_ms > lookahead_end {
                        break;
                    }

                    if let Some(sample_id) = note_event.sample_id {
                        if let Some(frames) = self.sound_cache.get_sound(sample_id) {
                            let delay_ms = note_event.time_ms - game_time;
                            let delay_samples = audio_manager.ms_to_samples(delay_ms.max(0.0));

                            let command = BgmCommand {
                                frames: Arc::clone(frames),
                                delay_samples,
                                volume: note_event.volume,
                                pan: note_event.pan,
                            };

                            // Push to BGM queue (ignore if queue is full — rare)
                            if let Err(_err) = audio_manager.push_bgm_command(command) {
                                log::warn!(
                                    "BGM queue full, dropping note: sample_id={} time={:.1}ms",
                                    sample_id, note_event.time_ms
                                );
                            } else {
                                scheduled_count += 1;
                            }
                        }
                    }

                    self.next_bgm_event_idx += 1;
                }
                _ => {
                    // Skip non-note events
                    self.next_bgm_event_idx += 1;
                }
            }
        }

        scheduled_count
    }

    /// Clear all pending judgments (original O2Jam behavior: instant replace).
    /// When a new judgment spawns, the previous one is immediately killed.
    pub fn clear_pending_judgments(&mut self) {
        self.pending_judgments.clear();
    }

    /// Add a new pending judgment, clearing all previous ones first (instant replace).
    pub fn add_pending_judgment(&mut self, judgment: PendingJudgment) {
        // Original O2Jam behavior: kill previous judgment entity immediately
        self.clear_pending_judgments();
        self.pending_judgments.push(judgment);
    }

    /// Auto-play judgment: automatically judge all notes that have reached the judgment line.
    /// In auto-play mode, all notes are judged as COOL.
    pub fn auto_judge_notes(&mut self) {
        if !self.auto_play {
            return;
        }

        let render_time = self.clock.render_time() as f64;
        let bpm = self.clock.bpm() as f64;

        // Collect judgments to add after iteration (avoid borrow conflicts)
        let mut judgments_to_add: Vec<PendingJudgment> = Vec::new();
        // Collect lanes that got judged (for triggering effects after iteration)
        let mut click_effect_lanes: Vec<usize> = Vec::new();
        let mut longflare_effect_lanes: Vec<(usize, f64)> = Vec::new(); // (lane, hold_duration)

        // Judge tap notes that have reached the judgment line
        // Use a wider tolerance for auto-play to ensure all notes are hit
        let auto_play_tolerance_ms = 10.0; // 10ms tolerance for auto-play
        
        for note in &mut self.active_notes {
            if !note.judged && !note.missed {
                let time_diff = (render_time - note.target_time_ms).abs();
                if time_diff < auto_play_tolerance_ms {
                    note.judged = true;
                    note.judgment_type = Some(JudgmentType::Cool);
                    
                    self.stats.record_judgment(JudgmentType::Cool, false);
                    
                    click_effect_lanes.push(note.lane);
                    
                    // Instant replace: clear previous judgments, add new one
                    judgments_to_add.push(PendingJudgment::new(
                        JudgmentType::Cool,
                        note.lane,
                        render_time,
                    ));
                }
            }
        }
        
        // Trigger effects after iteration (avoid borrow conflicts)
        for lane in click_effect_lanes.drain(..) {
            self.trigger_note_click_effect(lane, render_time);
        }

        // Check for missed tap notes
        for note in &mut self.active_notes {
            if !note.judged && !note.missed {
                if is_missed(render_time, note.target_time_ms, bpm) {
                    note.missed = true;
                    note.judgment_type = Some(JudgmentType::Miss);
                    self.stats.record_judgment(JudgmentType::Miss, false);
                    // Instant replace: clear previous judgments, add new one
                    judgments_to_add.push(PendingJudgment::new(
                        JudgmentType::Miss,
                        note.lane,
                        render_time,
                    ));
                }
            }
        }

        // Judge long note heads
        for long_note in &mut self.active_long_notes {
            if !long_note.judged && !long_note.missed {
                let head_diff = (render_time - long_note.head_time_ms).abs();
                if head_diff < auto_play_tolerance_ms {
                    long_note.judged = true;
                    long_note.head_judgment = Some(JudgmentType::Cool);
                    long_note.holding = true;
                    self.stats.record_judgment(JudgmentType::Cool, false);

                    // Calculate hold duration for this long note
                    let hold_duration = (long_note.tail_time_ms - long_note.head_time_ms).max(0.0);
                    longflare_effect_lanes.push((long_note.lane, hold_duration));

                    // Instant replace: clear previous judgments, add new one
                    judgments_to_add.push(PendingJudgment::new(
                        JudgmentType::Cool,
                        long_note.lane,
                        render_time,
                    ));
                }
            }
        }

        // Trigger longflare effects after iteration (avoid borrow conflicts)
        for (lane, hold_duration) in longflare_effect_lanes.drain(..) {
            self.trigger_longflare_effect(lane, render_time, Some(hold_duration));
        }

        // Judge long note tails (auto-release when tail passes judgment line)
        for long_note in &mut self.active_long_notes {
            if long_note.judged && long_note.tail_judgment.is_none() {
                if render_time >= long_note.tail_time_ms {
                    // If player is still holding, judge the release timing
                    if long_note.holding {
                        long_note.holding = false;
                        let time_diff = (render_time - long_note.tail_time_ms).abs();
                        let release_judgment = judge_release(time_diff, bpm);
                        long_note.tail_judgment = Some(release_judgment);
                        self.stats.record_judgment(release_judgment, false);
                        // Instant replace: clear previous judgments, add new one
                        judgments_to_add.push(PendingJudgment::new(
                            release_judgment,
                            long_note.lane,
                            render_time,
                        ));
                    } else {
                        // Player released early or never held - auto-miss
                        long_note.tail_judgment = Some(JudgmentType::Miss);
                        self.stats.record_judgment(JudgmentType::Miss, false);
                        // Instant replace: clear previous judgments, add new one
                        judgments_to_add.push(PendingJudgment::new(
                            JudgmentType::Miss,
                            long_note.lane,
                            render_time,
                        ));
                    }
                    long_note.dead = true;
                }
            }
        }

        // Add all judgments (instant replace: only the last one survives)
        if !judgments_to_add.is_empty() {
            // Clear all previous, add only the last judgment
            self.clear_pending_judgments();
            if let Some(last) = judgments_to_add.pop() {
                self.pending_judgments.push(last);
            }
        }

        // Clean up dead pending judgments
        self.pending_judgments.retain(|j| j.is_active(render_time));
    }

    /// Handle key press for a lane.
    /// Uses instant-replace behavior: new judgment kills previous one immediately.
    pub fn handle_key_press(&mut self, lane: usize, _judgment_window_ms: f64) -> Option<JudgmentType> {
        // Track pressed state for visual feedback
        if lane < 7 {
            self.pressed_lanes[lane] = true;
        }

        let render_time = self.clock.render_time() as f64;
        let bpm = self.clock.bpm() as f64;
        
        // Collect judgment data during iteration, add after to avoid borrow conflicts
        let mut judgment_result: Option<(JudgmentType, bool)> = None; // (judgment, is_long_note)
        
        // Try to judge long note first
        for long_note in &mut self.active_long_notes {
            if long_note.lane == lane && !long_note.judged && !long_note.missed {
                let time_diff = (render_time - long_note.head_time_ms).abs();
                let bad_window = 60000.0 / bpm * 0.13021;
                if time_diff <= bad_window {
                    let judgment = judge_tap_note(time_diff, bpm);
                    long_note.judged = true;
                    long_note.holding = true;
                    long_note.head_judgment = Some(judgment);
                    judgment_result = Some((judgment, true));
                    break;
                }
            }
        }
        
        // Try to judge tap note if no long note was judged
        if judgment_result.is_none() {
            for note in &mut self.active_notes {
                if note.lane == lane && !note.judged && !note.missed {
                    let time_diff = (render_time - note.target_time_ms).abs();
                    let bad_window = 60000.0 / bpm * 0.13021;
                    if time_diff <= bad_window {
                        let judgment = judge_tap_note(time_diff, bpm);
                        note.judged = true;
                        note.judgment_type = Some(judgment);
                        judgment_result = Some((judgment, false));
                        break;
                    }
                }
            }
        }
        
        // Apply judgment after iteration (avoid borrow conflicts)
        if let Some((judgment, is_long)) = judgment_result {
            self.stats.record_judgment(judgment, false);
            self.add_pending_judgment(PendingJudgment::new(judgment, lane, render_time));
            
            // Trigger effects only for Cool and Good
            if matches!(judgment, JudgmentType::Cool | JudgmentType::Good) {
                if is_long {
                    self.trigger_longflare_effect(lane, render_time, None);
                } else {
                    self.trigger_note_click_effect(lane, render_time);
                }
            }
            
            return Some(judgment);
        }
        
        None
    }

    /// Handle key release for a lane.
    /// Uses instant-replace behavior: new judgment kills previous one immediately.
    /// Evaluates the release timing against the long note's tail time.
    /// Returns the release judgment type, or None if no long note was released.
    pub fn handle_key_release(&mut self, lane: usize) -> Option<JudgmentType> {
        // Clear pressed state for visual feedback
        if lane < 7 {
            self.pressed_lanes[lane] = false;
        }

        let render_time = self.clock.render_time() as f64;
        let bpm = self.clock.bpm() as f64;

        // Collect release data during iteration, add after to avoid borrow conflicts
        let mut release_result: Option<(JudgmentType, bool)> = None; // (judgment, was_miss)

        for long_note in &mut self.active_long_notes {
            if long_note.lane == lane && long_note.holding {
                long_note.holding = false;

                // If head was Bad or Miss, auto-miss the release
                if let Some(head_judgment) = long_note.head_judgment {
                    if head_judgment.breaks_combo() {
                        long_note.tail_judgment = Some(JudgmentType::Miss);
                        release_result = Some((JudgmentType::Miss, true));
                        return Some(JudgmentType::Miss);
                    }
                }

                // Evaluate release timing against tail time
                let time_diff = (render_time - long_note.tail_time_ms).abs();
                let release_judgment = judge_release(time_diff, bpm);

                long_note.tail_judgment = Some(release_judgment);
                release_result = Some((release_judgment, false));
                break;
            }
        }

        // Apply release judgment after iteration (avoid borrow conflicts)
        if let Some((judgment, was_miss)) = release_result {
            self.stats.record_judgment(judgment, false);
            self.add_pending_judgment(PendingJudgment::new(judgment, lane, render_time));
            return Some(judgment);
        }

        None
    }

    pub fn active_note_count(&self) -> usize {
        self.active_notes.len()
    }

    pub fn active_long_note_count(&self) -> usize {
        self.active_long_notes.len()
    }

    /// Trigger EFFECT_CLICK for a lane when a Cool or Good judgment occurs (tap note).
    ///
    /// Duration is pre-calculated from the sprite's frame count and frame speed during skin loading.
    pub fn trigger_note_click_effect(&mut self, lane: usize, render_time: f64) {
        if self.effect_click_sprite.is_some() {
            let duration = self.effect_click_duration_ms;
            self.note_click_effects.push(NoteClickEffect::new(lane, render_time, duration));
        }
    }

    /// Trigger EFFECT_LONGFLARE for a lane when a Cool or Good judgment occurs (long note head).
    ///
    /// Duration is pre-calculated from the sprite's frame count and frame speed during skin loading.
    /// If custom_duration_ms is provided, it overrides the sprite-based duration (used for autoplay
    /// to match the actual hold duration of the long note).
    pub fn trigger_longflare_effect(&mut self, lane: usize, render_time: f64, custom_duration_ms: Option<f64>) {
        if self.effect_longflare_sprite.is_some() {
            let duration = custom_duration_ms.unwrap_or(self.effect_longflare_duration_ms);
            self.long_flare_effects.push(LongFlareEffect::new(lane, render_time, duration));
        }
    }

    /// Clean up expired effects (no longer active).
    pub fn cleanup_effects(&mut self) {
        let render_time = self.clock.render_time() as f64;
        self.note_click_effects.retain(|e| e.is_active(render_time));
        self.long_flare_effects.retain(|e| e.is_active(render_time));
    }
}