//! The moving effects: chorus, phaser and flanger. Each sweeps with a slow wave (its LFO) and
//! gives its wet sound only, in place; the bus's chain mixes dry and wet.
//!
//! Settings arrive once a block and glide. Rate and the rest take the usual 20 ms, but the
//! chorus's Depth and the flanger's Depth and Width move a delay time, so they glide at a steady
//! pace (full range in 250 ms): a faster change would bend the pitch.

use crate::SAMPLE_RATE;
use crate::smooth::Smoother;
use std::f32::consts::{FRAC_PI_4, PI, TAU};

/// The settings the three moving effects share. Each effect reads the ones it uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModParams {
    /// How fast the sweep goes, 0.01..10 Hz (a rate synced to the tempo arrives already in Hz).
    pub rate_hz: f32,
    /// How far the sweep goes, 0..1.
    pub depth: f32,
    /// How much of the output goes back in, -0.95..0.95 (phaser and flanger; negative inverts).
    pub feedback: f32,
    /// How many copies the chorus sings with, 2..4.
    pub voices: u8,
    /// How many allpass stages the phaser has, 2..12.
    pub stages: u8,
    /// How far apart the left and right sweeps are, 0..1.
    pub width: f32,
}

/// Values smaller than this in a feedback loop are taken as silence, so a fading loop never
/// reaches the slow denormal numbers.
const TINY: f32 = 1e-24;

#[inline]
fn flush(v: f32) -> f32 {
    if v.abs() < TINY { 0.0 } else { v }
}

/// A soft limit for what goes round a feedback loop: straight for small values, never past 2.
#[inline]
fn soft(v: f32) -> f32 {
    let x = (v * 0.5).clamp(-3.0, 3.0);
    2.0 * x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// Samples in `ms` milliseconds.
#[inline]
fn ms(ms: f32) -> f32 {
    ms * SAMPLE_RATE / 1_000.0
}

/// A delay line of fixed length, read between samples with a cubic (Hermite) curve.
#[derive(Clone, Debug)]
struct Line {
    buf: Vec<f32>,
    mask: usize,
    /// Where the next sample goes.
    pos: usize,
}

impl Line {
    /// A line that can be read up to `longest` samples back.
    fn new(longest: f32) -> Self {
        let size = (longest as usize + 4).next_power_of_two();
        Self { buf: vec![0.0; size], mask: size - 1, pos: 0 }
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }

    #[inline]
    fn push(&mut self, x: f32) {
        self.buf[self.pos] = x;
        self.pos = (self.pos + 1) & self.mask;
    }

    /// The sample `k` back (1 is the newest).
    #[inline]
    fn at(&self, k: usize) -> f32 {
        self.buf[self.pos.wrapping_sub(k) & self.mask]
    }

    /// The sound `delay` samples back (2 or more), between samples.
    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let i = delay as usize;
        let f = delay - i as f32;
        let (ym, y0, y1, y2) = (self.at(i - 1), self.at(i), self.at(i + 1), self.at(i + 2));
        let c1 = 0.5 * (y1 - ym);
        let c2 = ym - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
        let c3 = 0.5 * (y2 - ym) + 1.5 * (y0 - y1);
        ((c3 * f + c2) * f + c1) * f + y0
    }
}

/// The slow wave every moving effect sweeps with: a phase 0..1 that turns at the (smoothed) rate.
#[derive(Clone, Debug)]
struct Lfo {
    phase: f64,
    rate: Smoother,
}

impl Lfo {
    fn new() -> Self {
        Self { phase: 0.0, rate: Smoother::new(Smoother::KNOB_MS) }
    }

    /// The phase for this sample, then a step on.
    #[inline]
    fn tick(&mut self) -> f32 {
        let now = self.phase as f32;
        self.phase += self.rate.next() as f64 / SAMPLE_RATE as f64;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        now
    }

    fn set_phase(&mut self, phase: f64) {
        let p = phase.rem_euclid(1.0);
        self.phase = if p < 1.0 { p } else { 0.0 };
    }
}

/// A setting that moves towards its target at a steady pace: full range (1) in 250 ms. Used for
/// what moves a delay time, where a fast glide would bend the pitch.
#[derive(Clone, Debug)]
struct Glide {
    value: f32,
    target: f32,
}

impl Glide {
    const STEP: f32 = 1.0 / (0.25 * SAMPLE_RATE);

    fn new() -> Self {
        Self { value: 0.0, target: 0.0 }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        self.value += (self.target - self.value).clamp(-Self::STEP, Self::STEP);
        self.value
    }

