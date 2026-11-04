//! The chain's distortion: ten curves the sound is pushed through, run at a higher rate
//! (1×, 2×, 4× or 8×, through the halfband [`Oversampler`]) so the new harmonics fold back
//! as little as they can. Bias leans the curve before it (taken back out, so silence stays
//! silent); a 5 Hz DC blocker, the Tone tilt and the Output gain come after.

use crate::{BLOCK, SAMPLE_RATE, StereoOversampler, db_to_gain};
use wide::{CmpGe, CmpLe, CmpLt, f32x4};

/// A distortion's type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShaperType {
    #[default]
    SoftClip,
    HardClip,
    Tube,
    Tape,
    Diode,
    Overdrive,
    Wavefold,
    Bitcrush,
    Downsample,
    Rectify,
}

impl ShaperType {
    pub const ALL: [ShaperType; 10] = [
        ShaperType::SoftClip,
        ShaperType::HardClip,
        ShaperType::Tube,
        ShaperType::Tape,
        ShaperType::Diode,
        ShaperType::Overdrive,
        ShaperType::Wavefold,
        ShaperType::Bitcrush,
        ShaperType::Downsample,
        ShaperType::Rectify,
    ];

    /// Whether it runs at the higher rate (Bitcrush and Downsample are meant to fold back).
    pub fn oversampled(self) -> bool {
        !matches!(self, ShaperType::Bitcrush | ShaperType::Downsample)
    }
}

/// A distortion's settings at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShaperParams {
    pub drive_db: f32,
    /// Bitcrush: 1..16.
    pub bits: f32,
    /// Downsample: the rate it holds at, Hz.
    pub rate_hz: f32,
    /// -1..1.
    pub bias: f32,
    /// -1..1: darker to brighter.
    pub tone: f32,
    pub output_db: f32,
}

impl Default for ShaperParams {
    fn default() -> Self {
        ShaperParams { drive_db: 12.0, bits: 8.0, rate_hz: 8000.0, bias: 0.0, tone: 0.0, output_db: 0.0 }
    }
}

/// How far below its knee the one-sided Diode curve goes.
const DIODE_FLOOR: f32 = 0.2;

#[inline]
fn overdrive_stage(x: f32) -> f32 {
    if x.abs() >= 1.0 { x.signum() } else { 1.5 * x - 0.5 * x * x * x }
}

/// The curves with a corner in them (or in their slope), which fold back most: these run with
/// first-order antiderivative antialiasing on top of the higher rate.
fn smoothed(kind: ShaperType) -> bool {
    matches!(
        kind,
        ShaperType::HardClip | ShaperType::Tube | ShaperType::Diode | ShaperType::Overdrive | ShaperType::Rectify
    )
}

fn splat(v: f32) -> f32x4 {
    f32x4::splat(v)
}

/// `e^w − 1 − w`, without losing the small values to rounding.
#[inline]
fn exp_less_line(w: f32x4) -> f32x4 {
    // The series up to w^10 for |w| < 0.5 (error under 1e-11), the exponential otherwise.
    let mut series = splat(1.0 / 3_628_800.0);
    for d in [362_880.0f32, 40_320.0, 5040.0, 720.0, 120.0, 24.0, 6.0, 2.0] {
        series = series * w + splat(1.0 / d);
    }
    let series = series * w * w;
    let big = w.exp() - splat(1.0) - w;
    w.abs().cmp_lt(splat(0.5)).blend(series, big)
}

/// `|u| − ln(1 + |u|)`, the Tube curve's antiderivative, without losing small values.
#[inline]
fn tube_area(u: f32x4) -> f32x4 {
    let a = u.abs();
    // Σ (−1)^k a^k / k from k = 2 to 13 for a < 0.25 (error under 1e-9).
    let mut series = splat(-1.0 / 13.0);
    for k in (2..13).rev() {
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        series = series * a + splat(sign / k as f32);
    }
    let series = series * a * a;
    let big = a - (splat(1.0) + a).ln();
    a.cmp_lt(splat(0.25)).blend(series, big)
}

