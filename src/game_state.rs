//! Game state machine: integrates clock, chart, audio, and note lifecycle.

use std::path::Path;

use anyhow::{Context, Result};
use log::info;

use crate::audio::manager::AudioManager;
use crate::audio::trigger::{AudioTriggerEvent, AudioTriggerSystem};
use crate::audio::cache::SoundCache;
use crate::gameplay::scroll::scroll_travel_time_ms;
use crate::parsing::ojn::{Chart, NoteType, TimedEvent};
use crate::resources::clock::Clock;
use crate::skin::prefab::NotePrefabs;
use crate::parsing::xml::Resources as SkinResources;

/// A note entity in the active game.
#[derive(Debug, Clone)]
pub struct ActiveNote {
    pub lane: usize,
    pub target_time_ms: f64,
    pub sample_id: Option<u32>,
    pub judged: bool,
    pub missed: bool,
}

/// A long note entity in the active game.
#[derive(Debug, Clone)]
pub struct ActiveLongNote {
    pub lane: usize,
    pub head_time_ms: f64,      // When the head reaches judgment line
    pub tail_time_ms: f64,      // When the tail reaches judgment line (end_time)
    pub sample_id: Option<u32>,
    pub judged: bool,           // Head has been judged
    pub missed: bool,
    pub holding: bool,          // Player is currently holding the key
    pub dead: bool,             // Marked for removal
}

/// The main game state.
pub struct GameState {
    pub clock: Clock,
    pub audio_triggers: AudioTriggerSystem,
    pub sound_cache: SoundCache,
    pub chart: Chart,
    pub note_prefabs: NotePrefabs,
    /// Active tap notes on screen (not yet judged or killed)
    pub active_notes: Vec<ActiveNote>,
    /// Active long notes on screen
    pub active_long_notes: Vec<ActiveLongNote>,
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
        let note_prefabs = if let Some(skin_res) = skin_resources {
            if let Some(skin) = skin_res.get_skin("o2jam") {
                info!("Building note prefabs from skin XML (o2jam)");
                NotePrefabs::from_skin(skin)
            } else {
                info!("Skin 'o2jam' not found, using default 7-lane layout");
                NotePrefabs::default_7lan(1000, 750, 600)
            }
        } else {
            info!("No skin resources provided, using default 7-lane layout");
            NotePrefabs::default_7lan(1000, 750, 600)
        };

        // 5. Calculate spawn lead time based on BPM and viewport
        let base_bpm = chart.header.bpm as f64;
        let viewport_height = note_prefabs.skin_height as f64;
        let travel_time = scroll_travel_time_ms(base_bpm, viewport_height, scroll_speed);
        let spawn_lead_time_ms = travel_time + 500.0; // extra margin

        // 6. Schedule audio triggers for BGM events (auto-play mode)
        let mut audio_triggers = AudioTriggerSystem::new();
        if auto_play {
            for event in &chart.events {
                if let TimedEvent::Note(note_event) = event {
                    // Skip Release events — the sample is triggered at the HEAD only,
                    // not at the TAIL. The long note's sound continues until release.
                    if note_event.note_type == NoteType::Release {
                        continue;
                    }
                    // Include ALL notes with sample_id (including AUTO_PLAY channels)
                    if let Some(sample_id) = note_event.sample_id {
                        audio_triggers.schedule(AudioTriggerEvent::new(
                            sample_id,
                            note_event.time_ms.round() as u64,  // round to prevent drift
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
            active_long_notes: Vec::new(),
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
                    match note_event.note_type {
                        NoteType::Tap => {
                            log::debug!("[SPAWN] Tap note at {}ms, lane {}, spawn_lead={}, render_time={}", 
                                note_event.time_ms, lane, self.spawn_lead_time_ms, render_time);
                            self.active_notes.push(ActiveNote {
                                lane,
                                target_time_ms: note_event.time_ms,
                                sample_id: note_event.sample_id,
                                judged: false,
                                missed: false,
                            });
                        }
                        NoteType::Hold => {
                            // Long note HEAD - spawn it
                            let end_time = note_event.end_time_ms.unwrap_or(note_event.time_ms + 500.0);
                            log::debug!("[SPAWN] Long note HEAD at {}ms, lane {}, tail at {}ms, render_time={}", 
                                note_event.time_ms, lane, end_time, render_time);
                            self.active_long_notes.push(ActiveLongNote {
                                lane,
                                head_time_ms: note_event.time_ms,
                                tail_time_ms: end_time,
                                sample_id: note_event.sample_id,
                                judged: false,
                                missed: false,
                                holding: false,
                                dead: false,
                            });
                        }
                        NoteType::Release => {
                            // Long note TAIL - skip it (already paired with HEAD during parsing)
                        }
                    }
                }
            }

            self.next_event_idx += 1;
        }
    }

    /// Remove notes that have passed the judgment line.
    ///
    /// Notes are killed as soon as they pass the judgment line.
    /// This prevents rendering off-screen notes and frees memory.
    pub fn cleanup_notes(&mut self) {
        let render_time = self.clock.render_time();

        // Clean up tap notes - remove immediately once they pass the judgment line
        self.active_notes.retain(|note| {
            note.target_time_ms >= render_time
        });
        
        // Clean up long notes - remove immediately when tail passes the judgment line
        self.active_long_notes.retain(|long_note| {
            render_time <= long_note.tail_time_ms
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

    /// Get the number of active long notes for debugging.
    pub fn active_long_note_count(&self) -> usize {
        self.active_long_notes.len()
    }

    /// Handle key press for a lane - judges the nearest note/long note head.
    ///
    /// Returns true if a note was successfully judged.
    pub fn handle_key_press(&mut self, lane: usize, judgment_window_ms: f64) -> bool {
        let render_time = self.clock.render_time();
        
        // Try to judge long note first
        for long_note in &mut self.active_long_notes {
            if long_note.lane == lane && !long_note.judged && !long_note.missed {
                let time_diff = (render_time - long_note.head_time_ms).abs();
                if time_diff <= judgment_window_ms {
                    long_note.judged = true;
                    long_note.holding = true;
                    return true;
                }
            }
        }
        
        // Try to judge tap note
        for note in &mut self.active_notes {
            if note.lane == lane && !note.judged && !note.missed {
                let time_diff = (render_time - note.target_time_ms).abs();
                if time_diff <= judgment_window_ms {
                    note.judged = true;
                    return true;
                }
            }
        }
        
        false
    }

    /// Handle key release for a lane - ends the long note hold.
    ///
    /// Returns true if a long note was released.
    pub fn handle_key_release(&mut self, lane: usize) -> bool {
        for long_note in &mut self.active_long_notes {
            if long_note.lane == lane && long_note.holding {
                long_note.holding = false;
                return true;
            }
        }
        false
    }
}
