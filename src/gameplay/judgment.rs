//! Judgment system: beat-based timing windows and judgment evaluation.
//!
//! Implements the O2Jam judgment system with 192 TPB (ticks per beat) system:
//! - COOL:  ±6/192  measures = ±0.03125 measures ≈ ±50ms @ 150 BPM
//! - GOOD:  ±18/192 measures = ±0.09375 measures ≈ ±150ms @ 150 BPM
//! - BAD:   ±25/192 measures = ±0.13021 measures ≈ ±208ms @ 150 BPM (tap)
//! - BAD:   ±24/192 measures = ±0.125   measures ≈ ±200ms @ 150 BPM (release)

/// 192 TPB (ticks per beat) system thresholds in measures.
/// A measure = 4 beats, so these are fractions of a full measure.
const COOL_MEASURES: f64 = 6.0 / 192.0;    // ±0.03125 measures
const GOOD_MEASURES: f64 = 18.0 / 192.0;   // ±0.09375 measures
const BAD_MEASURES_TAP: f64 = 25.0 / 192.0; // ±0.13021 measures (tap notes)
const BAD_MEASURES_RELEASE: f64 = 24.0 / 192.0; // ±0.125 measures (long note releases)

/// Convert measures to milliseconds at given BPM.
/// 1 measure = 4 beats = 4 * 60000 / BPM ms
fn measures_to_ms(measures: f64, bpm: f64) -> f64 {
    measures * 4.0 * 60000.0 / bpm
}

/// The judgment result from evaluating a note hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JudgmentType {
    /// Perfect hit, minimal timing error
    Cool,
    /// Acceptable hit, moderate timing error
    Good,
    /// Barely acceptable hit
    Bad,
    /// Outside judgment window or not hit
    Miss,
}

impl JudgmentType {
    /// Base score for this judgment type (before jam combo bonus).
    /// Miss has a negative score (-10) as penalty.
    pub fn base_score(self) -> i32 {
        match self {
            JudgmentType::Cool => 200,
            JudgmentType::Good => 100,
            JudgmentType::Bad => 4,
            JudgmentType::Miss => -10, // Penalty for missing
        }
    }

    /// Returns the HP change for this judgment on Hard difficulty.
    pub fn hp_change_hard(self) -> i32 {
        match self {
            JudgmentType::Cool => 1,
            JudgmentType::Good => 0,
            JudgmentType::Bad => -5,
            JudgmentType::Miss => -30,
        }
    }

    /// Returns the HP change for this judgment on Easy difficulty.
    pub fn hp_change_easy(self) -> i32 {
        match self {
            JudgmentType::Cool => 3,
            JudgmentType::Good => 2,
            JudgmentType::Bad => -10,
            JudgmentType::Miss => -50,
        }
    }

    /// Returns the HP change for this judgment on Normal difficulty.
    pub fn hp_change_normal(self) -> i32 {
        match self {
            JudgmentType::Cool => 2,
            JudgmentType::Good => 1,
            JudgmentType::Bad => -7,
            JudgmentType::Miss => -40,
        }
    }

    /// Returns whether this judgment breaks the combo.
    pub fn breaks_combo(self) -> bool {
        matches!(self, JudgmentType::Miss | JudgmentType::Bad)
    }
}

/// Get the COOL judgment window in milliseconds for the given BPM.
pub fn cool_window_ms(bpm: f64) -> f64 {
    measures_to_ms(COOL_MEASURES, bpm)
}

/// Get the GOOD judgment window in milliseconds for the given BPM.
pub fn good_window_ms(bpm: f64) -> f64 {
    measures_to_ms(GOOD_MEASURES, bpm)
}

/// Get the BAD judgment window in milliseconds for the given BPM (tap notes).
pub fn bad_window_ms_tap(bpm: f64) -> f64 {
    measures_to_ms(BAD_MEASURES_TAP, bpm)
}

/// Get the BAD judgment window in milliseconds for the given BPM (release notes).
pub fn bad_window_ms_release(bpm: f64) -> f64 {
    measures_to_ms(BAD_MEASURES_RELEASE, bpm)
}

/// Get the COOL judgment window in milliseconds for the given BPM.
/// Used as the minimum late window floor for midpoint culling.
pub fn cool_window_ms_floor(bpm: f64) -> f64 {
    measures_to_ms(COOL_MEASURES, bpm)
}

/// Judge a tap note based on the time difference.
pub fn judge_tap_note(time_diff_ms: f64, bpm: f64) -> JudgmentType {
    let abs_diff = time_diff_ms.abs();
    let cool_ms = cool_window_ms(bpm);
    let good_ms = good_window_ms(bpm);
    let bad_ms = bad_window_ms_tap(bpm);

    if abs_diff <= cool_ms {
        JudgmentType::Cool
    } else if abs_diff <= good_ms {
        JudgmentType::Good
    } else if abs_diff <= bad_ms {
        JudgmentType::Bad
    } else {
        JudgmentType::Miss
    }
}

