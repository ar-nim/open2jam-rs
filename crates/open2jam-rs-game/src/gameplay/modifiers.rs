//! Lane arrangement modifiers (Mirror, Random, Panic).
//!
//! These modifiers transform a note's visual lane at spawn time.
//! Key-to-lane mapping always stays on the original lane — the modifier
//! only affects visual note position, not input.

use std::cell::RefCell;
use std::collections::HashMap;

use open2jam_rs_core::game_options::ChannelMod;
use open2jam_rs_parsers::{Chart, NoteType, TimedEvent};

pub trait LaneModifier: Send {
    fn transform(
        &self,
        lane: usize,
        measure: u32,
        note_type: NoteType,
        is_release: bool,
        occupied_lanes: &[usize],
    ) -> usize;
    fn clear_release(&self, lane: usize) {
        // default no-op for Mirror, Random, None
    }
    fn prepare_measure(&self, _measure: u32) {
        // default no-op for Mirror, Random, None
    }
}

struct MirrorModifier;
impl LaneModifier for MirrorModifier {
    fn transform(
        &self,
        lane: usize,
        _measure: u32,
        _note_type: NoteType,
        _is_release: bool,
        _occupied: &[usize],
    ) -> usize {
        6usize.saturating_sub(lane)
    }
}

struct SeededRng(u64);
impl SeededRng {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0x5DEECE66D } else { seed })
    }
    fn shuffle<T>(&mut self, slice: &mut [T]) {
        for i in (1..slice.len()).rev() {
            let j = self.next_usize(i + 1);
            slice.swap(i, j);
        }
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0
    }
    fn next_usize(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

struct RandomModifier([usize; 7]);
impl RandomModifier {
    fn new(seed: u64) -> Self {
        let mut rng = SeededRng::new(seed);
        let mut perm: Vec<usize> = (0..7).collect();
        rng.shuffle(&mut perm);
        let perm: [usize; 7] = perm.try_into().unwrap();
        Self(perm)
    }
}
impl LaneModifier for RandomModifier {
    fn transform(
        &self,
        lane: usize,
        _measure: u32,
        _note_type: NoteType,
        _is_release: bool,
        _occupied: &[usize],
    ) -> usize {
        *self.0.get(lane).unwrap_or(&lane)
    }
}

/// Pre-computed data for Panic modifier, built once at load time.
pub struct PanicPrecomputed {
    /// Permutation for each measure: permutations[measure][orig_lane] = visual_lane.
    /// Seed = measure as u64. Fully deterministic.
    pub permutations: Vec<[usize; 7]>,
    /// Hot visual mask for each measure boundary.
    /// hot_masks[M] has bits set for visual lanes that had notes active in the
    /// last 1/16 of measure M (position > 0.9375).
    /// Bit i set = visual lane i is "hot" entering measure M+1.
    pub hot_masks: Vec<u8>,
}

impl PanicPrecomputed {
    /// Build pre-computed permutations and hot masks from a fully-parsed chart.
    /// Called once at load time before any gameplay.
    pub fn build(chart: &Chart) -> Self {
        let max_measure = chart
            .events
            .iter()
            .filter_map(|e| match e {
                TimedEvent::Note(n) => Some(n.measure),
                _ => None,
            })
            .max()
            .unwrap_or(0);

        // Step 1: Compute permutation for each measure (0..=max_measure)
        let mut permutations: Vec<[usize; 7]> = Vec::with_capacity(max_measure as usize + 1);
        for m in 0..=max_measure {
            let mut rng = SeededRng::new(m as u64);
            let mut perm: Vec<usize> = (0..7).collect();
            rng.shuffle(&mut perm);
            let perm_array: [usize; 7] = perm.try_into().unwrap();
            permutations.push(perm_array);
        }

        // Step 2: Simulate hold state machine to build hot_masks.
        // A visual lane is "hot" in measure M if any note is active in the
        // last 1/16 of measure M (position > 0.9375).
        // - Tap: instantaneous, hot if position > 0.9375
        // - Hold head: NOT hot (only tails matter)
        // - Release: hot if position > 0.9375 (uses lane_lock for visual lane)
        let mut hot_masks: Vec<u8> = vec![0u8; max_measure as usize + 1];
        let mut lane_lock_sim: HashMap<usize, usize> = HashMap::new(); // orig → visual
        let mut frozen_measure: Option<u32> = None;

        // BPM tracking for time calculations (not needed for position-based hot mask,
        // but needed to track measure boundaries correctly).
        for event in &chart.events {
            // BPM changes don't affect position-based hot mask calculation

            let TimedEvent::Note(note_event) = event else {
                continue;
            };
            let Some(lane) = note_event.channel.lane_index() else {
                continue;
            };
            if lane >= 7 {
                continue;
            }
            let lane = lane;
            let measure = note_event.measure;
            let position = note_event.position;

            // Determine visual lane using current permutation / frozen state
            let visual_lane = lane_lock_sim.get(&lane).copied().unwrap_or_else(|| {
                let perm = frozen_measure
                    .map(|fm| &permutations[fm as usize])
                    .unwrap_or(&permutations[measure as usize]);
                perm[lane]
            });

            match note_event.note_type {
                NoteType::Tap => {
                    if position > 0.9375 {
                        hot_masks[measure as usize] |= 1 << visual_lane;
                    }
                }
                NoteType::Hold => {
                    lane_lock_sim.insert(lane, visual_lane);
                    if lane_lock_sim.len() == 1 {
                        frozen_measure = Some(measure);
                    }
                }
                NoteType::Release => {
                    if position > 0.9375 {
                        if let Some(vl) = lane_lock_sim.get(&lane).copied() {
                            hot_masks[measure as usize] |= 1 << vl;
                        }
                    }
                    lane_lock_sim.remove(&lane);
                    if lane_lock_sim.is_empty() {
                        frozen_measure = None;
                    }
                }
            }
        }

        Self {
            permutations,
            hot_masks,
        }
    }
}

/// Panic modifier.
///
/// Permutation is per-measure (seed = measure as u64), BUT when holds are active
/// the permutation is frozen — it does not change even if we enter new measures,
/// until all active holds are released.
///
/// At each new measure, a best-effort swap is applied using the pre-computed
/// hot_masks to avoid routing a new note to a lane that was used in the last
/// 1/16 of the previous measure.
struct PanicModifier {
    precomputed: PanicPrecomputed,
    /// Channel locking: original_lane → shuffled_lane for ACTIVE holds.
    lane_lock: RefCell<HashMap<usize, usize>>,
    /// Frozen permutation while holds are active.
    /// The frozen permutation is captured when the first hold starts, and persists
    /// until all holds are released.
    frozen: RefCell<Option<(u32, [usize; 7])>>,
    /// Most recently prepared measure. Used to detect measure boundaries.
    last_prepared: RefCell<u32>,
    /// The measure where the current frozen window started.
    /// Used to properly capture frozen when transitioning from empty to non-empty lane_lock.
    frozen_start_measure: RefCell<Option<u32>>,
}

impl PanicModifier {
    fn new(precomputed: PanicPrecomputed) -> Self {
        Self {
            precomputed,
            lane_lock: RefCell::new(HashMap::new()),
            frozen: RefCell::new(None),
            last_prepared: RefCell::new(u32::MAX),
            frozen_start_measure: RefCell::new(None),
        }
    }

    /// Called at the start of each new measure (before processing any notes in that measure).
    /// Computes and stores the permutation for this measure, with best-effort swap applied.
    fn prepare_measure(&self, measure: u32) {
        let last = *self.last_prepared.borrow();
        if last == measure {
            return;
        }
        *self.last_prepared.borrow_mut() = measure;

        let lane_lock_empty = self.lane_lock.borrow().is_empty();
        let frozen_was_none = self.frozen.borrow().is_none();

        if lane_lock_empty {
            self.frozen.borrow_mut().take();
            *self.frozen_start_measure.borrow_mut() = None;
        }

        if !lane_lock_empty {
            if frozen_was_none && self.frozen_start_measure.borrow().is_some() {
                return;
            }
            return;
        }

        let frozen_perm = self.precomputed.permutations[measure as usize];

        let hot_mask = if measure > 0 {
            self.precomputed.hot_masks[(measure - 1) as usize]
        } else {
            0
        };

        if hot_mask != 0 {
            let mut perm = frozen_perm;
            'swap_loop: for (i, &v) in perm.iter().enumerate() {
                if (hot_mask >> v) & 1 == 1 {
                    for j in 0..7 {
                        if (hot_mask >> perm[j]) & 1 == 0 {
                            perm.swap(i, j);
                            break 'swap_loop;
                        }
                    }
                }
            }
            *self.frozen.borrow_mut() = Some((measure, perm));
        }
    }

    fn active_count(&self) -> usize {
        self.lane_lock.borrow().len()
    }

    fn get_frozen_or_fresh(&self, measure: u32) -> [usize; 7] {
        if self.active_count() == 0 {
            self.precomputed.permutations[measure as usize]
        } else {
            let frozen = self.frozen.borrow();
            if let Some((_, perm)) = frozen.as_ref() {
                *perm
            } else {
                self.precomputed.permutations[measure as usize]
            }
        }
    }
}

