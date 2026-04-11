//! BGM signal queue with sample-accurate scheduling.
//!
//! Provides a custom [`oddio::Signal`] implementation that mixes multiple
//! BGM notes within a single signal, avoiding the overwrite problem where
//! each note's sample() call would erase the previous note's audio.
//!
//! ## Architecture
//!
//! - **`ScheduledSignal`**: Wraps a single BGM note with a delay (in samples)
//!   before it starts playing. Outputs silence until the delay expires.
//! - **`BgmSignalQueue`**: A single `oddio::Signal` that internally manages
//!   multiple active `ScheduledSignal`s, mixing them by accumulation.
//!
//! ## Why a custom queue?
//!
//! `oddio::Mixer` mixes multiple signals by accumulation, but if we push each
//! BGM note as a separate signal to the mixer, we lose control over when they
//! start relative to the audio clock. `BgmSignalQueue` is a SINGLE signal that
//! the mixer sees as one entity — inside its `sample()` method, it mixes all
//! active notes by adding their samples together.

use std::sync::Arc;

use oddio::{Frames, FramesSignal, Signal};

use crate::audio::manager::StereoFrame;

/// A BGM command sent from the main thread to the audio thread via rtrb.
///
/// Contains everything needed to play a BGM note at a precise moment:
/// the audio samples, when to start (delay_samples), and how loud (volume/pan).
///
/// `source_id` is used for deduplication: when a keysound is pushed for the same
/// lane while the previous one is still playing, the duplicate is silently skipped
/// to prevent phase cancellation from overlapping identical samples.
#[derive(Debug, Clone)]
pub struct BgmCommand {
    /// The audio samples to play.
    pub frames: Arc<Frames<StereoFrame>>,
    /// Number of silence samples to output before the actual audio starts.
    /// This ensures sample-accurate alignment regardless of when the command
    /// was pushed from the main thread.
    pub delay_samples: u32,
    /// Volume gain (1.0 = full volume, 0.0 = silence).
    pub volume: f32,
    /// Pan position (-1.0 = full left, 0.0 = center, +1.0 = full right).
    pub pan: f32,
    /// Source identifier for deduplication (e.g., lane index for keysounds).
    /// When a command with the same source_id arrives while one is still active,
    /// the new one replaces the old instead of accumulating (like a voice steal).
    pub source_id: u64,
}

/// A signal that outputs silence for `delay` samples, then plays the inner signal.
///
/// This ensures BGM notes are perfectly aligned to the sample grid,
/// regardless of when the main loop pushed them.
pub struct ScheduledSignal {
    /// The actual audio signal (wrapped BGM samples).
    inner: FramesSignal<StereoFrame>,
    /// Remaining samples of silence before the audio starts.
    delay: u32,
    /// Volume gain applied to the signal.
    volume: f32,
    /// Pan position used to adjust left/right balance.
    pan: f32,
    /// Source identifier for deduplication.
    source_id: u64,
}

impl ScheduledSignal {
    /// Create a new scheduled signal with a delay before playback.
    ///
    /// # Arguments
    ///
    /// * `frames` - The audio samples to play
    /// * `delay_samples` - Number of silence samples before playback starts
    /// * `volume` - Volume gain (0.0 to 1.0+)
    /// * `pan` - Pan position (-1.0 to +1.0)
    /// * `source_id` - Source identifier for deduplication (e.g., lane index)
    pub fn new(frames: Arc<Frames<StereoFrame>>, delay_samples: u32, volume: f32, pan: f32, source_id: u64) -> Self {
        Self {
            inner: FramesSignal::from(frames),
            delay: delay_samples,
            volume,
            pan,
            source_id,
        }
    }

    /// Returns true if this signal has fully played (delay exhausted and inner
    /// signal done).
    /// Delegates to the inner signal's `is_finished()` once the delay period has passed.
    pub fn is_finished(&self) -> bool {
        self.delay == 0 && self.inner.is_finished()
    }
}

impl Signal for ScheduledSignal {
    type Frame = StereoFrame;

