//! Game state machine: integrates clock, chart, audio, and note lifecycle.

use std::path::Path;

use anyhow::{Context, Result};
use log::info;

use crate::audio::manager::AudioManager;
use crate::audio::trigger::{AudioTriggerEvent, AudioTriggerSystem};
use crate::audio::cache::SoundCache;
use crate::gameplay::scroll::scroll_travel_time_ms;
use crate::parsing::ojn::{Chart, TimedEvent};
use crate::resources::clock::Clock;
use crate::skin::prefab::NotePrefabs;

/// A note entity in the active game.
#[derive(Debug, Clone)]
pub struct ActiveNote {
    pub lane: usize,
    pub target_time_ms: f64,
    pub sample_id: Option<u32>,
    pub judged: bool,
    pub missed: bool,
}

/// The main game state.
pub struct GameState {
    pub clock: Clock,
    pub audio_triggers: AudioTriggerSystem,
    pub sound_cache: SoundCache,
    pub chart: Chart,
    pub note_prefabs: NotePrefabs,
    /// Active notes on screen (not yet judged or killed)
    pub active_notes: Vec<ActiveNote>,
    /// Iterator index into chart events
    pub next_event_idx: usize,
    /// Scroll speed multiplier
    pub scroll_speed: f64,
    /// Whether we're in auto-play mode
    pub auto_play: bool,
    /// Spawn lead time in milliseconds
    pub spawn_lead_time_ms: f64,
}

impl GameState {
    /// Load chart, audio, and skin from file paths.
    pub fn load(
        ojn_path: impl AsRef<Path>,
        scroll_speed: f64,
        auto_play: bool,
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

        // 4. Build note prefabs from defaults (no skin XML yet)
        let note_prefabs = NotePrefabs::default_7lan(1000, 750, 600);

        // 5. Calculate spawn lead time based on BPM and viewport
        let base_bpm = chart.header.bpm as f64;
        let viewport_height = 750.0; // default window height
        let travel_time = scroll_travel_time_ms(base_bpm, viewport_height, scroll_speed);
        let spawn_lead_time_ms = travel_time + 500.0; // extra margin

        // 6. Schedule audio triggers for BGM events (auto-play mode)
        let mut audio_triggers = AudioTriggerSystem::new();
        if auto_play {
            for event in &chart.events {
                if let TimedEvent::Note(note_event) = event {
                    // Include ALL notes with sample_id (including AUTO_PLAY channels)
                    if let Some(sample_id) = note_event.sample_id {
                        audio_triggers.schedule(AudioTriggerEvent::new(
                            sample_id,
                            note_event.time_ms as u64,
                        ).with_volume(note_event.volume).with_pan(note_event.pan));
                    }
                }
            }
            info!("Scheduled {} audio triggers (auto-play mode)", audio_triggers.pending_count());
        }

        let mut clock = Clock::new();
        clock.set_bpm(chart.header.bpm);
        clock.set_chart_padding(1500);

        Ok(Self {
            clock,
            audio_triggers,
            sound_cache,
            chart,
            note_prefabs,
            active_notes: Vec::new(),
            next_event_idx: 0,
            scroll_speed,
            auto_play,
            spawn_lead_time_ms,
        })
    }

    /// Advance the game clock and process events.
    pub fn update(&mut self, delta_ms: u64) {
        self.clock.advance_game_time(delta_ms);
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

            // If this event is past the spawn window, stop spawning
            if target_time > render_time + self.spawn_lead_time_ms {
                break;
            }

            // Spawn this note
            if let TimedEvent::Note(note_event) = event {
                if let Some(lane) = note_event.channel.lane_index() {
                    self.active_notes.push(ActiveNote {
                        lane,
                        target_time_ms: note_event.time_ms,
                        sample_id: note_event.sample_id,
                        judged: false,
                        missed: false,
                    });
                }
            }

            self.next_event_idx += 1;
        }
    }

    /// Remove notes that have passed the judgment line.
    pub fn cleanup_notes(&mut self) {
        let render_time = self.clock.render_time();
        let kill_tolerance = 500.0; // ms after target before removing

        self.active_notes.retain(|note| {
            let time_since_target = render_time - note.target_time_ms;
            time_since_target < kill_tolerance
        });
    }

    /// Process audio triggers for the current game time.
    pub fn process_audio(&mut self, audio_manager: &mut AudioManager) -> usize {
        self.audio_triggers.process(
            &self.clock,
            &self.sound_cache,
            audio_manager,
        )
    }

    /// Get the number of active notes for debugging.
    pub fn active_note_count(&self) -> usize {
        self.active_notes.len()
    }
}
