//! Soundtrack rendering.
//!
//! Every wall hit becomes one note, placed at the exact sample of the hit. The whole clip
//! is mixed offline, so the preview and the exported video play the very same audio.

pub mod chord;
pub mod instruments;

use anyhow::Context;
use bpe_scene::{Instrument, Scene};
use bpe_sim::{Event, EventKind};
use std::collections::HashMap;
use instruments::{load_mono, pluck, repitch};
use std::path::Path;
use std::sync::{Arc, Mutex};

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

/// Builds the soundtrack for a scene from its event log: a note for each hit and a strum for
/// each escape. Relative sample paths are resolved against `base_dir`, normally the folder the
/// scene file lives in.
pub fn render_soundtrack(scene: &Scene, events: &[Event], base_dir: Option<&Path>) -> anyhow::Result<Soundtrack> {
    let sound = &scene.sound;
    let mut track = Soundtrack::silent(scene.video.duration);
    if !sound.enabled || !events.iter().any(|e| e.kind.is_bounce()) {
        return Ok(track);
    }
    let progression = Progression::new(scene)?;
    let mut voice = Voice::new(&sound.instrument, base_dir)?;
    let (center, half_width) = (scene.container.center().x, scene.container.half_width());
    let speed_range = (sound.full_volume_speed - sound.min_impact_speed).max(1.0);
    let frames = track.frames();

    // First list every note to play, in the order of the hits.
    let mut notes: Vec<Placed> = Vec::new();
    // Counted over bounces only, so other events (spawns, beats) don't change which notes play.
    for (index, hit) in events.iter().filter(|e| e.kind.is_bounce()).enumerate() {
        if hit.speed < sound.min_impact_speed {
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
        let velocity = ((hit.speed - sound.min_impact_speed) / speed_range).clamp(0.0, 1.0);
        let pan = ((hit.position.x - center) / half_width.max(1.0)).clamp(-1.0, 1.0) * sound.stereo_width;
        let angle = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
        // An escape plays a quick rising strum of the chord, from the picked note upwards.
        let (count, gain, spacing) = match hit.kind {
            EventKind::Escape(_) => (ESCAPE_STRUM, 0.8, SAMPLE_RATE as usize * 35 / 1000),
            _ => (1, velocity * velocity.sqrt(), 0),
        };
        let (left, right) = (gain * angle.cos(), gain * angle.sin());
        let pick = pick.min(candidates.len().saturating_sub(count));
        for (n, &note) in candidates[pick..].iter().take(count).enumerate() {
            let start = start + n * spacing;
            if start >= frames {
                break;
            }
            notes.push(Placed { start, note, left, right });
        }
    }

    // Each note's sound is made once, all of them side by side.
    let mut wanted: Vec<u8> = notes.iter().map(|n| n.note).collect();
    wanted.sort_unstable();
    wanted.dedup();
    voice.prepare(&wanted);
    mix(&mut track.samples, &notes, &voice);
    normalize_loudness(&mut track.samples, sound.target_loudness);
    limit_peaks(&mut track.samples);
    Ok(track)
}

/// One note placed in the mix.
struct Placed {
    /// First frame.
    start: usize,
    note: u8,
    left: f32,
    right: f32,
}

/// How many threads to share work between.
fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 32)
}

/// Adds every note into `samples` (interleaved stereo). The clip is cut into stretches that are
/// mixed side by side on all cores. Within a stretch the notes are added in their listed order,
/// so each sample gets exactly the same sum, in the same order, as mixing them one by one.
fn mix(samples: &mut [f32], notes: &[Placed], voice: &Voice) {
    let frames = samples.len() / 2;
    let pieces = (threads() * 4).min(frames.div_ceil(4096)).max(1);
    let stretch = frames.div_ceil(pieces).max(1);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let chunks: Vec<std::sync::Mutex<&mut [f32]>> =
        samples.chunks_mut(stretch * 2).map(std::sync::Mutex::new).collect();
    let work = || {
        loop {
            let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let Some(chunk) = chunks.get(k) else { break };
            let mut out = chunk.lock().unwrap();
            let (first, last) = (k * stretch, k * stretch + out.len() / 2);
            for placed in notes {
                let wave = voice.get(placed.note);
                let end = (placed.start + wave.len()).min(frames);
                if placed.start >= last || end <= first {
                    continue;
                }
                let (from, to) = (placed.start.max(first), end.min(last));
                let wave = &wave[from - placed.start..to - placed.start];
                let out = &mut out[(from - first) * 2..(to - first) * 2];
                for (frame, &s) in out.chunks_exact_mut(2).zip(wave) {
                    frame[0] += s * placed.left;
                    frame[1] += s * placed.right;
                }
            }
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..threads().min(pieces) {
            scope.spawn(work);
        }
        work();
    });
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
    cache: HashMap<u8, Arc<Vec<f32>>>,
}