    fn aim(&mut self, target: f32, snap: bool) {
        self.target = target;
        if snap {
            self.value = target;
        }
    }

    fn arrive(&mut self) {
        self.value = self.target;
    }
}

/// Sets a smoother's target, or puts it straight there when `snap`.
fn aim(s: &mut Smoother, target: f32, snap: bool) {
    if snap { s.reset(target) } else { s.set_target(target) }
}

/// Puts a smoother at its target, with nothing left to glide.
fn arrive(s: &mut Smoother) {
    s.reset(s.target());
}

/// The most voices the chorus sings with.
const CHORUS_VOICES: usize = 4;
/// Each voice's middle delay, in ms. Fixed per voice, so adding a voice moves no other.
const CHORUS_CENTRE_MS: [f32; CHORUS_VOICES] = [12.0, 20.0, 16.0, 14.0];
/// How far each voice's delay swings either way at full depth, in ms (so 7..25 ms in all).
const CHORUS_SWING_MS: f32 = 5.0;
/// Where each voice is on the wave, as a part of a turn.
const CHORUS_OFFSET: [f32; CHORUS_VOICES] = [0.0, 0.5, 0.25, 0.75];

/// A chorus: 2..4 copies of the sound, each delayed 7..25 ms by its own point on the wave and
/// spread across the stereo field by Width (0 all in the middle, 1 from hard left to hard right).
/// The copies are scaled so the wet sound is as loud as what came in. It has no feedback.
#[derive(Clone, Debug)]
pub struct Chorus {
    lines: [Line; 2],
    lfo: Lfo,
    depth: Glide,
    /// Each voice's left and right gains (0 for a voice that is off).
    gains: [[Smoother; 2]; CHORUS_VOICES],
    /// The next `set` jumps straight to its values.
    fresh: bool,
}

impl Default for Chorus {
    fn default() -> Self {
        Self::new()
    }
}

impl Chorus {
    pub fn new() -> Self {
        let longest = ms(CHORUS_CENTRE_MS.iter().fold(0.0f32, |a, b| a.max(*b)) + CHORUS_SWING_MS);
        let s = || Smoother::new(Smoother::KNOB_MS);
        Self {
            lines: [Line::new(longest), Line::new(longest)],
            lfo: Lfo::new(),
            depth: Glide::new(),
            gains: std::array::from_fn(|_| [s(), s()]),
            fresh: true,
        }
    }

    /// Takes the rate, depth, voices and width (once a block; they glide).
    pub fn set(&mut self, p: &ModParams) {
        let snap = std::mem::take(&mut self.fresh);
        aim(&mut self.lfo.rate, p.rate_hz.clamp(0.0, 20.0), snap);
        self.depth.aim(p.depth.clamp(0.0, 1.0), snap);
        let voices = (p.voices as usize).clamp(2, CHORUS_VOICES);
        let width = p.width.clamp(0.0, 1.0);
        for (v, [gl, gr]) in self.gains.iter_mut().enumerate() {
            let (l, r) = if v < voices {
                // From the left (-1) to the right (+1), equal power.
                let pan = width * (2.0 * v as f32 / (voices - 1) as f32 - 1.0);
                let angle = (pan + 1.0) * FRAC_PI_4;
                (angle.cos(), angle.sin())
            } else {
                (0.0, 0.0)
            };
            aim(gl, l, snap);
            aim(gr, r, snap);
        }
    }

    /// Back to silence at the wave's start, with the settings where they were heading.
    pub fn reset(&mut self) {
        for l in &mut self.lines {
            l.clear();
        }
        self.lfo.phase = 0.0;
        arrive(&mut self.lfo.rate);
        self.depth.arrive();
        self.gains.iter_mut().flatten().for_each(arrive);
        self.fresh = true;
    }

    /// Puts the wave at `phase` (0..1 of a turn).
    pub fn set_phase(&mut self, phase: f64) {
        self.lfo.set_phase(phase);
    }

