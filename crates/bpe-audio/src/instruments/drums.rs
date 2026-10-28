//! A synthesised drum kit. Notes follow the General MIDI drum map, so drum tracks from MIDI
//! files play the right pieces; other notes pick a piece by their position in the octave.

use super::dsp::{Noise, SR, Svf, attack};
use std::f32::consts::TAU;

/// One piece of the drum kit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Piece {
    Kick,
    Snare,
    Clap,
    ClosedHat,
    OpenHat,
    LowTom,
    MidTom,
    HighTom,
    Crash,
    Ride,
    Rim,
    Woodblock,
}

impl Piece {
    pub const ALL: [Piece; 12] = [
        Piece::Kick,
        Piece::Snare,
        Piece::Clap,
        Piece::ClosedHat,
        Piece::OpenHat,
        Piece::LowTom,
        Piece::MidTom,
        Piece::HighTom,
        Piece::Crash,
        Piece::Ride,
        Piece::Rim,
        Piece::Woodblock,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Piece::Kick => "Kick",
            Piece::Snare => "Snare",
            Piece::Clap => "Clap",
            Piece::ClosedHat => "Closed hi-hat",
            Piece::OpenHat => "Open hi-hat",
            Piece::LowTom => "Low tom",
            Piece::MidTom => "Mid tom",
            Piece::HighTom => "High tom",
            Piece::Crash => "Crash",
            Piece::Ride => "Ride",
            Piece::Rim => "Rim",
            Piece::Woodblock => "Woodblock",
        }
    }

    /// The General MIDI drum note for this piece.
    pub fn note(self) -> u8 {
        match self {
            Piece::Kick => 36,
            Piece::Rim => 37,
            Piece::Snare => 38,
            Piece::Clap => 39,
            Piece::LowTom => 41,
            Piece::ClosedHat => 42,
            Piece::MidTom => 45,
            Piece::OpenHat => 46,
            Piece::HighTom => 48,
            Piece::Crash => 49,
            Piece::Ride => 51,
            Piece::Woodblock => 76,
        }
    }

    /// The piece a MIDI note plays: the General MIDI drum map, or for any other note, a piece
    /// picked by its place in the octave so melodies still turn into a varied beat.
    pub fn from_note(note: u8) -> Piece {
        match note {
            35 | 36 => Piece::Kick,
            37 => Piece::Rim,
            38 | 40 => Piece::Snare,
            39 => Piece::Clap,
            41 | 43 => Piece::LowTom,
            42 | 44 => Piece::ClosedHat,
            45 | 47 => Piece::MidTom,
            46 => Piece::OpenHat,
            48 | 50 => Piece::HighTom,
            49 | 52 | 55 | 57 => Piece::Crash,
            51 | 53 | 59 => Piece::Ride,
            76 | 77 => Piece::Woodblock,
            _ => [
                Piece::Kick,
                Piece::Rim,
                Piece::Snare,
                Piece::Clap,
                Piece::ClosedHat,
                Piece::LowTom,
                Piece::ClosedHat,
                Piece::MidTom,
                Piece::OpenHat,
                Piece::HighTom,
                Piece::Ride,
                Piece::Woodblock,
            ][(note % 12) as usize],
        }
    }

    pub fn from_name(name: &str) -> Option<Piece> {
        Piece::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(name))
    }

    pub fn render(self, velocity: f32) -> Vec<f32> {
        let mut out = match self {
            Piece::Kick => kick(velocity),
            Piece::Snare => snare(velocity),
            Piece::Clap => clap(),
            Piece::ClosedHat => hat(0.06, velocity),
            Piece::OpenHat => hat(0.45, velocity),
            Piece::LowTom => tom(85.0),
            Piece::MidTom => tom(125.0),
            Piece::HighTom => tom(175.0),
            Piece::Crash => cymbal(1.8, 5000.0),
            Piece::Ride => cymbal(1.2, 7000.0),
            Piece::Rim => rim(),
            Piece::Woodblock => woodblock(),
        };
        attack(&mut out, 0.0005);
        out
    }
}

fn buffer(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * SR) as usize]
}

