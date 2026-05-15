//! Note and effect cleanup routines.

use super::GameState;

impl GameState {
    /// Remove notes that are well past the judgment line and no longer hittable.
    pub fn cleanup_notes(&mut self) {
        let render_time = self.clock.render_time() as f64;
        let bpm = self.clock.bpm() as f64;
        let bad_window = crate::gameplay::judgment::bad_window_ms_tap(bpm);
        let cleanup_threshold = render_time - bad_window - 100.0;

        let mut write_idx = 0;
        for read_idx in 0..self.active_notes.len() {
            if self.active_notes[read_idx].target_time_ms >= cleanup_threshold {
                if write_idx != read_idx {
                    self.active_notes.swap(write_idx, read_idx);
                }
                write_idx += 1;
            }
        }
        self.active_notes.truncate(write_idx);

        let bad_window_release = crate::gameplay::judgment::bad_window_ms_release(bpm);
        let mut write_idx = 0;
        for read_idx in 0..self.active_long_notes.len() {
            let ln = &self.active_long_notes[read_idx];
            if (render_time - ln.tail_time_ms) <= bad_window_release + 100.0 {
                if write_idx != read_idx {
                    self.active_long_notes.swap(write_idx, read_idx);
                }
                write_idx += 1;
            }
        }
        self.active_long_notes.truncate(write_idx);
    }

    /// Remove expired visual effects.
    pub fn cleanup_effects(&mut self) {
        let render_time = self.clock.render_time() as f64;
        self.note_click_effects.retain(|e| e.is_active(render_time));
        self.long_flare_effects.retain(|e| e.is_active(render_time));
    }
}
