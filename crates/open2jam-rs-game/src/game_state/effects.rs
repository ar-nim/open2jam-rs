//! Visual effect types: note click effects and long note flare effects.

#[derive(Debug, Clone)]
pub struct NoteClickEffect {
    pub(crate) lane: usize,
    pub(crate) time_created_ms: f64,
    pub(crate) duration_ms: f64,
}

impl NoteClickEffect {
    pub fn new(lane: usize, time_ms: f64, duration_ms: f64) -> Self {
        Self {
            lane,
            time_created_ms: time_ms,
            duration_ms,
        }
    }

    pub fn is_active(&self, current_time_ms: f64) -> bool {
        (current_time_ms - self.time_created_ms) < self.duration_ms
    }

    pub fn frame_index(
        &self,
        current_time_ms: f64,
        frame_speed_ms: u32,
        frame_count: usize,
    ) -> usize {
        let elapsed = current_time_ms - self.time_created_ms;
        if frame_speed_ms == 0 || frame_count == 0 {
            return 0;
        }
        let idx = (elapsed / frame_speed_ms as f64) as usize;
        idx % frame_count
    }
}

#[derive(Debug, Clone)]
pub struct LongFlareEffect {
    pub(crate) lane: usize,
    pub(crate) time_created_ms: f64,
    pub(crate) active: bool,
}

impl LongFlareEffect {
    pub fn new(lane: usize, time_ms: f64) -> Self {
        Self {
            lane,
            time_created_ms: time_ms,
            active: true,
        }
    }

    pub fn is_active(&self, _current_time_ms: f64) -> bool {
        self.active
    }

    pub fn frame_index(
        &self,
        current_time_ms: f64,
        frame_speed_ms: u32,
        frame_count: usize,
    ) -> usize {
        let elapsed = current_time_ms - self.time_created_ms;
        if frame_speed_ms == 0 || frame_count == 0 {
            return 0;
        }
        let idx = (elapsed / frame_speed_ms as f64) as usize;
        idx % frame_count
    }
}

#[cfg(test)]
mod tests {
    use super::{LongFlareEffect, NoteClickEffect};

    // ===================================================================
    // NoteClickEffect tests
    // ===================================================================

    #[test]
    fn note_click_effect_is_active_within_duration() {
        let effect = NoteClickEffect::new(3, 0.0, 500.0);
        assert!(effect.is_active(0.0));
        assert!(effect.is_active(250.0));
        assert!(effect.is_active(499.0));
    }

    #[test]
    fn note_click_effect_is_inactive_after_duration() {
        let effect = NoteClickEffect::new(3, 0.0, 500.0);
        assert!(!effect.is_active(500.0));
        assert!(!effect.is_active(600.0));
    }

    #[test]
    fn note_click_effect_frame_index_loops() {
        let effect = NoteClickEffect::new(0, 0.0, 1000.0);
        assert_eq!(effect.frame_index(0.0, 60, 4), 0);
        assert_eq!(effect.frame_index(60.0, 60, 4), 1);
        assert_eq!(effect.frame_index(120.0, 60, 4), 2);
        assert_eq!(effect.frame_index(180.0, 60, 4), 3);
        assert_eq!(effect.frame_index(240.0, 60, 4), 0);
    }

    #[test]
    fn note_click_effect_frame_index_zero_speed_or_count() {
        let effect = NoteClickEffect::new(0, 0.0, 1000.0);
        assert_eq!(effect.frame_index(100.0, 0, 4), 0);
        assert_eq!(effect.frame_index(100.0, 60, 0), 0);
    }

    // ===================================================================
    // LongFlareEffect tests
    // ===================================================================

    #[test]
    fn long_flare_effect_is_active_while_held() {
        let effect = LongFlareEffect::new(5, 100.0);
        assert!(effect.is_active(0.0));
        assert!(effect.is_active(5000.0));
    }

    #[test]
    fn long_flare_effect_is_inactive_when_killed() {
        let mut effect = LongFlareEffect::new(5, 100.0);
        assert!(effect.is_active(0.0));
        effect.active = false;
        assert!(!effect.is_active(0.0));
    }

    #[test]
    fn long_flare_effect_frame_index_loops() {
        let effect = LongFlareEffect::new(0, 0.0);
        assert_eq!(effect.frame_index(0.0, 30, 3), 0);
        assert_eq!(effect.frame_index(30.0, 30, 3), 1);
        assert_eq!(effect.frame_index(60.0, 30, 3), 2);
        assert_eq!(effect.frame_index(90.0, 30, 3), 0);
    }

    #[test]
    fn long_flare_effect_frame_index_zero_params() {
        let effect = LongFlareEffect::new(0, 0.0);
        assert_eq!(effect.frame_index(100.0, 0, 3), 0);
        assert_eq!(effect.frame_index(100.0, 30, 0), 0);
    }
}
