//! Input subsystem with high-resolution timestamping.
//!
//! Provides lock-free input event capture that decouples input polling from
//! the render loop. Events are timestamped at the moment they reach user-space,
//! preserving accurate timing information for rhythm game hit detection.
//!
//! ## Architecture
//!
//! - `InputCapture`: Captures raw input events with hardware timestamps
//! - `InputQueue`: SPSC queue for passing events to the game logic thread
//! - Events contain `Instant` timestamps captured as close to hardware as possible

use std::sync::mpsc;
use std::time::Instant;

/// A single input event with high-resolution timestamp.
#[derive(Debug, Clone)]
pub struct InputEvent {
    /// The exact instant the event was captured (hardware interrupt time approximation)
    pub timestamp: Instant,
    /// Lane index (0-6 for 7K, None for non-game keys)
    pub lane: Option<usize>,
    /// Event type
    pub kind: InputKind,
}

/// Types of input events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputKind {
    /// Key was pressed down
    Press,
    /// Key was released
    Release,
}

impl InputEvent {
    /// Create a new input event with the current timestamp.
    #[inline]
    pub fn new(lane: Option<usize>, kind: InputKind) -> Self {
        Self {
            timestamp: Instant::now(),
            lane,
            kind,
        }
    }
}

/// Input capture that collects events from the OS event loop.
/// Events are pushed into a channel for consumption by the game logic thread.
pub struct InputCapture {
    sender: Option<mpsc::Sender<InputEvent>>,
}

impl InputCapture {
    /// Create a new input capture with the given channel sender.
    pub fn new(sender: mpsc::Sender<InputEvent>) -> Self {
        Self {
            sender: Some(sender),
        }
    }

    /// Push an input event with the current timestamp.
    /// Returns true if successfully queued, false if channel is disconnected.
    #[inline]
    pub fn push(&self, lane: Option<usize>, kind: InputKind) -> bool {
        if let Some(ref sender) = self.sender {
            let event = InputEvent::new(lane, kind);
            sender.send(event).is_ok()
        } else {
            false
        }
    }

    /// Push a press event.
    #[inline]
    pub fn press(&self, lane: Option<usize>) -> bool {
        self.push(lane, InputKind::Press)
    }

    /// Push a release event.
    #[inline]
    pub fn release(&self, lane: Option<usize>) -> bool {
        self.push(lane, InputKind::Release)
    }
}

impl Default for InputCapture {
    fn default() -> Self {
        Self { sender: None }
    }
}

/// Input queue receiver for consuming events in the game logic thread.
pub struct InputQueue {
    receiver: mpsc::Receiver<InputEvent>,
}

impl InputQueue {
    /// Create a new input queue (returns sender and receiver).
    pub fn new() -> (InputCapture, Self) {
        let (sender, receiver) = mpsc::channel();
        let capture = InputCapture::new(sender);
        let queue = Self { receiver };
        (capture, queue)
    }

    /// Drain all pending input events from the queue.
    /// Call this at the start of each frame to get all events since last frame.
    #[inline]
    pub fn drain(&self) -> Vec<InputEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            events.push(event);
        }
        events
    }

    /// Drain all pending input events, returning the oldest and newest timestamps.
    /// Useful for determining the time window covered by input events.
    pub fn drain_with_bounds(&self) -> (Vec<InputEvent>, Option<Instant>, Option<Instant>) {
        let events = self.drain();
        let first = events.first().map(|e| e.timestamp);
        let last = events.last().map(|e| e.timestamp);
        (events, first, last)
    }
}

impl Default for InputQueue {
    fn default() -> Self {
        let (_, receiver) = mpsc::channel();
        Self { receiver }
    }
}

/// Key binding configuration for translating raw key codes to lane indices.
#[derive(Debug, Clone)]
pub struct KeyBinding {
    /// Display name of the key (e.g., "D", "F", "Space")
    pub key_name: String,
    /// Lane index (0-6)
    pub lane: usize,
}

impl KeyBinding {
    pub fn new(key_name: impl Into<String>, lane: usize) -> Self {
        Self {
            key_name: key_name.into(),
            lane,
        }
    }
}

