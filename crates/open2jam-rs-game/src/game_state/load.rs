//! Chart loading pipeline: parse OJN, decode audio, build note prefabs.

use std::path::Path;

use anyhow::{Context, Result};
use log::info;

use super::GameState;
use super::stats::GameStats;

use crate::audio::cache::SoundCache;
use crate::audio::trigger::AudioTriggerSystem;
use crate::gameplay::clock::Clock;
use crate::gameplay::modifiers::{Modifiers, PanicPrecomputed};
use crate::gameplay::scroll::scroll_travel_time_ms;
use crate::gameplay::timing_data::TimingData;
use crate::skin::prefab::NotePrefabs;
use crate::game_state::count_playable_notes;

impl GameState {
    /// Load chart, audio, and skin from file paths.
    pub fn load(
        ojn_path: impl AsRef<Path>,
        scroll_speed: f64,
        auto_play: bool,
        difficulty: open2jam_rs_core::Difficulty,
        skin_resources: Option<&open2jam_rs_parsers::xml::Resources>,
        channel_modifier: open2jam_rs_core::game_options::ChannelMod,
    ) -> Result<Self> {
        let ojn_path = ojn_path.as_ref();
        let dir = ojn_path
            .parent()
            .context("OJN file must have a parent directory")?;

        // 1. Parse the OJN chart
        info!("Parsing chart: {}", ojn_path.display());
        let chart = open2jam_rs_parsers::parse_file(ojn_path, difficulty.into())
            .with_context(|| format!("Failed to parse OJN: {}", ojn_path.display()))?;
        info!(
            "Chart loaded: {} - {} ({} events, {} measures)",
            chart.header.title,
            chart.header.artist,
            chart.events.len(),
            chart
                .events
                .iter()
                .filter(|e| matches!(e, open2jam_rs_parsers::TimedEvent::Measure(_)))
                .count()
        );

        // Count playable notes
        let total_playable_notes = count_playable_notes(&chart);

        // 2. Find and parse the OJM audio file
        let ojm_filename = &chart.header.ojm_filename;
        let ojm_path = dir.join(ojm_filename);
        info!("Loading audio: {}", ojm_path.display());
        let sample_map = open2jam_rs_parsers::ojm::parse_file(&ojm_path)
            .with_context(|| format!("Failed to parse OJM: {}", ojm_path.display()))?;
        info!("OJM loaded: {} samples", sample_map.len());

        // 3. Decode audio samples into the sound cache
        let mut sound_cache = SoundCache::new();
        sound_cache.populate_from_sample_map(sample_map, &ojm_path.to_string_lossy());
        info!("Sound cache: {} decoded samples", sound_cache.len());

        // 4. Build note prefabs from skin XML if available, otherwise use defaults
        let (note_prefabs, click_sprite, click_duration, longflare_sprite, longflare_duration, longflare_y) =
            if let Some(skin_res) = skin_resources {
                if let Some(skin) = skin_res.get_skin("o2jam") {
                    info!("Building note prefabs from skin XML (o2jam)");
                    let prefabs = NotePrefabs::from_skin(skin);
                    let click_entity = skin.entities.iter().find(|e| e.id.as_deref() == Some("EFFECT_CLICK"));
                    let click_sprite = click_entity.and_then(|e| e.sprite.clone());
                    let click_duration = click_sprite
                        .as_ref()
                        .and_then(|sprite_id| skin_res.sprites.get(sprite_id))
                        .map(|s| s.frames.len() as f64 * s.frame_speed_ms as f64)
                        .unwrap_or(660.0);
                    let longflare_entity = skin.entities.iter().find(|e| e.id.as_deref() == Some("EFFECT_LONGFLARE"));
                    let longflare_sprite = longflare_entity.and_then(|e| e.sprite.clone());
                    let longflare_y = longflare_entity.map(|e| e.y).unwrap_or(460);
                    let longflare_duration = longflare_sprite
                        .as_ref()
                        .and_then(|sprite_id| skin_res.sprites.get(sprite_id))
                        .map(|s| s.frames.len() as f64 * s.frame_speed_ms as f64)
                        .unwrap_or(450.0);
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

        // 5. Calculate spawn lead time
        let base_bpm = chart.header.bpm as f64;
        let measure_basis = note_prefabs.judgment_line_y as f64;
        let travel_time = scroll_travel_time_ms(base_bpm, measure_basis, scroll_speed);
        let spawn_lead_time_ms = (travel_time * 2.0) + 500.0;

        // 6. Initialize empty audio trigger system
        let audio_triggers = AudioTriggerSystem::new();
        let next_bgm_event_idx = 0;
        let bgm_lookahead_ms = 500.0;

        // 7. Build clock
        let mut clock = Clock::new();
        clock.set_bpm(chart.header.bpm);
        clock.set_chart_padding(0);

        // 8. Build the velocity tree
        let mut timing = TimingData::new();
        timing.add(0.0, chart.header.bpm as f64);
        for event in &chart.events {
            if let open2jam_rs_parsers::TimedEvent::BpmChange(bpm_event) = event {
                timing.add(bpm_event.time_ms, bpm_event.bpm);
            }
        }
        timing.finish();
        info!("TimingData: {} BPM change points (base BPM={:.1})", timing.len(), chart.header.bpm);

        // 9. Initialize game stats
        let max_life = 1000;
        let stats = GameStats::new(total_playable_notes, max_life);

        // 10. Compute song end time
        let end_time_ms = compute_end_time(&chart);

        let song_duration_ms = chart.header.duration_hard as f64 * 1000.0;
        let chart_song_id = chart.header.song_id;
        let panic_precomputed = if channel_modifier == open2jam_rs_core::game_options::ChannelMod::Panic {
            Some(PanicPrecomputed::build(&chart))
        } else {
            None
        };

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
            difficulty,
            pressed_lanes: [false; 7],
            spawn_lead_time_ms,
            stats,
            pending_judgments: Vec::new(),
            combo_counter: crate::game_state::animations::ComboCounterState::new(210.0),
            jam_counter_visible_ms: 0.0,
            combo_title_visible_ms: 0.0,
            startup_delay_ms: 2000.0,
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
            prev_frame_combo: 0,
            modifiers: Modifiers::new(
                channel_modifier,
                (chart_song_id as u64) ^ (difficulty as u64) ^ 0x2F9E4A3B,
                panic_precomputed,
            ),
        })
    }
}

