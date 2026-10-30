//! Plays the mixed soundtrack through the default output device while previewing.

use bpe_mix::SAMPLE_RATE;
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
    /// A one-shot clip heard over everything, such as chords being edited, and how far into it
    /// the output is, in clip frames.
    clip: Arc<Vec<f32>>,
    clip_position: f64,
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

    /// Plays a clip once from its start, over the soundtrack whether that plays or not. It is
    /// made on another thread so the editor doesn't wait; it replaces any clip still playing.
    pub fn play_clip_with(&self, make: impl FnOnce() -> Vec<f32> + Send + 'static) {
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            let clip = Arc::new(make());
            let mut shared = shared.lock().unwrap();
            shared.clip = clip;
            shared.clip_position = 0.0;
        });
    }

    /// Where the output has got to, in seconds: the clock the preview follows while playing.
    pub fn position(&self) -> f64 {
        self.shared.lock().unwrap().position / SAMPLE_RATE as f64
    }
}

/// The clip's frame at `position` (in clip frames, read between samples), moving it on by `step`;
/// silence once the clip has ended.
fn clip_frame(clip: &[f32], position: &mut f64, step: f64) -> (f32, f32) {
    let i = *position as usize;
    if i + 1 >= clip.len() / 2 {
        return (0.0, 0.0);
    }
    let f = (*position - i as f64) as f32;
    *position += step;
    (clip[i * 2] + (clip[i * 2 + 2] - clip[i * 2]) * f, clip[i * 2 + 1] + (clip[i * 2 + 3] - clip[i * 2 + 1]) * f)
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
                let Shared { clip, clip_position, .. } = &mut *shared;
                let (l, r) = clip_frame(clip, clip_position, step);
                (left, right) = (left + l, right + r);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clip_plays_then_stops_at_the_end() {
        let clip = [0.5, -0.5, 1.0, -1.0, 0.0, 0.0];
        let mut at = 0.0;
        assert_eq!(clip_frame(&clip, &mut at, 0.5), (0.5, -0.5));
        assert_eq!(clip_frame(&clip, &mut at, 0.5), (0.75, -0.75), "read between samples");
        assert_eq!(clip_frame(&clip, &mut at, 0.5), (1.0, -1.0));
        assert_eq!(clip_frame(&clip, &mut at, 0.5), (0.5, -0.5));
        assert_eq!(clip_frame(&clip, &mut at, 0.5), (0.0, 0.0), "ended");
        assert_eq!(clip_frame(&clip, &mut at, 0.5), (0.0, 0.0));
        assert_eq!(clip_frame(&[], &mut 0.0, 1.0), (0.0, 0.0));
    }
}
