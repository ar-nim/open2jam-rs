//! Audio manager — oddio mixer hooked to cpal output stream.
//!
//! **Hybrid Phase-Locked Clock:**
//! The audio hardware is the sovereign authority. An atomic `samples_played`
//! counter is incremented in every cpal callback, and the `Instant` of each
//! callback is recorded. Visual time is computed as:
//!   T_visual = (samples_played / sample_rate) + (Instant_now - Instant_callback)
//! providing continuous, monotonic time for rendering while remaining phase-locked
//! to the discrete steps of the audio buffer.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{info, warn};
use oddio::{Mixer, MixerControl};
use rtrb::RingBuffer;

use super::bgm_signal::BgmSignalQueue;

pub type StereoFrame = [f32; 2];

/// A synchronization point mapping audio timeline to OS monotonic time.
/// Broadcasted by the audio thread every buffer fill, read by the input handler.
#[derive(Clone, Copy)]
pub struct AudioSyncPoint {
    /// The authoritative audio timeline position (derived from samples processed)
    pub audio_time_ms: f64,
    /// The precise OS timestamp captured the moment the audio buffer was filled
    pub os_time: Instant,
}

impl Default for AudioSyncPoint {
    fn default() -> Self {
        Self {
            audio_time_ms: 0.0,
            os_time: Instant::now(),
        }
    }
}

/// Thread-safe shared sync point, updated by cpal thread, read by main thread.
pub type SharedSyncPoint = Arc<RwLock<AudioSyncPoint>>;

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
    /// Max callback duration in microseconds (updated atomically from audio thread).
    pub max_callback_us: AtomicU32,
    /// Average callback duration in microseconds (exponential moving average, alpha=0.01).
    pub avg_callback_us: AtomicU32,
}

