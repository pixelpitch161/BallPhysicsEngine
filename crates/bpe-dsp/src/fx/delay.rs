//! The delay: echoes in stereo, ping-pong or from four taps, with three characters. Digital is
//! clean and jumps to a new time by crossfading; Analog darkens and saturates each repeat; Tape
//! adds wow, flutter and a little hiss. Analog and Tape glide to a new time, bending the pitch.

use super::reverb_common::{Line, Svf, SvfCoefs, flush};
use crate::{Noise, SAMPLE_RATE, Smoother};

/// How the echoes are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayMode {
    /// Left echoes left and right echoes right.
    Standard,
    /// Both sides summed, echoing left first, then right, then left...
    PingPong,
    /// Four taps, each at its own share of the time with its own level and pan.
    MultiTap,
}

/// How the echoes sound and how a time change behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Character {
    /// Clean repeats; a new time crossfades in over 50 ms with no pitch bend.
    Digital,
    /// Each repeat darker and gently saturated; a new time glides, bending the pitch.
    Analog,
    /// Saturated, with wow, flutter and a little hiss; a new time glides, bending the pitch.
    Tape,
}

/// One of the multi-tap mode's four taps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tap {
    /// When it repeats, as a share of the delay time (0.01..1).
    pub time: f32,
    /// Its level (a plain gain).
    pub gain: f32,
    /// Where it sits, -1 left to 1 right (full level on both sides in the middle).
    pub pan: f32,
}

/// The delay's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelayParams {
    pub mode: DelayMode,
    pub character: Character,
    /// The time between echoes, in samples (48..192000).
    pub time_samples: f32,
    /// How much of each echo comes back (0..1; Digital stops at 0.98).
    pub feedback: f32,
    /// The repeats' low cut in Hz; 20 or less is off.
    pub low_cut_hz: f32,
    /// The repeats' high cut in Hz; 20000 or more is off.
    pub high_cut_hz: f32,
    /// How much the cuts ring at their frequency (0..1).
    pub reso: f32,
    /// How wide the echoes are (0 mono, 1 full).
    pub width: f32,
    /// The multi-tap mode's taps; the longest one feeds back.
    pub taps: [Tap; 4],
}

impl Default for DelayParams {
    fn default() -> Self {
        let tap = |time: f32, pan: f32| Tap { time, gain: 1.0, pan };
        Self {
            mode: DelayMode::Standard,
            character: Character::Digital,
            time_samples: 12_000.0,
            feedback: 0.4,
            low_cut_hz: 20.0,
            high_cut_hz: 20_000.0,
            reso: 0.0,
            width: 1.0,
            taps: [tap(0.25, -0.5), tap(0.5, 0.5), tap(0.75, -1.0), tap(1.0, 1.0)],
        }
    }
}

/// The longest delay time, in samples (4 s).
pub const MAX_TIME_SAMPLES: usize = 192_000;

/// How many read times the delay keeps: the main time, then the four taps.
const READS: usize = 5;
/// The Digital character's crossfade to a new time, in samples (50 ms).
const FADE: f32 = 0.05 * SAMPLE_RATE;
/// How fast a gliding read head heads for its new time (its time constant, in samples).
const GLIDE: f32 = 0.12 * SAMPLE_RATE;
/// The fastest a gliding head moves, in samples per sample (0.6 bends the pitch by up to 0.4x).
const MAX_RATE: f32 = 0.6;
/// The tape's wow (0.5 Hz) and flutter (6 Hz) depths, in samples.
const WOW_DEPTH: f32 = 12.0;
const FLUTTER_DEPTH: f32 = 2.4;
/// The tape hiss level (-72 dBFS), reached when the loop is at -30 dBFS or louder.
const HISS: f32 = 2.512e-4;
/// The shortest read, so the cubic reads only samples already written.
const MIN_READ: f32 = 2.0;
const MAX_READ: f32 = MAX_TIME_SAMPLES as f32 + WOW_DEPTH + FLUTTER_DEPTH + 2.0;

/// Where the read heads are, and how they move to a new time.
#[derive(Clone, Debug)]
struct Heads {
    pos: [f32; READS],
    next: [f32; READS],
    vel: [f32; READS],
    fading: bool,
    fade: f32,
    /// How far into the crossfade the reads are (0 old, 1 new), this sample.
    mix: f32,
}

impl Heads {
    fn new() -> Self {
        Self { pos: [0.0; READS], next: [0.0; READS], vel: [0.0; READS], fading: false, fade: 0.0, mix: 0.0 }
    }

