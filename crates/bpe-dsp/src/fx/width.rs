//! Stereo width (mid and side, with the low end kept mono) and a correlation meter.

use crate::SAMPLE_RATE;
use crate::smooth::Smoother;
use std::f32::consts::{FRAC_1_SQRT_2, TAU};

/// Values smaller than this in a filter's memory are taken as silence, so a fading filter never
/// reaches the slow denormal numbers.
const TINY: f32 = 1e-24;

#[inline]
fn flush(v: f32) -> f32 {
    if v.abs() < TINY { 0.0 } else { v }
}

/// A Mono below at or under this is off.
const MONO_OFF_HZ: f32 = 20.0;

/// One second-order filter (the cookbook's), in transposed direct form II.
#[derive(Clone, Debug, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    s: [f32; 2],
}

impl Biquad {
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.s[0];
        self.s[0] = flush(self.b[1] * x - self.a[0] * y + self.s[1]);
        self.s[1] = flush(self.b[2] * x - self.a[1] * y);
        y
    }

    fn clear(&mut self) {
        self.s = [0.0; 2];
    }

    /// Sets the coefficients from un-normalised `b` and `a` (a0, a1, a2).
    fn coefs(&mut self, b: [f32; 3], a: [f32; 3]) {
        self.b = [b[0] / a[0], b[1] / a[0], b[2] / a[0]];
        self.a = [a[1] / a[0], a[2] / a[0]];
    }
}

/// Stereo width: the sound split into its middle (what both sides share) and its side (how they
/// differ), the side scaled by Width (0 mono, 1 as it came, 2 twice as wide) and put back
/// together. With Mono below on, a Linkwitz-Riley crossover (fourth order) takes the side's low
/// end away so the bass stays in the middle; the middle goes through the crossover's matching
/// allpass, so middle and side stay lined up and the middle's level is untouched. At width 1 with
/// Mono below off it gives back exactly what came in.
#[derive(Clone, Debug)]
pub struct Width {
    width: Smoother,
    /// How far the crossover is in: 0 off, 1 on (it fades when Mono below turns on or off).
    split: Smoother,
    /// The crossover's frequency now and where it is gliding to, in Hz.
    cutoff: f32,
    cutoff_target: f32,
    /// The side's two high-pass halves and the middle's allpass.
    high: [Biquad; 2],
    pass: Biquad,
    fresh: bool,
}

impl Default for Width {
    fn default() -> Self {
        Self::new()
    }
}

impl Width {
    pub fn new() -> Self {
        Self {
            width: Smoother::new(Smoother::KNOB_MS),
            split: Smoother::new(Smoother::KNOB_MS),
            cutoff: 100.0,
            cutoff_target: 100.0,
            high: Default::default(),
            pass: Biquad::default(),
            fresh: true,
        }
    }

    /// Takes the width (0..2, the side's gain) and the Mono below frequency in Hz (20 or less is
    /// off), once a block; both glide.
    pub fn set(&mut self, width: f32, mono_below_hz: f32) {
        let snap = std::mem::take(&mut self.fresh);
        let width = width.clamp(0.0, 2.0);
        let on = mono_below_hz > MONO_OFF_HZ;
        if snap {
            self.width.reset(width);
            self.split.reset(if on { 1.0 } else { 0.0 });
        } else {
            self.width.set_target(width);
            self.split.set_target(if on { 1.0 } else { 0.0 });
        }
        if on {
            self.cutoff_target = mono_below_hz.min(0.45 * SAMPLE_RATE);
            // Nothing is heard from the crossover yet: start it where it is going.
            if snap || self.split.value() == 0.0 {
                self.cutoff = self.cutoff_target;
            }
        }
    }

    /// Back to silence, with the settings where they were heading.
    pub fn reset(&mut self) {
        self.width.reset(self.width.target());
        self.split.reset(self.split.target());
        self.cutoff = self.cutoff_target;
        self.clear();
        self.fresh = true;
    }

    fn clear(&mut self) {
        self.high.iter_mut().for_each(Biquad::clear);
        self.pass.clear();
    }

