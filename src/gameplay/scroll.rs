//! Scroll system: converts beat-based timing to pixel positions.
//!
//! Notes scroll at a rate determined by the current BPM.
//! The scroll formula is the single most important calculation in the engine:
//!
//! ```text
//! distance_px = speed × beats_remaining × measure_size / 4
//! measure_size = 0.8 × viewport_height
//! ```
//!
//! For charts with BPM changes, use `scroll_distance_bpm_aware()` and
//! `note_y_position_bpm_aware()` which use `TimingData::getBeat()` to
//! correctly accumulate beats across BPM changes (matches Java HiSpeed).

use super::timing_data::TimingData;

/// The fraction of viewport height used as the measure size.
pub const MEASURE_SIZE_FRACTION: f64 = 0.8;

/// Default scroll speed multiplier (can be adjusted for difficulty).
pub const DEFAULT_SCROLL_SPEED: f64 = 1.0;

/// Calculate the scroll distance (in pixels) for a note.
///
/// # Arguments
/// * `render_time_ms` — The current interpolated render time (ms).
/// * `target_time_ms` — The note's target time (ms) when it should hit the judgment line.
/// * `bpm` — The current beats per minute.
/// * `viewport_height` — The viewport height in pixels.
/// * `speed` — Scroll speed multiplier (default 1.0).
///
/// # Returns
/// The distance in pixels from the judgment line.
/// Positive values mean the note is above the judgment line.
/// Negative values mean the note has passed the judgment line.
pub fn scroll_distance(
    render_time_ms: f64,
    target_time_ms: f64,
    bpm: f64,
    viewport_height: f64,
    speed: f64,
) -> f64 {
    if bpm <= 0.0 {
        return 0.0;
    }

    let beats_remaining = (target_time_ms - render_time_ms) / (60000.0 / bpm);
    let measure_size = viewport_height * MEASURE_SIZE_FRACTION;
    speed * beats_remaining * measure_size / 4.0
}

/// Calculate the Y position of a note on screen.
///
/// # Arguments
/// * `render_time_ms` — The current interpolated render time (ms).
/// * `target_time_ms` — The note's target time (ms).
/// * `bpm` — The current BPM.
/// * `judgment_line_y` — Y position of the judgment line in skin coordinates.
/// * `viewport_height` — Viewport height in pixels.
/// * `speed` — Scroll speed multiplier.
///
/// # Returns
/// The Y position (pixels from top of screen).
pub fn note_y_position(
    render_time_ms: f64,
    target_time_ms: f64,
    bpm: f64,
    judgment_line_y: f64,
    viewport_height: f64,
    speed: f64,
) -> f64 {
    let distance = scroll_distance(render_time_ms, target_time_ms, bpm, viewport_height, speed);
    judgment_line_y - distance
}

// ── BPM-aware scroll functions (uses TimingData velocity tree) ─────────

/// Calculate scroll distance using the BPM-aware velocity tree.
///
/// This matches Java's HiSpeed formula:
///   pixels = speed × (getBeat(target) - getBeat(now)) × measureSize / 4
///
/// Correctly accounts for all intermediate BPM changes between `render_time_ms`
/// and `target_time_ms`.
pub fn scroll_distance_bpm_aware(
    render_time_ms: f64,
    target_time_ms: f64,
    timing: &TimingData,
    viewport_height: f64,
    speed: f64,
) -> f64 {
    if timing.is_empty() {
        return 0.0;
    }
    let beats_remaining = timing.get_beat(target_time_ms) - timing.get_beat(render_time_ms);
    let measure_size = viewport_height * MEASURE_SIZE_FRACTION;
    speed * beats_remaining * measure_size / 4.0
}

/// Calculate the Y position of a note on screen using BPM-aware timing.
pub fn note_y_position_bpm_aware(
    render_time_ms: f64,
    target_time_ms: f64,
    timing: &TimingData,
    judgment_line_y: f64,
    viewport_height: f64,
    speed: f64,
) -> f64 {
    let distance = scroll_distance_bpm_aware(render_time_ms, target_time_ms, timing, viewport_height, speed);
    judgment_line_y - distance
}