#[inline]
fn overdrive4(x: f32x4) -> f32x4 {
    let c = x.max(splat(-1.0)).min(splat(1.0));
    splat(1.5) * c - splat(0.5) * c * c * c
}

#[inline]
fn overdrive_area4(x: f32x4) -> f32x4 {
    let a = x.abs();
    let inside = splat(0.75) * x * x - splat(0.125) * x * x * x * x;
    a.cmp_ge(splat(1.0)).blend(a - splat(0.375), inside)
}

#[inline]
fn rectified4(u: f32x4, bias: f32) -> f32x4 {
    let t = splat(bias.abs().min(1.0));
    let half = if bias >= 0.0 { u.max(f32x4::ZERO) } else { (-u).max(f32x4::ZERO) };
    (splat(1.0) - t) * u.abs() + t * half
}

/// The curve on four values at once (one overdrive stage for Overdrive).
#[inline]
fn curve4(kind: ShaperType, u: f32x4, bias: f32) -> f32x4 {
    match kind {
        ShaperType::SoftClip => {
            // 1 − 2/(e^{2u} + 1), kept where e^{2u} stays finite.
            let c = u.max(splat(-9.0)).min(splat(9.0));
            splat(1.0) - splat(2.0) / ((c + c).exp() + splat(1.0))
        }
        ShaperType::HardClip => u.max(splat(-1.0)).min(splat(1.0)),
        ShaperType::Tube => u / (splat(1.0) + u.abs()),
        ShaperType::Tape => u / (splat(1.0) + u * u).sqrt(),
        ShaperType::Diode => {
            let pos = splat(1.0) - (-u.max(f32x4::ZERO)).exp();
            let neg = splat(DIODE_FLOOR) * ((u.min(f32x4::ZERO) / splat(DIODE_FLOOR)).exp() - splat(1.0));
            u.cmp_ge(f32x4::ZERO).blend(pos, neg)
        }
        ShaperType::Overdrive => overdrive4(u),
        ShaperType::Wavefold => (splat(std::f32::consts::FRAC_PI_2) * u).sin(),
        ShaperType::Rectify => rectified4(u, bias),
        ShaperType::Bitcrush | ShaperType::Downsample => u,
    }
}

/// The antiderivative of [`curve4`] (one overdrive stage for Overdrive).
#[inline]
fn area4(kind: ShaperType, u: f32x4, bias: f32) -> f32x4 {
    match kind {
        ShaperType::HardClip => {
            let a = u.abs();
            a.cmp_le(splat(1.0)).blend(splat(0.5) * u * u, a - splat(0.5))
        }
        ShaperType::Tube => tube_area(u),
        ShaperType::Diode => {
            let c = DIODE_FLOOR;
            let pos = exp_less_line(-u.max(f32x4::ZERO));
            let neg = splat(c * c) * exp_less_line(u.min(f32x4::ZERO) / splat(c));
            u.cmp_ge(f32x4::ZERO).blend(pos, neg)
        }
        ShaperType::Overdrive => overdrive_area4(u),
        ShaperType::Rectify => {
            let t = splat(bias.abs().min(1.0));
            let full = splat(0.5) * u * u.abs();
            let half = if bias >= 0.0 {
                let p = u.max(f32x4::ZERO);
                splat(0.5) * p * p
            } else {
                let n = u.min(f32x4::ZERO);
                splat(-0.5) * n * n
            };
            (splat(1.0) - t) * full + t * half
        }
        _ => f32x4::ZERO,
    }
}

