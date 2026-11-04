//! Oversampling with polyphase IIR halfband filters (two paths of first-order allpass sections,
//! the elliptic design of Laurent de Soras' HIIR), cascaded for 2x, 4x and 8x.

use std::sync::OnceLock;

/// The most allpass sections one halfband filter has.
const MAX_COEFS: usize = 16;

/// The coefficients of a halfband filter with at least `attenuation_db` of stopband and a
/// transition band of `transition` (as a fraction of the higher rate, 0..0.5).
pub fn design(attenuation_db: f64, transition: f64) -> Vec<f64> {
    let (k, q) = transition_param(transition);
    let order = order(attenuation_db, q);
    let n = (order - 1) / 2;
    (0..n).map(|i| coef(i, k, q, order)).collect()
}

fn transition_param(transition: f64) -> (f64, f64) {
    let mut k = ((1.0 - transition * 2.0) * std::f64::consts::FRAC_PI_4).tan();
    k *= k;
    let kksqrt = (1.0 - k * k).powf(0.25);
    let e = 0.5 * (1.0 - kksqrt) / (1.0 + kksqrt);
    let e2 = e * e;
    let e4 = e2 * e2;
    let q = e * (1.0 + e4 * (2.0 + e4 * (15.0 + 150.0 * e4)));
    (k, q)
}

fn order(attenuation_db: f64, q: f64) -> usize {
    let attn_p2 = 10f64.powf(-attenuation_db / 10.0);
    let a = attn_p2 / (1.0 - attn_p2);
    let mut order = ((a * a / 16.0).ln() / q.ln()).ceil() as usize;
    if order.is_multiple_of(2) {
        order += 1;
    }
    order.max(3)
}

fn coef(index: usize, k: f64, q: f64, order: usize) -> f64 {
    let c = (index + 1) as f64;
    let pi = std::f64::consts::PI;
    let o = order as f64;
    let mut num = 0.0;
    let (mut i, mut j) = (0i32, 1.0);
    loop {
        let t = q.powi(i * (i + 1)) * ((i * 2 + 1) as f64 * c * pi / o).sin() * j;
        num += t;
        j = -j;
        i += 1;
        if t.abs() <= 1e-100 {
            break;
        }
    }
    let mut den = 0.0;
    let (mut i, mut j) = (1i32, -1.0);
    loop {
        let t = q.powi(i * i) * (i as f64 * 2.0 * c * pi / o).cos() * j;
        den += t;
        j = -j;
        i += 1;
        if t.abs() <= 1e-100 {
            break;
        }
    }
    let ww = num * q.powf(0.25) / (den + 0.5);
    let wwsq = ww * ww;
    let x = ((1.0 - wwsq * k) * (1.0 - wwsq / k)).sqrt() / (1.0 + wwsq);
    (1.0 - x) / (1.0 + x)
}

/// The coefficient sets for each stage of a cascade: the first (from the base rate) is steep,
/// later ones only have to clear the images of a signal that is already band-limited.
fn stage_coefs(stage: usize) -> &'static [f32] {
    static SETS: OnceLock<[Vec<f32>; 3]> = OnceLock::new();
    let sets = SETS.get_or_init(|| {
        [(110.0, 0.04), (110.0, 0.24), (110.0, 0.37)].map(|(a, t)| design(a, t).into_iter().map(|c| c as f32).collect())
    });
    &sets[stage.min(2)]
}

/// One 2x halfband filter, used either to go up (one sample in, two out) or down (two in, one
/// out); keep one for each direction.
#[derive(Clone, Debug)]
pub struct Halfband {
    coefs: [f32; MAX_COEFS],
    n: usize,
    x: [f32; MAX_COEFS],
    y: [f32; MAX_COEFS],
}

impl Halfband {
    /// A filter with these allpass coefficients (even ones on the first path, odd on the second).
    pub fn new(coefs: &[f32]) -> Self {
        let n = coefs.len().min(MAX_COEFS);
        let mut c = [0.0; MAX_COEFS];
        c[..n].copy_from_slice(&coefs[..n]);
        Self { coefs: c, n, x: [0.0; MAX_COEFS], y: [0.0; MAX_COEFS] }
    }

    /// The filter for stage `stage` (0, 1, 2) of a cascade.
    pub fn for_stage(stage: usize) -> Self {
        Self::new(stage_coefs(stage))
    }

    pub fn reset(&mut self) {
        self.x = [0.0; MAX_COEFS];
        self.y = [0.0; MAX_COEFS];
    }

    /// Runs the two paths, `a` through the even sections and `b` through the odd ones.
    #[inline]
    fn paths(&mut self, mut a: f32, mut b: f32) -> (f32, f32) {
        let mut i = 0;
        while i + 1 < self.n {
            let (xa, xb) = (self.x[i], self.x[i + 1]);
            self.x[i] = a;
            self.x[i + 1] = b;
            a = (a - self.y[i]) * self.coefs[i] + xa;
            b = (b - self.y[i + 1]) * self.coefs[i + 1] + xb;
            self.y[i] = a;
            self.y[i + 1] = b;
            i += 2;
        }
        if i < self.n {
            let xa = self.x[i];
            self.x[i] = a;
            a = (a - self.y[i]) * self.coefs[i] + xa;
            self.y[i] = a;
        }
        (a, b)
    }

