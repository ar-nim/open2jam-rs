//! HUD animation state types: pending judgment popups and combo counter.

use crate::gameplay::judgment::JudgmentType;

#[derive(Debug, Clone)]
pub struct PendingJudgment {
    pub(crate) judgment_type: JudgmentType,
    pub(crate) lane: usize,
    pub(crate) time_created_ms: f64,
    pub(crate) duration_ms: f64,
    pub(crate) pop_in_ms: f64,
}

impl PendingJudgment {
    pub fn new(judgment_type: JudgmentType, lane: usize, time_ms: f64) -> Self {
        Self {
            judgment_type,
            lane,
            time_created_ms: time_ms,
            duration_ms: 750.0,
            pop_in_ms: 100.0,
        }
    }

    pub fn is_active(&self, current_time_ms: f64) -> bool {
        (current_time_ms - self.time_created_ms) < self.duration_ms
    }

    pub fn scale_factor(&self, current_time_ms: f64) -> f64 {
        let elapsed = current_time_ms - self.time_created_ms;
        if elapsed < self.pop_in_ms {
            0.5 + (elapsed / self.pop_in_ms) * 0.5
        } else {
            1.0
        }
    }

    pub fn alpha(&self, current_time_ms: f64) -> f64 {
        let elapsed = current_time_ms - self.time_created_ms;
        let remaining = self.duration_ms - elapsed;
        if remaining < 200.0 {
            (remaining / 200.0).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct ComboCounterState {
    pub(crate) number: u32,
    pub(crate) base_y: f32,
    pub(crate) y_offset: f32,
    pub(crate) show_time_ms: f64,
    pub(crate) visible: bool,
}

impl ComboCounterState {
    pub fn new(base_y: f32) -> Self {
        Self {
            number: 0,
            base_y,
            y_offset: 0.0,
            show_time_ms: 0.0,
            visible: false,
        }
    }

    pub fn increment(&mut self) {
        self.number += 1;
        self.y_offset = 10.0;
        self.show_time_ms = 750.0;
        self.visible = true;
    }

    pub fn reset(&mut self) {
        self.number = 0;
        self.show_time_ms = 0.0;
        self.visible = false;
        self.y_offset = 0.0;
    }

    pub fn update(&mut self, delta_ms: f64) {
        if self.show_time_ms > 0.0 {
            self.show_time_ms -= delta_ms;
            if self.show_time_ms <= 0.0 {
                self.visible = false;
            }
        }

        if self.y_offset > 0.0 {
            self.y_offset -= delta_ms as f32 * 0.5;
            if self.y_offset < 0.0 {
                self.y_offset = 0.0;
            }
        }
    }

    pub fn current_y(&self) -> f32 {
        self.base_y + self.y_offset
    }
}

#[cfg(test)]
mod tests {
    use super::{ComboCounterState, PendingJudgment};
    use crate::gameplay::judgment::JudgmentType;

    // ===================================================================
    // PendingJudgment tests
    // ===================================================================

    #[test]
    fn pending_judgment_default_durations() {
        let j = PendingJudgment::new(JudgmentType::Cool, 3, 1000.0);
        assert_eq!(j.duration_ms, 750.0);
        assert_eq!(j.pop_in_ms, 100.0);
        assert_eq!(j.lane, 3);
        assert_eq!(j.judgment_type, JudgmentType::Cool);
    }

    #[test]
    fn pending_judgment_is_active_within_window() {
        let j = PendingJudgment::new(JudgmentType::Good, 0, 0.0);
        assert!(j.is_active(0.0));
        assert!(j.is_active(300.0));
        assert!(j.is_active(749.0));
    }

    #[test]
    fn pending_judgment_is_inactive_after_window() {
        let j = PendingJudgment::new(JudgmentType::Cool, 0, 0.0);
        assert!(!j.is_active(750.0));
        assert!(!j.is_active(1000.0));
    }

    #[test]
    fn pending_judgment_scale_factor_pop_in() {
        let j = PendingJudgment::new(JudgmentType::Cool, 0, 0.0);
        assert_eq!(j.scale_factor(0.0), 0.5);
        assert_eq!(j.scale_factor(50.0), 0.75);
        assert_eq!(j.scale_factor(100.0), 1.0);
    }

    #[test]
    fn pending_judgment_scale_factor_full_size() {
        let j = PendingJudgment::new(JudgmentType::Cool, 0, 0.0);
        assert_eq!(j.scale_factor(200.0), 1.0);
        assert_eq!(j.scale_factor(500.0), 1.0);
    }

    #[test]
    fn pending_judgment_alpha_full() {
        let j = PendingJudgment::new(JudgmentType::Cool, 0, 0.0);
        assert_eq!(j.alpha(0.0), 1.0);
        assert_eq!(j.alpha(300.0), 1.0);
        assert_eq!(j.alpha(550.0), 1.0);
    }

    #[test]
    fn pending_judgment_alpha_fade_timeline() {
        let j = PendingJudgment::new(JudgmentType::Cool, 0, 0.0);
        // At 0ms remaining (at expiry): alpha = 0
        assert_eq!(j.alpha(750.0), 0.0);
        // At 190ms remaining: alpha = 190/200 = 0.95
        assert!((j.alpha(560.0) - 0.95).abs() < 0.001);
        // At 200ms remaining: alpha = 200/200 = 1.0 (clamped)
        assert_eq!(j.alpha(550.0), 1.0);
    }

    // ===================================================================
    // ComboCounterState tests
    // ===================================================================

    #[test]
    fn combo_counter_increment() {
        let mut cc = ComboCounterState::new(210.0);
        cc.increment();
        assert_eq!(cc.number, 1);
        assert_eq!(cc.base_y, 210.0);
        assert!(cc.visible);
        assert_eq!(cc.y_offset, 10.0);
    }

    #[test]
    fn combo_counter_reset() {
        let mut cc = ComboCounterState::new(210.0);
        cc.increment();
        cc.increment();
        cc.reset();
        assert_eq!(cc.number, 0);
        assert!(!cc.visible);
        assert_eq!(cc.y_offset, 0.0);
        assert_eq!(cc.show_time_ms, 0.0);
    }

    #[test]
    fn combo_counter_update_slides_back() {
        let mut cc = ComboCounterState::new(210.0);
        cc.increment();
        assert_eq!(cc.y_offset, 10.0);
        cc.update(20.0);
        assert_eq!(cc.y_offset, 0.0);
        assert!(cc.visible);
    }

    #[test]
    fn combo_counter_update_hides_when_timer_expires() {
        let mut cc = ComboCounterState::new(210.0);
        cc.increment();
        cc.update(751.0);
        assert!(!cc.visible);
    }

    #[test]
    fn combo_counter_current_y() {
        let mut cc = ComboCounterState::new(210.0);
        cc.increment();
        assert_eq!(cc.current_y(), 220.0);
        cc.update(20.0);
        assert_eq!(cc.current_y(), 210.0);
    }

    #[test]
    fn combo_counter_increment_twice() {
        let mut cc = ComboCounterState::new(210.0);
        cc.increment();
        assert_eq!(cc.number, 1);
        cc.increment();
        assert_eq!(cc.number, 2);
    }
}
