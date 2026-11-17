//! What a liquid does to a note (liquid part 5): Pitch from Speed, Depth or Colour, and the muffle
//! of a note whose ball is under a liquid. Read from the sound's own world as the note starts
//! (`bpe_kit::things::liquid::wet_of`, `stats_of`); nothing here allocates.

use super::notes::{MAX_PICK, NotePick};
use super::slots::NotesSpec;
use crate::music::scale::in_scale;
use bpe_kit::sound::PitchFrom;
use bpe_kit::things::liquid::{stats_of, wet_of};
use bpe_runtime::{Party, World};

/// Under this saturation a colour has no hue: it plays the written note.
const GREY: f32 = 0.08;
/// A liquid touching a ball plays its own note in a Colour chord from this share of the contacts.
pub const CHORD_SHARE: f32 = 0.2;
/// A note's cutoff with no muffle (Hz).
const OPEN: f32 = 20_000.0;

/// Semitones Pitch from Speed or Depth adds to a note `ball` plays at `strength` (0..1).
pub fn pitch_shift(n: &NotesSpec, world: &World, ball: Option<Party>, strength: f32) -> f32 {
    let range = n.pitch_range;
    match n.pitch_from {
        PitchFrom::Speed => range * strength.clamp(0.0, 1.0),
        PitchFrom::Depth => {
            let Some(w) = ball.and_then(|b| wet_of(world, b)) else { return 0.0 };
            -range * (w.depth / n.deep_at).clamp(0.0, 1.0)
        }
        PitchFrom::Off | PitchFrom::Colour => 0.0,
    }
}

/// The hue of linear `rgb`, 0..1 round the wheel from red, or `None` for a grey.
pub fn hue(rgb: [f32; 3]) -> Option<f32> {
    let [r, g, b] = rgb.map(|c| if c.is_finite() { c.max(0.0) } else { 0.0 });
    let hi = r.max(g).max(b);
    let lo = r.min(g).min(b);
    if hi <= 1e-6 || (hi - lo) / hi < GREY {
        return None;
    }
    let d = hi - lo;
    let h = if hi == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if hi == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    Some((h / 6.0).rem_euclid(1.0))
}

/// The note colour `rgb` plays from `written`: its hue across the Pitch range, moved up into the
/// key. A grey plays the note as written.
pub fn colour_note(n: &NotesSpec, written: u8, rgb: [f32; 3]) -> u8 {
    let Some(h) = hue(rgb) else { return written };
    let at = (written as f32 + (h * n.pitch_range).round()).clamp(0.0, 127.0) as i32;
    // The nearest note of the key at or past it, the way the range goes.
    let way = if n.pitch_range < 0.0 { -1 } else { 1 };
    (0..12)
        .map(|k| at + way * k)
        .find(|&m| (0..=127).contains(&m) && in_scale(n.root, n.scale, m as u8))
        .unwrap_or(at)
        .clamp(0, 127) as u8
}

