//! The chain's distortion: ten curves the sound is pushed through, run at a higher rate
//! (1×, 2×, 4× or 8×, through the halfband [`Oversampler`]) so the new harmonics fold back
//! as little as they can. Bias leans the curve before it (taken back out, so silence stays
//! silent); a 5 Hz DC blocker, the Tone tilt and the Output gain come after.

use crate::{BLOCK, Oversampler, SAMPLE_RATE, db_to_gain};

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

/// The antiderivative of one overdrive stage.
fn overdrive_area(x: f64) -> f64 {
    let a = x.abs();
    if a >= 1.0 { a - 0.375 } else { 0.75 * x * x - 0.125 * x * x * x * x }
}

/// The antiderivative of `curve` (one overdrive stage for Overdrive; Rectify's blend by `bias`).
fn area(kind: ShaperType, u: f64, bias: f64) -> f64 {
    match kind {
        ShaperType::HardClip => {
            if u.abs() <= 1.0 {
                0.5 * u * u
            } else {
                u.abs() - 0.5
            }
        }
        ShaperType::Tube => u.abs() - u.abs().ln_1p(),
        ShaperType::Diode => {
            if u >= 0.0 {
                u + (-u).exp() - 1.0
            } else {
                let c = DIODE_FLOOR as f64;
                c * c * ((u / c).exp() - 1.0) - c * u
            }
        }
        ShaperType::Overdrive => overdrive_area(u),
        ShaperType::Rectify => {
            let t = bias.abs().min(1.0);
            let full = 0.5 * u * u.abs();
            let half = if bias >= 0.0 { 0.5 * u.max(0.0).powi(2) } else { -0.5 * u.min(0.0).powi(2) };
            (1.0 - t) * full + t * half
        }
        _ => 0.0,
    }
}

/// The curve between the last input and this one, by its antiderivative: `(F(u) − F(u1)) /
/// (u − u1)`, or the curve at the middle when they are too close to tell apart.
#[inline]
fn antialiased(kind: ShaperType, u: f64, prev: &mut f64, bias: f64, direct: impl Fn(f64) -> f64) -> f64 {
    let u1 = *prev;
    *prev = u;
    let d = u - u1;
    if d.abs() < 1e-6 { direct(0.5 * (u + u1)) } else { (area(kind, u, bias) - area(kind, u1, bias)) / d }
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
    over: Oversampler,
    /// Tape's gentle low pass at the higher rate.
    tape: f32,
    /// The DC blocker's last input and output.
    dc: (f32, f32),
    /// The tilt's low-pass state.
    tilt: f32,
    /// Downsample: the held value and the hold clock.
    held: f32,
    clock: f32,
    /// The antialiasing's last inputs (two for Overdrive's two stages).
    prev: [f64; 2],
}

impl Channel {
    fn new(factor: u8) -> Self {
        Channel {
            over: Oversampler::new(factor),
            tape: 0.0,
            dc: (0.0, 0.0),
            tilt: 0.0,
            held: 0.0,
            clock: 1.0,
            prev: [0.0; 2],
        }
    }

    fn reset(&mut self) {
        self.over.reset();
        self.tape = 0.0;
        self.dc = (0.0, 0.0);
        self.tilt = 0.0;
        self.held = 0.0;
        self.clock = 1.0;
        self.prev = [0.0; 2];
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
        let ch = [Channel::new(factor), Channel::new(factor)];
        let factor = ch[0].over.factor();
        Shaper {
            kind,
            factor,
            cur: [0.0; NP],
            step: [0.0; NP],
            left: 0,
            fresh: true,
            ch,
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
        self.ch[0].over.latency()
    }

    pub fn reset(&mut self) {
        self.ch.iter_mut().for_each(Channel::reset);
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
            let u = if self.kind == ShaperType::Rectify { 0.0 } else { t[1] as f64 };
            for ch in &mut self.ch {
                ch.prev = [u, overdrive_stage(u as f32) as f64];
            }
            return;
        }
        for ((s, t), c) in self.step.iter_mut().zip(t).zip(self.cur) {
            *s = (t - c) / BLOCK as f32;
        }
        self.left = BLOCK as u32;
    }

    #[inline]
    fn channel(&mut self, c: usize, x: f32) -> f32 {
        let [g, bias, offset, q, hold, lo, hi, out] = self.cur;
        let kind = self.kind;
        let ch = &mut self.ch[c];
        let shaped = match kind {
            ShaperType::Downsample => {
                ch.clock += hold;
                if ch.clock >= 1.0 {
                    ch.clock -= 1.0;
                    ch.held = x;
                }
                ch.held
            }
            ShaperType::Bitcrush => curve(kind, x + bias, q) - offset,
            _ => {
                let mut buf = [0.0f32; 8];
                let f = self.factor;
                ch.over.up_one(x, &mut buf[..f]);
                for u in &mut buf[..f] {
                    let mut v = g * *u;
                    if kind == ShaperType::Tape {
                        ch.tape += self.tape_a * (v - ch.tape);
                        v = ch.tape;
                    }
                    *u = if self.smooth {
                        let b = bias as f64;
                        let y = match kind {
                            ShaperType::Rectify => {
                                antialiased(kind, v as f64, &mut ch.prev[0], b, |m| rectified(m as f32, bias) as f64)
                            }
                            ShaperType::Overdrive => {
                                let one = antialiased(kind, (v + bias) as f64, &mut ch.prev[0], b, |m| {
                                    overdrive_stage(m as f32) as f64
                                });
                                antialiased(kind, one, &mut ch.prev[1], b, |m| overdrive_stage(m as f32) as f64)
                            }
                            _ => antialiased(kind, (v + bias) as f64, &mut ch.prev[0], b, |m| {
                                curve(kind, m as f32, q) as f64
                            }),
                        };
                        y as f32 - offset
                    } else if kind == ShaperType::Rectify {
                        rectified(v, bias)
                    } else {
                        curve(kind, v + bias, q) - offset
                    };
                }
                ch.over.down_one(&buf[..f])
            }
        };
        // DC blocker, then the tilt round 1 kHz, then the output.
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
        (self.channel(0, l), self.channel(1, r))
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
