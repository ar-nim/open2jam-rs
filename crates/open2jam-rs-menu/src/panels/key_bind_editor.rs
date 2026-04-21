//! Key binding editor panel — press-to-capture key bindings (always K7 layout).

use open2jam_rs_core::Config;

/// Lane label for the K7 layout.
const K7_LANE_NAMES: [&str; 7] = [
    "Lane 1", "Lane 2", "Lane 3", "Lane 4", "Lane 5", "Lane 6", "Lane 7",
];

/// State for the key capture flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCaptureState {
    /// Not capturing any key.
    Idle,
    /// Waiting for the user to press a key for the given lane index.
    Listening(usize),
}

/// Convert a winit key description (as displayed by egui) into the canonical
/// storage string used by `KeyMap.key`.
///
/// This mirrors how winit serialises `Key` / `NamedKey` to strings so that the
/// game's `key_to_lane` parser can round-trip it.
pub fn key_display_to_storage(key_text: &str) -> String {
    // winit named keys we care about
    match key_text {
        "Space" => "Space".to_owned(),
        "Enter" => "Enter".to_owned(),
        "Escape" => "Escape".to_owned(),
        "Tab" => "Tab".to_owned(),
        "Backspace" => "Backspace".to_owned(),
        "Delete" => "Delete".to_owned(),
        "Insert" => "Insert".to_owned(),
        "ArrowUp" => "ArrowUp".to_owned(),
        "ArrowDown" => "ArrowDown".to_owned(),
        "ArrowLeft" => "ArrowLeft".to_owned(),
        "ArrowRight" => "ArrowRight".to_owned(),
        "Home" => "Home".to_owned(),
        "End" => "End".to_owned(),
        "PageUp" => "PageUp".to_owned(),
        "PageDown" => "PageDown".to_owned(),
        "Shift" | "ShiftLeft" | "ShiftRight" => "Shift".to_owned(),
        "Control" | "ControlLeft" | "ControlRight" => "Control".to_owned(),
        "Alt" | "AltLeft" | "AltRight" => "Alt".to_owned(),
        // Character keys: winit displays them as "A", "1", "Comma", etc.
        // We store them as "KeyA", "Digit1", "Comma", matching winit's PhysicalKey::Code names.
        _ if key_text.len() == 1 && key_text.chars().next().unwrap().is_ascii_alphabetic() => {
            format!("Key{}", key_text.to_uppercase())
        }
        // Digit keys: "1" → "Digit1"
        _ if key_text.len() == 1 && key_text.chars().next().unwrap().is_ascii_digit() => {
            format!("Digit{}", key_text)
        }
        // Punctuation that winit names directly (Comma, Period, Minus, etc.)
        _ => key_text.to_owned(),
    }
}

pub fn ui_key_bind_editor(ui: &mut egui::Ui, config: &mut Config, capture: &mut KeyCaptureState) {
    ui.label(egui::RichText::new("Keyboard Configuration").strong());
    ui.label("Click a key slot, then press the key you want to bind.");

    ui.separator();

    egui::Grid::new("key_grid").striped(true).show(ui, |ui| {
        ui.label(egui::RichText::new("Key").strong());
        ui.label(egui::RichText::new("Lane").strong());
        ui.end_row();

        let lanes = &config.key_bindings.k7.lanes;
        let num_lanes = lanes.len().min(K7_LANE_NAMES.len());
        for i in 0..num_lanes {
            let current_key = &lanes[i].key;
            let is_listening = matches!(capture, KeyCaptureState::Listening(l) if *l == i);

            // Button shows current binding, or "Press a key..." when capturing
            let label = if is_listening {
                "⟳ Press a key..."
            } else {
                current_key.as_str()
            };

            let btn = ui.button(label);
            if btn.clicked() {
                if is_listening {
                    // Cancel capture
                    *capture = KeyCaptureState::Idle;
                } else {
                    *capture = KeyCaptureState::Listening(i);
                }
            }

            ui.label(K7_LANE_NAMES[i]);
            ui.end_row();
        }
    });
}