/// Judge a long note release based on the time difference.
pub fn judge_release(time_diff_ms: f64, bpm: f64) -> JudgmentType {
    let abs_diff = time_diff_ms.abs();
    let cool_ms = cool_window_ms(bpm);
    let good_ms = good_window_ms(bpm);
    let bad_ms = bad_window_ms_release(bpm);

    if abs_diff <= cool_ms {
        JudgmentType::Cool
    } else if abs_diff <= good_ms {
        JudgmentType::Good
    } else if abs_diff <= bad_ms {
        JudgmentType::Bad
    } else {
        JudgmentType::Miss
    }
}

/// Check if a tap note is within the accept window (player can hit it).
pub fn is_acceptable_tap(time_diff_ms: f64, bpm: f64) -> bool {
    time_diff_ms.abs() <= bad_window_ms_tap(bpm)
}

/// Check if a release note is within the accept window.
pub fn is_acceptable_release(time_diff_ms: f64, bpm: f64) -> bool {
    time_diff_ms.abs() <= bad_window_ms_release(bpm)
}

/// Check if a note has been missed (scrolled past without being hit).
pub fn is_missed(current_time_ms: f64, note_time_ms: f64, bpm: f64) -> bool {
    let diff = current_time_ms - note_time_ms;
    if diff < 0.0 {
        return false;
    }
    diff > bad_window_ms_tap(bpm)
}

/// Calculate score for a COOL judgment with jam combo bonus.
/// Formula: 200 + (jam_combo × 10)
pub fn cool_score_with_jam_bonus(jam_combo: u32) -> u32 {
    200 + (jam_combo * 10)
}

/// Calculate score for a GOOD judgment with jam combo bonus.
/// Formula: 100 + (jam_combo × 5)
pub fn good_score_with_jam_bonus(jam_combo: u32) -> u32 {
    100 + (jam_combo * 5)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cool_window_at_150_bpm() {
        // At 150 BPM, COOL window should be ~50ms
        let window = cool_window_ms(150.0);
        assert!((window - 50.0).abs() < 1.0, "COOL window at 150 BPM should be ~50ms, got {}", window);
    }

    #[test]
    fn test_good_window_at_150_bpm() {
        // At 150 BPM, GOOD window should be ~150ms
        let window = good_window_ms(150.0);
        assert!((window - 150.0).abs() < 1.0, "GOOD window at 150 BPM should be ~150ms, got {}", window);
    }

    #[test]
    fn test_bad_window_tap_at_150_bpm() {
        // At 150 BPM, BAD window (tap) should be ~208ms
        let window = bad_window_ms_tap(150.0);
        assert!((window - 208.3).abs() < 1.0, "BAD window at 150 BPM should be ~208ms, got {}", window);
    }

    #[test]
    fn test_judge_tap_cool() {
        let result = judge_tap_note(30.0, 150.0);
        assert_eq!(result, JudgmentType::Cool);
    }

    #[test]
    fn test_judge_tap_good() {
        let result = judge_tap_note(100.0, 150.0);
        assert_eq!(result, JudgmentType::Good);
    }

    #[test]
    fn test_judge_tap_bad() {
        let result = judge_tap_note(180.0, 150.0);
        assert_eq!(result, JudgmentType::Bad);
    }

    #[test]
    fn test_judge_tap_miss() {
        let result = judge_tap_note(250.0, 150.0);
        assert_eq!(result, JudgmentType::Miss);
    }

    #[test]
    fn test_cool_score_with_jam_bonus() {
        assert_eq!(cool_score_with_jam_bonus(0), 200);
        assert_eq!(cool_score_with_jam_bonus(10), 300);
        assert_eq!(cool_score_with_jam_bonus(50), 700);
    }

    #[test]
    fn test_good_score_with_jam_bonus() {
        assert_eq!(good_score_with_jam_bonus(0), 100);
        assert_eq!(good_score_with_jam_bonus(10), 150);
        assert_eq!(good_score_with_jam_bonus(50), 350);
    }

    #[test]
    fn test_judgment_type_hp_changes() {
        assert_eq!(JudgmentType::Cool.hp_change_hard(), 1);
        assert_eq!(JudgmentType::Good.hp_change_hard(), 0);
        assert_eq!(JudgmentType::Bad.hp_change_hard(), -5);
        assert_eq!(JudgmentType::Miss.hp_change_hard(), -30);

        assert_eq!(JudgmentType::Cool.hp_change_easy(), 3);
        assert_eq!(JudgmentType::Good.hp_change_easy(), 2);
        assert_eq!(JudgmentType::Bad.hp_change_easy(), -10);
        assert_eq!(JudgmentType::Miss.hp_change_easy(), -50);

        assert_eq!(JudgmentType::Cool.hp_change_normal(), 2);
        assert_eq!(JudgmentType::Good.hp_change_normal(), 1);
        assert_eq!(JudgmentType::Bad.hp_change_normal(), -7);
        assert_eq!(JudgmentType::Miss.hp_change_normal(), -40);
    }

    #[test]
    fn test_judgment_breaks_combo() {
        assert!(!JudgmentType::Cool.breaks_combo());
        assert!(!JudgmentType::Good.breaks_combo());
        assert!(JudgmentType::Bad.breaks_combo());
        assert!(JudgmentType::Miss.breaks_combo());
    }
}