/// Calculate the time (ms) it takes for a note to scroll from top of screen to judgment line.
///
/// This determines how far ahead of time notes need to be spawned.
pub fn scroll_travel_time_ms(
    bpm: f64,
    viewport_height: f64,
    speed: f64,
) -> f64 {
    if bpm <= 0.0 || speed <= 0.0 {
        return 0.0;
    }
    let measure_size = viewport_height * MEASURE_SIZE_FRACTION;
    // Time for a note to travel the full viewport height
    // distance = speed × beats × measure_size / 4
    // beats = 4 × distance / (speed × measure_size)
    // time_ms = beats × 60000 / bpm
    let beats_needed = 4.0 * viewport_height / (speed * measure_size);
    beats_needed * 60000.0 / bpm
}

/// Check if a note is within the spawn window (near the top of the screen).
///
/// # Arguments
/// * `render_time_ms` — Current render time.
/// * `target_time_ms` — Note target time.
/// * `spawn_lead_time_ms` — How far ahead to spawn notes (ms).
///
/// # Returns
/// `true` if the note should be spawned now.
pub fn should_spawn_note(
    render_time_ms: f64,
    target_time_ms: f64,
    spawn_lead_time_ms: f64,
) -> bool {
    target_time_ms - render_time_ms <= spawn_lead_time_ms && target_time_ms >= render_time_ms
}

