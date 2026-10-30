//! The instruments a sound module can pick from, all synthesised so the program stays small.
//! Every note is a mono wave at [`SAMPLE_RATE`](crate::SAMPLE_RATE) that starts at its onset.

pub mod drums;
pub mod dsp;
pub mod synth;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bpe_core::Choice;

/// A built-in instrument.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Choice)]
pub enum Instrument {
    #[default]
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
    /// A drum kit: the key picks the piece (General MIDI drum map).
    Drums,
}

impl Instrument {
    pub const ALL: &'static [Instrument] = &[
        Instrument::Pluck,
        Instrument::ElectricPiano,
        Instrument::GrandPiano,
        Instrument::Marimba,
        Instrument::Vibraphone,
        Instrument::Glass,
        Instrument::Harp,
        Instrument::Bells,
        Instrument::Kalimba,
        Instrument::Supersaw,
        Instrument::Pad,
        Instrument::SynthBass,
        Instrument::Drums,
    ];

    /// The name shown in the editor.
    pub fn label(self) -> &'static str {
        match self {
            Instrument::Pluck => "Pluck",
            Instrument::ElectricPiano => "Electric piano",
            Instrument::GrandPiano => "Grand piano",
            Instrument::Marimba => "Marimba",
            Instrument::Vibraphone => "Vibraphone",
            Instrument::Glass => "Glass",
            Instrument::Harp => "Harp",
            Instrument::Bells => "Bells",
            Instrument::Kalimba => "Kalimba",
            Instrument::Supersaw => "Supersaw lead",
            Instrument::Pad => "Pad",
            Instrument::SynthBass => "Synth bass",
            Instrument::Drums => "Drums",
        }
    }

    /// One note at MIDI `key`, hit at `velocity` (0 to 1). The pluck also reads `decay` seconds
    /// and `brightness` (0 to 1); the others have their own character.
    pub fn render(self, key: u8, velocity: f32, decay: f32, brightness: f32) -> Vec<f32> {
        let v = velocity.clamp(0.0, 1.0);
        let key = key.min(127);
        let wave = match self {
            Instrument::Pluck => synth::pluck(key, decay.max(0.02), brightness.clamp(0.0, 1.0)),
            Instrument::ElectricPiano => synth::electric_piano(key, v),
            Instrument::GrandPiano => synth::grand_piano(key, v),
            Instrument::Marimba => synth::marimba(key, v),
            Instrument::Vibraphone => synth::vibraphone(key, v),
            Instrument::Glass => synth::glass(key, v),
            Instrument::Harp => synth::harp(key, v),
            Instrument::Bells => synth::bells(key, v),
            Instrument::Kalimba => synth::kalimba(key, v),
            Instrument::Supersaw => synth::supersaw(key, v),
            Instrument::Pad => synth::pad(key, v),
            Instrument::SynthBass => synth::synth_bass(key, v),
            Instrument::Drums => drums::render(key, v),
        };
        let mut wave = dsp::trim(wave);
        dsp::normalize(&mut wave, self.level() * (0.55 + 0.45 * v));
        wave
    }

    /// Peak level of a full-strength note, so instruments sit at similar loudness. Long,
    /// sustained sounds are set lower because they add up.
    fn level(self) -> f32 {
        match self {
            Instrument::Pad => 0.45,
            Instrument::Supersaw | Instrument::Bells | Instrument::Vibraphone | Instrument::Glass => 0.6,
            Instrument::SynthBass => 0.7,
            _ => 0.8,
        }
    }
}

/// Velocity steps rendered for instruments whose sound changes with how hard they're played.
const LAYERS: u32 = 8;

/// A note in the cache: instrument, key, velocity layer, and the pluck's decay and brightness bits.
type Slot = (Instrument, u8, u8, u32, u32);

