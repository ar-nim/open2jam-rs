//! Audio processing: schedules BGM keysounds via lookahead.

use super::GameState;
use crate::audio::manager::AudioManager;
use crate::audio::bgm_signal::BgmCommand;
use open2jam_rs_parsers::{Channel, NoteType, TimedEvent};
use std::sync::Arc;

impl GameState {
    /// Process audio triggers for the current game time.
    pub fn process_audio(&mut self, audio_manager: &mut AudioManager) -> usize {
        if !audio_manager.is_active() { return 0; }

        let game_time = self.clock.game_time() as f64;
        let lookahead_end = game_time + self.bgm_lookahead_ms;
        let mut scheduled_count = 0;

        while self.next_bgm_event_idx < self.chart.events.len() {
            let event = &self.chart.events[self.next_bgm_event_idx];
            match event {
                TimedEvent::Note(note_event) => {
                    if note_event.note_type == NoteType::Release {
                        self.next_bgm_event_idx += 1; continue;
                    }
                    if note_event.time_ms < game_time {
                        self.next_bgm_event_idx += 1; continue;
                    }
                    if note_event.time_ms > lookahead_end { break; }
                    if !self.auto_play {
                        if let Channel::Note(_) = note_event.channel {
                            self.next_bgm_event_idx += 1; continue;
                        }
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
                                source_id: ((note_event.time_ms as u64) << 16) | (sample_id as u64),
                            };
                            if let Err(_err) = audio_manager.push_bgm_command(command) {
                                log::warn!("BGM queue full, dropping note: sample_id={}", sample_id);
                            } else { scheduled_count += 1; }
                        }
                    }
                    self.next_bgm_event_idx += 1;
                }
                _ => { self.next_bgm_event_idx += 1; }
            }
        }
        scheduled_count
    }
}
