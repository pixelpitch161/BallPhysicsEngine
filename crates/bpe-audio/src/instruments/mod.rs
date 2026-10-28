//! The instrument library: built-in synthesised instruments, a synthesised drum kit, folders of
//! WAV samples and SoundFonts. Everything renders single notes as mono waves at
//! [`SAMPLE_RATE`](crate::SAMPLE_RATE), cached by note and strength.

mod dsp;
pub mod drums;
mod samples;
mod synth;

pub use drums::Piece;

use anyhow::Context;
use bpe_scene::Instrument;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Built-in instruments, all synthesised so the program stays small.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    Pluck,
    ElectricPiano,
    GrandPiano,
    Marimba,
    Vibraphone,
    Glass,
    Harp,
    Bells,
    Kalimba,
    Supersaw,
    Pad,
    SynthBass,
    DrumKit,
}

impl Builtin {
    pub const ALL: [Builtin; 13] = [
        Builtin::Pluck,
        Builtin::ElectricPiano,
        Builtin::GrandPiano,
        Builtin::Marimba,
        Builtin::Vibraphone,
        Builtin::Glass,
        Builtin::Harp,
        Builtin::Bells,
        Builtin::Kalimba,
        Builtin::Supersaw,
        Builtin::Pad,
        Builtin::SynthBass,
        Builtin::DrumKit,
    ];

    /// The name scenes store, also shown in the editor.
    pub fn name(self) -> &'static str {
        match self {
            Builtin::Pluck => "Pluck",
            Builtin::ElectricPiano => "Electric piano",
            Builtin::GrandPiano => "Grand piano",
            Builtin::Marimba => "Marimba",
            Builtin::Vibraphone => "Vibraphone",
            Builtin::Glass => "Glass",
            Builtin::Harp => "Harp",
            Builtin::Bells => "Bells",
            Builtin::Kalimba => "Kalimba",
            Builtin::Supersaw => "Supersaw lead",
            Builtin::Pad => "Pad",
            Builtin::SynthBass => "Synth bass",
            Builtin::DrumKit => "Drum kit",
        }
    }

    /// A shelf in the library browser.
    pub fn family(self) -> &'static str {
        match self {
            Builtin::Pluck | Builtin::Harp | Builtin::Kalimba => "Plucked",
            Builtin::ElectricPiano | Builtin::GrandPiano => "Keys",
            Builtin::Marimba | Builtin::Vibraphone | Builtin::Glass | Builtin::Bells => "Mallets and bells",
            Builtin::Supersaw | Builtin::Pad | Builtin::SynthBass => "Synths",
            Builtin::DrumKit => "Drums",
        }
    }

    /// One sentence for the editor's tooltip.
    pub fn description(self) -> &'static str {
        match self {
            Builtin::Pluck => "A soft plucked tone, the original bounce sound.",
            Builtin::ElectricPiano => "A warm electric piano with a bell-like attack.",
            Builtin::GrandPiano => "A bright acoustic piano with a long ring.",
            Builtin::Marimba => "Wooden bars hit with soft mallets. Short and round.",
            Builtin::Vibraphone => "Metal bars with a long ring and a gentle wobble.",
            Builtin::Glass => "Pure, glassy tones that swell in softly.",
            Builtin::Harp => "A plucked string that rings out.",
            Builtin::Bells => "Tubular bells with a deep hum.",
            Builtin::Kalimba => "A thumb piano: clear notes with a metal tick.",
            Builtin::Supersaw => "A big, bright synth lead made of seven saws.",
            Builtin::Pad => "A soft synth pad that swells in and fades slowly.",
            Builtin::SynthBass => "A punchy synth bass.",
            Builtin::DrumKit => "Kick, snare, clap, hi-hats, toms and cymbals. The note picks the drum.",
        }
    }

    pub fn from_name(name: &str) -> Option<Builtin> {
        Builtin::ALL.into_iter().find(|b| b.name().eq_ignore_ascii_case(name.trim()))
    }

    /// Renders one note at `velocity` (0 to 1), with its peak scaled to the instrument's level.
    pub fn render(self, note: u8, velocity: f32) -> Vec<f32> {
        let v = velocity.clamp(0.0, 1.0);
        let wave = match self {
            Builtin::Pluck => synth::pluck(note, 0.9, 0.3 + 0.3 * v),
            Builtin::ElectricPiano => synth::electric_piano(note, v),
            Builtin::GrandPiano => synth::grand_piano(note, v),
            Builtin::Marimba => synth::marimba(note, v),
            Builtin::Vibraphone => synth::vibraphone(note, v),
            Builtin::Glass => synth::glass(note, v),
            Builtin::Harp => synth::harp(note, v),
            Builtin::Bells => synth::bells(note, v),
            Builtin::Kalimba => synth::kalimba(note, v),
            Builtin::Supersaw => synth::supersaw(note, v),
            Builtin::Pad => synth::pad(note, v),
            Builtin::SynthBass => synth::synth_bass(note, v),
            Builtin::DrumKit => Piece::from_note(note).render(v),
        };
        let mut wave = dsp::trim(wave);
        dsp::normalize(&mut wave, self.level() * (0.55 + 0.45 * v));
        wave
    }

    /// Peak level of a full-strength note, so instruments sit at similar loudness. Long,
    /// sustained sounds are set lower because they add up.
    fn level(self) -> f32 {
        match self {
            Builtin::Pad => 0.45,
            Builtin::Supersaw | Builtin::Bells | Builtin::Vibraphone | Builtin::Glass => 0.6,
            Builtin::SynthBass => 0.7,
            _ => 0.8,
        }
    }
}

