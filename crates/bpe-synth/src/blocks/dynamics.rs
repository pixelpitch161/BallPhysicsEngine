//! Dynamics: a compressor with a sidechain, an oversampled clipper, and a true-peak limiter
//! (the master stage uses the same limiter over a whole track).

use super::filter::flush;
use super::space::pole;
use crate::{BLOCK, Block, Buf, Ctx, Knobs, wired};
use std::collections::VecDeque;
use std::sync::OnceLock;

/// Taps of the 4x interpolation filter, at the 4x rate. Odd, so its delay is a whole number of
/// frames: 32 at 4x, which is 8 at the normal rate.
const TAPS: usize = 65;
/// How far the oversampler's filter delays the sound, in frames at the normal rate.
pub const OVERSAMPLE_DELAY: usize = 8;

/// A windowed-sinc low-pass at the original rate's Nyquist, summing to 4 (for zero-stuffed
/// upsampling).
fn taps() -> &'static [f32; TAPS] {
    static TAPS_: OnceLock<[f32; TAPS]> = OnceLock::new();
    TAPS_.get_or_init(|| {
        let mut h = [0.0f64; TAPS];
        let centre = (TAPS - 1) as f64 / 2.0;
        let fc = 0.125 * 0.92;
        for (n, v) in h.iter_mut().enumerate() {
            let x = n as f64 - centre;
            let sinc =
                if x == 0.0 { 2.0 * fc } else { (std::f64::consts::TAU * fc * x).sin() / (std::f64::consts::PI * x) };
            let w = std::f64::consts::TAU * n as f64 / (TAPS - 1) as f64;
            let blackman = 0.42 - 0.5 * w.cos() + 0.08 * (2.0 * w).cos();
            *v = sinc * blackman;
        }
        let sum: f64 = h.iter().sum();
        h.map(|v| (v * 4.0 / sum) as f32)
    })
}

/// Input frames each phase of the 4x filter reaches back over.
const PHASE: usize = TAPS / 4 + 1;

/// The filter split into its four phases: phase `p` takes taps `p`, `p + 4`, `p + 8`...
fn phases() -> &'static [[f32; PHASE]; 4] {
    static PHASES: OnceLock<[[f32; PHASE]; 4]> = OnceLock::new();
    PHASES.get_or_init(|| {
        let h = taps();
        let mut out = [[0.0; PHASE]; 4];
        for (p, phase) in out.iter_mut().enumerate() {
            for (k, t) in phase.iter_mut().enumerate() {
                *t = h.get(p + 4 * k).copied().unwrap_or(0.0);
            }
        }
        out
    })
}

/// The filter scaled by a quarter, for coming back down.
fn down_taps() -> &'static [f32; TAPS] {
    static DOWN: OnceLock<[f32; TAPS]> = OnceLock::new();
    DOWN.get_or_init(|| taps().map(|t| t * 0.25))
}

/// A history of the last `N` samples, newest first, held twice over so it reads as one slice.
#[derive(Clone, Debug)]
struct History<const N: usize> {
    buf: Vec<f32>,
    pos: usize,
}

impl<const N: usize> History<N> {
    fn new() -> Self {
        Self { buf: vec![0.0; 2 * N], pos: 0 }
    }

    fn push(&mut self, x: f32) {
        self.pos = (self.pos + N - 1) % N;
        self.buf[self.pos] = x;
        self.buf[self.pos + N] = x;
    }

    /// Newest first.
    fn slice(&self) -> &[f32] {
        &self.buf[self.pos..self.pos + N]
    }
}

/// Four times the sample rate and back, for clipping without aliasing and for finding the
/// peaks between samples.
#[derive(Clone, Debug)]
pub struct Oversampler4 {
    up: History<PHASE>,
    down: History<TAPS>,
}

impl Default for Oversampler4 {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler4 {
    pub fn new() -> Self {
        Self { up: History::new(), down: History::new() }
    }

    /// The four samples at 4x for the next sample in.
    pub fn up(&mut self, x: f32) -> [f32; 4] {
        self.up.push(x);
        let window = self.up.slice();
        phases().map(|phase| phase.iter().zip(window).map(|(t, v)| t * v).sum())
    }