/// Process a raw keyboard input event. If we're in listening mode for a lane,
/// capture the key and update the config.
///
/// Returns `true` if the event was consumed (a key was captured).
pub fn handle_key_capture(
    key_text: &str,
    capture: &mut KeyCaptureState,
    config: &mut Config,
) -> bool {
    let lane = match capture {
        KeyCaptureState::Listening(l) => *l,
        KeyCaptureState::Idle => return false,
    };

    let storage_key = key_display_to_storage(key_text);

    // Safety: only update if the lane index is valid for the K7 layout
    if lane < config.key_bindings.k7.lanes.len() {
        config.key_bindings.k7.lanes[lane].key = storage_key;
    }
    *capture = KeyCaptureState::Idle;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── key_display_to_storage ──────────────────────────────────────────────

    #[test]
    fn test_alpha_char_becomes_key_prefix() {
        assert_eq!(key_display_to_storage("a"), "KeyA");
        assert_eq!(key_display_to_storage("Z"), "KeyZ");
        assert_eq!(key_display_to_storage("m"), "KeyM");
    }

    #[test]
    fn test_digit_becomes_digit_prefix() {
        assert_eq!(key_display_to_storage("1"), "Digit1");
        assert_eq!(key_display_to_storage("9"), "Digit9");
    }

    #[test]
    fn test_named_keys_preserved() {
        assert_eq!(key_display_to_storage("Space"), "Space");
        assert_eq!(key_display_to_storage("Enter"), "Enter");
        assert_eq!(key_display_to_storage("Escape"), "Escape");
        assert_eq!(key_display_to_storage("ShiftLeft"), "Shift");
        assert_eq!(key_display_to_storage("ControlRight"), "Control");
    }

    #[test]
    fn test_punctuation_passed_through() {
        assert_eq!(key_display_to_storage("Comma"), "Comma");
        assert_eq!(key_display_to_storage("Semicolon"), "Semicolon");
        assert_eq!(key_display_to_storage("Minus"), "Minus");
    }

    // ── KeyCaptureState transitions ──────────────────────────────────────────

    #[test]
    fn test_capture_transitions_from_idle_to_listening() {
        let mut capture = KeyCaptureState::Idle;
        // Simulate clicking lane 2 button
        let lane_to_capture = 2;
        if matches!(capture, KeyCaptureState::Idle) {
            capture = KeyCaptureState::Listening(lane_to_capture);
        }
        assert_eq!(capture, KeyCaptureState::Listening(2));
    }

    #[test]
    fn test_capture_cancels_on_click_while_listening() {
        let mut capture = KeyCaptureState::Listening(3);
        // User clicks same button again → cancel
        if let KeyCaptureState::Listening(l) = capture {
            if l == 3 {
                capture = KeyCaptureState::Idle;
            }
        }
        assert_eq!(capture, KeyCaptureState::Idle);
    }

    // ── handle_key_capture ──────────────────────────────────────────────────

    #[test]
    fn test_handle_key_capture_when_idle_does_nothing() {
        let mut config = Config::default();
        let original = config.key_bindings.k7.lanes[0].key.clone();
        let mut capture = KeyCaptureState::Idle;

        let consumed = handle_key_capture("a", &mut capture, &mut config);

        assert!(!consumed);
        assert_eq!(config.key_bindings.k7.lanes[0].key, original);
        assert_eq!(capture, KeyCaptureState::Idle);
    }

    #[test]
    fn test_handle_key_capture_stores_correct_key() {
        let mut config = Config::default();
        let mut capture = KeyCaptureState::Listening(0);

        let consumed = handle_key_capture("a", &mut capture, &mut config);

        assert!(consumed);
        assert_eq!(config.key_bindings.k7.lanes[0].key, "KeyA");
        assert_eq!(capture, KeyCaptureState::Idle);
    }

    #[test]
    fn test_handle_key_capture_for_space() {
        let mut config = Config::default();
        let mut capture = KeyCaptureState::Listening(3);

        let consumed = handle_key_capture("Space", &mut capture, &mut config);

        assert!(consumed);
        assert_eq!(config.key_bindings.k7.lanes[3].key, "Space");
        assert_eq!(capture, KeyCaptureState::Idle);
    }

    #[test]
    fn test_handle_key_capture_for_digit() {
        let mut config = Config::default();
        let mut capture = KeyCaptureState::Listening(5);

        let consumed = handle_key_capture("7", &mut capture, &mut config);

        assert!(consumed);
        assert_eq!(config.key_bindings.k7.lanes[5].key, "Digit7");
        assert_eq!(capture, KeyCaptureState::Idle);
    }

    #[test]
    fn test_handle_key_capture_ignores_invalid_lane() {
        let mut config = Config::default();
        let mut capture = KeyCaptureState::Listening(99);

        // Should not panic, should reset to idle
        let consumed = handle_key_capture("a", &mut capture, &mut config);

        assert!(consumed);
        assert_eq!(capture, KeyCaptureState::Idle);
    }
}