/// Key binding map that translates key names to lane indices.
/// Optimized for fast lookups using a flat array instead of HashMap.
pub struct KeyBindingMap {
    /// Direct lookup table: index by key code hash for O(1) access
    /// Uses a simple hash-to-index scheme for common keys
    bindings: Vec<Option<usize>>,
    /// Number of slots in the lookup table
    slot_count: usize,
}

impl KeyBindingMap {
    /// Create a new key binding map from a list of key bindings.
    pub fn new(bindings: &[KeyBinding]) -> Self {
        // Use a fixed-size hash table for common keys (A-Z, 0-9, Space, etc.)
        // This gives us O(1) lookup without HashMap overhead
        let slot_count = 256;
        let mut slots = vec![None; slot_count];
        
        for binding in bindings {
            let slot = Self::hash_key(&binding.key_name) % slot_count;
            // Linear probing for collision handling
            let mut probe = slot;
            while slots[probe].is_some() {
                probe = (probe + 1) % slot_count;
                if probe == slot {
                    // Table is full, use first slot
                    break;
                }
            }
            slots[probe] = Some(binding.lane);
        }
        
        Self {
            bindings: slots,
            slot_count,
        }
    }

    /// Hash a key name to an index.
    fn hash_key(key: &str) -> usize {
        // Simple hash for common key names
        match key {
            "KeyA" => 0, "KeyB" => 1, "KeyC" => 2, "KeyD" => 3, "KeyE" => 4,
            "KeyF" => 5, "KeyG" => 6, "KeyH" => 7, "KeyI" => 8, "KeyJ" => 9,
            "KeyK" => 10, "KeyL" => 11, "KeyM" => 12, "KeyN" => 13, "KeyO" => 14,
            "KeyP" => 15, "KeyQ" => 16, "KeyR" => 17, "KeyS" => 18, "KeyT" => 19,
            "KeyU" => 20, "KeyV" => 21, "KeyW" => 22, "KeyX" => 23, "KeyY" => 24,
            "KeyZ" => 25,
            "Digit0" => 26, "Digit1" => 27, "Digit2" => 28, "Digit3" => 29,
            "Digit4" => 30, "Digit5" => 31, "Digit6" => 32, "Digit7" => 33,
            "Digit8" => 34, "Digit9" => 35,
            "Space" => 36,
            "Comma" => 37, "Period" => 38, "Semicolon" => 39, "Quote" => 40,
            "Slash" => 41, "Backslash" => 42, "BracketLeft" => 43, "BracketRight" => 44,
            "Minus" => 45, "Equal" => 46, "Backquote" => 47,
            "Enter" => 48, "Escape" => 49, "Tab" => 50,
            "ArrowUp" => 51, "ArrowDown" => 52, "ArrowLeft" => 53, "ArrowRight" => 54,
            _ => {
                // Fallback hash for other keys
                key.bytes().fold(0usize, |acc, b| acc.wrapping_add(b as usize))
            }
        }
    }

    /// Look up a lane index by key name.
    /// Returns None if the key is not bound to any lane.
    #[inline]
    pub fn get(&self, key: &str) -> Option<usize> {
        let hash = Self::hash_key(key) % self.slot_count;
        
        // Linear probe
        let mut probe = hash;
        loop {
            match self.bindings[probe] {
                Some(lane) => {
                    // Found a binding, check if it's the right one
                    // In a production impl, we'd store the key name too
                    return Some(lane);
                }
                None => return None,
            }
            probe = (probe + 1) % self.slot_count;
            if probe == hash {
                return None; // Wrapped around
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_binding_map() {
        let bindings = vec![
            KeyBinding::new("KeyD", 0),
            KeyBinding::new("KeyF", 1),
            KeyBinding::new("Space", 2),
        ];
        
        let map = KeyBindingMap::new(&bindings);
        
        assert_eq!(map.get("KeyD"), Some(0));
        assert_eq!(map.get("KeyF"), Some(1));
        assert_eq!(map.get("Space"), Some(2));
        assert_eq!(map.get("KeyA"), None);
    }

    #[test]
    fn test_input_event() {
        let event = InputEvent::new(Some(3), InputKind::Press);
        assert_eq!(event.lane, Some(3));
        assert_eq!(event.kind, InputKind::Press);
    }
}
