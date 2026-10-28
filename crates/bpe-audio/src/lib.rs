//! Soundtrack rendering.
//!
//! Every wall hit becomes one note, placed at the exact sample of the hit. The whole clip
//! is mixed offline, so the preview and the exported video play the very same audio.

pub mod chord;

use anyhow::Context;
use bpe_scene::{Instrument, Scene};
use bpe_sim::{HitKind, WallHit};
use std::collections::HashMap;
use std::path::Path;

pub const SAMPLE_RATE: u32 = 48_000;
/// Peak ceiling for the final mix, about -1 dBFS, which streaming platforms expect.
const PEAK_CEILING: f32 = 0.89;
/// Notes in the strum that plays when a ball escapes a ring.
const ESCAPE_STRUM: usize = 4;

/// Interleaved stereo audio at [`SAMPLE_RATE`].
#[derive(Clone, Debug, Default)]
pub struct Soundtrack {
    pub samples: Vec<f32>,
}

impl Soundtrack {
    pub fn silent(seconds: f32) -> Self {
        Self { samples: vec![0.0; (seconds.max(0.0) * SAMPLE_RATE as f32) as usize * 2] }
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn write_wav(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path.as_ref(), spec)?;
        for &s in &self.samples {
            writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
        }
        writer.finalize()?;
        Ok(())
    }
}

/// Builds the soundtrack for a scene from its wall hits. Relative sample paths are resolved
/// against `base_dir`, normally the folder the scene file lives in.
pub fn render_soundtrack(scene: &Scene, hits: &[WallHit], base_dir: Option<&Path>) -> anyhow::Result<Soundtrack> {
    let sound = &scene.sound;
    let mut track = Soundtrack::silent(scene.video.duration);
    if !sound.enabled || hits.is_empty() {
        return Ok(track);
    }
    let progression = Progression::new(scene)?;
    let mut voice = Voice::new(&sound.instrument, base_dir)?;
    let (center, half_width) = (scene.container.center().x, scene.container.half_width());
    let speed_range = (sound.full_volume_speed - sound.min_impact_speed).max(1.0);
    let frames = track.frames();

    for (index, hit) in hits.iter().enumerate() {
        if hit.impact_speed < sound.min_impact_speed {
            continue;
        }
        let start = (hit.time * SAMPLE_RATE as f64).round() as usize;
        if start >= frames {
            break;
        }
        let candidates = progression.notes_at(hit.time);
        if candidates.is_empty() {
            continue;
        }
        let pick = (mix64(sound.seed ^ index as u64) % candidates.len() as u64) as usize;
        let velocity = ((hit.impact_speed - sound.min_impact_speed) / speed_range).clamp(0.0, 1.0);
        let pan = ((hit.position.x - center) / half_width.max(1.0)).clamp(-1.0, 1.0) * sound.stereo_width;
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        // An escape plays a quick rising strum of the chord, from the picked note upwards.
        let (notes, gain, spacing) = match hit.kind {
            HitKind::Escape(_) => (ESCAPE_STRUM, 0.8, SAMPLE_RATE as usize * 35 / 1000),
            HitKind::Wall | HitKind::Obstacle(_) => (1, velocity * velocity.sqrt(), 0),
        };
        let (left, right) = (gain * angle.cos(), gain * angle.sin());
        let pick = pick.min(candidates.len().saturating_sub(notes));
        for (n, &note) in candidates[pick..].iter().take(notes).enumerate() {
            let start = start + n * spacing;
            if start >= frames {
                break;
            }
            let wave = voice.note(note);
            let end = (start + wave.len()).min(frames);
            let out = &mut track.samples[start * 2..end * 2];
            for (frame, &s) in out.chunks_exact_mut(2).zip(wave.iter()) {
                frame[0] += s * left;
                frame[1] += s * right;
            }
        }
    }

    normalize_loudness(&mut track.samples, sound.target_loudness);
    limit_peaks(&mut track.samples);
    Ok(track)
}

/// Chord progression laid out in time, with the playable notes of each chord precomputed.
struct Progression {
    steps: Vec<(f64, Vec<u8>)>,
    cycle: f64,
}

