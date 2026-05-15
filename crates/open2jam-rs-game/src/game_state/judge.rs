//! Judgment processing and key input handling.

use super::GameState;
use super::animations::PendingJudgment;
use super::entities::{ActiveLongNote, ActiveNote};
use super::effects::{LongFlareEffect, NoteClickEffect};
use crate::gameplay::judgment::{judge_release, judge_tap_note, JudgmentType, bad_window_ms_tap, bad_window_ms_release};
use crate::audio::bgm_signal::BgmCommand;
use std::sync::Arc;

impl GameState {
    pub fn clear_pending_judgments(&mut self) {
        self.pending_judgments.clear();
    }

    pub fn add_pending_judgment(&mut self, judgment: PendingJudgment) {
        self.clear_pending_judgments();
        self.pending_judgments.push(judgment);
    }

    pub fn detect_missed_notes(&mut self, _render_time: f64, bpm: f64) {
        let current_audio_time = self.clock.game_time() as f64;
        let base_bad_window = bad_window_ms_tap(bpm);
        let mut missed_lanes: Vec<usize> = Vec::new();

        for note in &mut self.active_notes {
            if !note.judged && !note.missed {
                if current_audio_time - note.target_time_ms > base_bad_window {
                    note.missed = true;
                    note.judgment_type = Some(JudgmentType::Miss);
                    self.stats.record_judgment(JudgmentType::Miss, false, self.difficulty);
                    missed_lanes.push(note.lane);
                }
            }
        }

        for long_note in &mut self.active_long_notes {
            if !long_note.judged && !long_note.missed {
                if current_audio_time - long_note.head_time_ms > base_bad_window {
                    long_note.missed = true;
                    long_note.head_judgment = Some(JudgmentType::Miss);
                    long_note.dead = true;
                    self.stats.record_judgment(JudgmentType::Miss, false, self.difficulty);
                    missed_lanes.push(long_note.lane);
                }
            }
        }

        if !missed_lanes.is_empty() {
            self.clear_pending_judgments();
            if let Some(&last_lane) = missed_lanes.last() {
                self.pending_judgments.push(PendingJudgment::new(JudgmentType::Miss, last_lane, current_audio_time));
            }
        }
    }

    pub fn process_judgments(&mut self, _audio_manager: &mut crate::audio::manager::AudioManager) {
        let render_time = self.clock.render_time() as f64;
        let bpm = self.clock.bpm() as f64;
        let mut judgments_to_add: Vec<PendingJudgment> = Vec::new();
        let mut click_effect_lanes: Vec<usize> = Vec::new();

        if self.auto_play {
            let auto_play_tolerance_ms = 10.0;
            let mut auto_longflare_lanes: Vec<usize> = Vec::new();
            for note in &mut self.active_notes {
                if note.judged || note.missed { continue; }
                if (render_time - note.target_time_ms).abs() < auto_play_tolerance_ms {
                    note.judged = true;
                    note.judgment_type = Some(JudgmentType::Cool);
                    self.stats.record_judgment(JudgmentType::Cool, false, self.difficulty);
                    click_effect_lanes.push(note.lane);
                    judgments_to_add.push(PendingJudgment::new(JudgmentType::Cool, note.lane, render_time));
                }
            }
            for long_note in &mut self.active_long_notes {
                if long_note.judged || long_note.missed { continue; }
                if render_time >= long_note.head_time_ms {
                    long_note.judged = true;
                    long_note.head_judgment = Some(JudgmentType::Cool);
                    long_note.holding = true;
                    self.stats.record_judgment(JudgmentType::Cool, false, self.difficulty);
                    auto_longflare_lanes.push(long_note.lane);
                    judgments_to_add.push(PendingJudgment::new(JudgmentType::Cool, long_note.lane, render_time));
                }
            }
            for lane in &auto_longflare_lanes { self.trigger_longflare_effect(*lane, render_time); }
        }

        let mut flare_lanes_to_kill: Vec<usize> = Vec::new();
        for ln in &mut self.active_long_notes {
            if ln.judged && ln.tail_judgment.is_none() {
                let tail_diff = render_time - ln.tail_time_ms;
                let bad_win_rel = bad_window_ms_release(bpm);
                if self.auto_play && tail_diff >= 0.0 {
                    ln.tail_judgment = Some(JudgmentType::Cool); ln.holding = false; ln.dead = true;
                    self.stats.record_judgment(JudgmentType::Cool, false, self.difficulty);
                    flare_lanes_to_kill.push(ln.lane);
                    judgments_to_add.push(PendingJudgment::new(JudgmentType::Cool, ln.lane, render_time));
                } else if tail_diff > bad_win_rel {
                    ln.tail_judgment = Some(JudgmentType::Miss);
                    self.stats.record_judgment(JudgmentType::Miss, false, self.difficulty);
                    flare_lanes_to_kill.push(ln.lane);
                    judgments_to_add.push(PendingJudgment::new(JudgmentType::Miss, ln.lane, render_time));
                    ln.dead = true;
                }
            }
        }
        for lane in &flare_lanes_to_kill { self.kill_longflare(*lane); }
        for lane in click_effect_lanes.drain(..) { self.trigger_note_click_effect(lane, render_time); }
        self.detect_missed_notes(render_time, bpm);

        if !judgments_to_add.is_empty() {
            self.clear_pending_judgments();
            if let Some(last) = judgments_to_add.pop() { self.pending_judgments.push(last); }
        }
        self.pending_judgments.retain(|j| j.is_active(render_time));
    }

