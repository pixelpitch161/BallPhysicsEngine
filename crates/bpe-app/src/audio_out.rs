//! Plays the precomputed soundtrack through the default output device during preview.

use bpe_audio::SAMPLE_RATE;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Shared {
    /// Interleaved stereo at [`SAMPLE_RATE`].
    track: Arc<Vec<f32>>,
    /// Read position in soundtrack frames (fractional, for resampling to the device rate).
    position: f64,
    playing: bool,
    muted: bool,
}

pub struct AudioOut {
    shared: Arc<Mutex<Shared>>,
    _stream: cpal::Stream,
}

impl AudioOut {
    /// Opens the default output device. Returns an error if there is none, in which case the
    /// editor simply previews without sound.
    pub fn new() -> anyhow::Result<Self> {
        let device =
            cpal::default_host().default_output_device().ok_or_else(|| anyhow::anyhow!("no audio output device"))?;
        let supported = device.default_output_config()?;
        let config = supported.config();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, shared.clone())?,
            cpal::SampleFormat::I16 => build::<i16>(&device, config, shared.clone())?,
            cpal::SampleFormat::U16 => build::<u16>(&device, config, shared.clone())?,
            cpal::SampleFormat::I32 => build::<i32>(&device, config, shared.clone())?,
            other => anyhow::bail!("unsupported audio sample format {other:?}"),
        };
        stream.play()?;
        Ok(Self { shared, _stream: stream })
    }

    pub fn set_track(&self, track: Arc<Vec<f32>>) {
        self.shared.lock().unwrap().track = track;
    }

    pub fn play_from(&self, seconds: f64) {
        let mut shared = self.shared.lock().unwrap();
        shared.position = seconds.max(0.0) * SAMPLE_RATE as f64;
        shared.playing = true;
    }

    pub fn pause(&self) {
        self.shared.lock().unwrap().playing = false;
    }

    pub fn set_muted(&self, muted: bool) {
        self.shared.lock().unwrap().muted = muted;
    }

    /// A handle that can start sounds from another thread.
    pub fn handle(&self) -> Handle {
        Handle { shared: self.shared.clone() }
    }
}

/// Plays a short sound from start to finish, such as an instrument preview. Can be sent to the
/// thread that makes the sound.
#[derive(Clone)]
pub struct Handle {
    shared: Arc<Mutex<Shared>>,
}

impl Handle {
    /// Plays interleaved stereo at [`SAMPLE_RATE`] from its start, replacing whatever played.
    pub fn play(&self, samples: Vec<f32>) {
        let mut shared = self.shared.lock().unwrap();
        shared.track = Arc::new(samples);
        shared.position = 0.0;
        shared.playing = true;
    }

    pub fn stop(&self) {
        self.shared.lock().unwrap().playing = false;
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    shared: Arc<Mutex<Shared>>,
) -> anyhow::Result<cpal::Stream> {
    let channels = config.channels as usize;
    let step = SAMPLE_RATE as f64 / config.sample_rate as f64;
    let stream = device.build_output_stream(
        config,
        move |out: &mut [T], _| {
            let mut shared = shared.lock().unwrap();
            let frames = shared.track.len() / 2;
            for frame in out.chunks_mut(channels) {
                let (mut left, mut right) = (0.0, 0.0);
                if shared.playing {
                    let i = shared.position as usize;
                    if i + 1 < frames {
                        let f = (shared.position - i as f64) as f32;
                        let t = &shared.track;
                        left = t[i * 2] + (t[i * 2 + 2] - t[i * 2]) * f;
                        right = t[i * 2 + 1] + (t[i * 2 + 3] - t[i * 2 + 1]) * f;
                    }
                    shared.position += step;
                    if shared.muted {
                        (left, right) = (0.0, 0.0);
                    }
                }
                for (c, sample) in frame.iter_mut().enumerate() {
                    let value = match (channels, c) {
                        (1, _) => (left + right) * 0.5,
                        (_, 0) => left,
                        (_, 1) => right,
                        _ => 0.0,
                    };
                    *sample = T::from_sample(value);
                }
            }
        },
        |error| eprintln!("audio output error: {error}"),
        None,
    )?;
    Ok(stream)
}
