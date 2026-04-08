//! Audio manager — oddio mixer hooked to cpal output stream.
//!
//! **Hybrid Phase-Locked Clock:**
//! The audio hardware is the sovereign authority. An atomic `samples_played`
//! counter is incremented in every cpal callback, and the `Instant` of each
//! callback is recorded. Visual time is computed as:
//!   T_visual = (samples_played / sample_rate) + (Instant_now - Instant_callback)
//! providing continuous, monotonic time for rendering while remaining phase-locked
//! to the discrete steps of the audio buffer.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{info, warn};
use oddio::{Mixer, MixerControl};

pub type StereoFrame = [f32; 2];

/// Shared state between the main thread and the cpal audio callback.
/// All fields are lock-free atomics so they can be safely read/written
/// from both threads.
pub struct AudioState {
    pub sample_rate: AtomicU32,
    pub active: AtomicBool,
    /// Total stereo frames (samples) consumed by the cpal callback.
    /// This is the authoritative "audio clock" — it only ever increases.
    pub samples_played: AtomicU64,
    /// The `Instant` (as elapsed nanoseconds) of the most recent cpal callback.
    /// Used together with `samples_played` to interpolate visual time.
    pub last_callback_instant: AtomicU64,
    /// A stable reference instant captured once at stream start.
    /// All `last_callback_instant` values are relative to this via `.elapsed().as_nanos()`.
    pub callback_token: AtomicU64,
}

pub struct AudioManager {
    mixer: Option<MixerControl<StereoFrame>>,
    _stream: Option<cpal::Stream>,
    state: Arc<AudioState>,
    active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioPlayError {
    #[error("AudioManager is not available")]
    NoManager,
}

impl AudioManager {
    pub fn new() -> Self {
        match Self::init() {
            Ok((mixer, stream, state)) => {
                info!("AudioManager initialised (oddio + cpal).");
                Self {
                    mixer: Some(mixer),
                    _stream: Some(stream),
                    state,
                    active: true,
                }
            }
            Err(e) => {
                warn!("AudioManager init failed: {}", e);
                Self {
                    mixer: None,
                    _stream: None,
                    state: Arc::new(AudioState {
                        sample_rate: AtomicU32::new(44100),
                        active: AtomicBool::new(false),
                        samples_played: AtomicU64::new(0),
                        last_callback_instant: AtomicU64::new(0),
                        callback_token: AtomicU64::new(0),
                    }),
                    active: false,
                }
            }
        }
    }

    fn init() -> anyhow::Result<(MixerControl<StereoFrame>, cpal::Stream, Arc<AudioState>)> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No default output device"))?;

        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate();
        let channels = config.channels();
        info!(
            "Audio device: rate={}, channels={}, format={:?}",
            sample_rate, channels, config.sample_format()
        );

        let (mixer_control, mut mixer) = Mixer::<StereoFrame>::new();

        // Capture a stable reference instant before stream creation.
        // All callback timestamps will be relative to this via `.elapsed().as_nanos()`.
        let callback_token = Instant::now();

        let state = Arc::new(AudioState {
            sample_rate: AtomicU32::new(sample_rate),
            active: AtomicBool::new(true),
            samples_played: AtomicU64::new(0),
            last_callback_instant: AtomicU64::new(0),
            callback_token: AtomicU64::new(callback_token.elapsed().as_nanos() as u64),
        });

        let stream_config = cpal::StreamConfig {
            sample_rate,
            channels,
            buffer_size: cpal::BufferSize::Fixed(256),
        };

        // Helper: record frames played and timestamp into the shared state.
        let record_callback = move |frames_len: usize, state: &Arc<AudioState>| {
            state
                .samples_played
                .fetch_add(frames_len as u64, Ordering::Relaxed);
            let now_ns = callback_token.elapsed().as_nanos() as u64;
            state
                .last_callback_instant
                .store(now_ns, Ordering::Relaxed);
        };