    /// Two samples at twice the rate for one in.
    #[inline]
    pub fn up(&mut self, x: f32) -> [f32; 2] {
        let (a, b) = self.paths(x, x);
        [a, b]
    }

    /// One sample at half the rate for two in.
    #[inline]
    pub fn down(&mut self, x: [f32; 2]) -> f32 {
        let (a, b) = self.paths(x[1], x[0]);
        (a + b) * 0.5
    }

    /// The delay of going up (or down) through it at low frequencies, in samples at the higher
    /// rate.
    pub fn delay(&self) -> f64 {
        let gd = |c: f32| (1.0 - c as f64) / (1.0 + c as f64);
        let even: f64 = self.coefs[..self.n].iter().step_by(2).map(|&c| gd(c)).sum();
        let odd: f64 = self.coefs[..self.n].iter().skip(1).step_by(2).map(|&c| gd(c)).sum();
        even + odd + 0.5
    }
}

/// 1x, 2x, 4x or 8x the rate and back, through a cascade of halfband filters.
#[derive(Clone, Debug)]
pub struct Oversampler {
    factor: usize,
    ups: [Halfband; 3],
    downs: [Halfband; 3],
}

impl Oversampler {
    /// `factor` is 1, 2, 4 or 8 (anything else is taken as the nearest of those below it).
    pub fn new(factor: u8) -> Self {
        let factor = match factor {
            0 | 1 => 1,
            2 | 3 => 2,
            4..=7 => 4,
            _ => 8,
        };
        let stage = Halfband::for_stage;
        Self { factor, ups: [stage(0), stage(1), stage(2)], downs: [stage(0), stage(1), stage(2)] }
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    fn stages(&self) -> usize {
        self.factor.trailing_zeros() as usize
    }

    pub fn reset(&mut self) {
        for h in self.ups.iter_mut().chain(self.downs.iter_mut()) {
            h.reset();
        }
    }

    /// Fills `output` (`factor` times as long as `input`) with `input` at the higher rate.
    pub fn up(&mut self, input: &[f32], output: &mut [f32]) {
        let f = self.factor;
        debug_assert!(output.len() >= input.len() * f);
        for (i, &x) in input.iter().enumerate() {
            self.up_one(x, &mut output[i * f..(i + 1) * f]);
        }
    }

    /// One input sample to `factor` output samples.
    #[inline]
    pub fn up_one(&mut self, x: f32, out: &mut [f32]) {
        let stages = self.stages();
        let mut buf = [0.0f32; 8];
        buf[0] = x;
        let mut len = 1;
        for s in 0..stages {
            // Each stage takes its samples oldest first.
            let from = buf;
            for i in 0..len {
                let [a, b] = self.ups[s].up(from[i]);
                buf[2 * i] = a;
                buf[2 * i + 1] = b;
            }
            len *= 2;
        }
        out[..len].copy_from_slice(&buf[..len]);
    }

    /// Fills `output` with `input` (`factor` times as long) back at the base rate.
    pub fn down(&mut self, input: &[f32], output: &mut [f32]) {
        let f = self.factor;
        debug_assert!(input.len() >= output.len() * f);
        for (i, o) in output.iter_mut().enumerate() {
            *o = self.down_one(&input[i * f..(i + 1) * f]);
        }
    }

    /// `factor` input samples to one output sample.
    #[inline]
    pub fn down_one(&mut self, input: &[f32]) -> f32 {
        let stages = self.stages();
        let mut buf = [0.0f32; 8];
        let mut len = self.factor;
        buf[..len].copy_from_slice(&input[..len]);
        for s in (0..stages).rev() {
            for i in 0..len / 2 {
                buf[i] = self.downs[s].down([buf[2 * i], buf[2 * i + 1]]);
            }
            len /= 2;
        }
        buf[0]
    }

    /// The delay of going up and back down, in base-rate samples (at low frequencies).
    pub fn latency_exact(&self) -> f64 {
        let mut d = 0.0;
        let mut rate = 2.0;
        for s in 0..self.stages() {
            // Each stage delays both ways by its delay at its own higher rate, less the one
            // higher-rate sample that coming down on the odd phase takes back.
            d += (2.0 * self.ups[s].delay() - 1.0) / rate;
            rate *= 2.0;
        }
        d
    }

    /// [`Oversampler::latency_exact`] rounded to whole samples.
    pub fn latency(&self) -> usize {
        self.latency_exact().round() as usize
    }
}

/// A streaming 4x true-peak detector for one channel.
#[derive(Clone, Debug)]
pub struct PeakDetector {
    up: Oversampler,
}

impl Default for PeakDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl PeakDetector {
    pub fn new() -> Self {
        Self { up: Oversampler::new(4) }
    }

    pub fn reset(&mut self) {
        self.up.reset();
    }