    fn snap(&mut self, to: &[f32; READS]) {
        *self = Self { pos: *to, next: *to, ..Self::new() };
    }

    /// One sample on, heading for `to`: Digital crossfades, the others glide.
    #[inline]
    fn step(&mut self, to: &[f32; READS], digital: bool) {
        if self.fading {
            self.fade += 1.0 / FADE;
            if self.fade >= 1.0 {
                self.pos = self.next;
                self.fading = false;
                self.mix = 0.0;
            } else {
                let f = self.fade;
                self.mix = f * f * (3.0 - 2.0 * f);
            }
        } else if digital {
            self.vel = [0.0; READS];
            if self.pos.iter().zip(to).any(|(p, t)| (p - t).abs() > 1e-3) {
                self.next = *to;
                self.fading = true;
                self.fade = 0.0;
                self.mix = 0.0;
            }
        } else {
            let k = 1.0 / (0.02 * SAMPLE_RATE);
            for ((p, v), t) in self.pos.iter_mut().zip(self.vel.iter_mut()).zip(to) {
                let e = t - *p;
                if e.abs() < 1e-3 && v.abs() < 1e-5 {
                    *p = *t;
                    *v = 0.0;
                } else {
                    *v += ((e / GLIDE).clamp(-MAX_RATE, MAX_RATE) - *v) * k;
                    *p += *v;
                }
            }
        }
    }

    /// Read `k` from `line`, `wow` samples later.
    #[inline]
    fn read(&self, line: &Line, k: usize, wow: f32) -> f32 {
        let a = line.hermite((self.pos[k] + wow).clamp(MIN_READ, MAX_READ));
        if self.fading {
            let b = line.hermite((self.next[k] + wow).clamp(MIN_READ, MAX_READ));
            a + self.mix * (b - a)
        } else {
            a
        }
    }
}

/// This sample's settings for what happens to the sound on its way into the line.
struct Colour {
    low: SvfCoefs,
    high: SvfCoefs,
    low_on: f32,
    high_on: f32,
    dark: f32,
    dark_c: f32,
    sat: f32,
    hiss: f32,
}

/// One line's filters, darkening and hiss gate.
#[derive(Clone, Debug, Default)]
struct LoopState {
    filters: [Svf; 2],
    dark: f32,
    env: f32,
}

impl LoopState {
    #[inline]
    fn process(&mut self, x: f32, c: &Colour, noise: &mut Noise) -> f32 {
        let (_, hp) = self.filters[0].run(x, &c.low);
        let mut x = x + c.low_on * (hp - x);
        let (lp, _) = self.filters[1].run(x, &c.high);
        x += c.high_on * (lp - x);
        self.dark = flush(self.dark + c.dark_c * (x - self.dark));
        x += c.dark * (self.dark - x);
        x += c.sat * (x.tanh() - x);
        // The hiss follows the loop's level, so silence in is silence out once the tail is gone.
        self.env = flush(x.abs().max(self.env * 0.999_79));
        x += noise.white() * HISS * c.hiss * (self.env * 30.0).min(1.0);
        soft_clip(x)
    }
}

/// Clean up to full scale, then bending smoothly towards 2: the loop can never run away.
#[inline]
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= 1.0 { x } else { (1.0 + (a - 1.0).tanh()).copysign(x) }
}

/// A stereo delay with up to 4 s of echoes. Its output is the echoes only.
pub struct Delay {
    lines: [Line; 2],
    loops: [LoopState; 2],
    heads: Heads,
    targets: [f32; READS],
    /// Which read feeds back in multi-tap mode (the longest tap).
    longest: usize,
    mode: DelayMode,
    want_mode: DelayMode,
    mode_gain: f32,
    character: Character,
    feedback: Smoother,
    width: Smoother,
    low_g: Smoother,
    high_g: Smoother,
    k: Smoother,
    low_on: Smoother,
    high_on: Smoother,
    dark: Smoother,
    dark_c: Smoother,
    sat: Smoother,
    wow: Smoother,
    hiss: Smoother,
    tap_gain: [Smoother; 4],
    tap_pan: [Smoother; 4],
    wow_phase: f32,
    flutter_phase: f32,
    noise: Noise,
    fresh: bool,
}

impl Default for Delay {
    fn default() -> Self {
        Self::new()
    }
}

impl Delay {
    const SEED: u64 = 0xDE1A;