impl Progression {
    fn new(scene: &Scene) -> anyhow::Result<Self> {
        let sound = &scene.sound;
        let (low, high) = (sound.low_note.min(sound.high_note), sound.high_note.max(sound.low_note));
        let mut steps = Vec::new();
        let mut end = 0.0;
        for step in &sound.progression {
            let classes =
                chord::pitch_classes(&step.chord).with_context(|| format!("unknown chord {:?}", step.chord))?;
            let notes = (low..=high).filter(|n| classes.contains(&(n % 12))).collect();
            end += step.seconds.max(0.01) as f64;
            steps.push((end, notes));
        }
        if steps.is_empty() {
            // No progression: any note of C major.
            steps.push((1.0, (low..=high).filter(|n| [0, 2, 4, 5, 7, 9, 11].contains(&(n % 12))).collect()));
            end = 1.0;
        }
        Ok(Self { steps, cycle: end })
    }

    fn notes_at(&self, time: f64) -> &[u8] {
        let t = time.rem_euclid(self.cycle);
        let index = self.steps.iter().position(|(end, _)| t < *end).unwrap_or(self.steps.len() - 1);
        &self.steps[index].1
    }
}

/// Renders single notes for an instrument and caches them by MIDI note.
struct Voice {
    source: Source,
    cache: HashMap<u8, Vec<f32>>,
}

enum Source {
    Pluck { decay: f32, brightness: f32 },
    Sample { wave: Vec<f32>, rate: f32, root_note: u8 },
}

impl Voice {
    fn new(instrument: &Instrument, base_dir: Option<&Path>) -> anyhow::Result<Self> {
        let source = match instrument {
            Instrument::Pluck { decay, brightness } => {
                Source::Pluck { decay: decay.max(0.02), brightness: brightness.clamp(0.0, 1.0) }
            }
            Instrument::Sample { path, root_note } => {
                let full = match base_dir {
                    Some(dir) if Path::new(path).is_relative() => dir.join(path),
                    _ => path.into(),
                };
                let (wave, rate) = load_mono(&full).with_context(|| format!("loading sample {}", full.display()))?;
                Source::Sample { wave, rate, root_note: *root_note }
            }
        };
        Ok(Self { source, cache: HashMap::new() })
    }

    fn note(&mut self, note: u8) -> &[f32] {
        let source = &self.source;
        self.cache.entry(note).or_insert_with(|| match source {
            Source::Pluck { decay, brightness } => pluck(note, *decay, *brightness),
            Source::Sample { wave, rate, root_note } => repitch(wave, *rate, note as f32 - *root_note as f32),
        })
    }
}

fn midi_to_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// A soft plucked tone: a few harmonics that fade faster the higher they are.
fn pluck(note: u8, decay: f32, brightness: f32) -> Vec<f32> {
    let freq = midi_to_hz(note as f32);
    let sr = SAMPLE_RATE as f32;
    let len = (decay * sr) as usize;
    let attack = 0.003 * sr;
    let partials: Vec<(f32, f32, f32)> = (1..=8)
        .map(|k| {
            let k = k as f32;
            let amp = brightness.powf(k - 1.0) / k;
            // -60 dB (ln 1000 = 6.9) over `decay`, faster for higher partials.
            let rate = 6.9 * (1.0 + (k - 1.0) * 0.8) / decay;
            (freq * k, amp, rate)
        })
        .filter(|(f, amp, _)| *f < sr * 0.45 && *amp > 1e-3)
        .collect();
    let norm: f32 = partials.iter().map(|p| p.1).sum();
    (0..len)
        .map(|i| {
            let t = i as f32 / sr;
            let onset = (i as f32 / attack).min(1.0);
            let s: f32 = partials
                .iter()
                .map(|(f, amp, rate)| amp * (-rate * t).exp() * (std::f32::consts::TAU * f * t).sin())
                .sum();
            s * onset / norm
        })
        .collect()
}

/// Resamples a mono sample so it plays `semitones` higher (or lower) at [`SAMPLE_RATE`].
fn repitch(wave: &[f32], rate: f32, semitones: f32) -> Vec<f32> {
    let step = 2f32.powf(semitones / 12.0) * rate / SAMPLE_RATE as f32;
    let len = (wave.len() as f32 / step) as usize;
    (0..len)
        .map(|i| {
            let x = i as f32 * step;
            let j = x as usize;
            let f = x - j as f32;
            let a = wave.get(j).copied().unwrap_or(0.0);
            let b = wave.get(j + 1).copied().unwrap_or(0.0);
            a + (b - a) * f
        })
        .collect()
}

fn load_mono(path: &Path) -> anyhow::Result<(Vec<f32>, f32)> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    let mono = interleaved.chunks(channels).map(|c| c.iter().sum::<f32>() / channels as f32).collect();
    Ok((mono, spec.sample_rate as f32))
}