        let state_for_f32 = Arc::clone(&state);
        let state_for_i16 = Arc::clone(&state);
        let state_for_u16 = Arc::clone(&state);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = oddio::frame_stereo(data);
                    oddio::run(&mut mixer, sample_rate, frames);
                    record_callback(frames.len(), &state_for_f32);
                },
                |err| warn!("Audio error: {}", err),
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let mut buf = [0.0f32; 8192];
                    let len = data.len().min(buf.len());
                    let frames = oddio::frame_stereo(&mut buf[..len]);
                    let frame_count = frames.len();
                    oddio::run(&mut mixer, sample_rate, frames);
                    for i in 0..len {
                        data[i] = (buf[i] * 32767.0).clamp(-32768.0, 32767.0) as i16;
                    }
                    record_callback(frame_count, &state_for_i16);
                },
                |err| warn!("Audio error: {}", err),
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    let mut buf = [0.0f32; 8192];
                    let len = data.len().min(buf.len());
                    let frames = oddio::frame_stereo(&mut buf[..len]);
                    let frame_count = frames.len();
                    oddio::run(&mut mixer, sample_rate, frames);
                    for i in 0..len {
                        data[i] = ((buf[i] * 32767.0 + 32767.0).clamp(0.0, 65535.0)) as u16;
                    }
                    record_callback(frame_count, &state_for_u16);
                },
                |err| warn!("Audio error: {}", err),
                None,
            )?,
            fmt => return Err(anyhow::anyhow!("Unsupported sample format: {:?}", fmt)),
        };

        stream.play()?;
        Ok((mixer_control, stream, state))
    }

    pub fn mixer(&mut self) -> Option<&mut MixerControl<StereoFrame>> {
        self.mixer.as_mut()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn state(&self) -> &Arc<AudioState> {
        &self.state
    }

    // ── Hybrid Phase-Locked Clock ──────────────────────────────────

    /// Returns the authoritative audio time in milliseconds:
    /// `(samples_played / sample_rate) * 1000`.
    /// This is the "discrete" audio clock — it only updates each callback.
    pub fn audio_time_ms(&self, _base_instant: Instant) -> f64 {
        let samples = self.state.samples_played.load(Ordering::Relaxed);
        let rate = self.state.sample_rate.load(Ordering::Relaxed) as f64;
        samples as f64 / rate * 1000.0
    }

    /// Returns the **hybrid visual time** in milliseconds.
    ///
    /// This combines the discrete audio clock with a wall-clock offset to produce
    /// a smooth, continuous, monotonic time value suitable for frame rendering
    /// at any refresh rate (60Hz, 144Hz, 240Hz+).
    ///
    /// ```text
    /// T_visual = (samples_played / sample_rate) * 1000
    ///          + (base_instant.elapsed() - last_callback_instant) in ms
    /// ```
    ///
    /// The caller should pass an `Instant` that is close to the current moment
    /// (typically `Instant::now()` or the same base used by the rendering loop).
    pub fn get_hybrid_time_ms(&self, base_instant: Instant) -> f64 {
        let samples = self.state.samples_played.load(Ordering::Relaxed);
        let rate = self.state.sample_rate.load(Ordering::Relaxed) as f64;
        let last_ts_ns = self.state.last_callback_instant.load(Ordering::Relaxed);
        let token_ns = self.state.callback_token.load(Ordering::Relaxed);

        let audio_ms = samples as f64 / rate * 1000.0;

        // wall_offset = (base_instant.elapsed() - token) - last_ts_ns
        //             = base_instant.elapsed() - (token + last_ts_ns)
        let base_ns = base_instant.elapsed().as_nanos() as u64;
        let callback_abs_ns = token_ns.saturating_add(last_ts_ns);
        let wall_offset_ns = base_ns.saturating_sub(callback_abs_ns);
        let wall_offset_ms = wall_offset_ns as f64 / 1_000_000.0;

        audio_ms + wall_offset_ms
    }

    /// Converts a duration in milliseconds to an equivalent number of audio samples.
    pub fn ms_to_samples(&self, ms: f64) -> u32 {
        let rate = self.state.sample_rate.load(Ordering::Relaxed) as f64;
        (ms * rate / 1000.0).max(0.0) as u32
    }

    /// Converts a number of audio samples to milliseconds.
    pub fn samples_to_ms(&self, samples: u32) -> f64 {
        let rate = self.state.sample_rate.load(Ordering::Relaxed) as f64;
        samples as f64 / rate * 1000.0
    }

    // ── Debug / Validation Helpers (Step 1 Test Suite) ─────────────

    /// Tracks the health of the hybrid clock across frames.
    /// Call once per frame with the same `base_instant` used for rendering.
    ///
    /// Checks:
    /// 1. **Monotonicity** — time must never go backwards.
    /// 2. **Frame-time variance** — warns if the delta deviates from the
    ///    running average by more than `max_jitter_ms` (typically 2.0ms).
    ///
    /// Returns `(current_time_ms, delta_ms, is_monotonic)`.
    ///
    /// The first frame is treated as a **warmup period** (no jitter warnings)
    /// since the running average hasn't converged yet. After warmup, the EMA
    /// is seeded to a sensible default (16.67ms) so startup isn't noisy.
    pub fn validate_hybrid_clock(
        &self,
        base_instant: Instant,
        max_jitter_ms: f64,
        prev_time: &mut Option<f64>,
        prev_delta: &mut Option<f64>,
        frame_count: &mut u64,
    ) -> (f64, f64, bool) {
        let now = self.get_hybrid_time_ms(base_instant);
        let delta = if let Some(prev) = *prev_time {
            now - prev
        } else {
            0.0
        };
        let monotonic = delta >= 0.0;

        if prev_time.is_some() && !monotonic {
            warn!(
                "Hybrid clock went backwards! delta={:.3}ms",
                delta
            );
        }

        // Warmup: skip jitter warnings for the first 10 frames so the EMA can
        // converge. Also skip frames with delta==0 (first call).
        let is_warmup = delta == 0.0 || *frame_count < 10;

        if !is_warmup {
            if let Some(pd) = *prev_delta {
                let jitter = (delta - pd).abs();
                if jitter > max_jitter_ms {
                    warn!(
                        "Hybrid clock jitter: delta={:.3}ms vs avg={:.3}ms (jitter={:.3}ms, max={:.1}ms), samples_played={}",
                        delta, pd, jitter, max_jitter_ms,
                        self.state.samples_played.load(Ordering::Relaxed)
                    );
                }
            }
        }

        *prev_time = Some(now);
        *frame_count += 1;

        // Exponential moving average of frame delta (alpha=0.1 for responsiveness).
        // On the very first frame, seed to 16.67ms so the EMA converges faster.
        *prev_delta = if prev_delta.is_none() && *frame_count == 1 {
            Some(16.67 * 0.9 + delta * 0.1)
        } else if let Some(pd) = *prev_delta {
            Some(pd * 0.9 + delta * 0.1)
        } else {
            Some(delta)
        };

        (now, delta, monotonic)
    }

    pub fn play_frames(
        &mut self,
        frames: &Arc<oddio::Frames<[f32; 2]>>,
        gain: f32,
        _position: [f32; 3],
    ) -> Result<(), AudioPlayError> {
        let mixer = self.mixer.as_mut().ok_or(AudioPlayError::NoManager)?;

        let frames_clone = Arc::clone(frames);
        let base_signal = oddio::FramesSignal::from(frames_clone);
        let (_, mut signal) = oddio::Gain::new(base_signal);
        signal.set_gain(gain);

        mixer.play(signal);
        Ok(())
    }
}

impl Default for AudioManager {
    fn default() -> Self {
        Self::new()
    }
}
