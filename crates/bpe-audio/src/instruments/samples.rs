//! Instruments from files: single WAV samples, folders of WAVs named by note, and SoundFonts.

use super::dsp::{self, SR};
use anyhow::Context;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Files read lately, by path, size and change time, so remaking the soundtrack after every
/// edit doesn't read a big SoundFont or sample folder again.
type Kept<T> = Mutex<Vec<(PathBuf, u64, Option<SystemTime>, Arc<T>)>>;
const KEPT: usize = 4;

fn kept<T>(cache: &Kept<T>, path: &Path, load: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<Arc<T>> {
    let meta = std::fs::metadata(path).with_context(|| format!("can't find {}", path.display()))?;
    let key = (path.to_path_buf(), meta.len(), meta.modified().ok());
    if let Some((.., value)) = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(p, l, c, _)| (p, l, c) == (&key.0, &key.1, &key.2))
    {
        return Ok(value.clone());
    }
    let value = Arc::new(load()?);
    let mut kept = cache.lock().unwrap_or_else(|e| e.into_inner());
    kept.retain(|(p, ..)| *p != key.0);
    if kept.len() >= KEPT {
        kept.remove(0);
    }
    kept.push((key.0, key.1, key.2, value.clone()));
    Ok(value)
}

type Waves = Vec<(u8, Vec<f32>, f32)>;
static FOLDERS: Kept<Waves> = Mutex::new(Vec::new());
static FONTS: Kept<rustysynth::SoundFont> = Mutex::new(Vec::new());