/// Antiderivative antialiasing on four values in a row, `n` of them real (the rest repeat the
/// last): `(F(u) − F(u_before)) / (u − u_before)`, or the curve at the middle where the two are
/// too close to tell apart. `prev` and `prev_area` carry the last value on.
#[inline]
fn antialiased4(kind: ShaperType, u: f32x4, n: usize, prev: &mut f32, prev_area: &mut f32, bias: f32) -> f32x4 {
    let ua = u.to_array();
    let f = area4(kind, u, bias);
    let fa = f.to_array();
    let before = f32x4::from([*prev, ua[0], ua[1], ua[2]]);
    let f_before = f32x4::from([*prev_area, fa[0], fa[1], fa[2]]);
    *prev = ua[n - 1];
    *prev_area = fa[n - 1];
    let d = u - before;
    let ratio = (f - f_before) / d;
    let close = d.abs().cmp_lt(splat(1e-3) * (splat(1.0) + u.abs()));
    if close.none() {
        return ratio;
    }
    close.blend(curve4(kind, splat(0.5) * (u + before), bias), ratio)
}

/// The curve itself at `u` (already driven and biased). Bitcrush steps by `q` levels.
#[inline]
fn curve(kind: ShaperType, u: f32, q: f32) -> f32 {
    match kind {
        ShaperType::SoftClip => u.tanh(),
        ShaperType::HardClip => u.clamp(-1.0, 1.0),
        ShaperType::Tube => u / (1.0 + u.abs()),
        ShaperType::Tape => u / (1.0 + u * u).sqrt(),
        ShaperType::Diode => {
            if u >= 0.0 {
                1.0 - (-u).exp()
            } else {
                DIODE_FLOOR * ((u / DIODE_FLOOR).exp() - 1.0)
            }
        }
        ShaperType::Overdrive => overdrive_stage(overdrive_stage(u)),
        ShaperType::Wavefold => (std::f32::consts::FRAC_PI_2 * u).sin(),
        ShaperType::Bitcrush => (u * q).round() / q,
        ShaperType::Downsample => u,
        ShaperType::Rectify => u.abs(),
    }
}

/// Levels each side of zero for `bits`.
fn levels(bits: f32) -> f32 {
    let b = if bits.is_finite() { bits.clamp(1.0, 16.0) } else { 8.0 };
    (b - 1.0).exp2()
}

/// Rectify's Bias blends full-wave towards half-wave (positive Bias keeps the top half,
/// negative the bottom) instead of adding DC.
#[inline]
fn rectified(u: f32, bias: f32) -> f32 {
    let t = bias.abs().min(1.0);
    let half = if bias >= 0.0 { u.max(0.0) } else { (-u).max(0.0) };
    (1.0 - t) * u.abs() + t * half
}

/// The static curve the editor draws: `x` in, the curve's output (before Tone and Output).
/// Bitcrush reads `drive_db` as its bits; Downsample is drawn straight.
pub fn transfer(kind: ShaperType, drive_db: f32, bias: f32, x: f32) -> f32 {
    match kind {
        ShaperType::Bitcrush => {
            let q = levels(drive_db);
            curve(kind, x + bias, q) - curve(kind, bias, q)
        }
        ShaperType::Downsample => x,
        ShaperType::Rectify => rectified(db_to_gain(drive_db) * x, bias),
        _ => {
            let g = db_to_gain(drive_db);
            curve(kind, g * x + bias, 1.0) - curve(kind, bias, 1.0)
        }
    }
}

/// The numbers glided over a block: drive gain, bias, the bias's own output (taken back),
/// levels (Bitcrush), hold step (Downsample), low and high gains of the tilt, output gain.
const NP: usize = 8;

fn targets(kind: ShaperType, p: &ShaperParams) -> [f32; NP] {
    let finite = |v: f32, d: f32| if v.is_finite() { v } else { d };
    let g = match kind {
        ShaperType::Bitcrush | ShaperType::Downsample => 1.0,
        _ => db_to_gain(finite(p.drive_db, 0.0).clamp(0.0, 60.0)),
    };
    let bias = finite(p.bias, 0.0).clamp(-1.0, 1.0);
    let q = levels(p.bits);
    let offset = match kind {
        ShaperType::Rectify | ShaperType::Downsample => 0.0,
        _ => curve(kind, bias, q),
    };
    let tone = finite(p.tone, 0.0).clamp(-1.0, 1.0);
    let step = finite(p.rate_hz, 8000.0).clamp(20.0, SAMPLE_RATE) / SAMPLE_RATE;
    [g, bias, offset, q, step, db_to_gain(-6.0 * tone), db_to_gain(6.0 * tone), db_to_gain(finite(p.output_db, 0.0))]
}