    /// A delay with its lines allocated for the longest time.
    pub fn new() -> Self {
        let knob = || Smoother::new(Smoother::KNOB_MS);
        let mut d = Self {
            lines: [Line::new(MAX_READ as usize + 4), Line::new(MAX_READ as usize + 4)],
            loops: Default::default(),
            heads: Heads::new(),
            targets: [0.0; READS],
            longest: 4,
            mode: DelayMode::Standard,
            want_mode: DelayMode::Standard,
            mode_gain: 1.0,
            character: Character::Digital,
            feedback: knob(),
            width: knob(),
            low_g: knob(),
            high_g: knob(),
            k: knob(),
            low_on: knob(),
            high_on: knob(),
            dark: Smoother::new(30.0),
            dark_c: Smoother::new(30.0),
            sat: Smoother::new(30.0),
            wow: Smoother::new(200.0),
            hiss: Smoother::new(30.0),
            tap_gain: std::array::from_fn(|_| knob()),
            tap_pan: std::array::from_fn(|_| knob()),
            wow_phase: 0.0,
            flutter_phase: 0.0,
            noise: Noise::new(Self::SEED),
            fresh: true,
        };
        d.set(&DelayParams::default());
        d.fresh = true;
        d
    }

    /// New settings, given once a block. They glide; the first ones after `new` or `reset`
    /// land at once.
    pub fn set(&mut self, p: &DelayParams) {
        let time = p.time_samples.clamp(48.0, MAX_TIME_SAMPLES as f32);
        self.targets[0] = time;
        for (t, tap) in self.targets[1..].iter_mut().zip(&p.taps) {
            *t = time * tap.time.clamp(0.01, 1.0);
        }
        self.longest = 1 + (0..4).fold(0, |m, i| if p.taps[i].time > p.taps[m].time { i } else { m });
        self.want_mode = p.mode;
        self.character = p.character;
        let cap = if p.character == Character::Digital { 0.98 } else { 1.0 };
        self.feedback.set_target(p.feedback.clamp(0.0, cap));
        self.width.set_target(p.width.clamp(0.0, 1.0));
        let warp = |hz: f32| (std::f32::consts::PI * hz.clamp(20.0, 0.45 * SAMPLE_RATE) / SAMPLE_RATE).tan();
        // An off filter keeps its last cutoff and fades out, so switching it never clicks.
        let low_on = p.low_cut_hz > 20.0;
        self.low_on.set_target(if low_on { 1.0 } else { 0.0 });
        if low_on || self.fresh {
            self.low_g.set_target(warp(p.low_cut_hz));
        }
        let high_on = p.high_cut_hz < 20_000.0;
        self.high_on.set_target(if high_on { 1.0 } else { 0.0 });
        if high_on || self.fresh {
            self.high_g.set_target(warp(p.high_cut_hz));
        }
        let q = std::f32::consts::FRAC_1_SQRT_2 + p.reso.clamp(0.0, 1.0) * (4.0 - std::f32::consts::FRAC_1_SQRT_2);
        self.k.set_target(1.0 / q);
        let (dark, hz, sat, tape) = match p.character {
            Character::Digital => (0.0, 6_000.0, 0.0, 0.0),
            Character::Analog => (1.0, 3_200.0, 1.0, 0.0),
            Character::Tape => (1.0, 6_000.0, 1.0, 1.0),
        };
        self.dark.set_target(dark);
        self.dark_c.set_target(1.0 - (-std::f32::consts::TAU * hz / SAMPLE_RATE).exp());
        self.sat.set_target(sat);
        self.wow.set_target(tape);
        self.hiss.set_target(tape);
        for (i, tap) in p.taps.iter().enumerate() {
            self.tap_gain[i].set_target(tap.gain);
            self.tap_pan[i].set_target(tap.pan.clamp(-1.0, 1.0));
        }
        if self.fresh {
            self.snap();
            self.fresh = false;
        }
    }

    /// Lands every setting on its target, as if it had always been there.
    fn snap(&mut self) {
        for s in [
            &mut self.feedback,
            &mut self.width,
            &mut self.low_g,
            &mut self.high_g,
            &mut self.k,
            &mut self.low_on,
            &mut self.high_on,
            &mut self.dark,
            &mut self.dark_c,
            &mut self.sat,
            &mut self.wow,
            &mut self.hiss,
        ]
        .into_iter()
        .chain(self.tap_gain.iter_mut())
        .chain(self.tap_pan.iter_mut())
        {
            s.reset(s.target());
        }
        self.heads.snap(&self.targets);
        self.mode = self.want_mode;
        self.mode_gain = 1.0;
    }

