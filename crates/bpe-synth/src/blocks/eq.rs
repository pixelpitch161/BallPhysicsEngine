//! A three-band EQ (low shelf, peak, high shelf) with low and high cuts, from RBJ biquads.

use super::filter::flush;
use crate::{BLOCK, Block, Buf, Ctx, Knobs, wired};

/// One biquad in transposed direct form II, with state per side.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [[f32; 2]; 2],
}

impl Biquad {
    fn from(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b: [(b0 / a0) as f32, (b1 / a0) as f32, (b2 / a0) as f32],
            a: [(a1 / a0) as f32, (a2 / a0) as f32],
            z: [[0.0; 2]; 2],
        }
    }

    fn w(hz: f32, rate: f32) -> (f64, f64) {
        let w = std::f64::consts::TAU * hz.clamp(10.0, rate * 0.49) as f64 / rate as f64;
        (w.cos(), w.sin())
    }

    pub fn peak(hz: f32, q: f32, db: f32, rate: f32) -> Self {
        let a = 10f64.powf(db as f64 / 40.0);
        let (c, s) = Self::w(hz, rate);
        let alpha = s / (2.0 * q.max(0.05) as f64);
        Self::from(1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a)
    }

    pub fn shelf(hz: f32, db: f32, rate: f32, high: bool) -> Self {
        let a = 10f64.powf(db as f64 / 40.0);
        let (c, s) = Self::w(hz, rate);
        let alpha = s / 2.0 * std::f64::consts::SQRT_2;
        let r = 2.0 * a.sqrt() * alpha;
        let sign = if high { -1.0 } else { 1.0 };
        let (ap, am) = (a + 1.0, a - 1.0);
        Self::from(
            a * (ap - sign * am * c + r),
            sign * 2.0 * a * (am - sign * ap * c),
            a * (ap - sign * am * c - r),
            ap + sign * am * c + r,
            -sign * 2.0 * (am + sign * ap * c),
            ap + sign * am * c - r,
        )
    }

    pub fn pass(hz: f32, rate: f32, high: bool) -> Self {
        let (c, s) = Self::w(hz, rate);
        let alpha = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        if high {
            Self::from((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
        } else {
            Self::from((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
        }
    }

    #[inline]
    pub fn tick(&mut self, side: usize, x: f32) -> f32 {
        let z = &mut self.z[side];
        let y = self.b[0] * x + z[0];
        z[0] = flush(self.b[1] * x - self.a[0] * y + z[1]);
        z[1] = flush(self.b[2] * x - self.a[1] * y);
        y
    }
}

/// The settings the bands are worked out from once the rate is known.
#[derive(Clone, Copy, Debug)]
struct Bands {
    low_cut: f32,
    low: (f32, f32),
    mid: (f32, f32, f32),
    high: (f32, f32),
    high_cut: f32,
}

pub struct Equaliser {
    bands: Bands,
    rate: f32,
    filters: Vec<Biquad>,
}

impl Equaliser {
    pub fn new(k: &Knobs) -> Self {
        let gain = |n| k.number(n, 0.0).clamp(-18.0, 18.0);
        Self {
            bands: Bands {
                low_cut: k.number("low_cut", 20.0),
                low: (k.number("low_freq", 120.0), gain("low_gain")),
                mid: (k.number("mid_freq", 1_000.0), k.number("mid_q", 0.7), gain("mid_gain")),
                high: (k.number("high_freq", 8_000.0), gain("high_gain")),
                high_cut: k.number("high_cut", 20_000.0),
            },
            rate: 0.0,
            filters: Vec::new(),
        }
    }

    /// The filters that do something: a cut at its extreme and a band at 0 dB are left out.
    fn build(&mut self, rate: f32) {
        let b = self.bands;
        let mut f = Vec::new();
        if b.low_cut > 20.0 {
            f.push(Biquad::pass(b.low_cut, rate, true));
        }
        if b.low.1 != 0.0 {
            f.push(Biquad::shelf(b.low.0, b.low.1, rate, false));
        }
        if b.mid.2 != 0.0 {
            f.push(Biquad::peak(b.mid.0, b.mid.1, b.mid.2, rate));
        }
        if b.high.1 != 0.0 {
            f.push(Biquad::shelf(b.high.0, b.high.1, rate, true));
        }
        if b.high_cut < 20_000.0 {
            f.push(Biquad::pass(b.high_cut, rate, false));
        }
        self.filters = f;
        self.rate = rate;
    }
}

impl Block for Equaliser {
    fn set_knob(&mut self, name: &str, value: f32) {
        let gain = value.clamp(-18.0, 18.0);
        match name {
            "low_gain" => self.bands.low.1 = gain,
            "mid_gain" => self.bands.mid.2 = gain,
            "high_gain" => self.bands.high.1 = gain,
            _ => return,
        }
        // New coefficients, keeping each filter's memory when the same filters remain.
        let old = std::mem::take(&mut self.filters);
        let rate = if self.rate > 0.0 { self.rate } else { 48_000.0 };
        self.build(rate);
        if old.len() == self.filters.len() {
            for (new, old) in self.filters.iter_mut().zip(old) {
                new.z = old.z;
            }
        }
    }

    fn reset(&mut self, cx: &Ctx) {
        self.build(cx.rate);
    }

    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let Some(input) = wired(inputs, 0) else {
            outputs[0] = crate::SILENT;
            return;
        };
        if self.rate != cx.rate {
            self.build(cx.rate);
        }
        outputs[0] = *input;
        for f in &mut self.filters {
            for side in 0..2 {
                for s in 0..BLOCK {
                    outputs[0][side][s] = f.tick(side, outputs[0][side][s]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{magnitude, run, sine};
    use crate::{BLOCK, Ctx, Knobs, Rng};
    use bpe_graph::Param;

    fn eq(knobs: &[(&str, f32)]) -> Equaliser {
        let mut k = Knobs::default();
        for (n, v) in knobs {
            k = k.with(n, Param::Number(*v));
        }
        Equaliser::new(&k)
    }

    fn gain_at(e: &mut Equaliser, hz: f32) -> f64 {
        let out = run(e, &mut Ctx::default(), 1500, |b, _| vec![Some(sine(hz, 48_000.0, b * BLOCK))]);
        let tail: Vec<f32> = out[48_000..].iter().map(|f| f[0]).collect();
        20.0 * magnitude(&tail, hz as f64, 48_000.0).log10()
    }

    #[test]
    fn eq_peak_gain_at_centre() {
        let db = gain_at(&mut eq(&[("mid_gain", 6.0), ("mid_freq", 1_000.0)]), 1_000.0);
        assert!((db - 6.0).abs() < 0.5, "{db}");
        let low = gain_at(&mut eq(&[("mid_gain", 6.0), ("mid_freq", 1_000.0)]), 100.0);
        assert!(low.abs() < 1.0, "{low}");
    }

    #[test]
    fn eq_flat_is_transparent() {
        let mut rng = Rng::new(3);
        let blocks: Vec<crate::Buf> = (0..200)
            .map(|_| {
                let mut b = crate::SILENT;
                for s in 0..BLOCK {
                    b[0][s] = rng.next() * 0.5;
                    b[1][s] = rng.next() * 0.5;
                }
                b
            })
            .collect();
        let out = run(&mut eq(&[]), &mut Ctx::default(), 200, |b, _| vec![Some(blocks[b])]);
        let err: f32 = out.iter().enumerate().map(|(i, f)| (f[0] - blocks[i / BLOCK][0][i % BLOCK]).powi(2)).sum();
        let sig: f32 = blocks.iter().flat_map(|b| b[0]).map(|v| v * v).sum();
        assert!(10.0 * (err / sig).max(1e-30).log10() < -80.0);
    }
}
