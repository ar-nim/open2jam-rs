//! Game state update tick and accessors.

use super::GameState;

impl GameState {
    /// Startup delay duration in milliseconds.
    pub const STARTUP_DELAY_MS: f64 = 2000.0;

    /// Update game state to the given absolute elapsed time.
    pub fn update(&mut self, absolute_ms: u64) {
        let delta_ms = absolute_ms.saturating_sub(self.last_absolute_ms);
        self.last_absolute_ms = absolute_ms;
        let delta = delta_ms as f64;

        self.clock.set_raw_time(absolute_ms);

        if !self.is_rendering {
            if self.startup_delay_ms > 0.0 {
                self.startup_delay_ms -= delta;
                self.startup_life_percent =
                    (1.0 - self.startup_delay_ms / Self::STARTUP_DELAY_MS).min(1.0) as f32;
            }
            if self.startup_delay_ms <= 0.0 {
                self.startup_delay_ms = 0.0;
                self.is_rendering = true;
                self.startup_life_percent = 1.0;
                self.clock.start();
                self.startup_audio_pending = true;
                log::info!("Startup delay complete, gameplay begins now");
                self.analyze_chart_density();
            }
        }

        if self.jam_counter_visible_ms > 0.0 {
            self.jam_counter_visible_ms -= delta;
            if self.jam_counter_visible_ms < 0.0 { self.jam_counter_visible_ms = 0.0; }
        }
        if self.combo_title_visible_ms > 0.0 {
            self.combo_title_visible_ms -= delta;
            if self.combo_title_visible_ms < 0.0 { self.combo_title_visible_ms = 0.0; }
        }

        self.combo_counter.update(delta);

        if !self.is_song_ended && self.end_time_ms > 0.0 {
            let game_time = self.clock.game_time() as f64;
            if game_time >= self.end_time_ms {
                self.is_song_ended = true;
                log::info!("Song ended: game_time={:.1}ms >= end_time={:.1}ms", game_time, self.end_time_ms);
            }
        }
    }

    /// Chart density analysis (debug utility).
    fn analyze_chart_density(&self) {
        log::debug!("[CHART_ANALYSIS] === Chart Note Density Analysis ===");
        log::debug!("[CHART_ANALYSIS] Base BPM: {:.1}, Total events: {}", self.chart.header.bpm, self.chart.events.len());

        let mut lane_notes: [Vec<f64>; 7] = Default::default();
        for event in &self.chart.events {
            if let open2jam_rs_parsers::TimedEvent::Note(note_event) = event {
                if let Some(lane) = note_event.channel.lane_index() {
                    if matches!(note_event.note_type, open2jam_rs_parsers::NoteType::Tap | open2jam_rs_parsers::NoteType::Hold) {
                        lane_notes[lane].push(note_event.time_ms);
                    }
                }
            }
        }

        let base_bpm = self.chart.header.bpm as f64;
        let beat_ms = 60000.0 / base_bpm;
        let sixteenth_ms = beat_ms / 4.0;

        log::debug!("[CHART_ANALYSIS] At base BPM {:.1}: beat={:.2}ms, 1/16 note gap={:.2}ms", base_bpm, beat_ms, sixteenth_ms);
        for lane in 0..7 {
            let notes = &lane_notes[lane];
            log::debug!("[CHART_ANALYSIS] Lane {}: {} notes", lane, notes.len());
            if notes.len() < 2 { continue; }

            let mut min_gap = f64::MAX;
            let mut max_gap = 0.0f64;
            let mut tight_gaps = 0;
            let mut total_gap = 0.0f64;

            for i in 1..notes.len() {
                let gap = notes[i] - notes[i - 1];
                if gap < 0.0 { continue; }
                min_gap = min_gap.min(gap);
                max_gap = max_gap.max(gap);
                total_gap += gap;
                if gap <= sixteenth_ms * 1.5 { tight_gaps += 1; }
            }
            let avg_gap = if notes.len() > 1 { total_gap / (notes.len() - 1) as f64 } else { 0.0 };
            log::debug!("[CHART_ANALYSIS]   Stats: min={:.2}ms, max={:.2}ms, avg={:.2}ms, tight_gaps={}", min_gap, max_gap, avg_gap, tight_gaps);
        }

        let total_tap: usize = lane_notes.iter().map(|v| v.len()).sum();
        log::debug!("[CHART_ANALYSIS] Total playable notes: {}", total_tap);
        log::debug!("[CHART_ANALYSIS] ==========================================");
    }

    // ── Accessors ───────────────────────────────────────────────────

    pub fn is_song_ended(&self) -> bool { self.is_song_ended }
    pub fn song_duration_ms(&self) -> f64 { self.song_duration_ms }
    pub fn game_time_ms(&self) -> f64 { self.clock.game_time() as f64 }

    pub fn life_percent_for_display(&self) -> f32 {
        if !self.is_rendering { self.startup_life_percent }
        else { self.stats.life_percent() }
    }

    pub fn show_jam_counter(&mut self) { self.jam_counter_visible_ms = 750.0; }
    pub fn show_combo_title(&mut self) { self.combo_title_visible_ms = 750.0; }
    pub fn is_jam_counter_visible(&self) -> bool { self.jam_counter_visible_ms > 0.0 }
    pub fn is_combo_title_visible(&self) -> bool { self.combo_title_visible_ms > 0.0 }
    pub fn active_note_count(&self) -> usize { self.active_notes.len() }
    pub fn active_long_note_count(&self) -> usize { self.active_long_notes.len() }
}
