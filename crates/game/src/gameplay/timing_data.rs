//! BPM-aware timing system (velocity tree).
//!
//! Equivalent to open2jam-modern's `TimingData` and `VelocityChange`.
//! Stores a sorted list of BPM change points, each with an absolute time
//! and the cumulative beat count at that moment. This allows correct
//! beat calculation even when BPM changes mid-chart.
//!
//! The scroll formula in HiSpeed uses:
//!   beats = timing.getBeat(target) - timing.getBeat(now)
//!   pixels = speed × beats × measureSize / 4

/// A single BPM change point in the velocity tree.
#[derive(Debug, Clone)]
pub struct VelocityChange {
    /// Absolute game time in milliseconds when this BPM takes effect.
    pub time_ms: f64,
    /// The new BPM value.
    pub bpm: f64,
    /// Cumulative beat count at this time (built after sorting).
    pub beat: f64,
}

impl VelocityChange {
    /// Calculate cumulative beats from this change point to a target time.
    pub fn beats_to(&self, target_time: f64) -> f64 {
        self.beat + (target_time - self.time_ms) * self.bpm / 60000.0
    }
}

/// BPM-aware timing calculator (velocity tree).
///
/// Built during chart loading from BPM change events.
/// Provides `getBeat(time)` which correctly accumulates beats across
/// all intermediate BPM changes.
#[derive(Debug, Clone)]
pub struct TimingData {
    /// Sorted list of BPM change points (built during chart load).
    changes: Vec<VelocityChange>,
}

impl Default for TimingData {
    fn default() -> Self {
        Self {
            changes: Vec::new(),
        }
    }
}

impl TimingData {
    /// Create an empty timing data.
    pub fn new() -> Self {
        Self {
            changes: Vec::new(),
        }
    }

    /// Add a BPM change at the given time.
    ///
    /// Called during chart parsing as events are encountered.
    /// The `beat` field is left at 0.0 and populated by `finish()`.
    pub fn add(&mut self, time_ms: f64, bpm: f64) {
        self.changes.push(VelocityChange {
            time_ms,
            bpm,
            beat: 0.0,
        });
    }

    /// Finalize the velocity tree by sorting entries by time and
    /// computing cumulative beat counts.
    ///
    /// Must be called after all `add()` calls and before any `getBeat()` calls.
    pub fn finish(&mut self) {
        // Sort by time
        self.changes.sort_by(|a, b| {
            a.time_ms
                .partial_cmp(&b.time_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Compute cumulative beats. Entries at the same time get the same beat
        // (zero delta), so the last entry's BPM wins — matching Java behavior
        // where a BPM change at position 0.0 of the first measure overrides
        // the header BPM.
        if self.changes.is_empty() {
            return;
        }

        self.changes[0].beat = 0.0;
        for i in 1..self.changes.len() {
            let prev = &self.changes[i - 1];
            self.changes[i].beat = prev.beats_to(self.changes[i].time_ms);
        }
    }

    /// Get the cumulative beat count at the given game time.
    ///
    /// Uses binary search to find the correct BPM segment, then
    /// calculates beats from that segment's starting point.
    /// This correctly accounts for all intermediate BPM changes.
    pub fn get_beat(&self, time_ms: f64) -> f64 {
        if self.changes.is_empty() {
            return 0.0;
        }

        // Find the last entry with time_ms <= target (partition_point).
        // This matches Java's binary search: when multiple entries share
        // the same time, the last one's BPM is used.
        let idx = self
            .changes
            .partition_point(|vc| vc.time_ms <= time_ms)
            .saturating_sub(1);

        self.changes[idx].beats_to(time_ms)
    }

    /// Get the BPM at the given game time.
    pub fn get_bpm(&self, time_ms: f64) -> f64 {
        if self.changes.is_empty() {
            return 0.0;
        }

        let idx = match self.changes.binary_search_by(|vc| {
            vc.time_ms
                .partial_cmp(&time_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };

        self.changes[idx].bpm
    }

    /// Get the number of BPM change points.
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Check if there are any BPM change points.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Get all velocity changes (for debugging).
    pub fn changes(&self) -> &[VelocityChange] {
        &self.changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_bpm() {
        let mut td = TimingData::new();
        td.add(1500.0, 130.0);
        td.finish();

        // At 130 BPM, 1 beat = 60000/130 ≈ 461.54ms
        let beat1 = td.get_beat(1500.0);
        assert!(
            beat1.abs() < 0.001,
            "beat at start should be 0, got {}",
            beat1
        );

        let beat2 = td.get_beat(1500.0 + 60000.0 / 130.0);
        assert!(
            (beat2 - 1.0).abs() < 0.001,
            "beat after 1 beat duration should be ~1.0, got {}",
            beat2
        );
    }

    #[test]
    fn test_bpm_change() {
        let mut td = TimingData::new();
        td.add(1500.0, 120.0); // 120 BPM → 500ms per beat
        td.add(3500.0, 240.0); // 240 BPM → 250ms per beat at t=3500ms
        td.finish();

        // At t=1500ms: beat = 0
        let b0 = td.get_beat(1500.0);
        assert!(b0.abs() < 0.001, "beat at 1500ms should be 0, got {}", b0);

        // At t=3500ms: 2000ms at 120 BPM = 2000/500 = 4.0 beats
        let b1 = td.get_beat(3500.0);
        assert!(
            (b1 - 4.0).abs() < 0.01,
            "beat at 3500ms should be 4.0, got {}",
            b1
        );

        // At t=4000ms: 4.0 + 500ms at 240 BPM = 4.0 + 500/250 = 6.0 beats
        let b2 = td.get_beat(4000.0);
        assert!(
            (b2 - 6.0).abs() < 0.01,
            "beat at 4000ms should be 6.0, got {}",
            b2
        );
    }

    #[test]
    fn test_beat_difference_across_bpm_change() {
        let mut td = TimingData::new();
        td.add(0.0, 120.0); // 500ms/beat
        td.add(2000.0, 240.0); // 250ms/beat
        td.finish();

        // Beats between t=1000ms and t=3000ms:
        // t=1000: 1000/500 = 2.0 beats
        // t=3000: 4.0 + (3000-2000)/250 = 4.0 + 4.0 = 8.0 beats
        // difference = 6.0 beats
        let diff = td.get_beat(3000.0) - td.get_beat(1000.0);
        assert!(
            (diff - 6.0).abs() < 0.01,
            "beat difference should be 6.0, got {}",
            diff
        );
    }

    #[test]
    fn test_get_bpm() {
        let mut td = TimingData::new();
        td.add(0.0, 120.0);
        td.add(2000.0, 240.0);
        td.finish();

        assert!((td.get_bpm(0.0) - 120.0).abs() < 0.01);
        assert!((td.get_bpm(1000.0) - 120.0).abs() < 0.01);
        assert!((td.get_bpm(2000.0) - 240.0).abs() < 0.01);
        assert!((td.get_bpm(3000.0) - 240.0).abs() < 0.01);
    }

    #[test]
    fn test_empty() {
        let td = TimingData::new();
        assert!(td.is_empty());
        assert_eq!(td.get_beat(1000.0), 0.0);
    }
}
