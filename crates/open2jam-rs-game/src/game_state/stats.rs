//! Game statistics: scoring, combo, life, judgment counts.

use crate::gameplay::judgment::{
    cool_score_with_jam_bonus, good_score_with_jam_bonus, JudgmentType,
};

#[derive(Debug, Clone)]
pub struct GameStats {
    pub(crate) score: u32,
    pub(crate) combo: u32,
    pub(crate) max_combo: u32,
    pub(crate) jam_counter: u32,
    pub(crate) jam_combo: u32,
    pub(crate) max_jam_combo: u32,
    pub(crate) cool_count: u32,
    pub(crate) good_count: u32,
    pub(crate) bad_count: u32,
    pub(crate) miss_count: u32,
    pub(crate) life: i32,
    pub(crate) max_life: i32,
    pub(crate) pill_count: u32,
    pub(crate) total_notes: u32,
    pub(crate) consecutive_cools: u32,
}

impl GameStats {
    pub fn new(total_notes: u32, max_life: i32) -> Self {
        Self {
            score: 0,
            combo: 0,
            max_combo: 0,
            jam_counter: 0,
            jam_combo: 0,
            max_jam_combo: 0,
            cool_count: 0,
            good_count: 0,
            bad_count: 0,
            miss_count: 0,
            life: max_life,
            max_life,
            pill_count: 0,
            total_notes,
            consecutive_cools: 0,
        }
    }

    pub fn record_judgment(
        &mut self,
        judgment: JudgmentType,
        has_pill: bool,
        difficulty: open2jam_rs_core::Difficulty,
    ) -> JudgmentType {
        let use_pill = has_pill && judgment == JudgmentType::Bad && self.pill_count > 0;
        let effective_judgment = if use_pill {
            self.pill_count -= 1;
            JudgmentType::Cool
        } else {
            judgment
        };

        let note_score: i32 = match effective_judgment {
            JudgmentType::Cool => cool_score_with_jam_bonus(self.jam_combo) as i32,
            JudgmentType::Good => good_score_with_jam_bonus(self.jam_combo) as i32,
            JudgmentType::Bad => 4,
            JudgmentType::Miss => -10,
        };
        if note_score >= 0 {
            self.score += note_score as u32;
        } else {
            let penalty = (-note_score) as u32;
            self.score = self.score.saturating_sub(penalty);
        }

        let is_first_hit = self.combo == 0 && self.cool_count == 0 && self.good_count == 0;
        match effective_judgment {
            JudgmentType::Cool => {
                self.cool_count += 1;
                if !is_first_hit {
                    self.combo += 1;
                    self.consecutive_cools += 1;
                }
                self.jam_counter += 4;
            }
            JudgmentType::Good => {
                self.good_count += 1;
                if !is_first_hit {
                    self.combo += 1;
                }
                self.consecutive_cools = 0;
                self.jam_counter += 2;
            }
            JudgmentType::Bad => {
                self.bad_count += 1;
                self.combo = 0;
                self.jam_counter = 0;
                self.jam_combo = 0;
            }
            JudgmentType::Miss => {
                self.miss_count += 1;
                self.combo = 0;
                self.jam_counter = 0;
                self.jam_combo = 0;
                self.consecutive_cools = 0;
            }
        }

        while self.jam_counter >= 100 {
            self.jam_counter -= 100;
            self.jam_combo += 1;
            if self.jam_combo > self.max_jam_combo {
                self.max_jam_combo = self.jam_combo;
            }
        }

        if self.combo > self.max_combo {
            self.max_combo = self.combo;
        }

        if self.consecutive_cools > 0 && self.consecutive_cools % 15 == 0 {
            let expected_buffers = (self.consecutive_cools / 15).min(5);
            if expected_buffers > self.pill_count {
                self.pill_count = expected_buffers;
            }
        }

        self.life += match difficulty {
            open2jam_rs_core::Difficulty::Easy => effective_judgment.hp_change_easy(),
            open2jam_rs_core::Difficulty::Normal => effective_judgment.hp_change_normal(),
            open2jam_rs_core::Difficulty::Hard => effective_judgment.hp_change_hard(),
        };
        self.life = self.life.clamp(0, self.max_life);

        effective_judgment
    }

