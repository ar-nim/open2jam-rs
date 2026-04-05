//! Clock resource for time authority in the game engine.
//!
//! Three time sources:
//! - `raw_time()` — wall-clock time since engine start (ms)
//! - `game_time()` — game logic time, starts at 0 on player input
//! - `render_time()` — interpolated time for visual smoothing

pub const DEFAULT_CHART_PADDING_MS: u64 = 1500;

#[derive(Debug, Clone)]
pub struct Clock {
    raw_time_ms: u64,
    game_start_offset_ms: Option<u64>,
    render_interpolation: f32,
    chart_padding_ms: u64,
    current_bpm: f32,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            raw_time_ms: 0,
            game_start_offset_ms: None,
            render_interpolation: 0.0,
            chart_padding_ms: DEFAULT_CHART_PADDING_MS,
            current_bpm: 120.0,
        }
    }
}

impl Clock {
    pub fn new() -> Self {
        Self::default()
    }

    // Raw time
    pub fn raw_time(&self) -> u64 {
        self.raw_time_ms
    }

    pub fn set_raw_time(&mut self, time_ms: u64) {
        self.raw_time_ms = time_ms;
    }

    // Game time
    pub fn game_time(&self) -> u64 {
        self.game_start_offset_ms
            .map(|offset| self.raw_time_ms.saturating_sub(offset))
            .unwrap_or(0)
    }

    pub fn game_time_with_padding(&self) -> u64 {
        let gt = self.game_time();
        gt.saturating_sub(self.chart_padding_ms)
    }

    pub fn start(&mut self) {
        self.game_start_offset_ms = Some(self.raw_time_ms);
    }

    pub fn is_started(&self) -> bool {
        self.game_start_offset_ms.is_some()
    }

    pub fn reset(&mut self) {
        self.game_start_offset_ms = None;
    }

    pub fn set_chart_padding(&mut self, padding_ms: u64) {
        self.chart_padding_ms = padding_ms;
    }

    pub fn chart_padding(&self) -> u64 {
        self.chart_padding_ms
    }

    // Render time
    pub fn render_time(&self) -> f64 {
        let game_time = self.game_time() as f64;
        let interp = self.render_interpolation as f64;
        game_time + interp * 16.67
    }

    pub fn set_render_interpolation(&mut self, factor: f32) {
        self.render_interpolation = factor.clamp(0.0, 1.0);
    }

    // BPM / beat calculations
    pub fn set_bpm(&mut self, bpm: f32) {
        if bpm > 0.0 {
            self.current_bpm = bpm;
        }
    }

    pub fn bpm(&self) -> f32 {
        self.current_bpm
    }

    pub fn beats_to_ms(&self, beats: f64) -> f64 {
        beats * 60_000.0 / self.current_bpm as f64
    }

    pub fn ms_to_beats(&self, ms: f64) -> f64 {
        ms * self.current_bpm as f64 / 60_000.0
    }

    pub fn current_beat(&self) -> f64 {
        self.ms_to_beats(self.game_time() as f64)
    }

    pub fn beat_time_ms(&self, beat: f64) -> f64 {
        self.beats_to_ms(beat)
    }

    // Audio latency compensation
    pub fn audio_latency_ms(&self) -> u64 {
        50
    }

    // Test helpers
    pub fn set_game_time_direct(&mut self, game_time_ms: u64) {
        if game_time_ms == 0 {
            self.reset();
        } else {
            self.game_start_offset_ms =
                Some(self.raw_time_ms.saturating_sub(game_time_ms));
        }
    }

    pub fn advance_game_time(&mut self, delta_ms: u64) {
        if !self.is_started() {
            self.start();
        }
        self.raw_time_ms = self.raw_time_ms.saturating_add(delta_ms);
        self.game_start_offset_ms = self
            .game_start_offset_ms
            .map(|offset| offset.saturating_sub(delta_ms));
    }
}