impl LaneModifier for PanicModifier {
    fn transform(
        &self,
        lane: usize,
        measure: u32,
        note_type: NoteType,
        is_release: bool,
        occupied: &[usize],
    ) -> usize {
        // Detect new measure and prepare permutation
        self.prepare_measure(measure);

        // RELEASE: look up stored lane (channel locking — tail must match head)
        if is_release {
            return *self.lane_lock.borrow().get(&lane).unwrap_or(&lane);
        }

        let perm = self.get_frozen_or_fresh(measure);
        let shuffled = *perm.get(lane).unwrap_or(&lane);

        // HOLD head: store in lane_lock for head/tail consistency
        // Also capture frozen if this is the first hold in a frozen window
        if matches!(note_type, NoteType::Hold) {
            let lane_lock_len_before = self.lane_lock.borrow().len();
            self.lane_lock.borrow_mut().insert(lane, shuffled);
            // If lane_lock was empty before (transitioning from empty to non-empty),
            // this is the first hold in a frozen window - capture frozen now
            if lane_lock_len_before == 0 {
                *self.frozen.borrow_mut() = Some((measure, perm));
                *self.frozen_start_measure.borrow_mut() = Some(measure);
            }
        }

        // Tap collision: redirect if shuffled lane is occupied by active long note
        if !occupied.contains(&shuffled) {
            return shuffled;
        }
        for &l in &perm {
            if !occupied.contains(&l) {
                return l;
            }
        }
        lane // fallback
    }