/// Scales the mix so its loud passages sit at `target_db` RMS (roughly LUFS for this kind of
/// dense, even material). Uses the 90th percentile of 400 ms windows so quiet starts and
/// endings do not drag the level around.
fn normalize_loudness(samples: &mut [f32], target_db: f32) {
    let window = (SAMPLE_RATE as usize * 2 * 4) / 10;
    let mut levels: Vec<f32> = samples
        .chunks(window)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .filter(|&rms| rms > 1e-6)
        .collect();
    if levels.is_empty() {
        return;
    }
    levels.sort_by(f32::total_cmp);
    let reference = levels[(levels.len() * 9 / 10).min(levels.len() - 1)];
    let gain = 10f32.powf(target_db / 20.0) / reference;
    samples.iter_mut().for_each(|s| *s *= gain);
}

/// Lookahead peak limiter: never lets a sample exceed [`PEAK_CEILING`], recovering over ~80 ms.
fn limit_peaks(samples: &mut [f32]) {
    let frames = samples.len() / 2;
    let lookahead = (SAMPLE_RATE as usize) / 200;
    let needed: Vec<f32> = samples
        .chunks_exact(2)
        .map(|f| {
            let peak = f[0].abs().max(f[1].abs());
            if peak > PEAK_CEILING { PEAK_CEILING / peak } else { 1.0 }
        })
        .collect();
    // Minimum of `needed` over [i, i + lookahead], with a monotonic queue.
    let mut window_min = vec![1.0f32; frames];
    let mut queue = std::collections::VecDeque::new();
    for i in (0..frames).rev() {
        while queue.back().is_some_and(|&j: &usize| needed[j] >= needed[i]) {
            queue.pop_back();
        }
        queue.push_back(i);
        while queue.front().is_some_and(|&j| j > i + lookahead) {
            queue.pop_front();
        }
        window_min[i] = needed[queue[0]];
    }
    let release = 1.0 - (-1.0 / (0.08 * SAMPLE_RATE as f32)).exp();
    let mut gain = 1.0f32;
    for (frame, &target) in samples.chunks_exact_mut(2).zip(&window_min) {
        gain = if target < gain { target } else { gain + (target - gain) * release };
        frame[0] *= gain;
        frame[1] *= gain;
    }
}

/// SplitMix64: a small, well-mixed hash, so note choice is random-looking but repeatable.
fn mix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_track() -> (Scene, Vec<WallHit>, Soundtrack) {
        let scene = Scene::default();
        let hits = bpe_sim::collect_wall_hits(&scene);
        let track = render_soundtrack(&scene, &hits, None).unwrap();
        (scene, hits, track)
    }

    #[test]
    fn soundtrack_matches_clip_length_and_stays_under_ceiling() {
        let (scene, _, track) = default_track();
        assert_eq!(track.frames(), (scene.video.duration * SAMPLE_RATE as f32) as usize);
        let peak = track.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= PEAK_CEILING + 1e-4, "peak {peak}");
        assert!(peak > 0.1);
    }

    #[test]
    fn silent_until_first_wall_hit() {
        let (_, hits, track) = default_track();
        let first = (hits[0].time * SAMPLE_RATE as f64) as usize;
        assert!(track.samples[..first * 2].iter().all(|&s| s == 0.0));
        assert!(track.samples[first * 2..(first + 2000) * 2].iter().any(|&s| s != 0.0));
    }

    #[test]
    fn notes_follow_the_progression() {
        let scene = Scene::default();
        let progression = Progression::new(&scene).unwrap();
        // 0-4.2 s is C major, 4.2-8.2 s is Dsus4.
        assert!(progression.notes_at(1.0).iter().all(|n| [0, 4, 7].contains(&(n % 12))));
        assert!(progression.notes_at(5.0).iter().all(|n| [2, 7, 9].contains(&(n % 12))));
    }

    #[test]
    fn escapes_strum_the_chord() {
        let scene = Scene::escape_rings();
        let hits = bpe_sim::collect_wall_hits(&scene);
        let escape = hits.iter().find(|h| matches!(h.kind, HitKind::Escape(_))).unwrap();
        let track = render_soundtrack(&scene, &[*escape], None).unwrap();
        // The strum's notes start one after another, so the sound keeps getting new onsets.
        let start = (escape.time * SAMPLE_RATE as f64) as usize;
        let spacing = SAMPLE_RATE as usize * 35 / 1000;
        let at = |frame: usize| track.samples[frame * 2..(frame + 200) * 2].iter().map(|s| s.abs()).fold(0.0, f32::max);
        assert!(at(start) > 0.0);
        assert!(at(start + 3 * spacing) > 0.0);
        assert!(track.samples[..start * 2].iter().all(|&s| s == 0.0));
    }
}
