//! Song analysis: decode a song once, then find its beats, tempo and how loud its bass, mids
//! and treble are at every moment.
//!
//! Beat sync, audio-reactive effects, "pulse with music" settings, rules and the scorer all
//! read the same [`AudioFeatures`]. Analysis is cached on disk by the file's contents, so a
//! song loads instantly the second time. Everything here is pure Rust.

mod beats;
mod cache;
#[cfg(feature = "decode")]
mod decode;
pub mod midi;

pub use cache::{analyze_file, cache_dir};
#[cfg(feature = "decode")]
pub use decode::decode;

use realfft::RealFftPlanner;

/// Sample rate songs are decoded to, matching the soundtrack.
pub const SAMPLE_RATE: u32 = 48_000;
/// Analysis frames per second.
pub const FRAME_RATE: f32 = 200.0;
/// Band edges in Hz: bass is below the first, treble above the second.
const BASS_TOP: f32 = 250.0;
const TREBLE_BOTTOM: f32 = 4000.0;
/// Highest frequency looked at, in Hz.
const TOP: f32 = 16_000.0;
/// How quickly the band levels rise and fall, in seconds.
const ATTACK: f32 = 0.01;
const RELEASE: f32 = 0.15;

/// Decoded audio.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    /// Interleaved samples, -1 to 1.
    pub samples: Vec<f32>,
}

impl Pcm {
    /// Number of sample frames (one sample per channel each).
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }

    /// The channels averaged into one.
    pub fn mono(&self) -> Vec<f32> {
        let channels = self.channels.max(1) as usize;
        if channels == 1 {
            return self.samples.clone();
        }
        self.samples.chunks_exact(channels).map(|frame| frame.iter().sum::<f32>() / channels as f32).collect()
    }
}

/// What a song sounds like over time: band levels and onsets 200 times a second, plus beats.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioFeatures {
    /// Frames per second of the level lists ([`FRAME_RATE`]).
    pub rate: f32,
    /// Bass, mid and treble loudness, 0 to 1, relative to the loudest parts of this song.
    pub bass: Vec<f32>,
    pub mid: Vec<f32>,
    pub treble: Vec<f32>,
    /// Overall loudness, 0 to 1, where 1 is a full-scale sine wave.
    pub level: Vec<f32>,
    /// How strongly something new starts at each moment (drum hits, notes), 0 to 1.
    pub onset: Vec<f32>,
    /// Beat times in seconds, earliest first.
    pub beats: Vec<f64>,
    /// The beats that start a bar, assuming four beats to the bar.
    pub downbeats: Vec<f64>,
    /// Beats per minute, or 0 when no steady beat was found.
    pub bpm: f32,
}

/// The song's levels at one moment.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub level: f32,
    pub onset: f32,
}

impl AudioFeatures {
    /// Length covered by the analysis, in seconds.
    pub fn duration(&self) -> f64 {
        self.level.len() as f64 / self.rate.max(1.0) as f64
    }

    /// The levels at `time` seconds, blended between frames. Silent outside the song.
    pub fn at(&self, time: f64) -> Sample {
        let n = self.level.len();
        if n == 0 || !time.is_finite() || time < 0.0 {
            return Sample::default();
        }
        let x = time * self.rate as f64;
        let i = x.floor() as usize;
        if i >= n {
            return Sample::default();
        }
        let j = (i + 1).min(n - 1);
        let t = (x - i as f64) as f32;
        let lerp = |v: &[f32]| v[i] + (v[j] - v[i]) * t;
        Sample {
            bass: lerp(&self.bass),
            mid: lerp(&self.mid),
            treble: lerp(&self.treble),
            level: lerp(&self.level),
            onset: lerp(&self.onset),
        }
    }

    /// How far through the current beat `time` is: 0 on a beat, rising to 1 just before the
    /// next. 0 before the first beat, after the last, or with no beats.
    pub fn beat_phase(&self, time: f64) -> f32 {
        let next = self.beats.partition_point(|&b| b <= time);
        if next == 0 || next >= self.beats.len() {
            return 0.0;
        }
        let (a, b) = (self.beats[next - 1], self.beats[next]);
        (((time - a) / (b - a).max(1e-6)) as f32).clamp(0.0, 1.0)
    }

    /// The index of the latest beat at or before `time`.
    pub fn beat_index(&self, time: f64) -> Option<usize> {
        self.beats.partition_point(|&b| b <= time).checked_sub(1)
    }
}

/// Analyses a whole song.
pub fn analyze(pcm: &Pcm) -> AudioFeatures {
    analyze_mono(&pcm.mono(), pcm.rate)
}