    fn sample(&mut self, dt: f32, out: &mut [StereoFrame]) {
        let out_len = out.len() as u32;

        if self.delay >= out_len {
            // Entire chunk is silence
            for frame in out.iter_mut() {
                *frame = [0.0; 2];
            }
            self.delay -= out_len;
        } else {
            // Partial silence, then audio
            let silence_len = self.delay as usize;

            // Output silence
            for frame in out[..silence_len].iter_mut() {
                *frame = [0.0; 2];
            }

            // Output audio (with volume/pan applied)
            if silence_len < out.len() {
                self.inner.sample(dt, &mut out[silence_len..]);

                // Apply volume and pan to the audio portion
                for frame in out[silence_len..].iter_mut() {
                    frame[0] *= self.volume;
                    frame[1] *= self.volume;

                    let (left_gain, right_gain) = if self.pan < 0.0 {
                        (1.0, 1.0 + self.pan)
                    } else {
                        (1.0 - self.pan, 1.0)
                    };

                    frame[0] *= left_gain;
                    frame[1] *= right_gain;
                }
            }

            self.delay = 0;
        }
    }
}

/// A single `oddio::Signal` that internally manages and mixes multiple BGM notes.
///
/// This is the key to fixing the "overwrite" bug: instead of each BGM note
/// being a separate signal that overwrites the output buffer, this queue
/// accumulates all active notes' samples together in its `sample()` method.
///
/// ## How it works
///
/// 1. Main thread pushes `BgmCommand`s via `push_command()` (lock-free SPSC queue)
/// 2. Audio thread drains the queue in `sample()` and creates `ScheduledSignal`s
/// 3. Each active signal's `sample()` is called, accumulating into the output buffer
/// 4. Finished signals are removed from the active list
///
/// ## Thread safety
///
/// The `rtrb` SPSC queue ensures lock-free, real-time safe communication:
/// - Producer (main thread): pushes commands via `Producer<BgmCommand>`
/// - Consumer (audio thread): drains commands via `Consumer<BgmCommand>`
pub struct BgmSignalQueue {
    /// Active signals currently playing.
    active_signals: Vec<ScheduledSignal>,
    /// The lock-free consumer end of the rtrb queue (set by audio thread).
    consumer: Option<rtrb::Consumer<BgmCommand>>,
    /// Pre-allocated buffer to avoid allocations in sample()
    temp_buffer: Vec<StereoFrame>,
}

impl BgmSignalQueue {
    /// Create a new empty BGM signal queue.
    pub fn new() -> Self {
        Self {
            active_signals: Vec::new(),
            consumer: None,
            temp_buffer: Vec::with_capacity(8192),
        }
    }

    /// Set the consumer end of the rtrb queue. Must be called before the audio callback starts.
    ///
    /// This is called once during initialization to connect the audio thread's
    /// consumer to the queue. After this, the queue will drain commands pushed
    /// by the main thread's producer.
    pub fn set_consumer(&mut self, consumer: rtrb::Consumer<BgmCommand>) {
        self.consumer = Some(consumer);
    }

    /// Drain pending commands from the rtrb queue and schedule them as signals.
    ///
    /// This is called from the audio thread's `sample()` method to pick up
    /// any new BGM commands that the main thread has pushed.
    ///
    /// **Deduplication**: if a command arrives with a `source_id` that matches an
    /// already-active signal, the old signal is removed and the new one takes its
    /// place. This prevents phase cancellation from overlapping identical samples
    /// (e.g. rapid key presses on the same lane).
    fn drain_commands(&mut self) {
        if let Some(consumer) = &mut self.consumer {
            while let Ok(cmd) = consumer.pop() {
                // Remove any existing signal with the same source_id (voice steal)
                self.active_signals.retain(|s| s.source_id != cmd.source_id);

                let signal = ScheduledSignal::new(cmd.frames, cmd.delay_samples, cmd.volume, cmd.pan, cmd.source_id);
                self.active_signals.push(signal);
            }
        }
    }
}

impl Default for BgmSignalQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl Signal for BgmSignalQueue {
    type Frame = StereoFrame;