fn kick(velocity: f32) -> Vec<f32> {
    let mut out = buffer(0.45);
    let mut phase = 0.0f32;
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let freq = 45.0 + (110.0 + 60.0 * velocity) * (-t / 0.035).exp();
        phase += TAU * freq / SR;
        let body = phase.sin() * (-t / 0.22).exp();
        let click = if t < 0.002 { (1.0 - t / 0.002) * 0.4 * velocity } else { 0.0 };
        *s = body + click;
    }
    out
}

fn snare(velocity: f32) -> Vec<f32> {
    let mut out = buffer(0.3);
    let mut noise = Noise::new(38);
    let mut svf = Svf::default();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let tone = ((TAU * 185.0 * t).sin() + 0.5 * (TAU * 330.0 * t).sin()) * (-t / 0.05).exp();
        let rattle = svf.run(noise.next(), 4000.0, 0.5).high * (-t / (0.08 + 0.05 * velocity)).exp();
        *s = tone * 0.5 + rattle * 0.8;
    }
    out
}

fn clap() -> Vec<f32> {
    let mut out = buffer(0.3);
    let mut noise = Noise::new(39);
    let mut svf = Svf::default();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        // Three quick slaps, then the tail.
        let burst = (0..3).map(|k| t - k as f32 * 0.011).filter(|&d| d >= 0.0).map(|d| (-d / 0.006).exp()).fold(0.0, f32::max);
        let env = burst.max(if t > 0.022 { (-(t - 0.022) / 0.09).exp() * 0.7 } else { 0.0 });
        *s = svf.run(noise.next(), 1200.0, 1.5).band * env * 2.0;
    }
    out
}

/// Six square waves at clashing, unrelated pitches: the classic drum machine metal.
fn metal(t: f32) -> f32 {
    [205.3f32, 304.4, 369.6, 522.7, 540.0, 800.0]
        .iter()
        .map(|f| if (f * 1.7 * t).fract() < 0.5 { 1.0 } else { -1.0 })
        .sum::<f32>()
        / 6.0
}

fn hat(length: f32, velocity: f32) -> Vec<f32> {
    let mut out = buffer(length * 4.0);
    let mut noise = Noise::new(42);
    let mut svf = Svf::default();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let raw = metal(t) * 0.6 + noise.next() * 0.4;
        *s = svf.run(raw, 8000.0 + 2000.0 * velocity, 0.7).high * (-t * 6.9 / length).exp() * 0.7;
    }
    out
}

fn tom(freq: f32) -> Vec<f32> {
    let mut out = buffer(0.6);
    let mut phase = 0.0f32;
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let f = freq * (1.0 + 0.6 * (-t / 0.04).exp());
        phase += TAU * f / SR;
        *s = phase.sin() * (-t / 0.18).exp();
    }
    out
}

fn cymbal(length: f32, cutoff: f32) -> Vec<f32> {
    let mut out = buffer(length);
    let mut noise = Noise::new(49);
    let mut svf = Svf::default();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let raw = metal(t * 1.3) * 0.5 + noise.next() * 0.5;
        *s = svf.run(raw, cutoff, 0.6).high * (-t * 6.9 / length).exp() * 0.6;
    }
    out
}

fn rim() -> Vec<f32> {
    let mut out = buffer(0.08);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        *s = ((TAU * 1700.0 * t).sin() * 0.7 + (TAU * 480.0 * t).sin() * 0.3) * (-t / 0.012).exp();
    }
    out
}

fn woodblock() -> Vec<f32> {
    let mut out = buffer(0.15);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        *s = ((TAU * 820.0 * t).sin() + 0.4 * (TAU * 2240.0 * t).sin()) * (-t / 0.03).exp();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_midi_notes_map_to_their_pieces_and_back() {
        for piece in Piece::ALL {
            assert_eq!(Piece::from_note(piece.note()), piece, "{}", piece.name());
            assert_eq!(Piece::from_name(piece.name()), Some(piece));
        }
        // Melody notes still map to some piece.
        for note in 0..=127 {
            let _ = Piece::from_note(note);
        }
    }

    #[test]
    fn every_piece_makes_a_finite_sound() {
        for piece in Piece::ALL {
            let wave = piece.render(0.8);
            let peak = wave.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.05 && peak.is_finite(), "{} peak {peak}", piece.name());
        }
    }
}
