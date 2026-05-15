//! Note spawning: populates active notes from chart events.

use super::GameState;
use super::entities::{ActiveLongNote, ActiveNote};
use open2jam_rs_parsers::{NoteType, TimedEvent};

impl GameState {
    /// Spawn notes that are within the spawn window.
    pub fn spawn_notes(&mut self) {
        let render_time = self.clock.render_time();
        let spawn_until = render_time as f64 + self.spawn_lead_time_ms;

        let mut spawned_count = 0;
        while self.next_event_idx < self.chart.events.len() {
            let event = &self.chart.events[self.next_event_idx];
            let target_time = match event {
                TimedEvent::Note(n) => n.time_ms,
                _ => {
                    self.next_event_idx += 1;
                    continue;
                }
            };

            if target_time > spawn_until {
                break;
            }

            if let TimedEvent::Note(note_event) = event {
                if let Some(lane) = note_event.channel.lane_index() {
                    let occupied_lanes: Vec<usize> = self
                        .active_long_notes
                        .iter()
                        .filter(|ln| ln.tail_time_ms > note_event.time_ms)
                        .map(|ln| ln.lane)
                        .collect();

                    match note_event.note_type {
                        NoteType::Tap => {
                            let transformed_lane = self.modifiers.transform_lane(
                                lane, note_event.measure, note_event.note_type, false, &occupied_lanes,
                            );
                            self.active_notes.push(ActiveNote {
                                lane: transformed_lane,
                                target_time_ms: note_event.time_ms,
                                sample_id: note_event.sample_id,
                                volume: note_event.volume,
                                pan: note_event.pan,
                                judged: false,
                                missed: false,
                                judgment_type: None,
                            });
                            spawned_count += 1;
                        }
                        NoteType::Hold => {
                            let end_time = note_event.end_time_ms.unwrap_or(note_event.time_ms + 500.0);
                            let transformed_lane = self.modifiers.transform_lane(
                                lane, note_event.measure, note_event.note_type, false, &occupied_lanes,
                            );
                            self.active_long_notes.push(ActiveLongNote {
                                lane: transformed_lane,
                                head_time_ms: note_event.time_ms,
                                tail_time_ms: end_time,
                                sample_id: note_event.sample_id,
                                volume: note_event.volume,
                                pan: note_event.pan,
                                judged: false,
                                missed: false,
                                holding: false,
                                dead: false,
                                head_judgment: None,
                                tail_judgment: None,
                            });
                            spawned_count += 1;
                        }
                        NoteType::Release => {
                            let transformed_lane = self.modifiers.transform_lane(
                                lane, note_event.measure, note_event.note_type, true, &occupied_lanes,
                            );
                            self.modifiers.clear_release(lane);
                        }
                    }
                }
            }
            self.next_event_idx += 1;
        }

        if spawned_count > 0 {
            log::debug!(
                "[SPAWN]   +{} notes this frame (total: {} active, {} long)",
                spawned_count,
                self.active_notes.len(),
                self.active_long_notes.len()
            );
        }
    }
}