    fn sample(&mut self, dt: f32, out: &mut [StereoFrame]) {
        // Drain any new commands from the main thread
        self.drain_commands();

        // Clear output buffer to silence
        for frame in out.iter_mut() {
            *frame = [0.0; 2];
        }

        // If no active signals, we're done
        if self.active_signals.is_empty() {
            return;
        }

        // Ensure temp buffer is large enough
        let out_len = out.len();
        if self.temp_buffer.len() < out_len {
            self.temp_buffer.resize(out_len, [0.0; 2]);
        }

        // Mix all active signals by accumulation
        let mut finished_indices = Vec::new();

        for (i, signal) in self.active_signals.iter_mut().enumerate() {
            // Sample into temp buffer
            let temp = &mut self.temp_buffer[..out_len];
            signal.sample(dt, temp);

            // Accumulate into output buffer
            for (out_frame, temp_frame) in out.iter_mut().zip(temp.iter()) {
                out_frame[0] += temp_frame[0];
                out_frame[1] += temp_frame[1];
            }

            // Remove signals that have fully played
            if signal.is_finished() {
                finished_indices.push(i);
            }
        }

        // Remove finished signals (in reverse order to avoid index shifting)
        for &idx in finished_indices.iter().rev() {
            self.active_signals.remove(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduled_signal_initial_silence() {
        let frames = Frames::from_slice(44100, &[[0.5f32, 0.5f32]; 100]);
        let mut signal = ScheduledSignal::new(frames, 10, 1.0, 0.0, 1);

        // First 10 samples should be silence
        let mut out = vec![[0.0f32; 2]; 10];
        signal.sample(1.0 / 44100.0, &mut out);
        assert!(out.iter().all(|f| f[0] == 0.0 && f[1] == 0.0), "First 10 samples should be silence");
        assert_eq!(signal.delay, 0, "Delay should be 0 after first call");

        // Next call: delay is 0, should output audio from the start
        let mut out = vec![[0.0f32; 2]; 10];
        signal.sample(1.0 / 44100.0, &mut out);
        // The inner FramesSignal should output from position 0
        eprintln!("Second output: {:?}", &out[..3]);
        assert!(out.iter().all(|f| f[0] == 0.5 && f[1] == 0.5), "Second 10 samples should be audio");
    }

    #[test]
    fn test_scheduled_signal_pan_left() {
        let frames = Frames::from_slice(44100, &[[1.0f32, 1.0f32]; 10]);
        let mut signal = ScheduledSignal::new(frames, 0, 1.0, -1.0, 1); // Full left

        let mut out = vec![[0.0f32; 2]; 10];
        signal.sample(1.0 / 44100.0, &mut out);

        // Left channel should be full, right should be silent
        assert!(out.iter().all(|f| f[0] == 1.0 && f[1] == 0.0));
    }

    #[test]
    fn test_scheduled_signal_pan_right() {
        let frames = Frames::from_slice(44100, &[[1.0f32, 1.0f32]; 10]);
        let mut signal = ScheduledSignal::new(frames, 0, 1.0, 1.0, 1); // Full right

        let mut out = vec![[0.0f32; 2]; 10];
        signal.sample(1.0 / 44100.0, &mut out);

        // Left channel should be silent, right should be full
        assert!(out.iter().all(|f| f[0] == 0.0 && f[1] == 1.0));
    }

    #[test]
    fn test_bgm_signal_queue_mixing() {
        use rtrb::RingBuffer;

        let (mut prod, cons) = RingBuffer::new(1024);

        // Create two overlapping audio signals
        let frames1 = Frames::from_slice(44100, &[[0.5f32, 0.0f32]; 100]); // Left only
        let frames2 = Frames::from_slice(44100, &[[0.0f32, 0.5f32]; 100]); // Right only

        // Push both with no delay (immediate playback)
        prod.push(BgmCommand {
            frames: frames1,
            delay_samples: 0,
            volume: 1.0,
            pan: 0.0,
            source_id: 1,
        })
        .unwrap();

        prod.push(BgmCommand {
            frames: frames2,
            delay_samples: 0,
            volume: 1.0,
            pan: 0.0,
            source_id: 2,
        })
        .unwrap();

        // Create queue and set consumer
        let mut queue = BgmSignalQueue::new();
        queue.set_consumer(cons);

        // Sample the queue
        let mut out = vec![[0.0f32; 2]; 10];
        queue.sample(1.0 / 44100.0, &mut out);

        // Both signals should be mixed (both channels active)
        assert!(out.iter().all(|f| f[0] == 0.5 && f[1] == 0.5));
    }
}