    pub fn is_game_over(&self) -> bool {
        self.life <= 0
    }

    pub fn life_percent(&self) -> f32 {
        if self.max_life <= 0 {
            0.0
        } else {
            (self.life as f32 / self.max_life as f32).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GameStats;
    use crate::gameplay::judgment::JudgmentType;

    // ===================================================================
    // GameStats::new
    // ===================================================================

    #[test]
    fn game_stats_initial_state() {
        let stats = GameStats::new(100, 1000);
        assert_eq!(stats.score, 0);
        assert_eq!(stats.combo, 0);
        assert_eq!(stats.max_combo, 0);
        assert_eq!(stats.life, 1000);
        assert_eq!(stats.max_life, 1000);
        assert_eq!(stats.total_notes, 100);
        assert_eq!(stats.pill_count, 0);
        assert_eq!(stats.jam_combo, 0);
    }

    #[test]
    fn game_stats_is_not_game_over_at_full_life() {
        let stats = GameStats::new(100, 1000);
        assert!(!stats.is_game_over());
    }

    #[test]
    fn game_stats_is_game_over_at_zero_life() {
        let mut stats = GameStats::new(100, 1000);
        stats.life = 0;
        assert!(stats.is_game_over());
    }

    #[test]
    fn game_stats_life_percent_full() {
        let stats = GameStats::new(100, 1000);
        assert_eq!(stats.life_percent(), 1.0);
    }

    #[test]
    fn game_stats_life_percent_half() {
        let mut stats = GameStats::new(100, 1000);
        stats.life = 500;
        assert_eq!(stats.life_percent(), 0.5);
    }

    #[test]
    fn game_stats_life_percent_clamped_at_zero() {
        let mut stats = GameStats::new(100, 1000);
        stats.life = -100;
        assert_eq!(stats.life_percent(), 0.0);
    }

    // ===================================================================
    // Judgment recording — Cool
    // ===================================================================

    #[test]
    fn record_cool_increments_cool_count() {
        let mut stats = GameStats::new(100, 1000);
        let result = stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(result, JudgmentType::Cool);
        assert_eq!(stats.cool_count, 1);
        assert_eq!(stats.good_count, 0);
        assert_eq!(stats.bad_count, 0);
        assert_eq!(stats.miss_count, 0);
    }

    #[test]
    fn record_cool_adds_to_score() {
        let mut stats = GameStats::new(100, 1000);
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert!(stats.score > 0);
    }

    #[test]
    fn record_cool_increments_combo() {
        let mut stats = GameStats::new(100, 1000);
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.combo, 0); // first hit: is_first_hit guard
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.combo, 1);
    }