/// The original pluck and sample player, unchanged, for scenes from before the instrument library.
pub use samples::{load_mono, repitch};
pub use synth::pluck;

/// Renders an instrument's notes and keeps them, so each note is only made once per soundtrack.
pub struct Voice {
    source: Source,
    cache: HashMap<(u8, u8), Arc<[f32]>>,
}

enum Source {
    Pluck { decay: f32, brightness: f32 },
    Sample { wave: Vec<f32>, rate: f32, root_note: u8 },
    Builtin(Builtin),
    Folder(samples::Folder),
    SoundFont(samples::Font),
}

/// Velocity layers rendered for instruments whose sound changes with how hard they're played.
const LAYERS: u8 = 4;

impl Voice {
    /// Prepares an instrument. Relative paths are resolved against `base_dir`, normally the
    /// folder the scene file lives in.
    pub fn new(instrument: &Instrument, base_dir: Option<&Path>) -> anyhow::Result<Self> {
        let source = match instrument {
            Instrument::Pluck { decay, brightness } => {
                Source::Pluck { decay: decay.max(0.02), brightness: brightness.clamp(0.0, 1.0) }
            }
            Instrument::Sample { path, root_note } => {
                let full = resolve(path, base_dir);
                let (wave, rate) =
                    samples::load_mono(&full).with_context(|| format!("loading sample {}", full.display()))?;
                Source::Sample { wave, rate, root_note: *root_note }
            }
            Instrument::Library { name } => {
                Source::Builtin(Builtin::from_name(name).with_context(|| format!("unknown instrument {name:?}"))?)
            }
            Instrument::SampleFolder { path } => Source::Folder(samples::Folder::load(&resolve(path, base_dir))?),
            Instrument::SoundFont { path, bank, preset } => {
                Source::SoundFont(samples::Font::load(&resolve(path, base_dir), *bank, *preset)?)
            }
        };
        Ok(Self { source, cache: HashMap::new() })
    }

    /// Whether harder hits change the sound itself, not just the volume.
    fn layered(&self) -> bool {
        !matches!(self.source, Source::Pluck { .. } | Source::Sample { .. })
    }

    /// The cache slot for a note at a strength.
    fn slot(&self, note: u8, velocity: f32) -> (u8, u8) {
        let layer = if self.layered() { (velocity.clamp(0.0, 1.0) * (LAYERS - 1) as f32).round() as u8 } else { 0 };
        (note.min(127), layer)
    }

    fn render(&self, (note, layer): (u8, u8)) -> Vec<f32> {
        let v = layer as f32 / (LAYERS - 1) as f32;
        match &self.source {
            Source::Pluck { decay, brightness } => synth::pluck(note, *decay, *brightness),
            Source::Sample { wave, rate, root_note } => samples::repitch(wave, *rate, note as f32 - *root_note as f32),
            Source::Builtin(builtin) => builtin.render(note, v),
            Source::Folder(folder) => folder.note(note, v),
            Source::SoundFont(font) => font.note(note, v),
        }
    }