    /// Replaces `l` and `r` with the chorus's wet sound.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let phase = self.lfo.tick();
            let swing = ms(CHORUS_SWING_MS) * self.depth.next();
            let (mut sum, mut power) = ([0.0f32; 2], [0.0f32; 2]);
            for v in 0..CHORUS_VOICES {
                let g = [self.gains[v][0].next(), self.gains[v][1].next()];
                if g == [0.0, 0.0] {
                    continue;
                }
                let delay = ms(CHORUS_CENTRE_MS[v]) + swing * (TAU * (phase + CHORUS_OFFSET[v])).sin();
                for c in 0..2 {
                    sum[c] += g[c] * self.lines[c].read(delay);
                    power[c] += g[c] * g[c];
                }
            }
            self.lines[0].push(*a);
            self.lines[1].push(*b);
            // The copies are unrelated, so their powers add: scale back to one copy's.
            *a = if power[0] > 0.0 { sum[0] / power[0].sqrt() } else { 0.0 };
            *b = if power[1] > 0.0 { sum[1] / power[1].sqrt() } else { 0.0 };
        }
    }
}

/// The most allpass stages the phaser has.
const MAX_STAGES: usize = 12;
/// The phaser's sweep, from its lowest to its highest corner, in Hz.
const PHASER_LOW_HZ: f32 = 200.0;
const PHASER_HIGH_HZ: f32 = 5_000.0;

/// A phaser: 2..12 first-order allpass stages in a row per channel, their corner swept from
/// 200 Hz to 5 kHz (evenly in pitch; Depth 0 holds it at 1 kHz) by the wave, with feedback from
/// the last stage back to the first. Its "wet" sound already holds the notches: it is the stages'
/// output mixed half and half with what came in, which makes one notch for every two stages. A
/// dry/wet Mix on top only makes the notches shallower. Width 1 puts the right sweep a quarter
/// turn after the left.
#[derive(Clone, Debug)]
pub struct Phaser {
    lfo: Lfo,
    depth: Smoother,
    feedback: Smoother,
    width: Smoother,
    /// How much each stage count's output is heard; only the chosen count's goes to 1, so a
    /// change of stages crossfades.
    taps: [Smoother; MAX_STAGES + 1],
    /// Each channel's stage memories.
    state: [[f32; MAX_STAGES]; 2],
    /// Each channel's last output, for the feedback.
    last: [f32; 2],
    fresh: bool,
}

impl Default for Phaser {
    fn default() -> Self {
        Self::new()
    }
}

impl Phaser {
    pub fn new() -> Self {
        let s = || Smoother::new(Smoother::KNOB_MS);
        Self {
            lfo: Lfo::new(),
            depth: s(),
            feedback: s(),
            width: s(),
            taps: std::array::from_fn(|_| s()),
            state: [[0.0; MAX_STAGES]; 2],
            last: [0.0; 2],
            fresh: true,
        }
    }

    /// Takes the rate, depth, feedback, stages and width (once a block; they glide).
    pub fn set(&mut self, p: &ModParams) {
        let snap = std::mem::take(&mut self.fresh);
        aim(&mut self.lfo.rate, p.rate_hz.clamp(0.0, 20.0), snap);
        aim(&mut self.depth, p.depth.clamp(0.0, 1.0), snap);
        aim(&mut self.feedback, p.feedback.clamp(-0.95, 0.95), snap);
        aim(&mut self.width, p.width.clamp(0.0, 1.0), snap);
        let stages = (p.stages as usize).clamp(2, MAX_STAGES);
        for (k, t) in self.taps.iter_mut().enumerate() {
            aim(t, if k == stages { 1.0 } else { 0.0 }, snap);
        }
    }

    /// Back to silence at the wave's start, with the settings where they were heading.
    pub fn reset(&mut self) {
        self.state = [[0.0; MAX_STAGES]; 2];
        self.last = [0.0; 2];
        self.lfo.phase = 0.0;
        arrive(&mut self.lfo.rate);
        arrive(&mut self.depth);
        arrive(&mut self.feedback);
        arrive(&mut self.width);
        self.taps.iter_mut().for_each(arrive);
        self.fresh = true;
    }

    /// Puts the wave at `phase` (0..1 of a turn).
    pub fn set_phase(&mut self, phase: f64) {
        self.lfo.set_phase(phase);
    }