/// Resamples a mono sample so it plays `semitones` higher (or lower) at the mix rate.
pub fn repitch(wave: &[f32], rate: f32, semitones: f32) -> Vec<f32> {
    let step = 2f32.powf(semitones / 12.0) * rate / crate::SAMPLE_RATE as f32;
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

pub fn load_mono(path: &Path) -> anyhow::Result<(Vec<f32>, f32)> {
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

/// Reads a note name such as "C4", "F#3", "Bb2" or "60" (C4 is middle C, MIDI 60).
pub fn parse_note(text: &str) -> Option<u8> {
    let text = text.trim();
    if let Ok(number) = text.parse::<u8>() {
        return (number <= 127).then_some(number);
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
    let (shift, octave) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b') {
        (-1, r)
    } else {
        (0, rest)
    };
    let octave: i32 = octave.parse().ok()?;
    let note = (octave + 1) * 12 + class + shift;
    (0..=127).contains(&note).then_some(note as u8)
}

/// The note a sample file is named after: the last part of its name, so `piano_C4.wav`,
/// `C4.wav` and `60.wav` all work.
fn note_from_file_name(path: &Path) -> Option<u8> {
    let stem = path.file_stem()?.to_str()?;
    stem.rsplit(['_', '-', ' ', '.']).next().and_then(parse_note)
}

/// A folder of WAV files, one per note, such as `C4.wav`, `E4.wav`, `G4.wav`.
pub struct Folder {
    /// (root note, mono wave, sample rate), sorted by note.
    samples: Arc<Waves>,
}

impl Folder {
    /// Reads the folder, or reuses it if it hasn't changed since it was last read.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        Ok(Self { samples: kept(&FOLDERS, dir, || Self::read(dir))? })
    }

    fn read(dir: &Path) -> anyhow::Result<Waves> {
        let mut samples = Vec::new();
        let entries = std::fs::read_dir(dir).with_context(|| format!("opening sample folder {}", dir.display()))?;
        for entry in entries {
            let path = entry?.path();
            let is_wav = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"));
            let Some(note) = note_from_file_name(&path).filter(|_| is_wav) else { continue };
            let (wave, rate) = load_mono(&path).with_context(|| format!("loading sample {}", path.display()))?;
            samples.push((note, wave, rate));
        }
        anyhow::ensure!(!samples.is_empty(), "no WAV files named by note (like C4.wav) in {}", dir.display());
        samples.sort_by_key(|s| s.0);
        Ok(samples)
    }

    /// Plays the nearest sample, pitched to `note`. Strength only changes the volume.
    pub fn note(&self, note: u8, velocity: f32) -> Vec<f32> {
        let (root, wave, rate) =
            self.samples.iter().min_by_key(|(root, ..)| (root.abs_diff(note), *root)).expect("folder is never empty");
        let mut out = repitch(wave, *rate, note as f32 - *root as f32);
        let gain = 0.55 + 0.45 * velocity;
        out.iter_mut().for_each(|s| *s *= gain);
        out
    }

    pub fn notes(&self) -> Vec<u8> {
        self.samples.iter().map(|s| s.0).collect()
    }
}

/// A SoundFont preset, played through rustysynth one note at a time.
pub struct Font {
    font: Arc<rustysynth::SoundFont>,
    bank: u32,
    preset: u32,
}

/// How long a SoundFont note is held before it's let go, and the longest it may ring after.
const FONT_HOLD: f32 = 0.6;
const FONT_RELEASE: f32 = 3.0;

impl Font {
    pub fn load(path: &Path, bank: u32, preset: u32) -> anyhow::Result<Self> {
        let font = kept(&FONTS, path, || {
            let mut file =
                std::fs::File::open(path).with_context(|| format!("opening SoundFont {}", path.display()))?;
            rustysynth::SoundFont::new(&mut file)
                .map_err(|e| anyhow::anyhow!("reading SoundFont {}: {e:?}", path.display()))
        })?;
        Ok(Self { font, bank, preset })
    }

    /// The presets in the file as (bank, preset, name), sorted.
    pub fn presets(&self) -> Vec<(u32, u32, String)> {
        let mut all: Vec<_> = self
            .font
            .get_presets()
            .iter()
            .map(|p| (p.get_bank_number() as u32, p.get_patch_number() as u32, p.get_name().to_string()))
            .collect();
        all.sort();
        all
    }

    pub fn note(&self, note: u8, velocity: f32) -> Vec<f32> {
        let mut settings = rustysynth::SynthesizerSettings::new(crate::SAMPLE_RATE as i32);
        settings.enable_reverb_and_chorus = false;
        let Ok(mut synth) = rustysynth::Synthesizer::new(&self.font, &settings) else { return Vec::new() };
        // Drum kits live in bank 128, which General MIDI plays on channel 10.
        let channel = if self.bank >= 128 { 9 } else { 0 };
        // On the drum channel the synth adds 128 to the bank it's sent.
        let bank = if channel == 9 { self.bank - 128 } else { self.bank };
        synth.process_midi_message(channel, 0xb0, 0x00, bank.min(127) as i32);
        synth.process_midi_message(channel, 0xc0, self.preset.min(127) as i32, 0);
        let key = note.min(127) as i32;
        synth.note_on(channel, key, (1.0 + velocity.clamp(0.0, 1.0) * 126.0) as i32);
        let hold = (FONT_HOLD * SR) as usize;
        let release = (FONT_RELEASE * SR) as usize;
        let mut left = vec![0.0f32; hold + release];
        let mut right = vec![0.0f32; hold + release];
        synth.render(&mut left[..hold], &mut right[..hold]);
        synth.note_off(channel, key);
        synth.render(&mut left[hold..], &mut right[hold..]);
        let mono = left.iter().zip(&right).map(|(l, r)| (l + r) * 0.5).collect();
        dsp::trim(mono)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn note_names() {
        assert_eq!(parse_note("C4"), Some(60));
        assert_eq!(parse_note("A4"), Some(69));
        assert_eq!(parse_note("F#3"), Some(54));
        assert_eq!(parse_note("Bb2"), Some(46));
        assert_eq!(parse_note("c-1"), Some(0));
        assert_eq!(parse_note("72"), Some(72));
        assert_eq!(parse_note("200"), None);
        assert_eq!(parse_note("H2"), None);
        assert_eq!(parse_note("G9"), Some(127));
        assert_eq!(parse_note("A9"), None);
        assert_eq!(note_from_file_name(Path::new("dir/piano_C4.wav")), Some(60));
        assert_eq!(note_from_file_name(Path::new("E4.wav")), Some(64));
        assert_eq!(note_from_file_name(Path::new("readme.txt")), None);
    }

    fn write_sine(path: &Path, freq: f32, rate: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..rate / 2 {
            let s = (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin() * 0.5;
            writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    fn crossings(wave: &[f32]) -> usize {
        wave.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
    }

    #[test]
    fn sample_folders_play_the_nearest_sample_in_tune() {
        let dir = std::env::temp_dir().join(format!("bpe-folder-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        write_sine(&dir.join("A4.wav"), 440.0, 44_100);
        write_sine(&dir.join("piano_A2.wav"), 110.0, 48_000);
        std::fs::write(dir.join("notes.txt"), "not a sample").unwrap();
        let folder = Folder::load(&dir).unwrap();
        assert_eq!(folder.notes(), vec![45, 69]);
        // A5 comes from A4 an octave up, at the mix rate: 880 Hz, so 220 crossings in 0.25 s.
        let a5 = folder.note(81, 1.0);
        assert_eq!(a5.len(), 22_050 * 48_000 / 44_100 / 2);
        assert!((218..=222).contains(&crossings(&a5[..12_000])), "{}", crossings(&a5[..12_000]));
        // E3 is nearer A2 than A4.
        let e3 = folder.note(52, 1.0);
        let expected = 110.0 * 2f32.powf(7.0 / 12.0) * 0.25;
        assert!((crossings(&e3[..12_000]) as f32 - expected).abs() <= 2.0);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(Folder::load(&dir).is_err());
    }

    /// Writes the smallest useful SoundFont: one instrument playing a looped sine wave rooted at
    /// A4, as preset 0 in bank 0.
    pub(crate) fn tiny_soundfont() -> Vec<u8> {
        fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = id.to_vec();
            out.extend((body.len() as u32).to_le_bytes());
            out.extend(body);
            if body.len() % 2 == 1 {
                out.push(0);
            }
            out
        }
        fn list(kind: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
            let mut body = kind.to_vec();
            chunks.iter().for_each(|c| body.extend(c));
            chunk(b"LIST", &body)
        }
        fn name(text: &str) -> [u8; 20] {
            let mut out = [0u8; 20];
            out[..text.len()].copy_from_slice(text.as_bytes());
            out
        }
        // 100 periods of a 480 Hz sine at 48 kHz: exactly 10,000 samples, a seamless loop.
        let len = 10_000u32;
        let mut smpl = Vec::new();
        for i in 0..len {
            let s = (std::f32::consts::TAU * 480.0 * i as f32 / 48_000.0).sin() * 16_000.0;
            smpl.extend((s as i16).to_le_bytes());
        }
        smpl.extend([0u8; 92]); // 46 zero samples after the sample, as the format asks
        let info =
            list(b"INFO", &[chunk(b"ifil", &[2, 0, 1, 0]), chunk(b"isng", b"EMU8000\0"), chunk(b"INAM", b"Tiny\0\0")]);
        let sdta = list(b"sdta", &[chunk(b"smpl", &smpl)]);
        let mut phdr = Vec::new();
        for (preset_name, bag) in [("Sine", 0u16), ("EOP", 1)] {
            phdr.extend(name(preset_name));
            phdr.extend(0u16.to_le_bytes()); // preset
            phdr.extend(0u16.to_le_bytes()); // bank
            phdr.extend(bag.to_le_bytes());
            phdr.extend([0u8; 12]);
        }
        // Preset zone: one generator, instrument 0 (generator 41).
        let pbag: Vec<u8> = [0u16, 0, 1, 0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let pmod = vec![0u8; 10];
        let pgen: Vec<u8> = [41u16, 0, 0, 0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut inst = Vec::new();
        for (inst_name, bag) in [("Sine", 0u16), ("EOI", 1)] {
            inst.extend(name(inst_name));
            inst.extend(bag.to_le_bytes());
        }
        let ibag: Vec<u8> = [0u16, 0, 3, 0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let imod = vec![0u8; 10];
        // Generators: sampleModes = loop (54 = 1), overridingRootKey (58), sampleID (53 = 0).
        let igen: Vec<u8> = [54u16, 1, 58, 69, 53, 0, 0, 0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut shdr = Vec::new();
        for (sample_name, start, end, loop_start, loop_end) in [("Sine", 0u32, len, 0u32, len), ("EOS", 0, 0, 0, 0)] {
            shdr.extend(name(sample_name));
            for v in [start, end, loop_start, loop_end, 48_000] {
                shdr.extend(v.to_le_bytes());
            }
            // Original pitch 69 (A4) with no correction; the sample really is 480 Hz, so the
            // test below checks the font plays what the file says, not a perfect A.
            shdr.extend([69u8, 0]);
            shdr.extend(0u16.to_le_bytes());
            shdr.extend(1u16.to_le_bytes()); // mono
        }
        let pdta = list(
            b"pdta",
            &[
                chunk(b"phdr", &phdr),
                chunk(b"pbag", &pbag),
                chunk(b"pmod", &pmod),
                chunk(b"pgen", &pgen),
                chunk(b"inst", &inst),
                chunk(b"ibag", &ibag),
                chunk(b"imod", &imod),
                chunk(b"igen", &igen),
                chunk(b"shdr", &shdr),
            ],
        );
        let mut body = b"sfbk".to_vec();
        body.extend(info);
        body.extend(sdta);
        body.extend(pdta);
        chunk(b"RIFF", &body)
    }

    #[test]
    fn soundfonts_load_and_play() {
        let path = std::env::temp_dir().join(format!("bpe-tiny-{}.sf2", std::process::id()));
        std::fs::write(&path, tiny_soundfont()).unwrap();
        let font = Font::load(&path, 0, 0).unwrap();
        assert_eq!(font.presets(), vec![(0, 0, "Sine".to_string())]);
        // A4 plays the sample as recorded (480 Hz); A5 an octave up.
        for (note, freq) in [(69u8, 480.0f32), (81, 960.0)] {
            let wave = font.note(note, 1.0);
            assert!(wave.len() > (FONT_HOLD * SR) as usize, "note {note} too short: {}", wave.len());
            let window = &wave[4800..4800 + 12_000];
            let expected = freq * 0.25;
            assert!((crossings(window) as f32 - expected).abs() <= 3.0, "note {note}: {}", crossings(window));
        }
        std::fs::remove_file(&path).unwrap();
        assert!(Font::load(&path, 0, 0).is_err());
    }
}