    /// The largest level near this sample, between samples as well as on them. It lags the
    /// sound by a few samples ([`PeakDetector::DELAY`] at most).
    #[inline]
    pub fn push(&mut self, x: f32) -> f32 {
        let mut out = [0.0f32; 4];
        self.up.up_one(x, &mut out);
        out.iter().fold(x.abs(), |m, v| m.max(v.abs()))
    }

    /// The most samples a peak shows up late by.
    pub const DELAY: usize = 8;
}

/// The true peak (4x oversampled) of `samples`, one channel, as a gain.
pub fn true_peak(samples: &[f32]) -> f32 {
    let mut d = PeakDetector::new();
    let tail = std::iter::repeat_n(0.0, 64);
    samples.iter().copied().chain(tail).fold(0.0f32, |m, x| m.max(d.push(x)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::sine;

    /// The level of the part of `x` that a sine at `hz` (any phase) doesn't explain, against the
    /// sine's own level, in dB.
    fn residual_db(x: &[f32], hz: f64, rate: f64) -> (f64, f64) {
        let (mut c, mut s) = (0.0, 0.0);
        for (i, &v) in x.iter().enumerate() {
            let w = std::f64::consts::TAU * hz * i as f64 / rate;
            c += v as f64 * w.cos();
            s += v as f64 * w.sin();
        }
        let n = x.len() as f64;
        let (c, s) = (2.0 * c / n, 2.0 * s / n);
        let amp = (c * c + s * s).sqrt();
        let mut err = 0.0;
        for (i, &v) in x.iter().enumerate() {
            let w = std::f64::consts::TAU * hz * i as f64 / rate;
            let e = v as f64 - (c * w.cos() + s * w.sin());
            err += e * e;
        }
        let rms = (err / n).sqrt();
        (20.0 * (rms / (amp / 2f64.sqrt())).log10(), amp)
    }

    #[test]
    fn the_first_stage_has_100_db_of_stopband() {
        assert!(stage_coefs(0).len() <= MAX_COEFS, "{}", stage_coefs(0).len());
        // A 20 kHz sine at 48 kHz, up to 96 kHz: its image sits at 48 - 20 = 28 kHz. (Lengths
        // are whole periods of both: 24 samples at 96 kHz.)
        let x = sine(20_000.0, 0.5, 0.0, 48_000);
        let mut h = Halfband::for_stage(0);
        let up: Vec<f32> = x.iter().flat_map(|&v| h.up(v)).collect();
        let tail = &up[4_800..];
        let (_, amp20) = residual_db(tail, 20_000.0, 96_000.0);
        let (_, image) = residual_db(tail, 28_000.0, 96_000.0);
        assert!((amp20 - 0.5).abs() < 0.001, "passband level {amp20}");
        assert!(20.0 * (image / amp20).log10() < -100.0, "image {} dB", 20.0 * (image / amp20).log10());
    }

    #[test]
    fn halfband_up_down_is_transparent() {
        for factor in [2u8, 4, 8] {
            let x = sine(1_000.0, 0.5, 0.3, 48_000);
            let mut o = Oversampler::new(factor);
            let f = o.factor();
            let mut up = vec![0.0; x.len() * f];
            let mut back = vec![0.0; x.len()];
            o.up(&x, &mut up);
            o.down(&up, &mut back);
            let latency = o.latency();
            assert!(latency < 40, "latency {latency}");
            // 900 whole periods.
            let (db, amp) = residual_db(&back[4_800..], 1_000.0, 48_000.0);
            assert!((amp - 0.5).abs() < 1e-4, "{factor}x level {amp}");
            assert!(db < -90.0, "{factor}x residual {db} dB");
        }
    }

    #[test]
    fn the_latency_matches_where_a_low_sine_comes_out() {
        let x = sine(200.0, 0.5, 0.0, 48_000);
        let mut o = Oversampler::new(4);
        let mut up = vec![0.0; x.len() * 4];
        let mut back = vec![0.0; x.len()];
        o.up(&x, &mut up);
        o.down(&up, &mut back);
        let d = o.latency_exact();
        let delayed = sine(200.0, 0.5, -std::f64::consts::TAU * 200.0 * d / 48_000.0, 48_000);
        let worst = back[4_000..].iter().zip(&delayed[4_000..]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-3, "{worst} with latency {d}");
    }

    #[test]
    fn true_peak_finds_the_peak_between_samples() {
        // A sine at a quarter of the rate, 45 degrees off: every sample is at 0.707 of its peak.
        // (Past the first moments: a sine that starts all at once really does overshoot.)
        let x = sine(12_000.0, 1.0, std::f64::consts::FRAC_PI_4, 4_800);
        let sample_peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((sample_peak - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        let mut d = PeakDetector::new();
        let tp =
            x.iter().enumerate().fold(0.0f32, |m, (i, &v)| if i < 480 { d.push(v).min(0.0) } else { m.max(d.push(v)) });
        // 4x finds it to within the 16 points it looks at a cycle: cos(pi / 16).
        assert!((0.98..1.01).contains(&tp), "{tp}");
    }
}