    /// Makes every note in `wanted` (note, strength) that isn't made yet, on all cores.
    pub fn prepare(&mut self, wanted: &[(u8, f32)]) {
        let mut missing: Vec<(u8, u8)> =
            wanted.iter().map(|&(n, v)| self.slot(n, v)).filter(|slot| !self.cache.contains_key(slot)).collect();
        missing.sort_unstable();
        missing.dedup();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let made = std::sync::Mutex::new(Vec::new());
        let this = &*self;
        let work = || {
            while let Some(&slot) = missing.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                let wave: Arc<[f32]> = this.render(slot).into();
                made.lock().unwrap().push((slot, wave));
            }
        };
        std::thread::scope(|scope| {
            for _ in 1..crate::threads().min(missing.len()) {
                scope.spawn(work);
            }
            work();
        });
        self.cache.extend(made.into_inner().unwrap());
    }

    /// One note at `velocity` (0 to 1). The wave starts at the note's onset.
    pub fn note(&mut self, note: u8, velocity: f32) -> Arc<[f32]> {
        let slot = self.slot(note, velocity);
        if let Some(wave) = self.cache.get(&slot) {
            return wave.clone();
        }
        let wave: Arc<[f32]> = self.render(slot).into();
        self.cache.insert(slot, wave.clone());
        wave
    }
}

/// The presets in a SoundFont file as (bank, preset, name), for the editor's picker.
pub fn soundfont_presets(path: &Path) -> anyhow::Result<Vec<(u32, u32, String)>> {
    Ok(samples::Font::load(path, 0, 0)?.presets())
}

/// The notes a sample folder has recordings for, lowest first.
pub fn folder_notes(path: &Path) -> anyhow::Result<Vec<u8>> {
    Ok(samples::Folder::load(path)?.notes())
}

/// A short phrase on an instrument, so it can be heard before it's picked: a rising arpeggio,
/// or a little beat on the drum kit. Interleaved stereo at [`SAMPLE_RATE`](crate::SAMPLE_RATE).
pub fn preview(instrument: &Instrument, base_dir: Option<&Path>) -> anyhow::Result<Vec<f32>> {
    let drums = matches!(instrument, Instrument::Library { name } if Builtin::from_name(name) == Some(Builtin::DrumKit))
        || matches!(instrument, Instrument::SoundFont { bank, .. } if *bank >= 128);
    let phrase: &[(f32, u8, f32)] = if drums {
        &[(0.0, 36, 1.0), (0.25, 42, 0.6), (0.5, 38, 0.9), (0.75, 42, 0.6), (1.0, 36, 1.0), (1.25, 36, 0.8), (1.5, 38, 0.9), (1.75, 49, 0.8)]
    } else {
        &[(0.0, 60, 0.8), (0.18, 64, 0.8), (0.36, 67, 0.8), (0.54, 72, 1.0)]
    };
    let mut voice = Voice::new(instrument, base_dir)?;
    let wanted: Vec<(u8, f32)> = phrase.iter().map(|&(_, n, v)| (n, v)).collect();
    voice.prepare(&wanted);
    let rate = crate::SAMPLE_RATE as f32;
    let notes: Vec<(usize, Arc<[f32]>)> =
        phrase.iter().map(|&(t, n, v)| ((t * rate) as usize, voice.note(n, v))).collect();
    // Long notes are cut at four seconds with a short fade, so a preview never drags on.
    let longest = (4.0 * rate) as usize;
    let length = notes.iter().map(|(start, wave)| start + wave.len()).max().unwrap_or(0).min(longest);
    let mut out = vec![0.0f32; length * 2];
    for (start, wave) in &notes {
        for (i, s) in wave.iter().enumerate() {
            let at = start + i;
            if at >= length {
                break;
            }
            let fade = ((length - at) as f32 / (0.05 * rate)).min(1.0);
            out[at * 2] += s * 0.7 * fade;
            out[at * 2 + 1] += s * 0.7 * fade;
        }
    }
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.9 {
        out.iter_mut().for_each(|s| *s *= 0.9 / peak);
    }
    Ok(out)
}