fn slot(instrument: Instrument, key: u8, velocity: f32, decay: f32, brightness: f32) -> Slot {
    let layer = (velocity.clamp(0.0, 1.0) * (LAYERS - 1) as f32).round() as u8;
    let (decay, brightness) = match instrument {
        Instrument::Pluck => (decay.to_bits(), brightness.to_bits()),
        _ => (0, 0),
    };
    (instrument, key.min(127), layer, decay, brightness)
}

/// Rendered notes, so each one is only made once per soundtrack.
#[derive(Default)]
pub struct NoteCache {
    notes: BTreeMap<Slot, Arc<Vec<f32>>>,
}

impl NoteCache {
    /// One note, rendered now if it isn't cached.
    pub fn get(
        &mut self,
        instrument: Instrument,
        key: u8,
        velocity: f32,
        decay: f32,
        brightness: f32,
    ) -> Arc<Vec<f32>> {
        let s = slot(instrument, key, velocity, decay, brightness);
        self.notes.entry(s).or_insert_with(|| Arc::new(render_slot(s))).clone()
    }

    /// Renders every note in `wanted` (instrument, key, velocity, decay, brightness) that isn't
    /// cached yet, on all cores.
    pub fn prepare(&mut self, wanted: impl IntoIterator<Item = (Instrument, u8, f32, f32, f32)>) {
        let mut missing: Vec<Slot> = wanted
            .into_iter()
            .map(|(i, key, velocity, decay, brightness)| slot(i, key, velocity, decay, brightness))
            .filter(|s| !self.notes.contains_key(s))
            .collect();
        missing.sort_unstable();
        missing.dedup();
        let next = AtomicUsize::new(0);
        let made = Mutex::new(Vec::new());
        let work = || {
            while let Some(&s) = missing.get(next.fetch_add(1, Ordering::Relaxed)) {
                let wave = Arc::new(render_slot(s));
                made.lock().unwrap().push((s, wave));
            }
        };
        std::thread::scope(|scope| {
            for _ in 1..crate::level::threads().min(missing.len()) {
                scope.spawn(work);
            }
            work();
        });
        self.notes.extend(made.into_inner().unwrap());
    }
}

fn render_slot((instrument, key, layer, decay, brightness): Slot) -> Vec<f32> {
    let velocity = layer as f32 / (LAYERS - 1) as f32;
    instrument.render(key, velocity, f32::from_bits(decay), f32::from_bits(brightness))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Choice;

    #[test]
    fn every_instrument_renders_a_finite_note_and_caches_it() {
        let mut cache = NoteCache::default();
        for &i in Instrument::ALL {
            let wave = i.render(60, 0.8, 0.9, 0.5);
            assert!(!wave.is_empty() && wave.iter().all(|s| s.is_finite()), "{i:?}");
            assert!(wave.len() <= 7 * 48_000 + 1000, "{i:?} is {} samples", wave.len());
            assert!(wave.iter().fold(0.0f32, |m, s| m.max(s.abs())) <= 0.8 + 1e-3, "{i:?}");
            assert!(!i.label().is_empty());
        }
        let a = cache.get(Instrument::Marimba, 64, 0.50, 0.9, 0.5);
        let b = cache.get(Instrument::Marimba, 64, 0.52, 0.9, 0.5);
        assert!(std::sync::Arc::ptr_eq(&a, &b), "nearby velocities share a cached note");
        let c = cache.get(Instrument::Marimba, 64, 0.9, 0.9, 0.5);
        assert!(!std::sync::Arc::ptr_eq(&a, &c));
        assert!(Instrument::Pluck.render(60, 1.0, 0.3, 0.5).len() < Instrument::Pluck.render(60, 1.0, 0.9, 0.5).len());
    }

    #[test]
    fn instrument_names_are_snake_case_and_round_trip() {
        assert_eq!(Instrument::NAMES.len(), 13);
        assert_eq!(Instrument::Pluck.name(), "pluck");
        assert_eq!(Instrument::from_name("electric_piano"), Some(Instrument::ElectricPiano));
        assert_eq!(Instrument::from_name("drums"), Some(Instrument::Drums));
        assert_eq!(Instrument::from_name("drum_kit"), None);
    }
}
