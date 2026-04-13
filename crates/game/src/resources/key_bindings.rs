//! Keyboard key bindings for the 7 note lanes, driven by user config.
//!
//! Builds a key-to-lane mapping from the config's K7 layout at startup,
//! supporting any winit key the user configures in the menu.

use winit::keyboard::{Key, NamedKey};

/// Build a key-to-lane mapping from the config's K7 layout.
///
/// The config stores key names as winit-style strings:
///   - Character keys: "KeyA".."KeyZ", "Digit0".."Digit9", "Comma", "Period", etc.
///   - Named keys: "Space", "Enter", "Escape", "Shift", "Control", "Alt", etc.
///
/// Returns a function that maps winit Key events to lane indices (0-6).
pub fn build_key_to_lane(config_keys: &[String]) -> impl Fn(&Key) -> Option<usize> + '_ {
    // Build a lookup: key_name -> lane_index
    let mapping: Vec<(String, usize)> = config_keys
        .iter()
        .enumerate()
        .map(|(lane, key_name)| (key_name.clone(), lane))
        .collect();

    move |key: &Key| -> Option<usize> {
        // Try matching by character first (case-insensitive)
        if let Key::Character(c) = key {
            let lower = c.to_lowercase();
            for (name, lane) in &mapping {
                let n = name.to_lowercase();
                if n == lower {
                    return Some(*lane);
                }
            }
        }

        // Try matching by named key
        if let Key::Named(named) = key {
            let named_str = named_key_to_str(named);
            for (name, lane) in &mapping {
                let n = name.to_lowercase();
                // Support both exact match and common aliases
                if n == named_str
                    || named_aliases_match(&n, &named_str)
                {
                    return Some(*lane);
                }
            }
        }

        None
    }
}

/// Convert a winit NamedKey to its canonical string representation.
fn named_key_to_str(named: &NamedKey) -> String {
    match named {
        NamedKey::Space => "Space",
        NamedKey::Enter => "Enter",
        NamedKey::Escape => "Escape",
        NamedKey::Tab => "Tab",
        NamedKey::Backspace => "Backspace",
        NamedKey::Delete => "Delete",
        NamedKey::Insert => "Insert",
        NamedKey::ArrowUp => "ArrowUp",
        NamedKey::ArrowDown => "ArrowDown",
        NamedKey::ArrowLeft => "ArrowLeft",
        NamedKey::ArrowRight => "ArrowRight",
        NamedKey::Home => "Home",
        NamedKey::End => "End",
        NamedKey::PageUp => "PageUp",
        NamedKey::PageDown => "PageDown",
        NamedKey::Shift => "Shift",
        NamedKey::Control => "Control",
        NamedKey::Alt => "Alt",
        NamedKey::F1 => "F1",
        NamedKey::F2 => "F2",
        NamedKey::F3 => "F3",
        NamedKey::F4 => "F4",
        NamedKey::F5 => "F5",
        NamedKey::F6 => "F6",
        NamedKey::F7 => "F7",
        NamedKey::F8 => "F8",
        NamedKey::F9 => "F9",
        NamedKey::F10 => "F10",
        NamedKey::F11 => "F11",
        NamedKey::F12 => "F12",
        _ => "Unknown",
    }
    .to_string()
}

/// Check if two key name strings are aliases of each other.
fn named_aliases_match(a: &str, b: &str) -> bool {
    let pairs = [
        ("shift", "shiftleft"),
        ("shift", "shiftright"),
        ("control", "controll"),
        ("control", "controlr"),
        ("alt", "altleft"),
        ("alt", "altright"),
    ];
    pairs.iter().any(|(x, y)| {
        (a == *x && b == *y) || (a == *y && b == *x)
    })
}