/// Analyses interleaved samples, such as a mixed soundtrack (song plus bounce sounds), for
/// visuals that follow everything you hear.
pub fn analyze_mix(samples: &[f32], rate: u32, channels: u16) -> AudioFeatures {
    analyze(&Pcm { rate, channels, samples: samples.to_vec() })
}

fn analyze_mono(mono: &[f32], rate: u32) -> AudioFeatures {
    let rate = rate.max(1);
    let hop = rate as f32 / FRAME_RATE;
    // About 21 ms: short enough to place onsets precisely, long enough to see the bass.
    let size = ((rate as f32 * 0.0213) as usize).next_power_of_two().max(64);
    let frames = (mono.len() as f32 / hop).ceil() as usize;
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(size);
    let window: Vec<f32> =
        (0..size).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos()).collect();
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let mut scratch = fft.make_scratch_vec();
    let bin_hz = rate as f32 / size as f32;
    let bin = |hz: f32| ((hz / bin_hz).round() as usize).min(size / 2);
    let (bass_top, treble_bottom, top) = (bin(BASS_TOP), bin(TREBLE_BOTTOM), bin(TOP).max(1));
    // A full-scale sine's peak bin, through a Hann window, has magnitude size / 4.
    let full = size as f32 / 4.0;

    let mut raw = [vec![0.0f32; frames], vec![0.0f32; frames], vec![0.0f32; frames]];
    let mut level = vec![0.0f32; frames];
    let mut flux = vec![0.0f32; frames];
    let mut previous = vec![0.0f32; top + 1];
    let mut magnitude = vec![0.0f32; top + 1];
    for frame in 0..frames {
        let center = (frame as f32 * hop) as isize;
        let start = center - size as isize / 2;
        let mut square = 0.0;
        for (i, slot) in input.iter_mut().enumerate() {
            let s = mono.get((start + i as isize) as usize).copied().filter(|_| start + i as isize >= 0).unwrap_or(0.0);
            square += s * s;
            *slot = s * window[i];
        }
        level[frame] = (2.0 * square / size as f32).sqrt().min(1.0);
        if fft.process_with_scratch(&mut input, &mut spectrum, &mut scratch).is_err() {
            continue;
        }
        let mut bands = [0.0f32; 3];
        let mut rise = 0.0;
        for k in 1..=top {
            let m = spectrum[k].norm() / full;
            let power = m * m;
            let band = if k < bass_top {
                0
            } else if k < treble_bottom {
                1
            } else {
                2
            };
            bands[band] += power;
            // Log-compressed, so quiet notes count as well as loud drums.
            magnitude[k] = (1.0 + 1000.0 * m).ln();
            rise += (magnitude[k] - previous[k]).max(0.0);
        }
        std::mem::swap(&mut magnitude, &mut previous);
        for (band, values) in raw.iter_mut().enumerate() {
            values[frame] = bands[band].sqrt();
        }
        flux[frame] = if frame == 0 { 0.0 } else { rise };
    }

    let attack = (-1.0 / (ATTACK * FRAME_RATE)).exp();
    let release = (-1.0 / (RELEASE * FRAME_RATE)).exp();
    let [bass, mid, treble] = raw.map(|values| smooth(&normalize(&values), attack, release));
    let onset_strength = beats::onset_envelope(&flux);
    let found = beats::track(&onset_strength, &level, size as f32 / rate as f32);
    let level = smooth(&level, attack, release);
    let peak = percentile(&onset_strength, 0.995).max(1e-6);
    let onset: Vec<f32> = onset_strength.iter().map(|&o| (o / peak).clamp(0.0, 1.0)).collect();
    AudioFeatures {
        rate: FRAME_RATE,
        bass,
        mid,
        treble,
        level,
        onset,
        beats: found.beats,
        downbeats: found.downbeats,
        bpm: found.bpm,
    }
}

/// Scales values so the song's loud parts sit near 1. Very quiet songs aren't blown up.
fn normalize(values: &[f32]) -> Vec<f32> {
    let top = percentile(values, 0.95).max(0.01);
    values.iter().map(|&v| (v / top).min(1.0)).collect()
}

/// Rises quickly and falls slowly, so levels pulse rather than flicker.
fn smooth(values: &[f32], attack: f32, release: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(values.len());
    let mut state = 0.0f32;
    for &v in values {
        let k = if v > state { attack } else { release };
        state = v + (state - v) * k;
        out.push(state);
    }
    out
}

fn percentile(values: &[f32], q: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f32> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return 0.0;
    }
    let k = ((sorted.len() - 1) as f32 * q).round() as usize;
    let (_, value, _) = sorted.select_nth_unstable_by(k, f32::total_cmp);
    *value
}

#[cfg(test)]
mod tests;
