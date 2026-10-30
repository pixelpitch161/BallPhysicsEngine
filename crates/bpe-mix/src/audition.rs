//! A short clip of chords to hear while editing them, played through a module's instrument.

use crate::chords::voicing;
use crate::{Instrument, SAMPLE_RATE, limit_peaks};
use bpe_core::Chord;

/// How long a note rings past its chord before it fades out, in seconds.
const RING: f64 = 0.25;

/// The chords one after another from the start, each held for its beats: interleaved stereo at
/// [`SAMPLE_RATE`]. Each chord is voiced from C3 up to C5, with its bass doubled an octave down.
pub fn audition(
    chords: &[Chord],
    instrument: Instrument,
    seconds_per_beat: f64,
    decay: f32,
    brightness: f32,
) -> Vec<f32> {
    let rate = SAMPLE_RATE as f64;
    let total: f64 = chords.iter().map(|c| c.beats as f64 * seconds_per_beat).sum();
    let mut out = vec![0.0f32; ((total + RING) * rate).ceil() as usize * 2];
    let mut start = 0.0;
    for c in chords {
        let length = c.beats as f64 * seconds_per_beat;
        let mut keys = voicing(c, 48, 72, 1);
        if let Some(&bass) = keys.first() {
            keys.push(bass.saturating_sub(12));
        }
        let from = (start * rate) as usize;
        let frames = ((length + RING) * rate) as usize;
        let fade = (RING * rate) as usize;
        for key in keys {
            let wave = instrument.render(key, 0.7, decay, brightness);
            for (i, s) in wave.iter().take(frames).enumerate() {
                let Some(frame) = out.get_mut((from + i) * 2..(from + i) * 2 + 2) else { break };
                let left = frames - i;
                let gain = 0.35 * if left < fade { left as f32 / fade as f32 } else { 1.0 };
                frame[0] += s * gain;
                frame[1] += s * gain;
            }
        }
        start += length;
    }
    limit_peaks(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Extension, Quality};

    #[test]
    fn audition_places_chords_on_beats() {
        let chords =
            [Chord::new(0, Quality::Major, Extension::Triad, 1), Chord::new(7, Quality::Major, Extension::Triad, 1)];
        let clip = audition(&chords, Instrument::Marimba, 0.5, 0.4, 0.5);
        let rate = SAMPLE_RATE as usize;
        let first_sound = |from: usize| (from..clip.len() / 2).find(|&f| clip[f * 2].abs() > 1e-3).unwrap();
        assert!(first_sound(0) < rate / 1000, "the first chord starts at once");
        // The second chord's attack: the loudest moment just after half a second beats the
        // first chord's tail just before it.
        let peak = |a: usize, b: usize| (a..b).map(|f| clip[f * 2].abs()).fold(0.0, f32::max);
        let half = rate / 2;
        assert!(peak(half, half + rate / 100) > 2.0 * peak(half - rate / 100, half));
        assert!(clip.len() / 2 >= rate, "both chords fit");
        assert!(clip.iter().all(|s| s.abs() <= 1.0));
    }
}