    /// The next sample out, from four samples at 4x.
    pub fn down(&mut self, x: [f32; 4]) -> f32 {
        for v in x {
            self.down.push(v);
        }
        self.down.slice().iter().zip(down_taps().iter()).map(|(v, t)| v * t).sum()
    }
}

/// The largest level of interleaved stereo `samples` between the samples as well as on them:
/// the true peak, from 4x oversampling.
pub fn true_peak(samples: &[f32]) -> f32 {
    let mut ups = [Oversampler4::new(), Oversampler4::new()];
    let mut peak = 0.0f32;
    let tail = std::iter::repeat_n(0.0, 2 * (TAPS / 4 + 1));
    for (i, v) in samples.iter().copied().chain(tail).enumerate() {
        for u in ups[i % 2].up(v) {
            peak = peak.max(u.abs());
        }
    }
    peak
}

pub fn soft_clip(x: f32) -> f32 {
    x.tanh()
}

pub fn hard_clip(x: f32) -> f32 {
    x.clamp(-1.0, 1.0)
}

/// The streaming heart of the true-peak limiter: frames go in, and come out
/// [`LimiterCore::latency`] frames later, turned down just enough that their true peak stays
/// under the ceiling.
struct LimiterCore {
    ceiling: f32,
    lookahead: usize,
    /// Fixed release coefficient, or `None` for auto-release.
    release: Option<f32>,
    rate: f32,
    ups: [Oversampler4; 2],
    /// Frames waiting to come out.
    audio: VecDeque<[f32; 2]>,
    /// Gain each frame needs, for the sliding minimum.
    needed: VecDeque<(usize, f32)>,
    /// Sliding minimums waiting to be averaged, and their sum.
    mins: VecDeque<f32>,
    sum: f64,
    count: usize,
    gain: f32,
    /// Frames of the last second spent turned down, for auto-release.
    busy: VecDeque<bool>,
    busy_count: usize,
}

impl LimiterCore {
    fn new(rate: f32, ceiling_db: f32, release_ms: Option<f32>) -> Self {
        let lookahead = (0.005 * rate).round() as usize;
        Self {
            ceiling: 10f32.powf(ceiling_db.clamp(-24.0, 0.0) / 20.0),
            lookahead,
            release: release_ms.map(|ms| pole(ms.max(1.0) / 1_000.0, rate)),
            rate,
            ups: [Oversampler4::new(), Oversampler4::new()],
            audio: VecDeque::new(),
            needed: VecDeque::new(),
            mins: VecDeque::new(),
            sum: 0.0,
            count: 0,
            gain: 1.0,
            busy: VecDeque::new(),
            busy_count: 0,
        }
    }

    fn latency(&self) -> usize {
        OVERSAMPLE_DELAY + self.lookahead + 1
    }

    /// Takes the next frame and gives back the one [`LimiterCore::latency`] frames ago (silence
    /// at first), with the gain applied to it.
    fn push(&mut self, frame: [f32; 2]) -> ([f32; 2], f32) {
        let tp =
            self.ups[0].up(frame[0]).into_iter().chain(self.ups[1].up(frame[1])).fold(0.0f32, |m, v| m.max(v.abs()));
        let need = if tp > self.ceiling { self.ceiling / tp } else { 1.0 };
        self.audio.push_back(frame);
        // The detector lags the sound by the oversampler's delay; index by detection time.
        let at = self.count;
        self.count += 1;
        while self.needed.back().is_some_and(|&(_, v)| v >= need) {
            self.needed.pop_back();
        }
        self.needed.push_back((at, need));
        // The minimum over a lookahead window ending now, and a frame either side of it: a peak
        // between two frames is down to both.
        while self.needed.front().is_some_and(|&(i, _)| i + self.lookahead + 2 < at) {
            self.needed.pop_front();
        }
        let min = self.needed.front().map_or(1.0, |&(_, v)| v);
        self.mins.push_back(min);
        self.sum += min as f64;
        if self.mins.len() > self.lookahead + 1 {
            self.sum -= self.mins.pop_front().unwrap_or(1.0) as f64;
        }
        // Each minimum covers the frame a lookahead and one ago and its neighbours, so their average does too: it slides
        // down over the lookahead and is never above what that frame needs.
        let target = ((self.sum / self.mins.len() as f64) as f32).min(1.0);
        let release = self.release.unwrap_or_else(|| {
            // 50 ms, stretching to 500 ms as more of the last second is spent turned down.
            let share = self.busy_count as f32 / self.rate;
            pole(0.05 + 0.45 * share, self.rate)
        });
        self.gain = if target < self.gain { target } else { self.gain + (target - self.gain) * release };
        let busy = self.gain < 0.999;
        self.busy.push_back(busy);
        self.busy_count += busy as usize;
        if self.busy.len() > self.rate as usize {
            self.busy_count -= self.busy.pop_front().unwrap_or(false) as usize;
        }
        let out = if self.audio.len() > self.latency() { self.audio.pop_front().unwrap_or([0.0; 2]) } else { [0.0; 2] };
        (out.map(|v| flush(v * self.gain)), self.gain)
    }
}

/// A true-peak limiter over whole buffers: 5 ms lookahead, 4x oversampled detection, release in
/// milliseconds or automatic.
pub struct TruePeakLimiter {
    core: LimiterCore,
}

impl TruePeakLimiter {
    pub fn new(rate: f32, ceiling_db: f32, release_ms: Option<f32>) -> Self {
        Self { core: LimiterCore::new(rate, ceiling_db, release_ms) }
    }

