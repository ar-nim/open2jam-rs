//! Entity types: ActiveNote and ActiveLongNote.

use crate::gameplay::judgment::JudgmentType;

#[derive(Debug, Clone)]
pub struct ActiveNote {
    pub(crate) lane: usize,
    pub(crate) target_time_ms: f64,
    pub(crate) sample_id: Option<u32>,
    pub(crate) volume: f32,
    pub(crate) pan: f32,
    pub(crate) judged: bool,
    pub(crate) missed: bool,
    pub(crate) judgment_type: Option<JudgmentType>,
}

#[derive(Debug, Clone)]
pub struct ActiveLongNote {
    pub(crate) lane: usize,
    pub(crate) head_time_ms: f64,
    pub(crate) tail_time_ms: f64,
    pub(crate) sample_id: Option<u32>,
    pub(crate) volume: f32,
    pub(crate) pan: f32,
    pub(crate) judged: bool,
    pub(crate) missed: bool,
    pub(crate) holding: bool,
    pub(crate) dead: bool,
    pub(crate) head_judgment: Option<JudgmentType>,
    pub(crate) tail_judgment: Option<JudgmentType>,
}

#[cfg(test)]
mod tests {
    use super::{ActiveLongNote, ActiveNote};
    use crate::gameplay::judgment::JudgmentType;

    // ===================================================================
    // ActiveNote tests
    // ===================================================================

    #[test]
    fn active_note_fields() {
        let note = ActiveNote {
            lane: 3,
            target_time_ms: 1234.5,
            sample_id: Some(7),
            volume: 0.8,
            pan: 0.5,
            judged: false,
            missed: false,
            judgment_type: None,
        };
        assert_eq!(note.lane, 3);
        assert_eq!(note.target_time_ms, 1234.5);
        assert_eq!(note.sample_id, Some(7));
        assert!(!note.judged);
        assert!(!note.missed);
        assert_eq!(note.judgment_type, None);
    }

    #[test]
    fn active_note_clone_is_independent() {
        let note = ActiveNote {
            lane: 0,
            target_time_ms: 100.0,
            sample_id: None,
            volume: 1.0,
            pan: 0.0,
            judged: false,
            missed: false,
            judgment_type: None,
        };
        let cloned = note.clone();
        assert_eq!(cloned.lane, note.lane);
        assert_eq!(cloned.target_time_ms, note.target_time_ms);
        assert_eq!(cloned.judgment_type, note.judgment_type);
    }

    #[test]
    fn active_note_sample_id_none_and_some() {
        let with_sample = ActiveNote {
            lane: 0,
            target_time_ms: 0.0,
            sample_id: Some(42),
            volume: 1.0,
            pan: 0.0,
            judged: false,
            missed: false,
            judgment_type: None,
        };
        let without_sample = ActiveNote {
            lane: 0,
            target_time_ms: 0.0,
            sample_id: None,
            volume: 1.0,
            pan: 0.0,
            judged: false,
            missed: false,
            judgment_type: None,
        };
        assert_eq!(with_sample.sample_id, Some(42));
        assert_eq!(without_sample.sample_id, None);
    }

    #[test]
    fn active_note_judgment_type_tracks_result() {
        let mut note = ActiveNote {
            lane: 5,
            target_time_ms: 500.0,
            sample_id: None,
            volume: 1.0,
            pan: 0.0,
            judged: false,
            missed: false,
            judgment_type: None,
        };
        note.judged = true;
        note.judgment_type = Some(JudgmentType::Cool);
        assert!(note.judged);
        assert_eq!(note.judgment_type, Some(JudgmentType::Cool));
    }

    // ===================================================================
    // ActiveLongNote tests
    // ===================================================================

    #[test]
    fn active_long_note_hold_state_flow() {
        let mut ln = ActiveLongNote {
            lane: 2,
            head_time_ms: 1000.0,
            tail_time_ms: 2000.0,
            sample_id: Some(5),
            volume: 0.9,
            pan: 0.3,
            judged: false,
            missed: false,
            holding: false,
            dead: false,
            head_judgment: None,
            tail_judgment: None,
        };
        ln.judged = true;
        ln.head_judgment = Some(JudgmentType::Good);
        ln.holding = true;
        assert!(ln.holding);
        assert_eq!(ln.head_judgment, Some(JudgmentType::Good));

        ln.holding = false;
        ln.dead = true;
        ln.tail_judgment = Some(JudgmentType::Cool);
        assert!(!ln.holding);
        assert!(ln.dead);
        assert_eq!(ln.tail_judgment, Some(JudgmentType::Cool));
    }

    #[test]
    fn active_long_note_miss_head() {
        let mut ln = ActiveLongNote {
            lane: 0,
            head_time_ms: 500.0,
            tail_time_ms: 1500.0,
            sample_id: None,
            volume: 1.0,
            pan: 0.0,
            judged: false,
            missed: false,
            holding: false,
            dead: false,
            head_judgment: None,
            tail_judgment: None,
        };
        ln.missed = true;
        ln.dead = true;
        ln.head_judgment = Some(JudgmentType::Miss);
        assert!(ln.missed);
        assert!(ln.dead);
        assert_eq!(ln.head_judgment, Some(JudgmentType::Miss));
    }

    #[test]
    fn active_long_note_clone_is_independent() {
        let ln = ActiveLongNote {
            lane: 4,
            head_time_ms: 300.0,
            tail_time_ms: 800.0,
            sample_id: Some(1),
            volume: 0.7,
            pan: 0.6,
            judged: false,
            missed: false,
            holding: false,
            dead: false,
            head_judgment: None,
            tail_judgment: None,
        };
        let cloned = ln.clone();
        assert_eq!(cloned.lane, ln.lane);
        assert_eq!(cloned.head_time_ms, ln.head_time_ms);
        assert_eq!(cloned.tail_time_ms, ln.tail_time_ms);
        assert_eq!(cloned.head_judgment, ln.head_judgment);
    }
}
