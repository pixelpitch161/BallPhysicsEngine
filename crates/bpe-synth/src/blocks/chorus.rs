//! Chorus: copies of the sound on slowly moving short delays, for width and shimmer.

use super::filter::knee;
use crate::{BLOCK, Block, Buf, Ctx, Knobs, wired};

/// The middle of the moving delays, in seconds.
const CENTRE: f32 = 0.012;

pub struct Chorus {
    rate: f32,
    depth: f32,
    voices: usize,
    lines: [Vec<f32>; 2],
    pos: usize,
    /// Equal-power dry and wet levels; the voices add up as unrelated sounds.
    dry: f32,
    wet: f32,
    mix: f32,
}

impl Chorus {
    pub fn new(k: &Knobs) -> Self {
        let mix = k.number("mix", 0.5).clamp(0.0, 1.0);
        let voices = k.number("voices", 2.0).clamp(1.0, 4.0).round() as usize;
        let angle = mix * std::f32::consts::FRAC_PI_2;
        Self {
            rate: k.number("rate", 0.8).clamp(0.01, 10.0),
            depth: k.number("depth", 3.0).clamp(0.0, 10.0) / 1_000.0,
            voices,
            lines: [Vec::new(), Vec::new()],
            pos: 0,
            dry: angle.cos(),
            wet: angle.sin() / (voices as f32).sqrt(),
            mix,
        }
    }
}

impl Block for Chorus {
    fn set_knob(&mut self, name: &str, value: f32) {
        match name {
            "rate" => self.rate = value.clamp(0.01, 10.0),
            "depth" => self.depth = value.clamp(0.0, 10.0) / 1_000.0,
            "mix" => {
                self.mix = value.clamp(0.0, 1.0);
                let angle = self.mix * std::f32::consts::FRAC_PI_2;
                self.dry = angle.cos();
                self.wet = angle.sin() / (self.voices as f32).sqrt();
            }
            _ => {}
        }
    }

    fn reset(&mut self, cx: &Ctx) {
        let frames = ((CENTRE + self.depth) * cx.rate) as usize + 4;
        self.lines = [vec![0.0; frames], vec![0.0; frames]];
        self.pos = 0;
    }

    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let input = wired(inputs, 0).copied().unwrap_or(crate::SILENT);
        let len = self.lines[0].len();
        let per_voice = 1.0 / self.voices as f32;
        for s in 0..BLOCK {
            for side in 0..2 {
                self.lines[side][self.pos] = input[side][s];
            }
            let time = cx.time + s as f64 / cx.rate as f64;
            let phase = (time * self.rate as f64).fract() as f32;
            for side in 0..2 {
                let mut wet = 0.0;
                for v in 0..self.voices {
                    // Voices spread evenly round the cycle; the right side runs opposite the left.
                    let p = phase + v as f32 * per_voice + side as f32 * 0.5;
                    let delay = (CENTRE + self.depth * (std::f32::consts::TAU * p).sin()) * cx.rate;
                    let back = self.pos as f32 - delay;
                    let at = back.rem_euclid(len as f32);
                    let (i, frac) = (at.floor() as usize % len, at.fract());
                    let line = &self.lines[side];
                    wet += line[i] * (1.0 - frac) + line[(i + 1) % len] * frac;
                }
                outputs[0][side][s] = knee(input[side][s] * self.dry + wet * self.wet);
            }
            self.pos = (self.pos + 1) % len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{run, sine};
    use crate::{BLOCK, Ctx, Knobs};

    #[test]
    fn chorus_keeps_level() {
        // Noise, since any single tone can sit in one of the comb's notches.
        let mut rng = crate::Rng::new(9);
        let input: Vec<crate::Buf> = (0..1500)
            .map(|_| {
                let mut b = crate::SILENT;
                for s in 0..BLOCK {
                    b[0][s] = rng.next() * 0.5;
                    b[1][s] = b[0][s];
                }
                b
            })
            .collect();
        let out = run(&mut Chorus::new(&Knobs::default()), &mut Ctx::default(), 1500, |b, _| vec![Some(input[b])]);
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let dry: Vec<f32> = input.iter().flat_map(|b| b[0]).skip(4_800).collect();
        let wet: Vec<f32> = out.iter().map(|f| f[0]).skip(4_800).collect();
        let db = 20.0 * (rms(&wet) / rms(&dry)).log10();
        let _ = sine;
        assert!(db.abs() < 3.0, "{db}");
    }
}