    fn play_sample(&self, sample_id: u32, target_time_ms: f64, volume: f32, pan: f32, audio_manager: &mut crate::audio::manager::AudioManager) {
        if let Some(frames) = self.sound_cache.get_sound(sample_id) {
            let source_id = ((sample_id as u64) << 32) | (target_time_ms as u64 & 0xFFFF_FFFF);
            let command = BgmCommand { frames: Arc::clone(frames), delay_samples: 0, volume, pan, source_id };
            if let Err(_) = audio_manager.push_bgm_command(command) {
                log::warn!("[AUDIO] keysound queue full, dropping sample_id={}", sample_id);
            }
        }
    }

    fn trigger_sampler_sound(&self, lane: usize, audio_manager: &mut crate::audio::manager::AudioManager) {
        if let Some(note) = self.active_notes.iter().find(|n| n.lane == lane && !n.judged && !n.missed) {
            if let Some(sample_id) = note.sample_id {
                self.play_sample(sample_id, note.target_time_ms, note.volume, note.pan, audio_manager);
                return;
            }
        }
        if let Some(ln) = self.active_long_notes.iter().find(|ln| ln.lane == lane && !ln.judged && !ln.missed) {
            if let Some(sample_id) = ln.sample_id {
                self.play_sample(sample_id, ln.head_time_ms, ln.volume, ln.pan, audio_manager);
                return;
            }
        }
        if let Some(note_event) = self.chart.events[self.next_event_idx..]
            .iter().filter_map(|e| if let open2jam_rs_parsers::TimedEvent::Note(n) = e { Some(n) } else { None })
            .find(|n| n.channel.lane_index() == Some(lane))
        {
            if let Some(sample_id) = note_event.sample_id {
                self.play_sample(sample_id, note_event.time_ms, note_event.volume, note_event.pan, audio_manager);
            }
        }
    }