/// One channel's memory.
#[derive(Clone, Debug)]
struct Channel {
    /// Tape's gentle low pass at the higher rate.
    tape: f32,
    /// The DC blocker's last input and output.
    dc: (f32, f32),
    /// The tilt's low-pass state.
    tilt: f32,
    /// Downsample: the held value and the hold clock.
    held: f32,
    clock: f32,
    /// The antialiasing's last inputs and their antiderivatives (two of each for Overdrive's
    /// two stages).
    prev: [f32; 2],
    prev_area: [f32; 2],
}

impl Channel {
    fn new() -> Self {
        Channel { tape: 0.0, dc: (0.0, 0.0), tilt: 0.0, held: 0.0, clock: 1.0, prev: [0.0; 2], prev_area: [0.0; 2] }
    }
}

/// A distortion of one type and quality, two channels.
#[derive(Clone, Debug)]
pub struct Shaper {
    kind: ShaperType,
    factor: usize,
    cur: [f32; NP],
    step: [f32; NP],
    left: u32,
    fresh: bool,
    over: StereoOversampler,
    ch: [Channel; 2],
    /// Coefficients: DC blocker pole, tilt low-pass, tape low-pass at the higher rate.
    dc_r: f32,
    tilt_a: f32,
    tape_a: f32,
    /// Antiderivative antialiasing (a cornered curve at 2× or more; at 1× its gentle top cut
    /// would show).
    smooth: bool,
}

fn one_pole(hz: f32, rate: f32) -> f32 {
    1.0 - (-std::f32::consts::TAU * hz / rate).exp()
}

impl Shaper {
    /// `factor` 1, 2, 4 or 8 (Bitcrush and Downsample always run at 1×).
    pub fn new(kind: ShaperType, factor: u8) -> Self {
        let factor = if kind.oversampled() { factor } else { 1 };
        let over = StereoOversampler::new(factor);
        let factor = over.factor();
        Shaper {
            kind,
            factor,
            cur: [0.0; NP],
            step: [0.0; NP],
            left: 0,
            fresh: true,
            over,
            ch: [Channel::new(), Channel::new()],
            dc_r: 1.0 - std::f32::consts::TAU * 5.0 / SAMPLE_RATE,
            tilt_a: one_pole(1000.0, SAMPLE_RATE),
            tape_a: one_pole(9000.0, SAMPLE_RATE * factor as f32),
            smooth: smoothed(kind) && factor >= 2,
        }
    }