fn resolve(path: &str, base_dir: Option<&Path>) -> PathBuf {
    match base_dir {
        Some(dir) if Path::new(path).is_relative() => dir.join(path),
        _ => path.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_plays_every_register() {
        for builtin in Builtin::ALL {
            assert_eq!(Builtin::from_name(builtin.name()), Some(builtin));
            for note in [24u8, 48, 60, 72, 96, 120] {
                for velocity in [0.0, 1.0] {
                    let wave = builtin.render(note, velocity);
                    assert!(!wave.is_empty(), "{} {note}", builtin.name());
                    assert!(wave.len() < 8 * crate::SAMPLE_RATE as usize, "{} {note}", builtin.name());
                    let peak = wave.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                    assert!(peak.is_finite() && peak > 0.1 && peak <= 0.81, "{} {note} peak {peak}", builtin.name());
                    // Starts quietly, so no click at the onset.
                    assert!(wave[0].abs() < 0.05, "{} {note} starts at {}", builtin.name(), wave[0]);
                }
            }
        }
    }

    /// The pitch of `wave` in Hz, between 50 Hz and 2 kHz (the YIN method).
    pub(crate) fn pitch(wave: &[f32]) -> f32 {
        let sr = crate::SAMPLE_RATE as usize;
        let n = sr / 20;
        let max_lag = sr / 50;
        let diff = |lag: usize| -> f32 { (0..n).map(|j| (wave[j] - wave[j + lag]).powi(2)).sum() };
        let d: Vec<f32> = (0..=max_lag).map(diff).collect();
        let mut running = 0.0;
        let normalized: Vec<f32> = (0..=max_lag)
            .map(|lag| {
                if lag == 0 {
                    return 1.0;
                }
                running += d[lag];
                d[lag] * lag as f32 / running.max(1e-12)
            })
            .collect();
        let mut lag = (sr / 2000..max_lag).find(|&lag| normalized[lag] < 0.15).unwrap_or_else(|| {
            (sr / 2000..max_lag).min_by(|&a, &b| normalized[a].total_cmp(&normalized[b])).unwrap()
        });
        while lag + 1 < max_lag && normalized[lag + 1] < normalized[lag] {
            lag += 1;
        }
        // Refine between whole samples with a parabola through the minimum.
        let (a, b, c) = (normalized[lag - 1], normalized[lag], normalized[lag + 1]);
        let shift = 0.5 * (a - c) / (a - 2.0 * b + c).max(1e-9);
        sr as f32 / (lag as f32 + shift.clamp(-1.0, 1.0))
    }

    #[test]
    fn notes_are_in_tune() {
        // Glass and bells have deliberately out-of-tune overtones, which confuse a pitch
        // detector, and drums have no pitch.
        let pitched = Builtin::ALL.into_iter().filter(|b| ![Builtin::DrumKit, Builtin::Glass, Builtin::Bells].contains(b));
        for builtin in pitched {
            for note in [45u8, 57, 69] {
                let wave = builtin.render(note, 0.5);
                let mut want = 440.0 * 2f32.powf((note as f32 - 69.0) / 12.0);
                if builtin == Builtin::SynthBass {
                    // Its sub oscillator sits an octave down.
                    want *= 0.5;
                }
                let got = pitch(&wave[(0.15 * crate::SAMPLE_RATE as f32) as usize..]);
                let cents = 1200.0 * (got / want).log2();
                assert!(cents.abs() < 15.0, "{} note {note}: {got:.1} Hz, wanted {want:.1}", builtin.name());
            }
        }
    }

    #[test]
    fn unknown_instrument_names_are_an_error() {
        assert!(Voice::new(&Instrument::library("Kazoo"), None).is_err());
        assert!(Voice::new(&Instrument::library("marimba"), None).is_ok());
    }

    #[test]
    fn notes_are_cached_by_strength_layer() {
        let mut voice = Voice::new(&Instrument::library("Electric piano"), None).unwrap();
        let soft = voice.note(60, 0.0);
        let soft_again = voice.note(60, 0.1);
        let hard = voice.note(60, 1.0);
        assert!(Arc::ptr_eq(&soft, &soft_again));
        assert!(!Arc::ptr_eq(&soft, &hard));
        // The legacy pluck ignores strength (the mixer scales it), as it always did.
        let mut pluck = Voice::new(&Instrument::Pluck { decay: 0.9, brightness: 0.5 }, None).unwrap();
        assert!(Arc::ptr_eq(&pluck.note(60, 0.0), &pluck.note(60, 1.0)));
        assert_eq!(&*pluck.note(60, 0.5), &synth::pluck(60, 0.9, 0.5)[..]);
    }
}
