//! Default keyboard bindings for the 7 note lanes.
//!
//! Keys can be remapped in the future via a config file.
//! For now, these are hardcoded defaults matching O2Jam's layout.

use winit::keyboard::{Key, NamedKey};

/// Map a winit Key event to a lane index (0-6).
/// Returns None if the key is not bound to any lane.
///
/// Default bindings:
///   Lane 1: S
///   Lane 2: D
///   Lane 3: F
///   Lane 4: Space
///   Lane 5: J
///   Lane 6: K
///   Lane 7: L
pub fn key_to_lane(key: &Key) -> Option<usize> {
    match key {
        Key::Character(c) => match c.as_str() {
            "s" | "S" => Some(0), // Lane 1
            "d" | "D" => Some(1), // Lane 2
            "f" | "F" => Some(2), // Lane 3
            "j" | "J" => Some(4), // Lane 5
            "k" | "K" => Some(5), // Lane 6
            "l" | "L" => Some(6), // Lane 7
            _ => None,
        },
        Key::Named(NamedKey::Space) => Some(3), // Lane 4
        _ => None,
    }
}