    pub fn kind(&self) -> ShaperType {
        self.kind
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    /// How many samples late the sound comes out (the higher rate's filters).
    pub fn latency(&self) -> usize {
        self.over.latency()
    }

    pub fn reset(&mut self) {
        self.over.reset();
        self.ch = [Channel::new(), Channel::new()];
        self.fresh = true;
        self.left = 0;
    }

    /// Takes on another shaper's memory (both of one type and quality); no allocation.
    pub fn copy_state_from(&mut self, other: &Shaper) {
        if other.kind != self.kind || other.factor != self.factor {
            self.reset();
            return;
        }
        self.cur = other.cur;
        self.step = other.step;
        self.left = other.left;
        self.fresh = other.fresh;
        self.over = other.over.clone();
        self.ch = other.ch.clone();
    }

    /// The settings for the next block, glided over it (taken at once after a reset).
    pub fn set(&mut self, p: &ShaperParams) {
        let t = targets(self.kind, p);
        if self.fresh {
            self.cur = t;
            self.left = 0;
            self.fresh = false;
            // The antialiasing starts from silence as this curve sees it.
            let u = if self.kind == ShaperType::Rectify { 0.0 } else { t[1] };
            let stage = if self.kind == ShaperType::Overdrive { ShaperType::Overdrive } else { self.kind };
            let first = area4(stage, splat(u), t[1]).to_array()[0];
            let second_in = overdrive_stage(u);
            let second = overdrive_area4(splat(second_in)).to_array()[0];
            for ch in &mut self.ch {
                ch.prev = [u, second_in];
                ch.prev_area = [first, second];
            }
            return;
        }
        for ((s, t), c) in self.step.iter_mut().zip(t).zip(self.cur) {
            *s = (t - c) / BLOCK as f32;
        }
        self.left = BLOCK as u32;
    }

    /// The curve on one channel's samples at the higher rate.
    #[inline]
    fn shape(&mut self, c: usize, buf: &mut [f32; 8]) {
        let [g, bias, offset, ..] = self.cur;
        let (kind, f) = (self.kind, self.factor);
        let ch = &mut self.ch[c];
        for v in &mut buf[..f] {
            *v *= g;
            if kind == ShaperType::Tape {
                ch.tape += self.tape_a * (*v - ch.tape);
                *v = ch.tape;
            }
        }
        let shift = if kind == ShaperType::Rectify { 0.0 } else { bias };
        let mut start = 0;
        while start < f {
            let n = (f - start).min(4);
            let mut lanes = [buf[start + n - 1] + shift; 4];
            for (l, v) in lanes.iter_mut().zip(&buf[start..start + n]) {
                *l = v + shift;
            }
            let u = f32x4::from(lanes);
            let y = if self.smooth {
                if kind == ShaperType::Overdrive {
                    let (p, a) = (&mut ch.prev, &mut ch.prev_area);
                    let one = antialiased4(kind, u, n, &mut p[0], &mut a[0], bias);
                    // The second stage's input repeats past `n` too.
                    let mut o = one.to_array();
                    for k in n..4 {
                        o[k] = o[n - 1];
                    }
                    antialiased4(kind, f32x4::from(o), n, &mut p[1], &mut a[1], bias)
                } else {
                    antialiased4(kind, u, n, &mut ch.prev[0], &mut ch.prev_area[0], bias)
                }
            } else if kind == ShaperType::Overdrive {
                overdrive4(overdrive4(u))
            } else {
                curve4(kind, u, bias)
            };
            let y = y.to_array();
            for k in 0..n {
                buf[start + k] = y[k] - offset;
            }
            start += 4;
        }
    }

    /// DC blocker, the tilt round 1 kHz and the output, on one channel.
    #[inline]
    fn after(&mut self, c: usize, shaped: f32) -> f32 {
        let [.., lo, hi, out] = self.cur;
        let ch = &mut self.ch[c];
        let y = shaped - ch.dc.0 + self.dc_r * ch.dc.1;
        ch.dc = (shaped, if y.abs() < 1e-20 { 0.0 } else { y });
        ch.tilt += self.tilt_a * (y - ch.tilt);
        if ch.tilt.abs() < 1e-20 {
            ch.tilt = 0.0;
        }
        (lo * ch.tilt + hi * (y - ch.tilt)) * out
    }

    /// One sample of each channel.
    #[inline]
    pub fn tick(&mut self, l: f32, r: f32) -> (f32, f32) {
        if self.left > 0 {
            for (c, s) in self.cur.iter_mut().zip(self.step) {
                *c += s;
            }
            self.left -= 1;
        }
        let [_, bias, offset, q, hold, ..] = self.cur;
        let (sl, sr) = match self.kind {
            ShaperType::Downsample => {
                let mut held = [0.0; 2];
                for (c, x) in [l, r].into_iter().enumerate() {
                    let ch = &mut self.ch[c];
                    ch.clock += hold;
                    if ch.clock >= 1.0 {
                        ch.clock -= 1.0;
                        ch.held = x;
                    }
                    held[c] = ch.held;
                }
                (held[0], held[1])
            }
            ShaperType::Bitcrush => (curve(self.kind, l + bias, q) - offset, curve(self.kind, r + bias, q) - offset),
            _ => {
                let (mut bl, mut br) = ([0.0f32; 8], [0.0f32; 8]);
                self.over.up(l, r, &mut bl, &mut br);
                self.shape(0, &mut bl);
                self.shape(1, &mut br);
                self.over.down(&bl, &br)
            }
        };
        (self.after(0, sl), self.after(1, sr))
    }

    /// One block in place.
    pub fn process(&mut self, l: &mut [f32; BLOCK], r: &mut [f32; BLOCK]) {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            (*a, *b) = self.tick(*a, *b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::fft;

    /// A band-limited saw at C8 (4186 Hz): its harmonics under 20 kHz.
    fn c8_saw(n: usize) -> Vec<f32> {
        let f = 4186.0f64;
        (0..n)
            .map(|i| {
                let t = i as f64 / SAMPLE_RATE as f64;
                let mut v = 0.0;
                let mut h = 1;
                while f * h as f64 <= 20_000.0 {
                    v += (std::f64::consts::TAU * f * h as f64 * t).sin() / h as f64;
                    h += 1;
                }
                (0.5 * v) as f32
            })
            .collect()
    }

    fn run(kind: ShaperType, factor: u8, p: &ShaperParams, x: &[f32]) -> Vec<f32> {
        let mut s = Shaper::new(kind, factor);
        x.chunks(BLOCK)
            .flat_map(|c| {
                s.set(p);
                c.iter().map(|&v| s.tick(v, v).0).collect::<Vec<_>>()
            })
            .collect()
    }

    /// The loudest component under 20 kHz that is not a harmonic of `f0`, against the loudest
    /// harmonic, dB. (What folds into 20..24 kHz is past hearing.)
    fn folded_db(y: &[f32], f0: f64) -> f64 {
        let n = 1 << 16;
        let y = &y[y.len() - n..];
        let (mut re, mut im) = (vec![0.0f64; n], vec![0.0f64; n]);
        for (i, v) in y.iter().enumerate() {
            // Blackman-Harris.
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let w = 0.35875 - 0.48829 * a.cos() + 0.14128 * (2.0 * a).cos() - 0.01168 * (3.0 * a).cos();
            re[i] = *v as f64 * w;
        }
        fft(&mut re, &mut im, false);
        let bin = SAMPLE_RATE as f64 / n as f64;
        let mag: Vec<f64> = (0..n / 2).map(|k| re[k].hypot(im[k])).collect();
        let near_harmonic = |k: usize| {
            let hz = k as f64 * bin;
            let h = (hz / f0).round();
            (hz - h * f0).abs() < 12.0 * bin || hz < 30.0
        };
        let loudest = mag.iter().copied().fold(0.0, f64::max);
        let audible = (20_000.0 / bin) as usize;
        let folded = (0..audible).filter(|&k| !near_harmonic(k)).map(|k| mag[k]).fold(0.0, f64::max);
        20.0 * (folded / loudest).log10()
    }

    #[test]
    fn c8_saw_through_every_type_is_alias_free_at_4x() {
        let x = c8_saw(1 << 17);
        let p = ShaperParams { drive_db: 12.0, ..ShaperParams::default() };
        for kind in ShaperType::ALL.into_iter().filter(|k| k.oversampled()) {
            let at4 = folded_db(&run(kind, 4, &p, &x), 4186.0);
            assert!(at4 < -70.0, "{kind:?} at 4x folds back at {at4:.1} dB");
        }
        // The measure sees folding where there is some: hard clipping at 1x.
        let at1 = folded_db(&run(ShaperType::HardClip, 1, &p, &x), 4186.0);
        assert!(at1 > -50.0, "1x folds back: {at1:.1} dB");
    }

    #[test]
    fn bias_adds_even_harmonics_to_tube() {
        // A whole number of cycles, so nothing leaks between harmonics.
        let n = 48_000;
        let f0 = 1000.0;
        let x: Vec<f32> =
            (0..n * 2).map(|i| (0.5 * (std::f64::consts::TAU * f0 * i as f64 / 48_000.0).sin()) as f32).collect();
        let second = |bias: f32| {
            let y = run(ShaperType::Tube, 4, &ShaperParams { drive_db: 12.0, bias, ..ShaperParams::default() }, &x);
            let y = &y[y.len() - n..];
            let at = |hz: f64| {
                let (mut c, mut s) = (0.0, 0.0);
                for (i, v) in y.iter().enumerate() {
                    let ph = std::f64::consts::TAU * hz * i as f64 / 48_000.0;
                    c += *v as f64 * ph.cos();
                    s += *v as f64 * ph.sin();
                }
                (c * c + s * s).sqrt()
            };
            20.0 * (at(2.0 * f0) / at(f0)).log10()
        };
        let with = second(0.5);
        let without = second(0.0);
        assert!(with > -40.0, "bias 50 %: 2nd harmonic at {with:.1} dB");
        assert!(without < -80.0, "bias 0: 2nd harmonic at {without:.1} dB");
    }

    #[test]
    fn max_drive_on_silence_stays_silent() {
        let x = vec![0.0f32; 48_000];
        for kind in ShaperType::ALL {
            for bias in [0.0, 0.7, -1.0] {
                let p = ShaperParams { drive_db: 48.0, bias, bits: 1.0, ..ShaperParams::default() };
                let y = run(kind, 8, &p, &x);
                let worst = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                assert!(worst < 1e-6, "{kind:?} bias {bias}: {worst}");
            }
        }
    }

    #[test]
    fn transfer_matches_process_at_low_freq() {
        let n = 96_000;
        let x: Vec<f32> =
            (0..n).map(|i| (0.8 * (std::f64::consts::TAU * 20.0 * i as f64 / 48_000.0).sin()) as f32).collect();
        for kind in ShaperType::ALL.into_iter().filter(|k| *k != ShaperType::Downsample && *k != ShaperType::Tape) {
            for factor in [1u8, 4] {
                let p = ShaperParams { drive_db: 6.0, bits: 4.0, bias: 0.0, ..ShaperParams::default() };
                let s = Shaper::new(kind, factor);
                let lat = s.latency();
                let y = run(kind, factor, &p, &x);
                // The same curve through an identical DC blocker.
                let r = 1.0 - std::f32::consts::TAU * 5.0 / SAMPLE_RATE;
                let (mut x1, mut y1) = (0.0f32, 0.0f32);
                let drive = if kind == ShaperType::Bitcrush { p.bits } else { p.drive_db };
                let want: Vec<f32> = x
                    .iter()
                    .map(|&v| {
                        let t = transfer(kind, drive, p.bias, v);
                        let o = t - x1 + r * y1;
                        (x1, y1) = (t, o);
                        o
                    })
                    .collect();
                let tol = if factor == 1 { 1e-4 } else { 0.03 };
                let worst = (48_000..n).map(|i| (y[i] - want[i - lat]).abs()).fold(0.0f32, f32::max);
                assert!(worst < tol, "{kind:?} at {factor}x: off by {worst}");
            }
        }
    }

    #[test]
    fn tone_tilts_round_1khz() {
        let tone_gain = |tone: f32, hz: f64| {
            let x: Vec<f32> =
                (0..48_000).map(|i| (0.01 * (std::f64::consts::TAU * hz * i as f64 / 48_000.0).sin()) as f32).collect();
            let p = ShaperParams { drive_db: 0.0, tone, ..ShaperParams::default() };
            let y = run(ShaperType::SoftClip, 1, &p, &x);
            let peak = y[24_000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            20.0 * (peak / 0.01).log10()
        };
        assert!((tone_gain(1.0, 15_000.0) - 6.0).abs() < 1.0);
        assert!((tone_gain(1.0, 50.0) + 6.0).abs() < 1.0);
        assert!(tone_gain(0.0, 15_000.0).abs() < 0.1);
    }
}