    fn clear_release(&self, lane: usize) {
        self.lane_lock.borrow_mut().remove(&lane);
        if self.lane_lock.borrow().is_empty() {
            *self.frozen_start_measure.borrow_mut() = None;
        }
    }
}

impl LaneModifier for Box<dyn LaneModifier + Send> {
    fn transform(
        &self,
        lane: usize,
        measure: u32,
        note_type: NoteType,
        is_release: bool,
        occupied: &[usize],
    ) -> usize {
        self.as_ref()
            .transform(lane, measure, note_type, is_release, occupied)
    }
    fn clear_release(&self, lane: usize) {
        self.as_ref().clear_release(lane);
    }
    fn prepare_measure(&self, measure: u32) {
        self.as_ref().prepare_measure(measure);
    }
}

enum MaybeLaneModifier {
    None,
    Mirror(MirrorModifier),
    Random(RandomModifier),
    Panic(PanicModifier),
}

impl LaneModifier for MaybeLaneModifier {
    fn transform(
        &self,
        lane: usize,
        measure: u32,
        note_type: NoteType,
        is_release: bool,
        occupied: &[usize],
    ) -> usize {
        match self {
            MaybeLaneModifier::None => lane,
            MaybeLaneModifier::Mirror(m) => {
                m.transform(lane, measure, note_type, is_release, occupied)
            }
            MaybeLaneModifier::Random(m) => {
                m.transform(lane, measure, note_type, is_release, occupied)
            }
            MaybeLaneModifier::Panic(m) => {
                m.transform(lane, measure, note_type, is_release, occupied)
            }
        }
    }
    fn clear_release(&self, lane: usize) {
        match self {
            MaybeLaneModifier::None => {}
            MaybeLaneModifier::Mirror(_) => {}
            MaybeLaneModifier::Random(_) => {}
            MaybeLaneModifier::Panic(m) => m.clear_release(lane),
        }
    }
    fn prepare_measure(&self, measure: u32) {
        match self {
            MaybeLaneModifier::None => {}
            MaybeLaneModifier::Mirror(_) => {}
            MaybeLaneModifier::Random(_) => {}
            MaybeLaneModifier::Panic(m) => m.prepare_measure(measure),
        }
    }
}

