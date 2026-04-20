//! Time-driven audio trigger system.
//!
//! Evaluates scheduled [`AudioTriggerEvent`] instances against the current
//! game time and fires them through the [`SoundCache`] when
//! `game_time >= event.target_time - audio_latency`.

use log::{info, warn};

use super::cache::SoundCache;
use super::manager::AudioPlayError;
use crate::gameplay::clock::Clock;

// ---------------------------------------------------------------------------
// Audio trigger event
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioTriggerEvent {
    pub sample_id: u32,
    pub target_game_time_ms: u64,
    pub volume: f32,
    pub pan: f32,
}

impl AudioTriggerEvent {
    pub fn new(sample_id: u32, target_game_time_ms: u64) -> Self {
        Self {
            sample_id,
            target_game_time_ms,
            volume: 1.0,
            pan: 0.0,
        }
    }

    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    pub fn with_pan(mut self, pan: f32) -> Self {
        self.pan = pan;
        self
    }

    pub fn should_fire(&self, game_time_ms: u64, audio_latency_ms: u64) -> bool {
        game_time_ms >= self.target_game_time_ms.saturating_sub(audio_latency_ms)
    }
}

// ---------------------------------------------------------------------------
// Trigger state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TriggerState {
    Pending,
    Fired,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct AudioTrigger {
    pub event: AudioTriggerEvent,
    state: TriggerState,
}

impl AudioTrigger {
    pub fn new(event: AudioTriggerEvent) -> Self {
        Self {
            event,
            state: TriggerState::Pending,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.state == TriggerState::Pending
    }

    pub fn is_fired(&self) -> bool {
        self.state == TriggerState::Fired
    }

    pub fn is_skipped(&self) -> bool {
        self.state == TriggerState::Skipped
    }
}

// ---------------------------------------------------------------------------
// Audio trigger system
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AudioTriggerSystem {
    triggers: Vec<AudioTrigger>,
    audio_latency_ms: u64,
    fire_count: u64,
    skip_count: u64,
}

impl Default for AudioTriggerSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioTriggerSystem {
    pub fn new() -> Self {
        Self {
            triggers: Vec::new(),
            audio_latency_ms: 50,
            fire_count: 0,
            skip_count: 0,
        }
    }

    pub fn set_audio_latency(&mut self, latency_ms: u64) {
        self.audio_latency_ms = latency_ms;
    }

    pub fn audio_latency(&self) -> u64 {
        self.audio_latency_ms
    }

    pub fn schedule(&mut self, event: AudioTriggerEvent) {
        let trigger = AudioTrigger::new(event);
        self.triggers.push(trigger);
        self.triggers.sort_by_key(|t| t.event.target_game_time_ms);
    }

    pub fn schedule_many(&mut self, events: Vec<AudioTriggerEvent>) {
        for event in events {
            self.triggers.push(AudioTrigger::new(event));
        }
        self.triggers.sort_by_key(|t| t.event.target_game_time_ms);
    }

    pub fn process(
        &mut self,
        clock: &Clock,
        sound_cache: &SoundCache,
        audio_manager: &mut crate::audio::AudioManager,
    ) -> usize {
        let game_time = clock.game_time();
        let latency = self.audio_latency_ms;
        let mut fired_count = 0;

        for trigger in self.triggers.iter_mut() {
            if !trigger.is_pending() {
                continue;
            }

            if game_time >= trigger.event.target_game_time_ms.saturating_sub(latency) {
                if let Some(frames) = sound_cache.get_sound(trigger.event.sample_id) {
                    let x = trigger.event.pan;
                    let position = [x, 0.0, 0.0];

                    match audio_manager.play_frames(frames, trigger.event.volume, position) {
                        Ok(()) => {
                            trigger.state = TriggerState::Fired;
                            fired_count += 1;
                            self.fire_count += 1;
                            info!(
                                "Audio trigger fired: sample_id={} target={}ms game={}ms latency={}ms",
                                trigger.event.sample_id,
                                trigger.event.target_game_time_ms,
                                game_time,
                                latency
                            );
                        }
                        Err(e) => {
                            trigger.state = TriggerState::Skipped;
                            self.skip_count += 1;
                            warn!("Audio trigger skipped: {} — {}", trigger.event.sample_id, e);
                        }
                    }
                } else {
                    trigger.state = TriggerState::Skipped;
                    self.skip_count += 1;
                    warn!(
                        "Audio trigger skipped: sample {} not found",
                        trigger.event.sample_id
                    );
                }
            } else {
                break;
            }
        }

        fired_count
    }

    pub fn fire_count(&self) -> u64 {
        self.fire_count
    }

    pub fn skip_count(&self) -> u64 {
        self.skip_count
    }

    pub fn pending_count(&self) -> usize {
        self.triggers.iter().filter(|t| t.is_pending()).count()
    }

    pub fn triggers(&self) -> &[AudioTrigger] {
        &self.triggers
    }

    pub fn clear(&mut self) {
        self.triggers.clear();
        self.fire_count = 0;
        self.skip_count = 0;
    }

    pub fn was_triggered_within_tolerance(&self, sample_id: u32, _tolerance_ms: u64) -> bool {
        self.triggers
            .iter()
            .any(|t| t.is_fired() && t.event.sample_id == sample_id)
    }

    pub fn get_trigger_drift(&self, sample_id: u32, target_time_ms: u64) -> Option<u64> {
        self.triggers.iter().find_map(|t| {
            if t.event.sample_id == sample_id
                && t.event.target_game_time_ms == target_time_ms
                && t.is_fired()
            {
                Some(0u64)
            } else {
                None
            }
        })
    }
}
