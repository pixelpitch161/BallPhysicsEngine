//! Reverb: a Freeverb-style network of combs and allpasses with gently moving allpass delays,
//! its tail set by the decay time (the time it takes to fall by 60 dB, with no damping).

use super::filter::{flush, knee};
use crate::{BLOCK, Block, Buf, Ctx, Knobs, wired};

/// Freeverb's comb and allpass lengths at 44.1 kHz; the right side's are 23 longer.
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;

/// A feedback comb with a damping low-pass in its loop. Its length is whole frames: an
/// interpolated read would lose highs on every pass and cut the tail short.
struct Comb {
    line: Vec<f32>,
    pos: usize,
    feedback: f32,
    damp: f32,
    store: f32,
}

impl Comb {
    fn tick(&mut self, x: f32) -> f32 {
        let out = self.line[self.pos];
        self.store = flush(out * (1.0 - self.damp) + self.store * self.damp);
        self.line[self.pos] = flush(knee(x + self.store * self.feedback));
        self.pos = (self.pos + 1) % self.line.len();
        out
    }
}

/// A diffusing allpass whose delay wobbles by up to a frame, so the tail doesn't ring
/// metallically.
struct Allpass {
    line: Vec<f32>,
    length: f32,
    pos: usize,
    /// The wobble's cycles per second and starting phase.
    wobble: (f32, f32),
}

impl Allpass {
    fn tick(&mut self, x: f32, time: f64) -> f32 {
        let len = self.line.len();
        let wobble = (std::f32::consts::TAU * ((time * self.wobble.0 as f64).fract() as f32 + self.wobble.1)).sin();
        let at = (self.pos as f32 - self.length - wobble).rem_euclid(len as f32);
        let (i, frac) = (at.floor() as usize % len, at.fract());
        let delayed = self.line[i] * (1.0 - frac) + self.line[(i + 1) % len] * frac;
        self.line[self.pos] = flush(x + delayed * 0.5);
        self.pos = (self.pos + 1) % len;
        delayed - x
    }
}

pub struct Reverb {
    size: f32,
    decay: f32,
    damping: f32,
    predelay: f32,
    width: f32,
    mix: f32,
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    pre: [Vec<f32>; 2],
    pre_pos: usize,
    /// Input gain, set so long decays don't build up to a roar.
    gain: f32,
}

impl Reverb {
    pub fn new(k: &Knobs) -> Self {
        Self {
            size: k.number("size", 0.6).clamp(0.0, 1.0),
            decay: k.number("decay", 2.0).clamp(0.1, 60.0),
            damping: k.number("damping", 0.4).clamp(0.0, 1.0),
            predelay: k.number("predelay", 10.0).clamp(0.0, 200.0) / 1_000.0,
            width: k.number("width", 1.0).clamp(0.0, 1.0),
            mix: k.number("mix", 0.3).clamp(0.0, 1.0),
            combs: [Vec::new(), Vec::new()],
            allpasses: [Vec::new(), Vec::new()],
            pre: [Vec::new(), Vec::new()],
            pre_pos: 0,
            gain: 0.015,
        }
    }
}