fn from_channel_mod(
    mod_: ChannelMod,
    seed: u64,
    panic_precomputed: Option<PanicPrecomputed>,
) -> MaybeLaneModifier {
    match mod_ {
        ChannelMod::None => MaybeLaneModifier::None,
        ChannelMod::Random => MaybeLaneModifier::Random(RandomModifier::new(seed)),
        ChannelMod::Panic => {
            MaybeLaneModifier::Panic(PanicModifier::new(panic_precomputed.unwrap()))
        }
        ChannelMod::Mirror => MaybeLaneModifier::Mirror(MirrorModifier),
    }
}

pub struct Modifiers {
    inner: Box<dyn LaneModifier + Send>,
}

impl Modifiers {
    pub fn new(
        channel_mod: ChannelMod,
        seed: u64,
        panic_precomputed: Option<PanicPrecomputed>,
    ) -> Self {
        Self {
            inner: Box::new(from_channel_mod(channel_mod, seed, panic_precomputed)),
        }
    }

    pub fn transform_lane(
        &self,
        lane: usize,
        measure: u32,
        note_type: NoteType,
        is_release: bool,
        occupied_lanes: &[usize],
    ) -> usize {
        self.inner
            .transform(lane, measure, note_type, is_release, occupied_lanes)
    }

    pub fn clear_release(&self, lane: usize) {
        self.inner.clear_release(lane);
    }