/// Check if a note has passed the judgment line and should be killed.
///
/// # Arguments
/// * `render_time_ms` — Current render time.
/// * `target_time_ms` — Note target time.
/// * `kill_tolerance_ms` — Extra time after target before killing (ms).
///
/// # Returns
/// `true` if the note has been missed and should be removed.
pub fn should_kill_note(
    render_time_ms: f64,
    target_time_ms: f64,
    kill_tolerance_ms: f64,
) -> bool {
    render_time_ms > target_time_ms + kill_tolerance_ms
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT_HEIGHT: f64 = 600.0;
    const JUDGMENT_LINE_Y: f64 = 480.0;
    const DEFAULT_BPM: f64 = 130.0;

    #[test]
    fn test_scroll_distance_at_target_time() {
        // When render_time equals target_time, distance should be 0 (at judgment line)
        let distance = scroll_distance(1000.0, 1000.0, DEFAULT_BPM, VIEWPORT_HEIGHT, DEFAULT_SCROLL_SPEED);
        assert!(distance.abs() < 0.01, "Distance at target time should be ~0, got {}", distance);
    }

    #[test]
    fn test_scroll_distance_before_target() {
        // 1 beat before target at 130 BPM = 60000/130 ≈ 461.5ms
        let beat_duration_ms = 60000.0 / DEFAULT_BPM;
        let render_time = 1000.0;
        let target_time = render_time + beat_duration_ms;

        let distance = scroll_distance(render_time, target_time, DEFAULT_BPM, VIEWPORT_HEIGHT, DEFAULT_SCROLL_SPEED);
        let expected = 1.0 * VIEWPORT_HEIGHT * MEASURE_SIZE_FRACTION / 4.0; // 1 beat = 1/4 measure

        assert!(
            (distance - expected).abs() < 1.0,
            "Distance should be ~{:.1}px, got {:.1}",
            expected,
            distance
        );
    }

    #[test]
    fn test_scroll_distance_after_target() {
        // 1 beat after target
        let beat_duration_ms = 60000.0 / DEFAULT_BPM;
        let render_time = 1000.0 + beat_duration_ms;
        let target_time = 1000.0;

        let distance = scroll_distance(render_time, target_time, DEFAULT_BPM, VIEWPORT_HEIGHT, DEFAULT_SCROLL_SPEED);
        let expected = -1.0 * VIEWPORT_HEIGHT * MEASURE_SIZE_FRACTION / 4.0;

        assert!(
            (distance - expected).abs() < 1.0,
            "Distance after target should be ~{:.1}px, got {:.1}",
            expected,
            distance
        );
    }

    #[test]
    fn test_scroll_distance_zero_bpm() {
        let distance = scroll_distance(1000.0, 2000.0, 0.0, VIEWPORT_HEIGHT, DEFAULT_SCROLL_SPEED);
        assert!(distance.abs() < 0.01, "Distance with 0 BPM should be 0");
    }

    #[test]
    fn test_scroll_distance_with_speed_multiplier() {
        let render_time = 1000.0;
        let target_time = 2000.0;
        let distance_slow = scroll_distance(render_time, target_time, DEFAULT_BPM, VIEWPORT_HEIGHT, 0.5);
        let distance_fast = scroll_distance(render_time, target_time, DEFAULT_BPM, VIEWPORT_HEIGHT, 2.0);

        assert!(distance_fast > distance_slow, "Faster speed should produce greater distance");
        assert!(
            (distance_fast - 4.0 * distance_slow).abs() < 0.1,
            "2x speed should produce 4x the distance of 0.5x speed"
        );
    }

    #[test]
    fn test_note_y_at_judgment_line() {
        // When at target time, note should be at judgment line
        let y = note_y_position(1000.0, 1000.0, DEFAULT_BPM, JUDGMENT_LINE_Y, VIEWPORT_HEIGHT, DEFAULT_SCROLL_SPEED);
        assert!(
            (y - JUDGMENT_LINE_Y).abs() < 0.01,
            "Note Y at target time should be at judgment line ({:.1}), got {:.1}",
            JUDGMENT_LINE_Y,
            y
        );
    }

    #[test]
    fn test_note_y_above_judgment_line() {
        // Before target, note should be above the judgment line
        let y = note_y_position(1000.0, 1500.0, DEFAULT_BPM, JUDGMENT_LINE_Y, VIEWPORT_HEIGHT, DEFAULT_SCROLL_SPEED);
        assert!(y < JUDGMENT_LINE_Y, "Note should be above judgment line (y={}, judgment={})", y, JUDGMENT_LINE_Y);
    }

    #[test]
    fn test_scroll_travel_time() {
        let travel_time = scroll_travel_time_ms(DEFAULT_BPM, VIEWPORT_HEIGHT, DEFAULT_SCROLL_SPEED);
        assert!(travel_time > 0.0, "Travel time should be positive");
        // At 130 BPM with 0.8 measure size and speed 1.0,
        // it takes ~4 beats to travel full viewport
        let expected_beats = 4.0 / MEASURE_SIZE_FRACTION; // ~5 beats
        let expected_ms = expected_beats * 60000.0 / DEFAULT_BPM;
        assert!(
            (travel_time - expected_ms).abs() < 1.0,
            "Travel time should be ~{:.1}ms, got {:.1}ms",
            expected_ms,
            travel_time
        );
    }

    #[test]
    fn test_should_spawn_note() {
        assert!(should_spawn_note(1000.0, 1500.0, 600.0), "Note within spawn window should spawn");
        assert!(!should_spawn_note(1000.0, 2000.0, 600.0), "Note outside spawn window should not spawn");
        assert!(!should_spawn_note(1000.0, 500.0, 600.0), "Past note should not spawn");
    }

    #[test]
    fn test_should_kill_note() {
        assert!(!should_kill_note(1000.0, 1100.0, 0.0), "Note before target should not be killed");
        assert!(!should_kill_note(1099.0, 1100.0, 0.0), "Note just before target should not be killed");
        assert!(should_kill_note(1101.0, 1100.0, 0.0), "Note just after target should be killed");
        assert!(should_kill_note(1400.0, 1100.0, 0.0), "Note well after target should be killed");
    }
}