    /// Back to silence, keeping the settings.
    pub fn reset(&mut self) {
        for l in &mut self.lines {
            l.clear();
        }
        self.loops = Default::default();
        self.wow_phase = 0.0;
        self.flutter_phase = 0.0;
        self.noise = Noise::new(Self::SEED);
        self.snap();
    }

    /// Replaces `l` and `r` with the echoes of what they held.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        use std::f32::consts::TAU;
        let digital = self.character == Character::Digital;
        let mode_step = 1.0 / (0.005 * SAMPLE_RATE);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            // A mode change fades the echoes out for 5 ms, switches, and fades them back in.
            if self.want_mode != self.mode {
                self.mode_gain -= mode_step;
                if self.mode_gain <= 0.0 {
                    self.mode_gain = 0.0;
                    self.mode = self.want_mode;
                }
            } else if self.mode_gain < 1.0 {
                self.mode_gain = (self.mode_gain + mode_step).min(1.0);
            }
            self.heads.step(&self.targets, digital);
            let k = self.k.next();
            let colour = Colour {
                low: SvfCoefs::from_g(self.low_g.next(), k),
                high: SvfCoefs::from_g(self.high_g.next(), k),
                low_on: self.low_on.next(),
                high_on: self.high_on.next(),
                dark: self.dark.next(),
                dark_c: self.dark_c.next(),
                sat: self.sat.next(),
                hiss: self.hiss.next(),
            };
            let wow = self.wow.next() * (WOW_DEPTH * self.wow_phase.sin() + FLUTTER_DEPTH * self.flutter_phase.sin());
            self.wow_phase = (self.wow_phase + TAU * 0.5 / SAMPLE_RATE) % TAU;
            self.flutter_phase = (self.flutter_phase + TAU * 6.0 / SAMPLE_RATE) % TAU;
            let fb = self.feedback.next();
            let mut gains = [[0.0; 2]; 4];
            for (g, (gain, pan)) in gains.iter_mut().zip(self.tap_gain.iter_mut().zip(self.tap_pan.iter_mut())) {
                let (gain, pan) = (gain.next(), pan.next());
                *g = [gain * (1.0 - pan).min(1.0), gain * (1.0 + pan).min(1.0)];
            }
            let (x, y) = (*a, *b);
            let [left, right] = &mut self.loops;
            let (out_l, out_r) = match self.mode {
                DelayMode::Standard => {
                    let (rl, rr) = (self.heads.read(&self.lines[0], 0, wow), self.heads.read(&self.lines[1], 0, wow));
                    let wl = left.process(x + fb * rl, &colour, &mut self.noise);
                    let wr = right.process(y + fb * rr, &colour, &mut self.noise);
                    self.lines[0].push(wl);
                    self.lines[1].push(wr);
                    (rl, rr)
                }
                DelayMode::PingPong => {
                    let (rl, rr) = (self.heads.read(&self.lines[0], 0, wow), self.heads.read(&self.lines[1], 0, wow));
                    let wl = left.process(0.5 * (x + y) + fb * rr, &colour, &mut self.noise);
                    let wr = right.process(fb * rl, &colour, &mut self.noise);
                    self.lines[0].push(wl);
                    self.lines[1].push(wr);
                    (rl, rr)
                }
                DelayMode::MultiTap => {
                    let (mut ol, mut or, mut back) = (0.0, 0.0, 0.0);
                    for (t, g) in gains.iter().enumerate() {
                        let v = self.heads.read(&self.lines[0], t + 1, wow);
                        ol += v * g[0];
                        or += v * g[1];
                        if t + 1 == self.longest {
                            back = v;
                        }
                    }
                    let w = left.process(0.5 * (x + y) + fb * back, &colour, &mut self.noise);
                    self.lines[0].push(w);
                    self.lines[1].push(0.0);
                    (ol, or)
                }
            };
            let width = self.width.next();
            let mid = 0.5 * (out_l + out_r);
            let side = 0.5 * (out_l - out_r) * width;
            *a = (mid + side) * self.mode_gain;
            *b = (mid - side) * self.mode_gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Noise, sine};

    const SR: usize = crate::SAMPLE_RATE as usize;

    fn params(mode: DelayMode, character: Character, time: f32, feedback: f32) -> DelayParams {
        DelayParams { mode, character, time_samples: time, feedback, ..DelayParams::default() }
    }

    fn run(d: &mut Delay, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut a, mut b) = (l.to_vec(), r.to_vec());
        for (ca, cb) in a.chunks_mut(crate::BLOCK).zip(b.chunks_mut(crate::BLOCK)) {
            d.process(ca, cb);
        }
        (a, b)
    }

    fn impulse(n: usize, amp: f32) -> Vec<f32> {
        let mut v = vec![0.0; n];
        v[0] = amp;
        v
    }

    fn max_step(x: &[f32]) -> f32 {
        x.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    /// The frequency in each window, from its upward zero crossings.
    fn freqs(x: &[f32], win: usize) -> Vec<f32> {
        x.chunks(win)
            .map(|w| {
                let ups: Vec<f32> = w
                    .windows(2)
                    .enumerate()
                    .filter(|(_, p)| p[0] < 0.0 && p[1] >= 0.0)
                    .map(|(i, p)| i as f32 + p[0] / (p[0] - p[1]))
                    .collect();
                if ups.len() < 2 {
                    0.0
                } else {
                    (ups.len() - 1) as f32 * crate::SAMPLE_RATE / (ups[ups.len() - 1] - ups[0])
                }
            })
            .collect()
    }

    #[test]
    fn delay_times_to_the_sample() {
        let mut d = Delay::new();
        d.set(&params(DelayMode::Standard, Character::Digital, 12_000.0, 0.5));
        let n = 40_000;
        let (a, b) = run(&mut d, &impulse(n, 1.0), &impulse(n, 1.0));
        for (ch, out) in [("left", &a), ("right", &b)] {
            for (i, v) in out.iter().enumerate() {
                let want = match i {
                    12_000 => 1.0,
                    24_000 => 0.5,
                    36_000 => 0.25,
                    _ => 0.0,
                };
                assert!((v - want).abs() < 1e-6, "{ch} sample {i}: {v}, want {want}");
            }
        }
    }

    #[test]
    fn ping_pong_alternates_sides() {
        let mut d = Delay::new();
        d.set(&params(DelayMode::PingPong, Character::Digital, 1_000.0, 0.5));
        let n = 4_500;
        let (a, b) = run(&mut d, &impulse(n, 1.0), &vec![0.0; n]);
        // The input is summed to mono (half of each side), so the first echo is 0.5.
        for i in 0..n {
            let (wl, wr) = match i {
                1_000 => (0.5, 0.0),
                2_000 => (0.0, 0.25),
                3_000 => (0.125, 0.0),
                4_000 => (0.0, 0.0625),
                _ => (0.0, 0.0),
            };
            assert!((a[i] - wl).abs() < 1e-6 && (b[i] - wr).abs() < 1e-6, "sample {i}: {} {}", a[i], b[i]);
        }
    }

    #[test]
    fn multi_tap_levels_and_pans() {
        let mut p = params(DelayMode::MultiTap, Character::Digital, 8_000.0, 0.5);
        p.taps = [
            Tap { time: 0.25, gain: 1.0, pan: -1.0 },
            Tap { time: 0.5, gain: 0.5, pan: 1.0 },
            Tap { time: 0.75, gain: 0.25, pan: 0.0 },
            Tap { time: 1.0, gain: 0.8, pan: 0.5 },
        ];
        let mut d = Delay::new();
        d.set(&p);
        let n = 17_000;
        let (a, b) = run(&mut d, &impulse(n, 1.0), &impulse(n, 1.0));
        for i in 0..n {
            let (wl, wr) = match i {
                2_000 => (1.0, 0.0),
                4_000 => (0.0, 0.5),
                6_000 => (0.25, 0.25),
                8_000 => (0.4, 0.8),
                // The longest tap feeds back: the taps again, at half level.
                10_000 => (0.5, 0.0),
                12_000 => (0.0, 0.25),
                14_000 => (0.125, 0.125),
                16_000 => (0.2, 0.4),
                _ => (0.0, 0.0),
            };
            assert!((a[i] - wl).abs() < 1e-6 && (b[i] - wr).abs() < 1e-6, "sample {i}: {} {}", a[i], b[i]);
        }
    }

    /// A 440 Hz sine through a delay whose time changes by 30 % at 1 s: (output, own step).
    fn time_change(character: Character) -> (Vec<f32>, f32) {
        let mut d = Delay::new();
        let p = params(DelayMode::Standard, character, 12_000.0, 0.0);
        d.set(&p);
        let x = sine(440.0, 0.5, 0.0, 3 * SR);
        let own = max_step(&x);
        let (mut a, mut b) = (x.clone(), x.clone());
        for (i, (ca, cb)) in a.chunks_mut(crate::BLOCK).zip(b.chunks_mut(crate::BLOCK)).enumerate() {
            if i * crate::BLOCK == SR {
                d.set(&DelayParams { time_samples: 15_600.0, ..p });
            }
            d.process(ca, cb);
        }
        (a, own)
    }

    #[test]
    fn tape_time_change_bends_without_clicks() {
        let (out, own) = time_change(Character::Tape);
        let after_first_echo = &out[12_100..];
        let step = max_step(after_first_echo);
        assert!(step < own * 1.15, "step {step}, the sine's own {own}");
        let f = freqs(&out[SR / 2..], SR / 10);
        println!("tape frequencies per 100 ms: {f:?}");
        assert!((f[0] - 440.0).abs() < 3.0, "steady before: {}", f[0]);
        assert!(f.iter().any(|&v| v < 0.95 * 440.0 && v > 0.0), "the pitch bends while the time glides");
        let last = f[f.len() - 2];
        assert!((last - 440.0).abs() < 3.0, "back to 440 Hz after the glide: {last}");
    }

    #[test]
    fn digital_time_change_crossfades() {
        let (out, own) = time_change(Character::Digital);
        let step = max_step(&out[12_100..]);
        assert!(step < own * 1.05, "step {step}, the sine's own {own}");
        let f = freqs(&out[SR / 2..], SR / 5);
        println!("digital frequencies per 200 ms: {f:?}");
        assert!(f.iter().all(|&v| (v - 440.0).abs() < 0.015 * 440.0), "no glide: {f:?}");
        // 50 ms after the change it reads exactly at the new time.
        let x = sine(440.0, 0.5, 0.0, 3 * SR);
        for i in SR + SR / 20 + 1..3 * SR {
            assert!((out[i] - x[i - 15_600]).abs() < 1e-4, "sample {i}: {} not {}", out[i], x[i - 15_600]);
        }
    }

    #[test]
    fn feedback_one_stays_bounded() {
        for character in [Character::Digital, Character::Analog, Character::Tape] {
            for (reso, low, high) in [(0.0, 20.0, 20_000.0), (1.0, 300.0, 3_000.0)] {
                let mut d = Delay::new();
                let p = DelayParams {
                    reso,
                    low_cut_hz: low,
                    high_cut_hz: high,
                    ..params(DelayMode::Standard, character, 4_800.0, 1.0)
                };
                d.set(&p);
                let mut n = Noise(5);
                let len = if cfg!(debug_assertions) { 5 * SR } else { 20 * SR };
                let l: Vec<f32> = (0..len).map(|_| n.next() * 0.5).collect();
                let (a, b) = run(&mut d, &l, &l);
                let peak =
                    a.iter().chain(&b).fold(0.0f32, |m, v| if v.is_finite() { m.max(v.abs()) } else { f32::INFINITY });
                assert!(peak < 4.0, "{character:?} reso {reso}: peak {peak}");
            }
        }
    }

    #[test]
    fn silence_after_the_tail() {
        for mode in [DelayMode::Standard, DelayMode::PingPong, DelayMode::MultiTap] {
            for character in [Character::Digital, Character::Analog, Character::Tape] {
                let mut d = Delay::new();
                d.set(&DelayParams {
                    low_cut_hz: 100.0,
                    high_cut_hz: 8_000.0,
                    ..params(mode, character, 6_000.0, 0.5)
                });
                let mut n = Noise(8);
                let mut l: Vec<f32> = (0..6 * SR).map(|_| n.next() * 0.5).collect();
                l[SR..].fill(0.0);
                let (a, b) = run(&mut d, &l, &l);
                let peak = a[5 * SR..].iter().chain(&b[5 * SR..]).fold(0.0f32, |m, v| m.max(v.abs()));
                let limit = if character == Character::Tape { -90.0 } else { -100.0 };
                assert!(crate::gain_to_db(peak) < limit, "{mode:?} {character:?}: {} dBFS", crate::gain_to_db(peak));
            }
        }
    }

    #[test]
    fn delay_is_deterministic() {
        let take = || {
            let mut d = Delay::new();
            d.set(&params(DelayMode::PingPong, Character::Tape, 7_000.0, 0.7));
            let mut n = Noise(1);
            let l: Vec<f32> = (0..SR).map(|_| n.next() * 0.5).collect();
            run(&mut d, &l, &l)
        };
        assert_eq!(take(), take());
    }
}