/// Compute the chart's end time using the original O2Jam position formula.
fn compute_end_time(chart: &open2jam_rs_parsers::Chart) -> f64 {
    use open2jam_rs_parsers::TimedEvent;
    const TICK_SIGNATURE: f64 = 240000.0;

    let max_pos = chart
        .events
        .iter()
        .filter_map(|e| match e {
            TimedEvent::Note(n) => Some(n.measure as f64 + n.position + 1.0),
            TimedEvent::BpmChange(b) => Some(b.measure as f64 + b.position + 1.0),
            TimedEvent::Measure(m) => Some(m.measure as f64 + 1.0),
        })
        .fold(0.0, f64::max);

    let end_position = max_pos.ceil() + 1.0;

    let mut ref_position = 1.0;
    let mut ref_time_ms = 0.0;
    let mut current_bpm = chart.header.bpm as f64;

    let mut bpm_events: Vec<(f64, f64)> = chart
        .events
        .iter()
        .filter_map(|e| match e {
            TimedEvent::BpmChange(b) => Some((b.measure as f64 + b.position + 1.0, b.bpm)),
            _ => None,
        })
        .collect();
    bpm_events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    for (bpm_pos, new_bpm) in &bpm_events {
        if *bpm_pos > ref_position {
            ref_time_ms += (*bpm_pos - ref_position) / current_bpm * TICK_SIGNATURE;
        }
        ref_position = *bpm_pos;
        current_bpm = *new_bpm;
    }
    if end_position > ref_position {
        ref_time_ms += (end_position - ref_position) / current_bpm * TICK_SIGNATURE;
    }
    ref_time_ms
}
