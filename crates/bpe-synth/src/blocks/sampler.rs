//! The sampler: plays a WAV file at the note's pitch, or the nearest note from a folder of WAVs
//! named by note (`C4.wav`, `F#3.wav`, `60.wav`).

use crate::{BLOCK, Block, Buf, Ctx, Knobs};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A loaded file: stereo frames and their rate.
#[derive(Debug)]
struct Wave {
    frames: Vec<[f32; 2]>,
    rate: f32,
}

/// Files already read, shared by every voice. A file that couldn't be read is cached as `None`.
static CACHE: Mutex<BTreeMap<PathBuf, Option<Arc<Wave>>>> = Mutex::new(BTreeMap::new());
static MISSING: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Sample files a patch asked for that couldn't be read, for the status line.
pub fn missing_samples() -> Vec<String> {
    MISSING.lock().map(|m| m.iter().cloned().collect()).unwrap_or_default()
}

fn read(path: &Path) -> Option<Wave> {
    let mut reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>().ok()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>().ok()?
        }
    };
    let frames = samples.chunks(channels).map(|c| [c[0], c[if channels > 1 { 1 } else { 0 }]]).collect();
    Some(Wave { frames, rate: spec.sample_rate as f32 })
}

fn load(path: &Path) -> Option<Arc<Wave>> {
    let mut cache = CACHE.lock().ok()?;
    let wave = cache.entry(path.to_path_buf()).or_insert_with(|| read(path).map(Arc::new)).clone();
    if wave.is_none()
        && let Ok(mut missing) = MISSING.lock()
    {
        missing.insert(path.display().to_string());
    }
    wave
}

/// Reads a note name such as "C4", "F#3", "Bb2" or "60" (C4 is MIDI 60).
fn parse_note(text: &str) -> Option<u8> {
    let text = text.trim();
    if let Ok(n) = text.parse::<u8>() {
        return (n <= 127).then_some(n);
    }
    let mut chars = text.chars();
    let class: i32 = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, octave) = match rest.strip_prefix('#') {
        Some(r) => (1, r),
        None => rest.strip_prefix('b').map_or((0, rest), |r| (-1, r)),
    };
    let note = (octave.parse::<i32>().ok()? + 1) * 12 + class + shift;
    (0..=127).contains(&note).then_some(note as u8)
}

/// The files to choose from: (root note, wave), sorted by note.
fn sources(file: &str, root: u8) -> Vec<(u8, Arc<Wave>)> {
    if file.is_empty() {
        return Vec::new();
    }
    let path = Path::new(file);
    if path.is_dir() {
        let mut found: Vec<(u8, Arc<Wave>)> = std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
            .filter_map(|p| {
                let stem = p.file_stem()?.to_str()?;
                let note = stem.rsplit(['_', '-', ' ', '.']).next().and_then(parse_note)?;
                Some((note, load(&p)?))
            })
            .collect();
        found.sort_by_key(|(n, _)| *n);
        found
    } else {
        load(path).map(|w| vec![(root, w)]).unwrap_or_default()
    }
}

pub struct Sampler {
    sources: Vec<(u8, Arc<Wave>)>,
    start: f32,
    looped: bool,
    playing: Option<(u8, Arc<Wave>)>,
    pos: f64,
}

impl Sampler {
    pub fn new(k: &Knobs) -> Self {
        let root = k.number("root", 60.0).clamp(0.0, 127.0) as u8;
        Self {
            sources: sources(k.text("file"), root),
            start: k.number("start", 0.0).clamp(0.0, 1.0),
            looped: k.flag("loop", false),
            playing: None,
            pos: 0.0,
        }
    }
}

impl Block for Sampler {
    fn reset(&mut self, cx: &Ctx) {
        let key = cx.note.round() as i32;
        self.playing = self.sources.iter().min_by_key(|(n, _)| (*n as i32 - key).abs()).cloned();
        self.pos = self.playing.as_ref().map_or(0.0, |(_, w)| self.start as f64 * w.frames.len() as f64);
    }

    fn process(&mut self, cx: &Ctx, _inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        outputs[0] = crate::SILENT;
        let Some((root, wave)) = &self.playing else { return };
        let len = wave.frames.len();
        if len < 2 {
            return;
        }
        let step = ((cx.note - *root as f32) / 12.0).exp2() as f64 * wave.rate as f64 / cx.rate as f64;
        let loop_start = self.start as f64 * len as f64;
        for s in 0..BLOCK {
            if self.pos >= (len - 1) as f64 {
                if !self.looped || loop_start >= (len - 1) as f64 {
                    return;
                }
                self.pos = loop_start + (self.pos - (len - 1) as f64);
            }
            let i = self.pos as usize;
            let f = (self.pos - i as f64) as f32;
            let (a, b) = (wave.frames[i], wave.frames[i + 1]);
            outputs[0][0][s] = a[0] + (b[0] - a[0]) * f;
            outputs[0][1][s] = a[1] + (b[1] - a[1]) * f;
            self.pos += step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::run;
    use bpe_graph::Param;

    fn write_sine(path: &Path, hz: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..48_000 {
            let v = (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin();
            w.write_sample((v * 30_000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bpe-sampler-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Rising zero crossings per second of the sampler's output for `note`.
    fn pitch(k: &Knobs, note: f32) -> f32 {
        let out = run(&mut Sampler::new(k), &mut Ctx { note, ..Ctx::default() }, 300, |_, _| vec![]);
        let ups = (1..out.len()).filter(|&i| out[i - 1][0] <= 0.0 && out[i][0] > 0.0).count();
        ups as f32 / (out.len() as f32 / 48_000.0)
    }

    #[test]
    fn a_sample_plays_at_its_root_and_an_octave_up_twice_as_fast() {
        let dir = temp_dir("root");
        let file = dir.join("tone.wav");
        write_sine(&file, 440.0);
        let k =
            Knobs::default().with("file", Param::Text(file.display().to_string())).with("root", Param::Number(69.0));
        assert!((pitch(&k, 69.0) - 440.0).abs() < 5.0);
        assert!((pitch(&k, 81.0) - 880.0).abs() < 10.0);
    }

    #[test]
    fn a_folder_picks_the_nearest_note() {
        let dir = temp_dir("folder");
        write_sine(&dir.join("A4.wav"), 440.0);
        write_sine(&dir.join("piano_A2.wav"), 110.0);
        let k = Knobs::default().with("file", Param::Text(dir.display().to_string()));
        // B2 is nearest the A2 file and plays it a tone up; A#4 plays the A4 file.
        assert!((pitch(&k, 47.0) - 123.47).abs() < 5.0);
        assert!((pitch(&k, 70.0) - 466.16).abs() < 8.0);
    }

    #[test]
    fn a_missing_file_is_silent_and_listed() {
        let file = "no/such/file-for-the-sampler.wav";
        let k = Knobs::default().with("file", Param::Text(file.into()));
        let out = run(&mut Sampler::new(&k), &mut Ctx::default(), 10, |_, _| vec![]);
        assert!(out.iter().all(|f| f[0] == 0.0));
        assert!(missing_samples().iter().any(|m| m.contains("file-for-the-sampler")));
    }
}