    /// Limits `l` and `r` in place, and gives the gain reduction in dB (0 or more) for each
    /// block of [`BLOCK`] frames.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) -> Vec<f32> {
        let frames = l.len().min(r.len());
        let latency = self.core.latency();
        let mut gains = Vec::with_capacity(frames);
        for i in 0..frames + latency {
            let input = if i < frames { [l[i], r[i]] } else { [0.0; 2] };
            let (out, gain) = self.core.push(input);
            if i >= latency {
                l[i - latency] = out[0];
                r[i - latency] = out[1];
                gains.push(gain);
            }
        }
        gains.chunks(BLOCK).map(|c| -20.0 * c.iter().copied().fold(1.0f32, f32::min).max(1e-6).log10()).collect()
    }
}

/// The limiter as a block: the sound comes out a little over 10 ms late.
pub struct Limiter {
    ceiling: f32,
    release: f32,
    core: Option<LimiterCore>,
}

impl Limiter {
    pub fn new(k: &Knobs) -> Self {
        Self { ceiling: k.number("ceiling", -1.0).clamp(-6.0, 0.0), release: k.number("release", 100.0), core: None }
    }
}

impl Block for Limiter {
    fn reset(&mut self, cx: &Ctx) {
        self.core = Some(LimiterCore::new(cx.rate, self.ceiling, Some(self.release)));
    }

    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let input = wired(inputs, 0).copied().unwrap_or(crate::SILENT);
        let core = self.core.get_or_insert_with(|| LimiterCore::new(cx.rate, self.ceiling, Some(self.release)));
        for s in 0..BLOCK {
            let (out, _) = core.push([input[0][s], input[1][s]]);
            outputs[0][0][s] = out[0];
            outputs[0][1][s] = out[1];
        }
    }
}

/// A clipper at four times the rate: drive in, clip, and the drive taken back out.
pub struct Clipper {
    hard: bool,
    drive: f32,
    sides: [Oversampler4; 2],
    downs: [Oversampler4; 2],
}

impl Clipper {
    pub fn new(k: &Knobs) -> Self {
        Self {
            hard: k.choice("mode", "soft") == "hard",
            drive: 10f32.powf(k.number("drive", 0.0).clamp(0.0, 24.0) / 20.0),
            sides: [Oversampler4::new(), Oversampler4::new()],
            downs: [Oversampler4::new(), Oversampler4::new()],
        }
    }

    /// One sample through the clipper, on `side`.
    pub fn tick(&mut self, side: usize, x: f32) -> f32 {
        let up = self.sides[side].up(x * self.drive);
        let clipped = up.map(|v| if self.hard { hard_clip(v) } else { soft_clip(v) });
        self.downs[side].down(clipped) / self.drive
    }

    pub fn with(hard: bool, drive_db: f32) -> Self {
        Self::new(
            &Knobs::default()
                .with("mode", bpe_graph::Param::Choice(if hard { "hard" } else { "soft" }.into()))
                .with("drive", bpe_graph::Param::Number(drive_db)),
        )
    }
}

impl Block for Clipper {
    fn reset(&mut self, _cx: &Ctx) {
        self.sides = [Oversampler4::new(), Oversampler4::new()];
        self.downs = [Oversampler4::new(), Oversampler4::new()];
    }

    fn process(&mut self, _cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        let input = wired(inputs, 0).copied().unwrap_or(crate::SILENT);
        for side in 0..2 {
            for s in 0..BLOCK {
                outputs[0][side][s] = self.tick(side, input[side][s]);
            }
        }
    }
}

/// A compressor: above the threshold, the level rises only 1/ratio as fast. The level is read
/// from the sidechain when one is wired, else from the input.
pub struct Compressor {
    threshold: f32,
    ratio: f32,
    knee: f32,
    makeup: f32,
    attack: f32,
    release: f32,
    coeffs: (f32, f32),
    rate: f32,
    /// The peak level, which jumps up at once and falls over the release.
    level: f32,
    /// The gain change in dB, eased in over the attack and out over the release.
    reduction: f32,
}