/// Pitch from Colour: `pick` with its notes moved to the colour round `ball` (each note by the
/// same step as its first); where two or more liquids touch the ball, each at least
/// [`CHORD_SHARE`] of the contacts, every one's own colour plays its note together, from the
/// pick's first note. A ball in no liquid keeps the pick.
pub fn colour_pick(n: &NotesSpec, world: &World, ball: Party, pick: NotePick) -> NotePick {
    let Some(first) = pick.notes().first().copied() else { return pick };
    let Some(w) = wet_of(world, ball) else { return pick };
    let mut out = NotePick::default();
    out.strum = pick.strum;
    let many = w.n >= 2 && w.liquids[1].1 >= CHORD_SHARE;
    if many {
        for &(liquid, share) in w.liquids.iter().take(w.n) {
            if share < CHORD_SHARE {
                continue;
            }
            let Some(s) = stats_of(world, |t| t == liquid) else { continue };
            let note = colour_note(n, first, s.colour);
            if !out.notes().contains(&note) && out.notes().len() < MAX_PICK {
                out.push(note);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    let note = colour_note(n, first, w.colour);
    for &p in pick.notes() {
        let moved = (p as i32 + note as i32 - first as i32).clamp(0, 127) as u8;
        if !out.notes().contains(&moved) {
            out.push(moved);
        }
    }
    out
}

/// The cutoff (Hz) a note `ball` plays is muffled to under a liquid, `None` when it isn't.
pub fn muffle_cutoff(n: &NotesSpec, world: &World, ball: Option<Party>) -> Option<f32> {
    if n.under <= 0.0 {
        return None;
    }
    let w = ball.and_then(|b| wet_of(world, b))?;
    let amount = n.under * (w.depth / n.deep_at).clamp(0.0, 1.0);
    (amount > 0.0).then(|| OPEN * (n.under_cutoff / OPEN).powf(amount))
}

/// A note's muffle: a 12 dB low-pass (two one-poles a side), set as the note starts.
#[derive(Clone, Copy, Debug, Default)]
pub struct Muffle {
    on: bool,
    a: f32,
    z: [[f32; 2]; 2],
}

impl Muffle {
    /// Off, or on at `cutoff` Hz, from silence.
    pub fn set(&mut self, cutoff: Option<f32>) {
        self.z = [[0.0; 2]; 2];
        self.retune(cutoff);
    }

    /// A new cutoff (or off) for a note that goes on sounding (Mono): what it holds stays.
    pub fn retune(&mut self, cutoff: Option<f32>) {
        match cutoff.filter(|c| c.is_finite() && *c > 0.0) {
            Some(c) => {
                self.on = true;
                self.a = 1.0 - (-std::f32::consts::TAU * c.min(OPEN) / crate::SAMPLE_RATE).exp();
            }
            None => self.on = false,
        }
    }

    /// Filters the note's block in place (nothing when off).
    pub fn run(&mut self, l: &mut [f32], r: &mut [f32]) {
        if !self.on {
            return;
        }
        let a = self.a;
        for (side, buf) in [l, r].into_iter().enumerate() {
            let [mut z1, mut z2] = self.z[side];
            for x in buf.iter_mut() {
                z1 += a * (*x - z1);
                z2 += a * (z1 - z2);
                *x = z2;
            }
            // Denormals off and no NaN kept.
            self.z[side] = [z1, z2].map(|z| if z.is_finite() && z.abs() > 1e-20 { z } else { 0.0 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_kit::sound::{Root, Scale, VoiceSettings};

    fn spec(range: f32) -> NotesSpec {
        let v = VoiceSettings {
            play_pitch_from: PitchFrom::Colour,
            play_pitch_range: range,
            notes_root: Root::C,
            notes_scale: Scale::Major,
            ..VoiceSettings::default()
        };
        NotesSpec::from_settings(&v, 4, [None, None, None])
    }

    #[test]
    fn hues_go_round_from_red() {
        assert_eq!(hue([1.0, 0.0, 0.0]), Some(0.0));
        assert!((hue([0.0, 1.0, 0.0]).unwrap() - 1.0 / 3.0).abs() < 1e-6);
        assert!((hue([0.0, 0.0, 1.0]).unwrap() - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(hue([0.5, 0.5, 0.5]), None, "grey");
        assert_eq!(hue([0.0, 0.0, 0.0]), None, "black");
    }

    #[test]
    fn a_colour_plays_a_note_of_the_key_across_the_range() {
        let n = spec(12.0);
        assert_eq!(colour_note(&n, 60, [1.0, 0.0, 0.0]), 60, "red plays the written note");
        // Blue is two thirds round: 8 semitones up, G# moves up into C major's A.
        assert_eq!(colour_note(&n, 60, [0.0, 0.0, 1.0]), 69);
        assert_eq!(colour_note(&n, 60, [0.4, 0.4, 0.4]), 60, "grey");
        for k in 0..12 {
            let rgb = [k as f32 / 11.0, 1.0 - k as f32 / 11.0, 0.3];
            let m = colour_note(&n, 60, rgb);
            assert!(in_scale(Root::C, Scale::Major, m) && (60..=72).contains(&m), "{m}");
        }
    }

    #[test]
    fn a_muffle_takes_the_highs_and_off_leaves_the_sound_alone() {
        let tone = |hz: f32| -> Vec<f32> {
            (0..4800).map(|i| (std::f32::consts::TAU * hz * i as f32 / crate::SAMPLE_RATE).sin()).collect()
        };
        let energy = |x: &[f32]| x[2400..].iter().map(|s| s * s).sum::<f32>();
        let mut m = Muffle::default();
        m.set(Some(300.0));
        let (mut hl, mut hr) = (tone(5000.0), tone(5000.0));
        m.run(&mut hl, &mut hr);
        assert!(energy(&hl) < energy(&tone(5000.0)) * 0.02, "5 kHz is cut");
        m.set(Some(300.0));
        let (mut ll, mut lr) = (tone(100.0), tone(100.0));
        m.run(&mut ll, &mut lr);
        assert!(energy(&ll) > energy(&tone(100.0)) * 0.6, "100 Hz passes");
        m.set(None);
        let (mut a, mut b) = (tone(5000.0), tone(5000.0));
        m.run(&mut a, &mut b);
        assert_eq!(a, tone(5000.0), "off, bit for bit");
    }
}