    /// The crossover's coefficients for the cutoff now.
    fn design(&mut self) {
        let w0 = TAU * self.cutoff / SAMPLE_RATE;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * FRAC_1_SQRT_2);
        let a = [1.0 + alpha, -2.0 * cos, 1.0 - alpha];
        for h in &mut self.high {
            h.coefs([(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0], a);
        }
        self.pass.coefs([1.0 - alpha, -2.0 * cos, 1.0 + alpha], a);
    }

    /// Widens or narrows `l` and `r` in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let off = self.split.settled() && self.split.value() == 0.0;
        if off {
            self.clear();
            if self.width.settled() && self.width.value() == 1.0 {
                return;
            }
        } else {
            // The cutoff glides once a block, evenly in pitch (most of the way in about 20 ms).
            let ratio = self.cutoff_target / self.cutoff;
            self.cutoff = if (ratio - 1.0).abs() < 1e-4 { self.cutoff_target } else { self.cutoff * ratio.powf(0.25) };
            self.design();
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let mut mid = 0.5 * (*a + *b);
            let mut side = 0.5 * (*a - *b);
            let width = self.width.next();
            if !off {
                let split = self.split.next();
                let half = self.high[0].run(side);
                let high = self.high[1].run(half);
                let passed = self.pass.run(mid);
                mid += split * (passed - mid);
                side += split * (high - side);
            }
            side *= width;
            *a = mid + side;
            *b = mid - side;
        }
    }
}

/// How the two sides move together, from 1 (the same: mono) through 0 (unrelated) to -1 (one
/// the other upside down), over about the last 300 ms. Silence reads 0.
#[derive(Clone, Debug)]
pub struct Correlation {
    /// Running averages of left × right, left² and right².
    lr: f64,
    ll: f64,
    rr: f64,
    k: f64,
}

impl Default for Correlation {
    fn default() -> Self {
        Self::new()
    }
}

impl Correlation {
    /// Below this average power (about -100 dB) a side counts as silent.
    const QUIET: f64 = 1e-10;

    pub fn new() -> Self {
        // An average with a 150 ms time constant weighs about the last 300 ms.
        let k = 1.0 - (-1.0 / (0.15 * SAMPLE_RATE as f64)).exp();
        Self { lr: 0.0, ll: 0.0, rr: 0.0, k }
    }

    /// Forgets everything heard.
    pub fn reset(&mut self) {
        self.lr = 0.0;
        self.ll = 0.0;
        self.rr = 0.0;
    }

    /// Takes in a block of sound.
    pub fn push(&mut self, l: &[f32], r: &[f32]) {
        let k = self.k;
        for (a, b) in l.iter().zip(r) {
            let (a, b) = (*a as f64, *b as f64);
            self.lr += k * (a * b - self.lr);
            self.ll += k * (a * a - self.ll);
            self.rr += k * (b * b - self.rr);
        }
        for v in [&mut self.lr, &mut self.ll, &mut self.rr] {
            if v.abs() < 1e-30 {
                *v = 0.0;
            }
        }
    }

    /// The correlation now, -1..1.
    pub fn value(&self) -> f32 {
        if self.ll < Self::QUIET || self.rr < Self::QUIET {
            return 0.0;
        }
        (self.lr / (self.ll * self.rr).sqrt()).clamp(-1.0, 1.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Noise, sine};
    use crate::{BLOCK, SAMPLE_RATE};

    fn noise(seed: u64, n: usize) -> Vec<f32> {
        let mut g = Noise(seed);
        (0..n).map(|_| g.next() * 0.5).collect()
    }

    fn run(w: &mut Width, l: &mut [f32], r: &mut [f32]) {
        for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
            w.process(a, b);
        }
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt() as f32
    }

    #[test]
    fn width_zero_is_mono() {
        for mono_below in [0.0, 120.0] {
            let mut w = Width::new();
            w.set(0.0, mono_below);
            let (mut l, mut r) = (noise(1, 9600), noise(2, 9600));
            run(&mut w, &mut l, &mut r);
            assert_eq!(l, r, "mono below {mono_below}");
            assert!(rms(&l) > 0.1);
        }
    }

