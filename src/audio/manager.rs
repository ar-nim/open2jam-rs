//! Audio manager — oddio mixer hooked to cpal output stream.
//!
//! MILESTONE 0: Basic audio initialisation with real-time safe callback.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{info, warn};
use oddio::{Mixer, MixerControl};

/// Stereo frame type for oddio.
pub type StereoFrame = [f32; 2];

/// Shared audio state between manager and cpal callback.
pub struct AudioState {
    /// Output sample rate.
    pub sample_rate: AtomicU32,
    /// Whether the stream is active.
    pub active: AtomicBool,
}

/// Audio manager wrapping oddio + cpal.
pub struct AudioManager {
    /// oddio mixer control (main thread).
    mixer: Option<MixerControl<StereoFrame>>,
    /// cpal stream (kept alive).
    _stream: Option<cpal::Stream>,
    /// Shared state.
    state: Arc<AudioState>,
    /// Whether initialisation succeeded.
    active: bool,
}

impl AudioManager {
    /// Create and initialise the audio manager.
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
                    }),
                    active: false,
                }
            }
        }
    }

    /// Initialise oddio + cpal.
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

        let state = Arc::new(AudioState {
            sample_rate: AtomicU32::new(sample_rate),
            active: AtomicBool::new(true),
        });

        let stream_config = cpal::StreamConfig {
            sample_rate,
            channels,
            buffer_size: cpal::BufferSize::Fixed(256),
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = oddio::frame_stereo(data);
                    oddio::run(&mut mixer, sample_rate, frames);
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
                    oddio::run(&mut mixer, sample_rate, frames);
                    for i in 0..len {
                        data[i] = (buf[i] * 32767.0).clamp(-32768.0, 32767.0) as i16;
                    }
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
                    oddio::run(&mut mixer, sample_rate, frames);
                    for i in 0..len {
                        data[i] = ((buf[i] * 32767.0 + 32767.0).clamp(0.0, 65535.0)) as u16;
                    }
                },
                |err| warn!("Audio error: {}", err),
                None,
            )?,
            fmt => return Err(anyhow::anyhow!("Unsupported sample format: {:?}", fmt)),
        };

        stream.play()?;
        Ok((mixer_control, stream, state))
    }

    /// Get the mixer control for playing sounds.
    pub fn mixer(&mut self) -> Option<&mut MixerControl<StereoFrame>> {
        self.mixer.as_mut()
    }

    /// Whether the manager is active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get shared audio state.
    pub fn state(&self) -> &Arc<AudioState> {
        &self.state
    }
}

impl Default for AudioManager {
    fn default() -> Self {
        Self::new()
    }
}