impl Compressor {
    pub fn new(k: &Knobs) -> Self {
        Self {
            threshold: k.number("threshold", -18.0).clamp(-60.0, 0.0),
            ratio: k.number("ratio", 4.0).clamp(1.0, 20.0),
            knee: k.number("knee", 6.0).clamp(0.0, 24.0),
            makeup: k.number("makeup", 0.0).clamp(-24.0, 24.0),
            attack: k.number("attack", 10.0).max(0.0) / 1_000.0,
            release: k.number("release", 100.0).max(1.0) / 1_000.0,
            coeffs: (1.0, 1.0),
            rate: 0.0,
            level: 0.0,
            reduction: 0.0,
        }
    }

    /// The gain change in dB for a level in dB, with a soft knee.
    fn curve(&self, level_db: f32) -> f32 {
        let over = level_db - self.threshold;
        let slope = 1.0 / self.ratio - 1.0;
        if 2.0 * over < -self.knee {
            0.0
        } else if 2.0 * over.abs() <= self.knee && self.knee > 0.0 {
            slope * (over + self.knee / 2.0).powi(2) / (2.0 * self.knee)
        } else {
            slope * over
        }
    }
}

impl Block for Compressor {
    fn set_knob(&mut self, name: &str, value: f32) {
        match name {
            "threshold" => self.threshold = value.clamp(-60.0, 0.0),
            "makeup" => self.makeup = value.clamp(-24.0, 24.0),
            _ => {}
        }
    }

    fn reset(&mut self, _cx: &Ctx) {
        self.level = 0.0;
        self.reduction = 0.0;
    }