pub struct AudioManager {
    mixer: Option<MixerControl<StereoFrame>>,
    _stream: Option<cpal::Stream>,
    state: Arc<AudioState>,
    active: bool,
    /// BGM signal queue producer (main thread pushes commands here)
    bgm_producer: Option<rtrb::Producer<super::bgm_signal::BgmCommand>>,
    /// Shared sync point — updated by cpal thread, read by input handler.
    /// Maps audio sample count to OS monotonic timestamps.
    shared_sync_point: SharedSyncPoint,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioPlayError {
    #[error("AudioManager is not available")]
    NoManager,
}

impl AudioManager {
    pub fn new() -> Self {
        match Self::init() {
            Ok((mixer, stream, state, bgm_producer, shared_sync_point)) => {
                info!("AudioManager initialised (oddio + cpal). Stream paused — call play() to start.");
                Self {
                    mixer: Some(mixer),
                    _stream: Some(stream),
                    state,
                    active: false, // Stream created but not started
                    bgm_producer: Some(bgm_producer),
                    shared_sync_point,
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
                        max_callback_us: AtomicU32::new(0),
                        avg_callback_us: AtomicU32::new(0),
                    }),
                    active: false,
                    bgm_producer: None,
                    shared_sync_point: Arc::new(RwLock::new(AudioSyncPoint::default())),
                }
            }
        }
    }

    fn init() -> anyhow::Result<(
        MixerControl<StereoFrame>,
        cpal::Stream,
        Arc<AudioState>,
        rtrb::Producer<super::bgm_signal::BgmCommand>,
        SharedSyncPoint,
    )> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No default output device"))?;

        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate();
        let channels = config.channels();
        info!(
            "Audio device: rate={}, channels={}, format={:?}",
            sample_rate,
            channels,
            config.sample_format()
        );

        let (mut mixer_control, mut mixer) = Mixer::<StereoFrame>::new();

        // Create BGM signal queue and rtrb channel
        let (producer, consumer) = RingBuffer::new(1024);
        let mut bgm_queue = BgmSignalQueue::new();
        bgm_queue.set_consumer(consumer);

        // Create the shared sync point — updated by cpal thread, read by input handler
        let shared_sync_point: SharedSyncPoint = Arc::new(RwLock::new(AudioSyncPoint::default()));

        // Capture a stable reference instant before stream creation.
        // All callback timestamps will be relative to this via `.elapsed().as_nanos()`.
        let callback_token = Instant::now();

        let state = Arc::new(AudioState {
            sample_rate: AtomicU32::new(sample_rate),
            active: AtomicBool::new(true),
            samples_played: AtomicU64::new(0),
            last_callback_instant: AtomicU64::new(0),
            callback_token: AtomicU64::new(callback_token.elapsed().as_nanos() as u64),
            max_callback_us: AtomicU32::new(0),
            avg_callback_us: AtomicU32::new(0),
        });

        let stream_config = cpal::StreamConfig {
            sample_rate,
            channels,
            buffer_size: cpal::BufferSize::Fixed(128),
        };

        // Helper: record frames played, timestamp, and CPU usage into the shared state.
        let record_callback = move |frames_len: usize, elapsed_us: u32, state: &Arc<AudioState>| {
            state
                .samples_played
                .fetch_add(frames_len as u64, Ordering::Relaxed);
            let now_ns = callback_token.elapsed().as_nanos() as u64;
            state.last_callback_instant.store(now_ns, Ordering::Relaxed);

            // Update max (compare-exchange loop)
            let mut current_max = state.max_callback_us.load(Ordering::Relaxed);
            while elapsed_us > current_max {
                match state.max_callback_us.compare_exchange_weak(
                    current_max,
                    elapsed_us,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(val) => current_max = val,
                }
            }

            // Update EMA (alpha = 0.01 for slow, stable response)
            let prev_avg = state.avg_callback_us.load(Ordering::Relaxed);
            let new_avg = ((prev_avg as u64 * 99 + elapsed_us as u64) / 100) as u32;
            state.avg_callback_us.store(new_avg, Ordering::Relaxed);
        };

        let state_for_f32 = Arc::clone(&state);
        let state_for_i16 = Arc::clone(&state);
        let state_for_u16 = Arc::clone(&state);
        let sync_for_f32 = Arc::clone(&shared_sync_point);
        let sync_for_i16 = Arc::clone(&shared_sync_point);
        let sync_for_u16 = Arc::clone(&shared_sync_point);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let start = Instant::now();
                    let frames = oddio::frame_stereo(data);
                    oddio::run(&mut mixer, sample_rate, frames);
                    let elapsed_us = start.elapsed().as_micros() as u32;
                    record_callback(frames.len(), elapsed_us, &state_for_f32);

                    // Broadcast sync point to main thread
                    if let Ok(mut sync) = sync_for_f32.write() {
                        *sync = AudioSyncPoint {
                            audio_time_ms: state_for_f32.samples_played.load(Ordering::Relaxed)
                                as f64
                                / state_for_f32.sample_rate.load(Ordering::Relaxed) as f64
                                * 1000.0,
                            os_time: Instant::now(),
                        };
                    }
                },
                |err| warn!("Audio error: {}", err),
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let start = Instant::now();
                    let mut buf = [0.0f32; 8192];
                    let len = data.len().min(buf.len());
                    let frames = oddio::frame_stereo(&mut buf[..len]);
                    let frame_count = frames.len();
                    oddio::run(&mut mixer, sample_rate, frames);
                    for i in 0..len {
                        data[i] = (buf[i] * 32767.0).clamp(-32768.0, 32767.0) as i16;
                    }
                    let elapsed_us = start.elapsed().as_micros() as u32;
                    record_callback(frame_count, elapsed_us, &state_for_i16);

                    // Broadcast sync point to main thread
                    if let Ok(mut sync) = sync_for_i16.write() {
                        *sync = AudioSyncPoint {
                            audio_time_ms: state_for_i16.samples_played.load(Ordering::Relaxed)
                                as f64
                                / state_for_i16.sample_rate.load(Ordering::Relaxed) as f64
                                * 1000.0,
                            os_time: Instant::now(),
                        };
                    }
                },
                |err| warn!("Audio error: {}", err),
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    let start = Instant::now();
                    let mut buf = [0.0f32; 8192];
                    let len = data.len().min(buf.len());
                    let frames = oddio::frame_stereo(&mut buf[..len]);
                    let frame_count = frames.len();
                    oddio::run(&mut mixer, sample_rate, frames);
                    for i in 0..len {
                        data[i] = ((buf[i] * 32767.0 + 32767.0).clamp(0.0, 65535.0)) as u16;
                    }
                    let elapsed_us = start.elapsed().as_micros() as u32;
                    record_callback(frame_count, elapsed_us, &state_for_u16);

                    // Broadcast sync point to main thread
                    if let Ok(mut sync) = sync_for_u16.write() {
                        *sync = AudioSyncPoint {
                            audio_time_ms: state_for_u16.samples_played.load(Ordering::Relaxed)
                                as f64
                                / state_for_u16.sample_rate.load(Ordering::Relaxed) as f64
                                * 1000.0,
                            os_time: Instant::now(),
                        };
                    }
                },
                |err| warn!("Audio error: {}", err),
                None,
            )?,
            fmt => return Err(anyhow::anyhow!("Unsupported sample format: {:?}", fmt)),
        };

        // Add the BGM queue to the mixer so it's always playing.
        // The queue is now owned by the mixer; we only keep the producer.
        mixer_control.play(bgm_queue);

        // Stream is created but NOT started. Call audio_mgr.play() to begin playback.
        Ok((mixer_control, stream, state, producer, shared_sync_point))
    }

    pub fn mixer(&mut self) -> Option<&mut MixerControl<StereoFrame>> {
        self.mixer.as_mut()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Start the audio stream. Call this after the startup animation completes.
    pub fn play(&mut self) {
        if let Some(ref stream) = self._stream {
            // Reset the samples_played counter so audio_time starts at 0.
            self.state.samples_played.store(0, Ordering::Relaxed);
            self.state.last_callback_instant.store(0, Ordering::Relaxed);
            self.state.max_callback_us.store(0, Ordering::Relaxed);
            self.state.avg_callback_us.store(0, Ordering::Relaxed);

            match stream.play() {
                Ok(()) => {
                    self.active = true;
                    info!("Audio stream STARTED. samples_played now tracking.");
                }
                Err(e) => warn!("Failed to start audio stream: {}", e),
            }
        } else {
            warn!("Audio stream play() called but no stream available!");
        }
    }

    pub fn state(&self) -> &Arc<AudioState> {
        &self.state
    }

    /// Get a clone of the shared sync point for reading from the main thread.
    pub fn sync_point(&self) -> SharedSyncPoint {
        Arc::clone(&self.shared_sync_point)
    }

    // ── CPU Usage Monitoring ───────────────────────────────────────

    /// Returns `(avg_us, max_us, budget_us, percent_used)`.
    ///
    /// - `avg_us`: Exponential moving average of callback duration
    /// - `max_us`: Worst-case callback duration since startup
    /// - `budget_us`: Maximum allowed time (buffer_size / sample_rate)
    /// - `percent_used`: avg / budget × 100
    ///
    /// **Rule of thumb:** `percent_used` should stay below 50%.
    /// If `max_us` approaches `budget_us`, you'll hear crackling (ALSA underruns).
    pub fn callback_cpu_usage(&self) -> (u32, u32, u32, f64) {
        let rate = self.state.sample_rate.load(Ordering::Relaxed) as f64;
        let budget_us = (256.0 / rate * 1_000_000.0) as u32;

        let avg = self.state.avg_callback_us.load(Ordering::Relaxed);
        let max = self.state.max_callback_us.load(Ordering::Relaxed);
        let percent = if budget_us > 0 {
            avg as f64 / budget_us as f64 * 100.0
        } else {
            0.0
        };

        (avg, max, budget_us, percent)
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
            warn!("Hybrid clock went backwards! delta={:.3}ms", delta);
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

    /// Push a BGM command to the sample-accurate scheduling queue.
    ///
    /// This sends a BGM note to the audio thread via a lock-free SPSC queue.
    /// The note will be played at the precise sample offset specified by
    /// `delay_samples`, ensuring sample-accurate timing regardless of when
    /// this method is called relative to the audio callback.
    ///
    /// Returns `Err(PushError)` if the queue is full (rare, capacity is 1024).
    pub fn push_bgm_command(
        &mut self,
        command: super::bgm_signal::BgmCommand,
    ) -> Result<(), rtrb::PushError<super::bgm_signal::BgmCommand>> {
        if let Some(producer) = &mut self.bgm_producer {
            producer.push(command)
        } else {
            // If no producer, silently succeed (degraded mode)
            Ok(())
        }
    }
}

impl Default for AudioManager {
    fn default() -> Self {
        Self::new()
    }
}