    /// Replaces `l` and `r` with the phaser's wet sound (the notches already in it).
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        // The most stages still heard; a tap only fades down while this call runs.
        let heard = |t: &Smoother| t.value() != 0.0 || t.target() != 0.0;
        let top = (2..=MAX_STAGES).rev().find(|&k| heard(&self.taps[k])).unwrap_or(2);
        let middle = (PHASER_LOW_HZ * PHASER_HIGH_HZ).sqrt();
        let reach = (PHASER_HIGH_HZ / middle).ln();
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let phase = self.lfo.tick();
            let depth = self.depth.next();
            let feedback = self.feedback.next();
            let offset = 0.25 * self.width.next();
            let mut taps = [0.0f32; MAX_STAGES + 1];
            for (k, t) in taps.iter_mut().enumerate().take(top + 1).skip(2) {
                *t = self.taps[k].next();
            }
            for (c, x) in [a, b].into_iter().enumerate() {
                let wave = -(TAU * (phase + c as f32 * offset)).cos();
                let corner = middle * (reach * depth * wave).exp();
                let t = (PI * corner / SAMPLE_RATE).tan();
                let coef = (t - 1.0) / (t + 1.0);
                let mut v = *x + soft(feedback * self.last[c]);
                let mut out = 0.0;
                for (k, s) in self.state[c].iter_mut().enumerate().take(top) {
                    let y = coef * v + *s;
                    *s = flush(v - coef * y);
                    v = y;
                    out += taps[k + 1] * v;
                }
                self.last[c] = flush(out);
                *x = 0.5 * (*x + out);
            }
        }
    }
}

/// The flanger's shortest and longest delay, in ms.
const FLANGER_SHORT_MS: f32 = 0.1;
const FLANGER_LONG_MS: f32 = 8.0;

/// A flanger: one delay per channel swept from 0.1 ms up to 8 ms at full depth, with feedback
/// (negative inverts it, for the hollow sound). What goes round the loop is softly limited, so it
/// stays bounded even at full feedback. Width 1 puts the right sweep a quarter turn after the left.
#[derive(Clone, Debug)]
pub struct Flanger {
    lines: [Line; 2],
    lfo: Lfo,
    depth: Glide,
    feedback: Smoother,
    width: Glide,
    fresh: bool,
}

impl Default for Flanger {
    fn default() -> Self {
        Self::new()
    }
}

impl Flanger {
    pub fn new() -> Self {
        let s = || Smoother::new(Smoother::KNOB_MS);
        Self {
            lines: [Line::new(ms(FLANGER_LONG_MS)), Line::new(ms(FLANGER_LONG_MS))],
            lfo: Lfo::new(),
            depth: Glide::new(),
            feedback: s(),
            width: Glide::new(),
            fresh: true,
        }
    }

    /// Takes the rate, depth, feedback and width (once a block; they glide).
    pub fn set(&mut self, p: &ModParams) {
        let snap = std::mem::take(&mut self.fresh);
        aim(&mut self.lfo.rate, p.rate_hz.clamp(0.0, 20.0), snap);
        self.depth.aim(p.depth.clamp(0.0, 1.0), snap);
        aim(&mut self.feedback, p.feedback.clamp(-0.95, 0.95), snap);
        self.width.aim(p.width.clamp(0.0, 1.0), snap);
    }

    /// Back to silence at the wave's start, with the settings where they were heading.
    pub fn reset(&mut self) {
        for l in &mut self.lines {
            l.clear();
        }
        self.lfo.phase = 0.0;
        arrive(&mut self.lfo.rate);
        self.depth.arrive();
        arrive(&mut self.feedback);
        self.width.arrive();
        self.fresh = true;
    }

    /// Puts the wave at `phase` (0..1 of a turn).
    pub fn set_phase(&mut self, phase: f64) {
        self.lfo.set_phase(phase);
    }

    /// Replaces `l` and `r` with the flanger's wet sound (the swept, fed-back delay).
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let short = ms(FLANGER_SHORT_MS);
        let span = ms(FLANGER_LONG_MS) - short;
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let phase = self.lfo.tick();
            let depth = self.depth.next();
            let feedback = self.feedback.next();
            let offset = 0.25 * self.width.next();
            for (c, x) in [a, b].into_iter().enumerate() {
                let wave = 0.5 - 0.5 * (TAU * (phase + c as f32 * offset)).cos();
                let y = self.lines[c].read(short + span * depth * wave);
                self.lines[c].push(flush(*x + soft(feedback * y)));
                *x = y;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Noise, sine};
    use crate::{BLOCK, SAMPLE_RATE};

    fn params(rate_hz: f32, depth: f32) -> ModParams {
        ModParams { rate_hz, depth, feedback: 0.0, voices: 3, stages: 4, width: 0.5 }
    }