    fn process(&mut self, cx: &Ctx, inputs: &[Option<&Buf>], outputs: &mut [Buf]) {
        if self.rate != cx.rate {
            self.rate = cx.rate;
            self.coeffs = (pole(self.attack, cx.rate), pole(self.release, cx.rate));
        }
        let input = wired(inputs, 0).copied().unwrap_or(crate::SILENT);
        let key = wired(inputs, 1).copied().unwrap_or(input);
        for s in 0..BLOCK {
            let now = key[0][s].abs().max(key[1][s].abs());
            self.level = if now > self.level { now } else { flush(self.level + (now - self.level) * self.coeffs.1) };
            let target = self.curve(20.0 * self.level.max(1e-6).log10());
            // More turning down eases in over the attack; letting go, over the release.
            let c = if target < self.reduction { self.coeffs.0 } else { self.coeffs.1 };
            self.reduction = flush(self.reduction + (target - self.reduction) * c);
            let gain = 10f32.powf((self.reduction + self.makeup) / 20.0);
            for side in 0..2 {
                outputs[0][side][s] = input[side][s] * gain;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limiter_ramps_into_a_peak() {
        // A steady level, then a jump far over the ceiling: the gain slides down over the
        // lookahead instead of stepping, and the loud part still stays under the ceiling.
        let mut core = LimiterCore::new(48_000.0, -1.0, None);
        let latency = core.latency();
        let input: Vec<f32> = (0..4_800).map(|i| if i < 2_400 { 0.3 } else { 1.5 }).collect();
        let mut gains = Vec::new();
        let mut out = Vec::new();
        for i in 0..input.len() + latency {
            let x = input.get(i).copied().unwrap_or(0.0);
            let (o, g) = core.push([x, x]);
            gains.push(g);
            out.push(o[0]);
        }
        let biggest = gains.windows(2).map(|w| (w[0] - w[1]).abs()).fold(0.0f32, f32::max);
        assert!(biggest < 0.01, "the gain jumps by {biggest} in one frame");
        let ceiling = 10f32.powf(-1.0 / 20.0);
        assert!(out.iter().all(|v| v.abs() <= ceiling * 1.0001), "{}", out.iter().fold(0.0f32, |m, v| m.max(v.abs())));
    }
    use crate::testing::{constant, run, sine};
    use crate::{BLOCK, Ctx, Knobs, Rng};
    use bpe_graph::Param;

    fn db(x: f32) -> f32 {
        20.0 * x.max(1e-9).log10()
    }

    fn peak(x: &[[f32; 2]]) -> f32 {
        x.iter().fold(0.0f32, |m, f| m.max(f[0].abs()))
    }

    #[test]
    fn compressor_follows_its_ratio() {
        let k = Knobs::default()
            .with("threshold", Param::Number(-20.0))
            .with("ratio", Param::Number(4.0))
            .with("knee", Param::Number(0.0));
        let amp = 10f32.powf(-8.0 / 20.0);
        let out = run(&mut Compressor::new(&k), &mut Ctx::default(), 1500, |b, _| {
            let mut s = sine(440.0, 48_000.0, b * BLOCK);
            s.iter_mut().flatten().for_each(|v| *v *= amp);
            vec![Some(s)]
        });
        let level = db(peak(&out[48_000..]));
        assert!((level + 17.0).abs() < 0.5, "{level}");
    }

    #[test]
    fn sidechain_ducks() {
        let k = Knobs::default();
        let quiet = |b: usize| {
            let mut s = sine(440.0, 48_000.0, b * BLOCK);
            s.iter_mut().flatten().for_each(|v| *v *= 0.1);
            s
        };
        let ducked =
            run(&mut Compressor::new(&k), &mut Ctx::default(), 1500, |b, _| vec![Some(quiet(b)), Some(constant(1.0))]);
        let free = run(&mut Compressor::new(&k), &mut Ctx::default(), 1500, |b, _| vec![Some(quiet(b)), None]);
        assert!(db(peak(&free[48_000..])) - db(peak(&ducked[48_000..])) > 6.0);
        assert!((db(peak(&free[48_000..])) + 20.0).abs() < 0.3);
    }

    /// A quarter-rate sine at 45 degrees: its samples sit at 0.707 but the wave peaks at 1.
    fn intersample(frames: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = amp * (std::f32::consts::FRAC_PI_2 * i as f32 + std::f32::consts::FRAC_PI_4).sin();
                [v, v]
            })
            .collect()
    }

    #[test]
    fn true_peak_sees_intersample_peaks() {
        let x = intersample(4_800, 1.0);
        let sample_peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((sample_peak - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert!(true_peak(&x) > 0.98, "{}", true_peak(&x));
    }

    #[test]
    fn limiter_holds_the_true_peak_ceiling() {
        let frames = 96_000;
        let square = |hz: f32, amp: f32| -> Vec<f32> {
            (0..frames)
                .flat_map(|i| {
                    let v = if ((i as f32 * hz / 48_000.0).fract()) < 0.5 { amp } else { -amp };
                    [v, v]
                })
                .collect()
        };
        let mut rng = Rng::new(4);
        let noise: Vec<f32> = (0..frames * 2).map(|_| rng.next() * 4.0).collect();
        for (name, mut x) in [
            ("square 100", square(100.0, 2.0)),
            ("square 5k", square(5_000.0, 2.0)),
            ("intersample", intersample(frames, 2.0)),
            ("noise", noise),
        ] {
            let (mut l, mut r): (Vec<f32>, Vec<f32>) = x.chunks_exact(2).map(|f| (f[0], f[1])).unzip();
            let reduction = TruePeakLimiter::new(48_000.0, -1.0, None).process(&mut l, &mut r);
            assert!(reduction.iter().any(|&g| g > 1.0), "{name} was limited");
            for (i, f) in x.chunks_exact_mut(2).enumerate() {
                f[0] = l[i];
                f[1] = r[i];
            }
            let tp = db(true_peak(&x));
            assert!(tp <= -0.9, "{name}: {tp} dBTP");
            assert!(x.iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn clipper_hard_never_exceeds_one() {
        let k = Knobs::default().with("mode", Param::Choice("hard".into())).with("drive", Param::Number(24.0));
        let out =
            run(&mut Clipper::new(&k), &mut Ctx::default(), 500, |b, _| vec![Some(sine(440.0, 48_000.0, b * BLOCK))]);
        assert!(peak(&out) <= 1.02, "{}", peak(&out));
        assert!(peak(&out[4_800..]) > 0.05, "it still makes sound");
    }

    #[test]
    fn the_limiter_block_holds_its_ceiling() {
        let k = Knobs::default().with("ceiling", Param::Number(-3.0));
        let out = run(&mut Limiter::new(&k), &mut Ctx::default(), 1500, |b, _| {
            let mut s = sine(997.0, 48_000.0, b * BLOCK);
            s.iter_mut().flatten().for_each(|v| *v *= 2.0);
            vec![Some(s)]
        });
        // A slice cut mid-wave jumps to and from silence at its ends, which rings over the
        // ceiling on its own: fade the ends in and out before measuring.
        let part = &out[4_800..];
        let fade = 2_000.0;
        let flat: Vec<f32> = part
            .iter()
            .enumerate()
            .flat_map(|(i, f)| {
                let edge = (i as f32).min((part.len() - 1 - i) as f32);
                let g = (edge / fade).min(1.0);
                [f[0] * g, f[1] * g]
            })
            .collect();
        assert!(db(true_peak(&flat)) <= -2.9, "{}", db(true_peak(&flat)));
    }
}
