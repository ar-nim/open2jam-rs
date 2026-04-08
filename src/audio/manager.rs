//! Audio manager — oddio mixer hooked to cpal output stream.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{info, warn};
use oddio::{Mixer, MixerControl};

pub type StereoFrame = [f32; 2];

pub struct AudioState {
    pub sample_rate: AtomicU32,
    pub active: AtomicBool,
    /// Total stereo frames consumed by the audio output callback.
    pub samples_played: AtomicU64,
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

        let state = Arc::new(AudioState {
            sample_rate: AtomicU32::new(sample_rate),
            active: AtomicBool::new(true),
            samples_played: AtomicU64::new(0),
        });

        let stream_config = cpal::StreamConfig {
            sample_rate,
            channels,
            buffer_size: cpal::BufferSize::Fixed(256),
        };

        let stream_state = Arc::clone(&state);
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let frames = oddio::frame_stereo(data);
                    oddio::run(&mut mixer, sample_rate, frames);
                    stream_state.samples_played.fetch_add(frames.len() as u64, std::sync::atomic::Ordering::Relaxed);
                },
                |err| warn!("Audio error: {}", err),
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    let mut buf = [0.0f32; 8192];
                    let len = data.len().min(buf.len());
                    let frame_count = len;
                    let frames = oddio::frame_stereo(&mut buf[..len]);
                    oddio::run(&mut mixer, sample_rate, frames);
                    for i in 0..len {
                        data[i] = (buf[i] * 32767.0).clamp(-32768.0, 32767.0) as i16;
                    }
                    stream_state.samples_played.fetch_add(frame_count as u64, std::sync::atomic::Ordering::Relaxed);
                },
                |err| warn!("Audio error: {}", err),
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
                    let mut buf = [0.0f32; 8192];
                    let len = data.len().min(buf.len());
                    let frame_count = len;
                    let frames = oddio::frame_stereo(&mut buf[..len]);
                    oddio::run(&mut mixer, sample_rate, frames);
                    for i in 0..len {
                        data[i] = ((buf[i] * 32767.0 + 32767.0).clamp(0.0, 65535.0)) as u16;
                    }
                    stream_state.samples_played.fetch_add(frame_count as u64, std::sync::atomic::Ordering::Relaxed);
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

    /// Get the current audio playback position in milliseconds.
    /// This is the authoritative game clock — derived from actual samples
    /// consumed by the audio output callback, not wall-clock time.
    pub fn playback_position_ms(&self) -> f64 {
        let samples = self.state.samples_played.load(std::sync::atomic::Ordering::Relaxed);
        let rate = self.state.sample_rate.load(std::sync::atomic::Ordering::Relaxed) as f64;
        if rate <= 0.0 {
            return 0.0;
        }
        samples as f64 / rate * 1000.0
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