    pub fn prepare_measure(&self, measure: u32) {
        self.inner.prepare_measure(measure);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_chart(events: Vec<TimedEvent>) -> Chart {
        use open2jam_rs_parsers::OjnHeader;
        Chart {
            header: OjnHeader {
                song_id: 100,
                encode_version: 0.0,
                genre: 0,
                bpm: 120.0,
                level_easy: 0,
                level_normal: 0,
                level_hard: 0,
                event_count_easy: 0,
                event_count_normal: 0,
                event_count_hard: 0,
                note_count_easy: 0,
                note_count_normal: 0,
                note_count_hard: 0,
                measure_count_easy: 0,
                measure_count_normal: 0,
                measure_count_hard: 0,
                title: String::new(),
                artist: String::new(),
                noter: String::new(),
                ojm_filename: String::new(),
                bmp_size: 0,
                cover_size: 0,
                duration_easy: 0,
                duration_normal: 0,
                duration_hard: 0,
                note_offset_easy: 0,
                note_offset_normal: 0,
                note_offset_hard: 0,
                cover_offset: 0,
            },
            events,
        }
    }

    fn note(measure: u32, position: f64, lane: usize, note_type: NoteType) -> TimedEvent {
        use open2jam_rs_parsers::{Channel, NoteEvent};
        TimedEvent::Note(NoteEvent {
            time_ms: 0.0,
            channel: Channel::Note((lane + 1) as u8),
            sample_id: None,
            volume: 1.0,
            pan: 0.0,
            note_type,
            measure,
            position,
            end_time_ms: None,
        })
    }

    #[test]
    fn test_mirror() {
        let m = MirrorModifier;
        assert_eq!(m.transform(0, 0, NoteType::Tap, false, &[]), 6);
        assert_eq!(m.transform(3, 0, NoteType::Tap, false, &[]), 3);
        assert_eq!(m.transform(6, 0, NoteType::Tap, false, &[]), 0);
    }

    #[test]
    fn test_random_deterministic() {
        let seed = 12345u64;
        let m1 = RandomModifier::new(seed);
        let m2 = RandomModifier::new(seed);
        for lane in 0..7 {
            let t1 = m1.transform(lane, 0, NoteType::Tap, false, &[]);
            let t2 = m2.transform(lane, 0, NoteType::Tap, false, &[]);
            assert_eq!(t1, t2, "Same seed must produce same permutation");
        }
    }

    #[test]
    fn test_random_is_bijection() {
        let m = RandomModifier::new(42);
        let mut seen = [false; 7];
        for lane in 0..7 {
            let t = m.transform(lane, 0, NoteType::Tap, false, &[]);
            assert!(t < 7, "transformed lane must be 0-6");
            seen[t] = true;
        }
        for (i, &v) in seen.iter().enumerate() {
            assert!(v, "Random permutation must contain lane {}", i);
        }
    }

    #[test]
    fn test_panic_head_tail_same_lane() {
        let chart = make_chart(vec![
            note(0, 0.0, 3, NoteType::Hold),
            note(5, 0.0, 3, NoteType::Release),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        let m = PanicModifier::new(pre);

        m.prepare_measure(0);
        let head_lane = m.transform(3, 0, NoteType::Hold, false, &[]);
        m.prepare_measure(5);
        let tail_lane = m.transform(3, 5, NoteType::Release, true, &[]);
        assert_eq!(
            head_lane, tail_lane,
            "Head and tail of same long note must share the same visual lane"
        );
    }

    #[test]
    fn test_panic_frozen_while_hold_active() {
        let chart = make_chart(vec![
            note(0, 0.0, 3, NoteType::Hold),
            note(5, 0.0, 3, NoteType::Release),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        let m = PanicModifier::new(pre);

        m.prepare_measure(0);
        let _shuffled = m.transform(3, 0, NoteType::Hold, false, &[]);
        m.prepare_measure(1);
        let tap_m1 = m.transform(0, 1, NoteType::Tap, false, &[]);
        m.prepare_measure(2);
        let tap_m2 = m.transform(0, 2, NoteType::Tap, false, &[]);
        assert_eq!(
            tap_m1, tap_m2,
            "Measures 1 and 2 should use frozen permutation while hold active"
        );
    }

    #[test]
    fn test_panic_reshuffle_after_release() {
        let chart = make_chart(vec![
            note(0, 0.0, 3, NoteType::Hold),
            note(5, 0.0, 3, NoteType::Release),
            note(6, 0.0, 0, NoteType::Tap),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        let m = PanicModifier::new(pre);

        m.prepare_measure(0);
        m.transform(3, 0, NoteType::Hold, false, &[]);
        m.prepare_measure(5);
        m.transform(3, 5, NoteType::Release, true, &[]);
        m.prepare_measure(6);
        let p6_a = m.transform(0, 6, NoteType::Tap, false, &[]);
        let p6_b = m.transform(1, 6, NoteType::Tap, false, &[]);
        assert_ne!(p6_a, p6_b, "Measure 6 should have fresh permutation");
    }

    #[test]
    fn test_panic_frozen_captures_measure() {
        let chart = make_chart(vec![
            note(10, 0.0, 3, NoteType::Hold),
            note(20, 0.0, 3, NoteType::Release),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        let m = PanicModifier::new(pre);

        m.prepare_measure(10);
        let shuffled_10 = m.transform(3, 10, NoteType::Hold, false, &[]);
        m.prepare_measure(20);
        let tap_at_20 = m.transform(3, 20, NoteType::Tap, false, &[]);
        assert_eq!(
            shuffled_10, tap_at_20,
            "Frozen permutation from measure 10 should persist across measures"
        );
    }

    #[test]
    fn test_panic_clear_release() {
        let chart = make_chart(vec![
            note(0, 0.0, 3, NoteType::Hold),
            note(5, 0.0, 3, NoteType::Release),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        let m = PanicModifier::new(pre);

        m.prepare_measure(0);
        m.transform(3, 0, NoteType::Hold, false, &[]);
        m.clear_release(3);
        assert!(
            m.lane_lock.borrow().get(&3).is_none(),
            "clear_release should remove lane_lock entry"
        );
    }

    #[test]
    fn test_panic_occupied_fallback() {
        let chart = make_chart(vec![]);
        let pre = PanicPrecomputed::build(&chart);
        // Extract shuffled lane before moving pre
        let shuffled = pre.permutations[0][3];
        let m = PanicModifier::new(pre);

        m.prepare_measure(0);
        let occupied = vec![shuffled];
        let result = m.transform(3, 0, NoteType::Tap, false, &occupied);
        assert_ne!(
            result, shuffled,
            "When shuffled lane is occupied, should fall through to different lane"
        );
        assert!(result < 7, "result must be a valid lane");
    }

    #[test]
    fn test_panic_fallback_to_original() {
        let chart = make_chart(vec![]);
        let pre = PanicPrecomputed::build(&chart);
        let m = PanicModifier::new(pre);

        m.prepare_measure(0);
        let occupied = vec![0, 1, 2, 3, 4, 5, 6];
        let result = m.transform(3, 0, NoteType::Tap, false, &occupied);
        assert_eq!(result, 3, "Fallback to original when all occupied");
    }

    #[test]
    fn test_panic_hot_mask_tap_last_16th() {
        // Tap at position 0.95 (> 0.9375) in measure 2 should set bit for its visual lane
        let chart = make_chart(vec![note(2, 0.95, 1, NoteType::Tap)]);
        let pre = PanicPrecomputed::build(&chart);
        // Measure 2's hot mask should have a bit set
        let mask = pre.hot_masks[2];
        let perm = pre.permutations[2];
        let visual_lane = perm[1];
        assert!(
            (mask >> visual_lane) & 1 == 1,
            "Tap at position 0.95 in measure 2 should mark its visual lane as hot"
        );
    }

    #[test]
    fn test_panic_hot_mask_ignore_hold_head() {
        // Hold at position 0.95 (> 0.9375) in measure 2 should NOT set hot mask
        // (only release tails matter for hot mask)
        let chart = make_chart(vec![
            note(2, 0.95, 1, NoteType::Hold),
            note(3, 0.0, 1, NoteType::Release),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        // Measure 2's hot mask should NOT have lane 1's visual lane set
        // (the hold head at 0.95 doesn't make it hot)
        let mask = pre.hot_masks[2];
        let perm = pre.permutations[2];
        let visual_lane = perm[1];
        assert!(
            (mask >> visual_lane) & 1 == 0,
            "Hold head at position 0.95 should NOT mark hot mask (only tails matter)"
        );
    }

    #[test]
    fn test_panic_hot_mask_release_last_16th() {
        // Release at position 0.95 (> 0.9375) in measure 2 should set hot mask
        let chart = make_chart(vec![
            note(2, 0.0, 1, NoteType::Hold),
            note(2, 0.95, 1, NoteType::Release),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        let mask = pre.hot_masks[2];
        let perm = pre.permutations[2];
        let visual_lane = perm[1];
        assert!(
            (mask >> visual_lane) & 1 == 1,
            "Release at position 0.95 should mark its visual lane as hot"
        );
    }

    #[test]
    fn test_panic_precompute_max_measure() {
        // Notes only in measures 0, 5, 10 — permutations array should cover all
        let chart = make_chart(vec![
            note(0, 0.0, 0, NoteType::Tap),
            note(5, 0.0, 1, NoteType::Tap),
            note(10, 0.0, 2, NoteType::Tap),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        assert!(
            pre.permutations.len() >= 11,
            "Should have permutations for measures 0 through at least 10"
        );
    }

    #[test]
    fn test_panic_precompute_frozen_from_first_hold() {
        // Hold starts at measure 3, tap at measure 5, release at measure 7
        // The frozen permutation should be from measure 3 (where first hold started)
        let chart = make_chart(vec![
            note(3, 0.0, 1, NoteType::Hold),
            note(5, 0.0, 2, NoteType::Tap),
            note(7, 0.0, 1, NoteType::Release),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        // Compute what lane 2 would map to in measure 3's permutation
        let perm_3_lane2 = pre.permutations[3][2];
        let m = PanicModifier::new(pre);

        m.prepare_measure(3);
        let _shuffled_3 = m.transform(1, 3, NoteType::Hold, false, &[]);
        m.prepare_measure(5);
        let tap_5 = m.transform(2, 5, NoteType::Tap, false, &[]);
        // Tap at measure 5 should use frozen permutation from measure 3
        assert_eq!(
            tap_5, perm_3_lane2,
            "Tap at measure 5 should use frozen permutation from when first hold started (measure 3)"
        );
    }

    #[test]
    fn test_seeded_rng_deterministic() {
        let mut rng1 = SeededRng::new(999);
        let mut rng2 = SeededRng::new(999);
        let mut v1 = vec![1, 2, 3, 4, 5];
        let mut v2 = vec![1, 2, 3, 4, 5];
        rng1.shuffle(&mut v1);
        rng2.shuffle(&mut v2);
        assert_eq!(v1, v2, "Same seed must produce same shuffle");
    }

    #[test]
    fn test_panic_different_perms_per_measure() {
        let chart = make_chart(vec![
            note(0, 0.0, 0, NoteType::Tap),
            note(1, 0.0, 0, NoteType::Tap),
            note(2, 0.0, 0, NoteType::Tap),
            note(3, 0.0, 0, NoteType::Tap),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        let m = PanicModifier::new(pre);

        m.prepare_measure(0);
        let m0 = m.transform(0, 0, NoteType::Tap, false, &[]);
        m.prepare_measure(1);
        let m1 = m.transform(0, 1, NoteType::Tap, false, &[]);
        m.prepare_measure(2);
        let m2 = m.transform(0, 2, NoteType::Tap, false, &[]);
        m.prepare_measure(3);
        let m3 = m.transform(0, 3, NoteType::Tap, false, &[]);

        let all_same = m0 == m1 && m1 == m2 && m2 == m3;
        assert!(
            !all_same,
            "Measures 0,1,2,3 should have different permutations (got {}, {}, {}, {})",
            m0, m1, m2, m3
        );
    }

    #[test]
    fn test_panic_first_measure_notes_all_different() {
        // Simulate actual gameplay: notes spawn in order, measure boundary detection
        let chart = make_chart(vec![
            note(0, 0.0, 0, NoteType::Tap),
            note(0, 0.25, 1, NoteType::Tap),
            note(0, 0.5, 2, NoteType::Tap),
            note(0, 0.75, 3, NoteType::Tap),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        let m = PanicModifier::new(pre);

        // Simulate measure boundary detection (first note triggers prepare_measure)
        let mut current_measure = 0u32;
        let mut results = vec![];

        for (i, ev) in chart.events.iter().enumerate() {
            let TimedEvent::Note(note_ev) = ev else {
                continue;
            };
            if note_ev.measure != current_measure {
                current_measure = note_ev.measure;
                m.prepare_measure(current_measure);
            }
            let visual_lane = m.transform(
                note_ev.channel.lane_index().unwrap(),
                note_ev.measure,
                note_ev.note_type,
                false,
                &[],
            );
            results.push(visual_lane);
        }

        // All 4 notes in measure 0 should use the same permutation
        // But they should be shuffled (not 0,1,2,3)
        assert_eq!(results.len(), 4);
        let is_identity = results == vec![0, 1, 2, 3];
        assert!(
            !is_identity,
            "Notes at lanes 0,1,2,3 should be shuffled, not identity (got {:?})",
            results
        );
    }

    #[test]
    fn test_panic_modifiers_api_first_measure() {
        // Test using the public Modifiers API like game_state does
        let chart = make_chart(vec![
            note(0, 0.0, 0, NoteType::Tap),
            note(0, 0.25, 1, NoteType::Tap),
            note(1, 0.0, 0, NoteType::Tap),
            note(1, 0.25, 1, NoteType::Tap),
        ]);
        let pre = PanicPrecomputed::build(&chart);
        let modifiers = Modifiers::new(ChannelMod::Panic, 12345, Some(pre));

        // Simulate spawn_notes: call prepare_measure on measure boundary, then transform_lane
        let mut current_measure = 0u32;
        let mut results = vec![];

        for ev in &chart.events {
            let TimedEvent::Note(note_ev) = ev else {
                continue;
            };
            if note_ev.measure != current_measure {
                current_measure = note_ev.measure;
                modifiers.prepare_measure(current_measure);
            }
            let visual_lane = modifiers.transform_lane(
                note_ev.channel.lane_index().unwrap(),
                note_ev.measure,
                note_ev.note_type,
                false,
                &[],
            );
            results.push((
                note_ev.measure,
                note_ev.channel.lane_index().unwrap(),
                visual_lane,
            ));
        }

        // Check measure 0 notes are shuffled but consistent
        let m0_notes: Vec<_> = results.iter().filter(|(m, _, _)| *m == 0).collect();
        let m0_original: Vec<_> = m0_notes.iter().map(|(_, orig, _)| *orig).collect();
        let m0_visual: Vec<_> = m0_notes.iter().map(|(_, _, visual)| *visual).collect();
        assert_eq!(m0_original, vec![0, 1]);
        assert_ne!(m0_visual, vec![0, 1], "Measure 0 should be shuffled");

        // Check measure 1 notes are shuffled and different from measure 0
        let m1_notes: Vec<_> = results.iter().filter(|(m, _, _)| *m == 1).collect();
        let m1_visual: Vec<_> = m1_notes.iter().map(|(_, _, visual)| *visual).collect();
        assert_ne!(m1_visual, vec![0, 1], "Measure 1 should be shuffled");
        assert_ne!(
            m0_visual, m1_visual,
            "Measure 0 and 1 should have different shuffles"
        );
    }

    #[test]
    fn test_panic_measure0_permutation_contents() {
        // Verify measure 0's permutation is actually shuffled (not identity)
        let chart = make_chart(vec![note(0, 0.0, 0, NoteType::Tap)]);
        let pre = PanicPrecomputed::build(&chart);
        let perm0 = &pre.permutations[0];

        // Permutation should not be identity [0,1,2,3,4,5,6]
        let is_identity = perm0 == &[0, 1, 2, 3, 4, 5, 6];
        assert!(
            !is_identity,
            "Measure 0 permutation should be shuffled, not identity: {:?}",
            perm0
        );

        // Also verify it's a valid bijection (each lane 0-6 appears exactly once)
        let mut seen = [false; 7];
        for &v in perm0 {
            assert!(v < 7, "Permutation value {} is out of range", v);
            seen[v] = true;
        }
        for (i, &v) in seen.iter().enumerate() {
            assert!(v, "Permutation should contain lane {}", i);
        }
    }
}