    #[test]
    fn width_one_is_transparent() {
        let mut w = Width::new();
        w.set(1.0, 0.0);
        let (dl, dr) = (noise(3, 9600), noise(4, 9600));
        let (mut l, mut r) = (dl.clone(), dr.clone());
        run(&mut w, &mut l, &mut r);
        assert!(l == dl && r == dr);
    }

    #[test]
    fn width_two_doubles_the_side() {
        let mut w = Width::new();
        w.set(2.0, 0.0);
        let s = sine(1000.0, 0.25, 0.0, 4800);
        let (mut l, mut r) = (s.clone(), s.iter().map(|v| -v).collect::<Vec<_>>());
        run(&mut w, &mut l, &mut r);
        for (a, b) in l.iter().zip(&s) {
            assert!((a - 2.0 * b).abs() < 1e-6);
        }
    }

    /// The side level left after running a side-only sine at `hz` through mono below 150 Hz, in dB.
    fn side_left_db(hz: f64) -> f32 {
        let mut w = Width::new();
        w.set(1.0, 150.0);
        let n = SAMPLE_RATE as usize;
        let s = sine(hz, 0.5, 0.0, n);
        let (mut l, mut r) = (s.clone(), s.iter().map(|v| -v).collect::<Vec<_>>());
        run(&mut w, &mut l, &mut r);
        let side: Vec<f32> = l.iter().zip(&r).map(|(a, b)| (a - b) / 2.0).collect();
        let skip = n / 2;
        20.0 * (rms(&side[skip..]) / rms(&s[skip..])).log10()
    }

    #[test]
    fn mono_below_removes_low_side() {
        let low = side_left_db(50.0);
        assert!(low < -30.0, "50 Hz side left at {low} dB");
        let high = side_left_db(2000.0);
        assert!(high.abs() < 1.0, "2 kHz side at {high} dB");
    }

    #[test]
    fn mono_below_keeps_the_middle_level() {
        let mut w = Width::new();
        w.set(1.0, 150.0);
        for hz in [40.0, 150.0, 1000.0] {
            w.reset();
            let s = sine(hz, 0.5, 0.0, 48_000);
            let (mut l, mut r) = (s.clone(), s.clone());
            run(&mut w, &mut l, &mut r);
            let db = 20.0 * (rms(&l[24_000..]) / rms(&s[24_000..])).log10();
            assert!(db.abs() < 0.1, "{hz} Hz: {db} dB");
        }
    }

    #[test]
    fn width_changes_do_not_click() {
        let mut w = Width::new();
        w.set(0.0, 0.0);
        let s = sine(300.0, 0.5, 0.0, 9600);
        let (mut l, mut r) = (s.clone(), s.iter().map(|v| -v).collect::<Vec<_>>());
        for (i, (a, b)) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).enumerate() {
            if i == 40 {
                w.set(2.0, 200.0);
            }
            w.process(a, b);
        }
        let step = l.windows(2).map(|p| (p[1] - p[0]).abs()).fold(0.0, f32::max);
        // A 300 Hz sine at 1.0 moves at most 0.04 a sample.
        assert!(step < 0.045, "step {step}");
    }

    #[test]
    fn correlation_reads_mono_one_and_opposite_minus_one() {
        let n = 48_000;
        let x = noise(5, n);
        let minus: Vec<f32> = x.iter().map(|v| -v).collect();
        let mut c = Correlation::new();
        assert_eq!(c.value(), 0.0, "silence");
        c.push(&x, &x);
        assert!(c.value() > 0.99, "mono {}", c.value());
        c.reset();
        c.push(&x, &minus);
        assert!(c.value() < -0.99, "opposite {}", c.value());
        c.reset();
        c.push(&x, &noise(6, n));
        assert!(c.value().abs() < 0.1, "unrelated {}", c.value());
        c.reset();
        c.push(&vec![0.0; n], &vec![0.0; n]);
        assert_eq!(c.value(), 0.0, "silence again");
        // It follows the last ~300 ms: a second of mono after opposite reads mono.
        c.push(&x, &minus);
        c.push(&x, &x);
        assert!(c.value() > 0.95, "follows {}", c.value());
    }
}