/// Pluck notes made by earlier mixes, by (decay, brightness, note), so that remixing after an
/// edit that doesn't change the instrument skips making them again.
static PLUCKS: Mutex<Option<KeptPlucks>> = Mutex::new(None);
type KeptPlucks = HashMap<(u32, u32, u8), Arc<Vec<f32>>>;
/// Most pluck notes kept between mixes (about 1.5 MB each for a long note).
const KEPT_PLUCKS: usize = 256;

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
            // The instrument library isn't in yet, so library instruments play the pluck.
            Instrument::Library { .. } => Source::Pluck { decay: 0.9, brightness: 0.5 },
            Instrument::Sample { path, root_note } => {
                let full = match base_dir {
                    Some(dir) if Path::new(path).is_relative() => dir.join(path),
                    _ => path.into(),
                };
                let (wave, rate) = load_mono(&full).with_context(|| format!("loading sample {}", full.display()))?;
                Source::Sample { wave, rate, root_note: *root_note }
            }
            // The library's instruments play through the voices table instead.
            other => anyhow::bail!("{other:?} doesn't play through the classic sound"),
        };
        Ok(Self { source, cache: HashMap::new() })
    }

    fn render(&self, note: u8) -> Vec<f32> {
        match &self.source {
            Source::Pluck { decay, brightness } => pluck(note, *decay, *brightness),
            Source::Sample { wave, rate, root_note } => repitch(wave, *rate, note as f32 - *root_note as f32),
        }
    }

    /// Makes the sound of each of `notes` that isn't made yet, on all cores.
    fn prepare(&mut self, notes: &[u8]) {
        let pluck_key = match self.source {
            Source::Pluck { decay, brightness } => Some((decay.to_bits(), brightness.to_bits())),
            Source::Sample { .. } => None,
        };
        if let Some((decay, brightness)) = pluck_key {
            let kept = PLUCKS.lock().unwrap_or_else(|e| e.into_inner());
            for &note in notes {
                if let Some(wave) = kept.as_ref().and_then(|k| k.get(&(decay, brightness, note))) {
                    self.cache.insert(note, wave.clone());
                }
            }
        }
        let missing: Vec<u8> = notes.iter().copied().filter(|n| !self.cache.contains_key(n)).collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let made = std::sync::Mutex::new(Vec::new());
        let this = &*self;
        let work = || {
            loop {
                let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&note) = missing.get(k) else { break };
                let wave = this.render(note);
                made.lock().unwrap().push((note, Arc::new(wave)));
            }
        };
        std::thread::scope(|scope| {
            for _ in 1..threads().min(missing.len()) {
                scope.spawn(work);
            }
            work();
        });
        let made = made.into_inner().unwrap();
        if let Some((decay, brightness)) = pluck_key {
            let mut kept = PLUCKS.lock().unwrap_or_else(|e| e.into_inner());
            let kept = kept.get_or_insert_with(HashMap::new);
            if kept.len() + made.len() > KEPT_PLUCKS {
                kept.clear();
            }
            kept.extend(made.iter().map(|(note, wave)| ((decay, brightness, *note), wave.clone())));
        }
        self.cache.extend(made);
    }

    /// A note made by [`Voice::prepare`].
    fn get(&self, note: u8) -> &[f32] {
        self.cache.get(&note).map_or(&[], |wave| wave.as_slice())
    }
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

    fn default_track() -> (Scene, Vec<Event>, Soundtrack) {
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
    fn mixing_on_many_cores_matches_mixing_one_note_at_a_time() {
        for scene in [Scene::default(), Scene::escape_rings(), Scene::multiplying_balls()] {
            let hits = bpe_sim::collect_wall_hits(&scene);
            let notes: Vec<Placed> = (0..400)
                .map(|i| Placed {
                    start: (hits[i % hits.len()].time * SAMPLE_RATE as f64) as usize + i * 37,
                    note: 50 + (i % 30) as u8,
                    left: 0.1 + (i % 7) as f32 * 0.05,
                    right: 0.4 - (i % 5) as f32 * 0.03,
                })
                .collect();
            let mut voice = Voice::new(&scene.sound.instrument, None).unwrap();
            let wanted: Vec<u8> = (50..80).collect();
            voice.prepare(&wanted);
            let mut parallel = Soundtrack::silent(scene.video.duration).samples;
            mix(&mut parallel, &notes, &voice);
            let mut serial = Soundtrack::silent(scene.video.duration).samples;
            let frames = serial.len() / 2;
            for placed in &notes {
                let wave = voice.get(placed.note);
                if placed.start >= frames {
                    continue;
                }
                let end = (placed.start + wave.len()).min(frames);
                for (frame, &s) in serial[placed.start * 2..end * 2].chunks_exact_mut(2).zip(wave) {
                    frame[0] += s * placed.left;
                    frame[1] += s * placed.right;
                }
            }
            assert!(parallel.iter().zip(&serial).all(|(a, b)| a.to_bits() == b.to_bits()));
        }
    }

    #[test]
    fn kept_notes_sound_the_same_as_new_ones() {
        let scene = Scene::default();
        let Instrument::Pluck { decay, brightness } = scene.sound.instrument else { panic!() };
        let mut voice = Voice::new(&scene.sound.instrument, None).unwrap();
        voice.prepare(&[60, 64]);
        let mut again = Voice::new(&scene.sound.instrument, None).unwrap();
        again.prepare(&[60, 64, 67]);
        assert_eq!(again.get(60), pluck(60, decay.max(0.02), brightness.clamp(0.0, 1.0)).as_slice());
        assert_eq!(again.get(67), voice.render(67).as_slice());
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
        let escape = hits.iter().find(|h| matches!(h.kind, EventKind::Escape(_))).unwrap();
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