impl Block for Reverb {
    fn reset(&mut self, cx: &Ctx) {
        let scale = cx.rate / 44_100.0 * (0.5 + self.size);
        let mut mean_feedback = 0.0;
        for side in 0..2 {
            self.combs[side] = COMBS
                .iter()
                .map(|&n| {
                    let length = ((n + side * SPREAD) as f32 * scale).max(8.0).round();
                    // Each pass round the loop loses its share of 60 dB over the decay time.
                    // and the damping's loss at 1 kHz is made up, so the decay holds through the mids.
                    let damp = self.damping * 0.4;
                    let w = std::f32::consts::TAU * 1_000.0 / cx.rate;
                    let loss = (1.0 - damp) / (1.0 - 2.0 * damp * w.cos() + damp * damp).sqrt();
                    let feedback = (10f32.powf(-3.0 * length / cx.rate / self.decay) / loss).min(0.999);
                    mean_feedback += feedback / 16.0;
                    Comb { line: vec![0.0; length as usize], pos: 0, feedback, damp, store: 0.0 }
                })
                .collect();
            self.allpasses[side] = ALLPASSES
                .iter()
                .enumerate()
                .map(|(i, &n)| {
                    let length = ((n + side * SPREAD) as f32 * cx.rate / 44_100.0).round();
                    Allpass {
                        line: vec![0.0; length as usize + 4],
                        length,
                        pos: 0,
                        wobble: (0.1 + 0.05 * i as f32, (i * 2 + side) as f32 / 8.0),
                    }
                })
                .collect();
            self.pre[side] = vec![0.0; (self.predelay * cx.rate) as usize + 1];
        }
        self.pre_pos = 0;
        // Freeverb's level for its usual feedback (0.84), held there for longer tails.
        let usual = (1.0f32 - 0.84 * 0.84).sqrt();
        self.gain = 0.015 * (1.0 - mean_feedback * mean_feedback).max(1e-6).sqrt() / usual;
    }

    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let input = wired(inputs, 0).copied().unwrap_or(crate::SILENT);
        let (wet1, wet2) = (self.width * 0.5 + 0.5, (1.0 - self.width) * 0.5);
        let pre_len = self.pre[0].len();
        for s in 0..BLOCK {
            let time = cx.time + s as f64 / cx.rate as f64;
            let mut wet = [0.0f32; 2];
            for side in 0..2 {
                let delayed = if pre_len > 1 {
                    let d = self.pre[side][self.pre_pos];
                    self.pre[side][self.pre_pos] = input[side][s];
                    d
                } else {
                    input[side][s]
                };
                let x = delayed * self.gain;
                let mut sum = 0.0;
                for comb in &mut self.combs[side] {
                    sum += comb.tick(x);
                }
                for ap in &mut self.allpasses[side] {
                    sum = ap.tick(sum, time);
                }
                wet[side] = sum * 3.0;
            }
            self.pre_pos = (self.pre_pos + 1) % pre_len;
            outputs[0][0][s] = knee(input[0][s] * (1.0 - self.mix) + (wet[0] * wet1 + wet[1] * wet2) * self.mix);
            outputs[0][1][s] = knee(input[1][s] * (1.0 - self.mix) + (wet[1] * wet1 + wet[0] * wet2) * self.mix);
        }
    }

    fn idle(&self) -> bool {
        self.combs.iter().flatten().all(|c| c.store == 0.0 && c.line.iter().all(|v| *v == 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{run, sine};
    use crate::{BLOCK, Ctx, Knobs, SILENT};
    use bpe_graph::Param;

    #[test]
    fn reverb_tail_reaches_minus_60_at_decay() {
        let k = Knobs::default()
            .with("decay", Param::Number(2.0))
            .with("mix", Param::Number(1.0))
            .with("predelay", Param::Number(0.0))
            // Damping shortens the highs by design; the decay is the time with none.
            .with("damping", Param::Number(0.0));
        let blocks = 4 * 48_000 / BLOCK;
        let out = run(&mut Reverb::new(&k), &mut Ctx::default(), blocks, |b, _| {
            let mut buf = SILENT;
            if b == 0 {
                buf[0][0] = 1.0;
                buf[1][0] = 1.0;
            }
            vec![Some(buf)]
        });
        // Schroeder's backward integration of the squared response.
        let mut edc: Vec<f64> = out.iter().map(|f| (f[0] as f64).powi(2)).collect();
        for i in (0..edc.len() - 1).rev() {
            edc[i] += edc[i + 1];
        }
        let total = edc[0];
        let at = |db: f64| edc.iter().position(|&e| 10.0 * (e / total).log10() < db).unwrap() as f64 / 48_000.0;
        let t60 = (at(-35.0) - at(-5.0)) * 2.0;
        assert!((t60 - 2.0).abs() < 0.2, "RT60 {t60}");
    }

    #[test]
    fn extreme_effect_knobs_stay_finite() {
        let reverb = Knobs::default().with("decay", Param::Number(60.0)).with("size", Param::Number(1.0));
        let delay = Knobs::default().with("feedback", Param::Number(1.0)).with("ms", Param::Number(37.0));
        let chorus = Knobs::default().with("depth", Param::Number(10.0)).with("voices", Param::Number(4.0));
        let mut blocks: Vec<Box<dyn crate::Block>> = vec![
            Box::new(Reverb::new(&reverb)),
            Box::new(super::super::Delay::new(&delay)),
            Box::new(super::super::Chorus::new(&chorus)),
        ];
        for b in blocks.iter_mut() {
            let out = run(b.as_mut(), &mut Ctx::default(), 60 * 48_000 / BLOCK, |n, _| {
                // A full-scale saw.
                let mut buf = SILENT;
                for s in 0..BLOCK {
                    let v = (((n * BLOCK + s) as f32 * 110.0 / 48_000.0).fract()) * 2.0 - 1.0;
                    buf[0][s] = v;
                    buf[1][s] = v;
                }
                vec![Some(buf)]
            });
            assert!(out.iter().all(|f| f[0].is_finite() && f[1].is_finite() && f[0].abs() < 4.0 && f[1].abs() < 4.0));
        }
        let _ = sine;
    }
}