    /// The three effects behind one face, so a test can run them all.
    trait Fx {
        fn set(&mut self, p: &ModParams);
        fn reset(&mut self);
        fn set_phase(&mut self, phase: f64);
        fn process(&mut self, l: &mut [f32], r: &mut [f32]);
    }
    macro_rules! fx {
        ($t:ty) => {
            impl Fx for $t {
                fn set(&mut self, p: &ModParams) {
                    <$t>::set(self, p)
                }
                fn reset(&mut self) {
                    <$t>::reset(self)
                }
                fn set_phase(&mut self, phase: f64) {
                    <$t>::set_phase(self, phase)
                }
                fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
                    <$t>::process(self, l, r)
                }
            }
        };
    }
    fx!(Chorus);
    fx!(Phaser);
    fx!(Flanger);

    fn all() -> Vec<(&'static str, Box<dyn Fx>)> {
        vec![
            ("chorus", Box::new(Chorus::new())),
            ("phaser", Box::new(Phaser::new())),
            ("flanger", Box::new(Flanger::new())),
        ]
    }

    /// Runs `fx` over `l` and `r` block by block, calling `before(block index, fx)` before each.
    fn run(fx: &mut dyn Fx, l: &mut [f32], r: &mut [f32], mut before: impl FnMut(usize, &mut dyn Fx)) {
        for (i, (a, b)) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).enumerate() {
            before(i, fx);
            fx.process(a, b);
        }
    }

    fn noise(seed: u64, n: usize) -> Vec<f32> {
        let mut g = Noise(seed);
        (0..n).map(|_| g.next() * 0.5).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt() as f32
    }

    fn max_step(x: &[f32]) -> f32 {
        x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    /// The level of `h` at `hz`, in dB.
    fn response_db(h: &[f32], hz: f64) -> f64 {
        let w = std::f64::consts::TAU * hz / SAMPLE_RATE as f64;
        let (mut re, mut im) = (0.0, 0.0);
        for (n, v) in h.iter().enumerate() {
            re += *v as f64 * (w * n as f64).cos();
            im -= *v as f64 * (w * n as f64).sin();
        }
        10.0 * (re * re + im * im).max(1e-30).log10()
    }

    #[test]
    fn phaser_notch_count_follows_stages() {
        for stages in [4u8, 8, 12] {
            let mut ph = Phaser::new();
            ph.set(&ModParams { stages, width: 0.0, ..params(0.01, 0.0) });
            let mut l = vec![0.0; 8192];
            let mut r = vec![0.0; 8192];
            l[0] = 1.0;
            for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
                ph.process(a, b);
            }
            let curve: Vec<f64> = (0..3000).map(|i| response_db(&l, 20.0 * 1000f64.powf(i as f64 / 2999.0))).collect();
            let notches = curve.windows(3).filter(|w| w[1] < w[0] && w[1] <= w[2] && w[1] < -20.0).count();
            assert_eq!(notches, stages as usize / 2, "{stages} stages");
            assert!(curve[0] > -1.0 && curve[2999] > -3.0, "flat away from the notches");
        }
    }

    #[test]
    fn flanger_feedback_stays_bounded() {
        for feedback in [0.95, -0.95] {
            let mut fl = Flanger::new();
            let n = 10 * SAMPLE_RATE as usize;
            let mut l: Vec<f32> = noise(1, n).iter().map(|v| v * 2.0).collect();
            let mut r: Vec<f32> = noise(2, n).iter().map(|v| v * 2.0).collect();
            fl.set(&ModParams { feedback, ..params(0.3, 0.7) });
            run(&mut fl, &mut l, &mut r, |_, _| {});
            let peak = l.iter().chain(&r).map(|v| v.abs()).fold(0.0, f32::max);
            assert!(l.iter().chain(&r).all(|v| v.is_finite()));
            assert!(peak < 6.0, "peak {peak} at feedback {feedback}");
            assert!(rms(&l) > 0.1, "it still sounds");
        }
    }

    #[test]
    fn phaser_feedback_stays_bounded() {
        for feedback in [0.95, -0.95] {
            let mut ph = Phaser::new();
            let n = 5 * SAMPLE_RATE as usize;
            let mut l = noise(3, n);
            let mut r = noise(4, n);
            ph.set(&ModParams { feedback, stages: 12, ..params(2.0, 1.0) });
            run(&mut ph, &mut l, &mut r, |_, _| {});
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 6.0));
        }
    }

    #[test]
    fn chorus_keeps_level() {
        for voices in 2..=4u8 {
            for width in [0.0, 0.5, 1.0] {
                let mut ch = Chorus::new();
                let n = 4 * SAMPLE_RATE as usize;
                let (dl, dr) = (noise(5, n), noise(6, n));
                let (mut l, mut r) = (dl.clone(), dr.clone());
                ch.set(&ModParams { voices, width, ..params(0.8, 0.6) });
                run(&mut ch, &mut l, &mut r, |_, _| {});
                let skip = 4800;
                for (wet, dry, side) in [(&l, &dl, "left"), (&r, &dr, "right")] {
                    let db = 20.0 * (rms(&wet[skip..]) / rms(&dry[skip..])).log10();
                    assert!(db.abs() < 1.5, "{voices} voices, width {width}, {side}: {db} dB");
                }
            }
        }
    }

    #[test]
    fn set_phase_is_deterministic() {
        for ((name, mut a), (_, mut b)) in all().into_iter().zip(all()) {
            let p = ModParams { feedback: 0.6, ..params(1.7, 0.8) };
            // `a` has a past; `b` is new. After a reset and the same phase they must agree.
            let mut ol = noise(7, 24_000);
            let mut or = noise(8, 24_000);
            a.set(&ModParams { voices: 2, stages: 10, ..params(5.0, 0.3) });
            run(a.as_mut(), &mut ol, &mut or, |_, _| {});
            a.reset();
            a.set(&p);
            a.set_phase(0.37);
            b.set(&p);
            b.set_phase(0.37);
            let (mut al, mut ar) = (noise(9, 48_000), noise(10, 48_000));
            let (mut bl, mut br) = (al.clone(), ar.clone());
            run(a.as_mut(), &mut al, &mut ar, |_, _| {});
            run(b.as_mut(), &mut bl, &mut br, |_, _| {});
            assert!(al == bl && ar == br, "{name}");
            // And the phase matters: another phase gives another sound.
            let mut c = all().into_iter().find(|(n, _)| *n == name).unwrap().1;
            c.set(&p);
            c.set_phase(0.87);
            let (mut cl, mut cr) = (noise(9, 48_000), noise(10, 48_000));
            run(c.as_mut(), &mut cl, &mut cr, |_, _| {});
            assert!(cl != al, "{name}: phase is ignored");
        }
    }

    /// Changing a setting at block `at` must not make a step bigger than the effect, or the sound
    /// going in, makes anyway.
    fn no_click(name: &str, fx: &mut dyn Fx, from: ModParams, to: ModParams) {
        let n = 2 * SAMPLE_RATE as usize;
        let at = 400;
        let mut l = sine(440.0, 0.5, 0.0, n);
        let mut r = sine(440.0, 0.5, 1.0, n);
        let dry = max_step(&l);
        run(fx, &mut l, &mut r, |i, fx| {
            if i == 0 {
                fx.set(&from);
            } else if i == at {
                fx.set(&to);
            }
        });
        let change = at * BLOCK;
        let around = change..change + 4800;
        for (x, side) in [(&l, "left"), (&r, "right")] {
            let steady = max_step(&x[2400..change]).max(max_step(&x[around.end..])).max(dry);
            let there = max_step(&x[around.clone()]);
            assert!(there <= steady * 1.1 + 1e-3, "{name} {side}: step {there} against {steady}");
        }
    }

    #[test]
    fn rate_changes_do_not_click() {
        for (name, mut fx) in all() {
            let from = ModParams { feedback: 0.5, ..params(0.2, 0.7) };
            no_click(name, fx.as_mut(), from, ModParams { rate_hz: 10.0, ..from });
            fx.reset();
            no_click(name, fx.as_mut(), ModParams { rate_hz: 10.0, ..from }, from);
        }
    }

    #[test]
    fn voice_and_stage_changes_do_not_click() {
        for (name, mut fx) in all() {
            let from = ModParams { voices: 2, stages: 2, width: 0.0, feedback: 0.3, ..params(0.5, 0.6) };
            let to = ModParams { voices: 4, stages: 12, width: 1.0, depth: 1.0, feedback: -0.3, ..from };
            no_click(name, fx.as_mut(), from, to);
            fx.reset();
            no_click(name, fx.as_mut(), to, from);
        }
    }

    #[test]
    fn silence_stays_silent() {
        for (name, mut fx) in all() {
            fx.set(&ModParams { feedback: 0.9, ..params(3.0, 1.0) });
            let mut l = vec![0.0; 4800];
            let mut r = vec![0.0; 4800];
            run(fx.as_mut(), &mut l, &mut r, |_, _| {});
            assert!(l.iter().chain(&r).all(|v| *v == 0.0), "{name}");
        }
    }
}
