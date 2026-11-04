//! A four-pole low-pass ladder in zero-delay-feedback form (Zavalishin's topology-preserving
//! transform): the feedback is solved for each sample, then the input saturates (tanh), so it
//! rings and sings at full resonance but stays bounded.

use super::svf::flush;

/// The ladder's tuning: `g = tan(π·cutoff/rate)`, feedback `k` (4 is where it sings) and the
/// input drive `d` (1 = none).
#[derive(Clone, Copy, Debug)]
pub struct LadderCoefs {
    pub g: f32,
    pub k: f32,
    pub d: f32,
}

/// One channel's four stages.
#[derive(Clone, Copy, Debug, Default)]
pub struct LadderState {
    s: [f32; 4],
}

impl LadderState {
    pub fn reset(&mut self) {
        self.s = [0.0; 4];
    }

    #[inline]
    pub fn tick(&mut self, c: &LadderCoefs, x: f32) -> f32 {
        let beta = 1.0 / (1.0 + c.g);
        let big_g = c.g * beta;
        let s = &mut self.s;
        // y4 = G⁴·u + S, with S what the stages' memories give.
        let sum = big_g * big_g * big_g * beta * s[0] + big_g * big_g * beta * s[1] + big_g * beta * s[2] + beta * s[3];
        let g4 = big_g * big_g * big_g * big_g;
        let u = ((c.d * x - c.k * sum) / (1.0 + c.k * g4)).tanh();
        let mut v = u;
        for st in s.iter_mut() {
            let w = (v - *st) * big_g;
            let y = w + *st;
            *st = flush(y + w);
            v = y;
        }
        // The loudness the feedback takes, made back up a little; the drive's taken back.
        v * (1.0 + 0.5 * c.k) / c.d.sqrt()
    }
}

/// The ladder's small-signal response at `tan_f = tan(π·f/rate)`, as (re, im).
pub fn response(c: &LadderCoefs, tan_f: f64) -> (f64, f64) {
    // Each stage 1/(1+s) at s = j·tan_f/g.
    let w = tan_f / c.g as f64;
    // (1 + jw)^-4
    let (mut r, mut i) = (1.0f64, 0.0f64);
    for _ in 0..4 {
        let (a, b) = (r - i * w, i + r * w);
        r = a;
        i = b;
    }
    let d2 = r * r + i * i;
    let (lr, li) = (r / d2, -i / d2);
    // H = L/(1 + k L) · makeup
    let k = c.k as f64;
    let (dr, di) = (1.0 + k * lr, k * li);
    let dd = dr * dr + di * di;
    // Small signals pass tanh untouched: the drive in, its square root back out.
    let m = (1.0 + 0.5 * k) * (c.d as f64).sqrt();
    ((lr * dr + li * di) / dd * m, (li * dr - lr * di) / dd * m)
}