    #[test]
    fn record_cool_increments_jam_counter() {
        let mut stats = GameStats::new(100, 1000);
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.jam_counter, 4);
    }

    // ===================================================================
    // Judgment recording — Good
    // ===================================================================

    #[test]
    fn record_good_increments_good_count() {
        let mut stats = GameStats::new(100, 1000);
        stats.record_judgment(
            JudgmentType::Good,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.good_count, 1);
    }

    #[test]
    fn record_good_resets_consecutive_cools() {
        let mut stats = GameStats::new(100, 1000);
        stats.consecutive_cools = 10;
        stats.record_judgment(
            JudgmentType::Good,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.consecutive_cools, 0);
    }

    #[test]
    fn record_good_adds_jam_counter_2() {
        let mut stats = GameStats::new(100, 1000);
        stats.record_judgment(
            JudgmentType::Good,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.jam_counter, 2);
    }

    // ===================================================================
    // Judgment recording — Bad
    // ===================================================================

    #[test]
    fn record_bad_resets_combo() {
        let mut stats = GameStats::new(100, 1000);
        stats.combo = 10;
        stats.record_judgment(
            JudgmentType::Bad,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.combo, 0);
    }

    #[test]
    fn record_bad_resets_jam_counter_and_combo_multiplier() {
        let mut stats = GameStats::new(100, 1000);
        stats.jam_counter = 50;
        stats.jam_combo = 2;
        stats.record_judgment(
            JudgmentType::Bad,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.jam_counter, 0);
        assert_eq!(stats.jam_combo, 0);
    }

    // ===================================================================
    // Judgment recording — Miss
    // ===================================================================

    #[test]
    fn record_miss_increments_miss_count() {
        let mut stats = GameStats::new(100, 1000);
        stats.record_judgment(
            JudgmentType::Miss,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.miss_count, 1);
    }

    #[test]
    fn record_miss_resets_combo() {
        let mut stats = GameStats::new(100, 1000);
        stats.combo = 20;
        stats.record_judgment(
            JudgmentType::Miss,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.combo, 0);
    }

    #[test]
    fn record_miss_resets_consecutive_cools() {
        let mut stats = GameStats::new(100, 1000);
        stats.consecutive_cools = 14;
        stats.record_judgment(
            JudgmentType::Miss,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.consecutive_cools, 0);
    }

    // ===================================================================
    // Pill conversion: BAD -> COOL when pill available
    // ===================================================================

    #[test]
    fn record_bad_with_pill_converts_to_cool() {
        let mut stats = GameStats::new(100, 1000);
        stats.pill_count = 1;
        let result = stats.record_judgment(
            JudgmentType::Bad,
            true,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(result, JudgmentType::Cool);
        assert_eq!(stats.pill_count, 0);
        assert_eq!(stats.cool_count, 1);
    }

    #[test]
    fn record_bad_without_pill_stays_bad() {
        let mut stats = GameStats::new(100, 1000);
        stats.pill_count = 0;
        let result = stats.record_judgment(
            JudgmentType::Bad,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(result, JudgmentType::Bad);
        assert_eq!(stats.pill_count, 0);
        assert_eq!(stats.bad_count, 1);
    }

    // ===================================================================
    // Pill awards: 1 per 15 consecutive Cool hits, max 5
    // ===================================================================

    #[test]
    fn pills_awarded_at_15_consecutive_cools() {
        let mut stats = GameStats::new(100, 1000);
        // First judgment: is_first_hit guard prevents consecutive_cools increment.
        // After 15 total judgments: consecutive_cools = 14, then 15 (awards pill).
        for _ in 0..15 {
            stats.record_judgment(
                JudgmentType::Cool,
                false,
                open2jam_rs_core::Difficulty::Normal,
            );
        }
        assert_eq!(stats.pill_count, 0); // 14 consecutive, not yet 15
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.pill_count, 1); // 15 consecutive at this point
    }

    #[test]
    fn pills_max_5() {
        let mut stats = GameStats::new(100, 1000);
        // After N judgments: consecutive_cools = N - 1.
        // Pill awarded when consecutive_cools % 15 == 0 (at 15, 30, 45, 60, 75).
        // After 75 judgments: consecutive_cools = 74 (fourth pill at 60, fifth would need 75).
        // 76 judgments → consecutive_cools = 75 → fifth pill.
        for _ in 0..76 {
            stats.record_judgment(
                JudgmentType::Cool,
                false,
                open2jam_rs_core::Difficulty::Normal,
            );
        }
        assert_eq!(stats.pill_count, 5);
    }

    // ===================================================================
    // Max combo tracking
    // ===================================================================

    #[test]
    fn max_combo_tracked() {
        let mut stats = GameStats::new(100, 1000);
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.max_combo, 1);
        stats.combo = 0;
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.max_combo, 3);
    }

    // ===================================================================
    // Life clamping
    // ===================================================================

    #[test]
    fn life_clamps_at_zero() {
        let mut stats = GameStats::new(100, 1000);
        stats.life = -100;
        stats.record_judgment(
            JudgmentType::Miss,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.life, 0);
    }

    #[test]
    fn life_clamps_at_max() {
        let mut stats = GameStats::new(100, 1000);
        stats.life = 1000;
        stats.record_judgment(
            JudgmentType::Cool,
            false,
            open2jam_rs_core::Difficulty::Normal,
        );
        assert_eq!(stats.life, 1000);
    }
}
