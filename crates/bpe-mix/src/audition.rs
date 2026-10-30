//! A short clip of chords to hear while editing them, played through a module's instrument.

use crate::chords::voicing;
use crate::{InstrumentChoice, SAMPLE_RATE, limit_peaks};
use bpe_core::{Chord, NoteLength, VoiceSettings};
use bpe_synth::PlayNote;

/// How long a note rings past its chord before it fades out, in seconds.
const RING: f64 = 0.25;

/// The chords one after another from the start, each held for its beats: interleaved stereo at
/// [`SAMPLE_RATE`]. Each chord is voiced from C3 up to C5, with its bass doubled an octave down.
pub fn audition(chords: &[Chord], instrument: &InstrumentChoice, seconds_per_beat: f64) -> Vec<f32> {
    let steps: Vec<(Vec<u8>, f64)> = chords
        .iter()
        .map(|c| {
            let mut keys = voicing(c, 48, 72, 1);
            if let Some(&bass) = keys.first() {
                keys.push(bass.saturating_sub(12));
            }
            (keys, c.beats as f64 * seconds_per_beat)
        })
        .collect();
    audition_keys(&steps, instrument, seconds_per_beat)
}

/// Each step's keys sounded together for its seconds, one step after another from the start.
pub fn audition_keys(steps: &[(Vec<u8>, f64)], instrument: &InstrumentChoice, seconds_per_beat: f64) -> Vec<f32> {
    let rate = SAMPLE_RATE as f64;
    let total: f64 = steps.iter().map(|(_, length)| length).sum();
    let len = ((total + RING) * rate).ceil() as usize;
    let mut out = vec![0.0f32; len * 2];
    let program = match instrument {
        InstrumentChoice::Patch { graph, .. } => bpe_synth::compile(graph).ok(),
        InstrumentChoice::BuiltIn { .. } => None,
    };
    let mut start = 0.0;
    for (keys, length) in steps {
        let length = *length;
        let from = (start * rate) as usize;
        let frames = ((length + RING) * rate) as usize;
        let fade = (RING * rate) as usize;
        let place = |out: &mut [f32], i: usize, l: f32, r: f32| {
            let Some(frame) = out.get_mut((from + i) * 2..(from + i) * 2 + 2) else { return false };
            let left = frames - i;
            let gain = 0.35 * if left < fade { left as f32 / fade as f32 } else { 1.0 };
            frame[0] += l * gain;
            frame[1] += r * gain;
            true
        };
        match (instrument, &program) {
            (InstrumentChoice::BuiltIn { which, decay, brightness }, _) => {
                for &key in keys {
                    let wave = which.render(key, 0.7, *decay, *brightness);
                    for (i, s) in wave.iter().take(frames).enumerate() {
                        if !place(&mut out, i, *s, *s) {
                            break;
                        }
                    }
                }
            }
            (InstrumentChoice::Patch { voices, .. }, Some(program)) => {
                let notes: Vec<PlayNote> = keys
                    .iter()
                    .map(|&key| PlayNote {
                        frame: 0,
                        key,
                        velocity: 0.7,
                        left: 1.0,
                        right: 1.0,
                        steal_at: None,
                        physics: None,
                    })
                    .collect();
                let held = VoiceSettings { note_length: NoteLength::Seconds(length as f32), mono: false, ..*voices };
                let audio =
                    bpe_synth::render_channel(program, &notes, &held, seconds_per_beat as f32, rate as f32, frames);
                for i in 0..frames {
                    if !place(&mut out, i, audio.samples[i * 2], audio.samples[i * 2 + 1]) {
                        break;
                    }
                }
            }
            // A patch that doesn't compile plays nothing.
            (InstrumentChoice::Patch { .. }, None) => {}
        }
        start += length;
    }
    limit_peaks(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Instrument;
    use bpe_core::{Extension, Quality};

    fn two_chords(instrument: &InstrumentChoice) -> Vec<f32> {
        let chords =
            [Chord::new(0, Quality::Major, Extension::Triad, 1), Chord::new(7, Quality::Major, Extension::Triad, 1)];
        audition(&chords, instrument, 0.5)
    }

    #[test]
    fn one_key_sounds_at_its_pitch() {
        // A4 through a plain patch: the clip crosses zero about 440 times a second.
        let patch = InstrumentChoice::Patch {
            name: "Patch".into(),
            graph: bpe_synth::blank_patch(),
            voices: VoiceSettings::default(),
        };
        let clip = audition_keys(&[(vec![69], 0.5)], &patch, 0.5);
        let rate = SAMPLE_RATE as usize;
        let left: Vec<f32> = clip.iter().step_by(2).copied().collect();
        let window = &left[rate / 10..rate / 10 + rate / 5];
        let crossings = window.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as f32 * 5.0;
        assert!((crossings - 440.0).abs() < 30.0, "{crossings} Hz");
    }

    #[test]
    fn audition_places_chords_on_beats() {
        let marimba = InstrumentChoice::BuiltIn { which: Instrument::Marimba, decay: 0.4, brightness: 0.5 };
        let patch = InstrumentChoice::Patch {
            name: "Patch".into(),
            graph: bpe_synth::blank_patch(),
            voices: VoiceSettings::default(),
        };
        for instrument in [marimba, patch] {
            let clip = two_chords(&instrument);
            let rate = SAMPLE_RATE as usize;
            let first_sound = |from: usize| (from..clip.len() / 2).find(|&f| clip[f * 2].abs() > 1e-3).unwrap();
            assert!(first_sound(0) < rate / 1000, "the first chord starts at once");
            assert!(clip.len() / 2 >= rate, "both chords fit");
            assert!(clip.iter().all(|s| s.abs() <= 1.0));
        }
        // The marimba's second attack beats the first chord's tail just before it.
        let clip = two_chords(&InstrumentChoice::BuiltIn { which: Instrument::Marimba, decay: 0.4, brightness: 0.5 });
        let rate = SAMPLE_RATE as usize;
        let peak = |a: usize, b: usize| (a..b).map(|f| clip[f * 2].abs()).fold(0.0, f32::max);
        let half = rate / 2;
        assert!(peak(half, half + rate / 100) > 2.0 * peak(half - rate / 100, half));
    }
}