    pub fn handle_key_press(&mut self, lane: usize, _os_timestamp: std::time::Instant, audio_manager: &mut crate::audio::manager::AudioManager) -> Option<JudgmentType> {
        if lane >= 7 { return None; }
        self.pressed_lanes[lane] = true;
        let press_time_ms = self.clock.game_time() as f64;
        let bpm = self.clock.bpm() as f64;
        let base_bad_window_ms = bad_window_ms_tap(bpm);
        let mut judgment_result = None;
        let mut sample_to_play = None;

        for note in &mut self.active_notes {
            if note.lane != lane || note.judged || note.missed { continue; }
            let time_diff = press_time_ms - note.target_time_ms;
            if time_diff.abs() <= base_bad_window_ms {
                let judgment = judge_tap_note(time_diff, bpm);
                note.judged = true;
                let has_pill = self.stats.pill_count > 0;
                let effective = self.stats.record_judgment(judgment, has_pill, self.difficulty);
                note.judgment_type = Some(effective);
                sample_to_play = note.sample_id.map(|id| (id, note.target_time_ms, note.volume, note.pan));
                judgment_result = Some(effective);
                break;
            }
            if time_diff < -base_bad_window_ms { break; }
        }

        if judgment_result.is_none() {
            for ln in &mut self.active_long_notes {
                if ln.lane != lane || ln.judged || ln.missed { continue; }
                let time_diff = press_time_ms - ln.head_time_ms;
                if time_diff.abs() <= base_bad_window_ms {
                    let judgment = judge_tap_note(time_diff, bpm);
                    ln.judged = true;
                    let has_pill = self.stats.pill_count > 0;
                    let effective = self.stats.record_judgment(judgment, has_pill, self.difficulty);
                    ln.head_judgment = Some(effective);
                    if effective == JudgmentType::Miss || effective == JudgmentType::Bad {
                        ln.tail_judgment = Some(JudgmentType::Miss);
                        self.stats.record_judgment(JudgmentType::Miss, false, self.difficulty);
                        ln.holding = false; ln.dead = true;
                    } else { ln.holding = true; }
                    sample_to_play = ln.sample_id.map(|id| (id, ln.head_time_ms, ln.volume, ln.pan));
                    judgment_result = Some(effective);
                    break;
                }
                if time_diff < -base_bad_window_ms { break; }
            }
        }

        if let Some(effective) = judgment_result {
            if let Some((id, t, v, p)) = sample_to_play { self.play_sample(id, t, v, p, audio_manager); }
            if matches!(effective, JudgmentType::Cool | JudgmentType::Good) {
                self.trigger_note_click_effect(lane, press_time_ms);
                for ln in &self.active_long_notes {
                    if ln.lane == lane && ln.holding && ln.head_judgment == Some(effective) {
                        self.trigger_longflare_effect(lane, press_time_ms); break;
                    }
                }
            }
            self.clear_pending_judgments();
            self.pending_judgments.push(PendingJudgment::new(effective, lane, press_time_ms));
            return Some(effective);
        }
        self.trigger_sampler_sound(lane, audio_manager);
        None
    }

    pub fn handle_key_release(&mut self, lane: usize, _os_timestamp: std::time::Instant) -> Option<JudgmentType> {
        if lane >= 7 { return None; }
        self.pressed_lanes[lane] = false;
        self.kill_longflare(lane);
        let current_time = self.game_time_ms();
        let bpm = self.clock.bpm() as f64;

        if let Some(ln) = self.active_long_notes.iter_mut().find(|ln| ln.lane == lane && ln.holding) {
            ln.holding = false; ln.dead = true;
            let time_diff = current_time - ln.tail_time_ms;
            let judgment = judge_release(time_diff, bpm);
            let has_pill = self.stats.pill_count > 0;
            let effective = self.stats.record_judgment(judgment, has_pill, self.difficulty);
            ln.tail_judgment = Some(effective);
            self.clear_pending_judgments();
            self.pending_judgments.push(PendingJudgment::new(effective, lane, current_time));
            return Some(effective);
        }
        None
    }

    // ── Effect triggers ────────────────────────────────────────────

    pub fn trigger_note_click_effect(&mut self, lane: usize, render_time: f64) {
        if self.effect_click_sprite.is_some() {
            self.note_click_effects.push(NoteClickEffect::new(lane, render_time, self.effect_click_duration_ms));
        }
    }

    pub fn trigger_longflare_effect(&mut self, lane: usize, render_time: f64) {
        if self.effect_longflare_sprite.is_some() {
            self.long_flare_effects.push(LongFlareEffect::new(lane, render_time));
        }
    }

    pub fn kill_longflare(&mut self, lane: usize) {
        for flare in &mut self.long_flare_effects {
            if flare.lane == lane && flare.active { flare.active = false; }
        }
    }
}
