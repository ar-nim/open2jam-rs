use std::collections::HashMap;

use crate::types::LaneIndex;

pub fn build_key_mapping(
    keys: &[open2jam_rs_core::key_bindings::KeyMap],
) -> HashMap<String, LaneIndex> {
    let mut map = HashMap::new();
    for (lane, key_map) in keys.iter().enumerate() {
        if let Some(lane_idx) = LaneIndex::new(lane as u8) {
            map.insert(key_map.key.clone(), lane_idx);
        }
    }
    log::info!("Key bindings loaded: {:?}", map);
    map
}

pub fn config_key_for_character(c: &str) -> &str {
    if c.len() == 1 {
        let ch = c.chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            return match ch.to_ascii_uppercase() {
                'A' => "KeyA",
                'B' => "KeyB",
                'C' => "KeyC",
                'D' => "KeyD",
                'E' => "KeyE",
                'F' => "KeyF",
                'G' => "KeyG",
                'H' => "KeyH",
                'I' => "KeyI",
                'J' => "KeyJ",
                'K' => "KeyK",
                'L' => "KeyL",
                'M' => "KeyM",
                'N' => "KeyN",
                'O' => "KeyO",
                'P' => "KeyP",
                'Q' => "KeyQ",
                'R' => "KeyR",
                'S' => "KeyS",
                'T' => "KeyT",
                'U' => "KeyU",
                'V' => "KeyV",
                'W' => "KeyW",
                'X' => "KeyX",
                'Y' => "KeyY",
                'Z' => "KeyZ",
                _ => c,
            };
        }
        if ch.is_ascii_digit() {
            return match ch {
                '0' => "Digit0",
                '1' => "Digit1",
                '2' => "Digit2",
                '3' => "Digit3",
                '4' => "Digit4",
                '5' => "Digit5",
                '6' => "Digit6",
                '7' => "Digit7",
                '8' => "Digit8",
                '9' => "Digit9",
                _ => c,
            };
        }
        return match ch {
            ',' => "Comma",
            '.' => "Period",
            ';' => "Semicolon",
            '\'' => "Quote",
            '/' => "Slash",
            '\\' => "Backslash",
            '[' => "BracketLeft",
            ']' => "BracketRight",
            '-' => "Minus",
            '=' => "Equal",
            '`' => "Backquote",
            ' ' => "Space",
            _ => c,
        };
    }
    c
}
