//! The hall reverb: a feedback delay network of 8 or 16 lines mixed by a Hadamard matrix. Each
//! line's length is a different prime, its loss is set from the decay time, and a one-pole on
//! each line makes the highs die sooner (Jot's way). The lines wobble slowly for a lusher tail,
//! and a pattern of early reflections comes first.

pub use super::reverb_common::ReverbParams;
use super::reverb_common::{FreezeAmount, Glide, Line, PreDelay, Ramp, WetOut, flush, loop_gain};
use crate::{BLOCK, SAMPLE_RATE};

/// The most lines.
const MAX_LINES: usize = 16;
/// The shortest and longest line at a size scale of 1, in samples (25 and 100 ms).
const SHORTEST: f32 = 1_200.0;
const LONGEST: f32 = 4_800.0;
/// The size scale at a size of 0 and of 1.
const SIZE_MIN: f32 = 0.3;
const SIZE_MAX: f32 = 1.3;
/// The most a line's length wobbles, in samples, at full modulation.
const WOBBLE: f32 = 10.0;
/// The early reflections at a size scale of 1: (ms, gain), left then right.
const EARLY: [[(f32, f32); 8]; 2] = [
    [(7.1, 0.85), (11.3, -0.7), (17.9, 0.62), (23.3, 0.55), (31.7, -0.46), (41.1, 0.38), (53.9, 0.3), (67.3, -0.24)],
    [(8.9, 0.82), (13.7, 0.68), (19.1, -0.6), (27.1, 0.52), (35.3, 0.44), (45.7, -0.36), (59.1, 0.29), (73.3, 0.22)],
];
/// Which output each line adds to with which sign, left then right (picked so the two differ).
const OUT_SIGNS: [[f32; MAX_LINES]; 2] = [
    [1., -1., 1., 1., -1., 1., -1., -1., 1., 1., -1., 1., -1., -1., 1., -1.],
    [1., 1., -1., 1., 1., -1., -1., 1., -1., 1., 1., -1., 1., -1., -1., -1.],
];

fn size_scale(size: f32) -> f32 {
    SIZE_MIN + (SIZE_MAX - SIZE_MIN) * size.clamp(0.0, 1.0)
}

fn is_prime(n: usize) -> bool {
    n >= 2 && (2..).take_while(|d| d * d <= n).all(|d| !n.is_multiple_of(d))
}

/// The first prime at or after `n`.
fn prime_from(n: usize) -> usize {
    (n..).find(|&p| is_prime(p)).unwrap_or(n)
}

/// A stereo hall reverb. Its output is the reverb only.
pub struct Hall {
    n: usize,
    pre: PreDelay,
    wet: WetOut,
    lines: Vec<Line>,
    lens: [Glide; MAX_LINES],
    lows: [f32; MAX_LINES],
    /// Each line's wobble as a turning pair (cos, sin) and its turn per sample.
    lfo: [(f32, f32); MAX_LINES],
    turn: [(f32, f32); MAX_LINES],
    early_lines: [Line; 2],
    scale: Glide,
    decay: Glide,
    damping: Glide,
    modulation: Glide,
    early: Ramp,
    early_target: f32,
    freeze: FreezeAmount,
    input: Ramp,
    fresh: bool,
}

impl Hall {
    /// A hall of `lines` lines: 8, or 16 for a denser tail (anything above 8 gives 16).
    pub fn new(lines: usize) -> Self {
        let n = if lines > 8 { 16 } else { 8 };
        let longest = (LONGEST * SIZE_MAX) as usize * 11 / 10 + WOBBLE as usize + 4;
        let early_longest = (73.3 / 1_000.0 * SAMPLE_RATE * SIZE_MAX) as usize + 4;
        let mut h = Self {
            n,
            pre: PreDelay::new(),
            wet: WetOut::new(),
            lines: (0..n).map(|_| Line::new(longest)).collect(),
            lens: std::array::from_fn(|_| Glide::new(150.0)),
            lows: [0.0; MAX_LINES],
            lfo: std::array::from_fn(|i| ((i as f32 * 2.4).cos(), (i as f32 * 2.4).sin())),
            turn: std::array::from_fn(|i| {
                let w = std::f32::consts::TAU * (0.13 + 0.071 * i as f32) / SAMPLE_RATE;
                (w.cos(), w.sin())
            }),
            early_lines: [Line::new(early_longest), Line::new(early_longest)],
            scale: Glide::new(150.0),
            decay: Glide::new(50.0),
            damping: Glide::new(30.0),
            modulation: Glide::new(30.0),
            early: Ramp::default(),
            early_target: 0.0,
            freeze: FreezeAmount::default(),
            input: Ramp::default(),
            fresh: true,
        };
        h.set(&ReverbParams::default());
        h.fresh = true;
        h
    }

    /// New settings, given once a block. They glide; the first ones after `new` or `reset`
    /// land at once.
    pub fn set(&mut self, p: &ReverbParams) {
        self.pre.set(p.pre_delay_samples);
        self.wet.set(p.low_cut_hz, p.high_cut_hz, p.width);
        let scale = size_scale(p.size);
        self.scale.set(scale);
        // Lengths spread evenly on a log scale, each moved up to a prime above the last, so no
        // two lines ever share a factor and their echoes never line up.
        let mut last = 0;
        for (i, len) in self.lens[..self.n].iter_mut().enumerate() {
            let at = SHORTEST * (LONGEST / SHORTEST).powf(i as f32 / (self.n - 1) as f32) * scale;
            last = prime_from((at.round() as usize).max(last + 1));
            len.set(last as f32);
        }
        self.decay.set(p.decay_s.clamp(0.1, 20.0));
        self.damping.set(p.damping.clamp(0.0, 1.0));
        self.modulation.set(p.modulation.clamp(0.0, 1.0));
        self.early_target = p.early.clamp(0.0, 1.0);
        self.freeze.target = if p.freeze { 1.0 } else { 0.0 };
        if self.fresh {
            self.snap();
            self.fresh = false;
        }
    }

    fn snap(&mut self) {
        self.pre.snap();
        self.wet.snap();
        for g in [&mut self.scale, &mut self.decay, &mut self.damping, &mut self.modulation]
            .into_iter()
            .chain(self.lens.iter_mut())
        {
            g.snap();
        }
        self.early.snap(self.early_target);
        self.freeze.value = self.freeze.target;
        self.input.snap(1.0 - self.freeze.value);
    }

    /// Back to silence, keeping the settings.
    pub fn reset(&mut self) {
        self.pre.clear();
        self.wet.clear();
        for l in self.lines.iter_mut().chain(&mut self.early_lines) {
            l.clear();
        }
        self.lows = [0.0; MAX_LINES];
        self.lfo = std::array::from_fn(|i| ((i as f32 * 2.4).cos(), (i as f32 * 2.4).sin()));
        self.snap();
    }

    /// Replaces `l` and `r` with their reverb.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        for start in (0..n).step_by(BLOCK) {
            let end = (start + BLOCK).min(n);
            self.block(&mut l[start..end], &mut r[start..end]);
        }
    }

    fn block(&mut self, l: &mut [f32], r: &mut [f32]) {
        let lines = self.n;
        let frozen = self.freeze.tick();
        let thaw = 1.0 - frozen;
        let scale = self.scale.tick();
        let decay = self.decay.tick();
        // Highs fall this much sooner than lows at full damping.
        let hf_ratio = 1.0 - 0.85 * self.damping.tick();
        let wobble = WOBBLE * self.modulation.tick() * thaw;
        let mut lens = [0.0f32; MAX_LINES];
        let mut gain = [0.0f32; MAX_LINES];
        let mut pole = [0.0f32; MAX_LINES];
        for i in 0..lines {
            lens[i] = self.lens[i].tick();
            let g = loop_gain(lens[i], decay);
            let b = if hf_ratio < 1.0 {
                let ratio = loop_gain(lens[i], decay * hf_ratio) / g;
                (1.0 - ratio) / (1.0 + ratio)
            } else {
                0.0
            };
            // Frozen, the loop is lossless: unity gain, no damping, no wobble.
            let (g, b) = (g + (1.0 - g) * frozen, b * thaw);
            gain[i] = g * (1.0 - b);
            pole[i] = b;
            // Keep each wobble on its circle.
            let (c, s) = self.lfo[i];
            let k = 1.5 - 0.5 * (c * c + s * s);
            self.lfo[i] = (c * k, s * k);
        }
        let early_at = EARLY.map(|side| side.map(|(ms, g)| (ms / 1_000.0 * SAMPLE_RATE * scale, g)));
        let norm = 1.0 / (lines as f32).sqrt();
        let n = l.len();
        self.input.to(thaw, n);
        self.early.to(self.early_target, n);
        self.wet.tick(n);

        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let (pl, pr) = self.pre.process(*a, *b);
            let feed = self.input.next();
            let ins = [pl * feed, pr * feed];
            let mut er = [0.0f32; 2];
            for ((e, line), taps) in er.iter_mut().zip(&self.early_lines).zip(&early_at) {
                for &(at, g) in taps {
                    *e += g * line.linear(at);
                }
            }
            for (line, x) in self.early_lines.iter_mut().zip(ins) {
                line.push(x);
            }

            let mut y = [0.0f32; MAX_LINES];
            let mut out = [0.0f32; 2];
            for i in 0..lines {
                let (c, s) = self.lfo[i];
                let (tc, ts) = self.turn[i];
                self.lfo[i] = (c * tc - s * ts, c * ts + s * tc);
                let x = self.lines[i].linear(lens[i] + wobble * s);
                self.lows[i] = flush(gain[i] * x + pole[i] * self.lows[i]);
                y[i] = self.lows[i];
                out[0] += OUT_SIGNS[0][i] * y[i];
                out[1] += OUT_SIGNS[1][i] * y[i];
            }
            hadamard(&mut y[..lines]);
            for i in 0..lines {
                let into = ins[i % 2] * if i % 4 < 2 { 1.0 } else { -1.0 };
                self.lines[i].push(y[i] * norm + into);
            }
            let early = self.early.next();
            let late = 0.5 * norm;
            (*a, *b) = self.wet.process(late * out[0] + early * er[0], late * out[1] + early * er[1]);
        }
        self.input.finish();
        self.early.finish();
        self.wet.finish();
    }
}

/// The fast Walsh-Hadamard transform in place (unscaled), for a power-of-two length.
#[inline]
fn hadamard(y: &mut [f32]) {
    let mut h = 1;
    while h < y.len() {
        for i in (0..y.len()).step_by(2 * h) {
            for j in i..i + h {
                let (a, b) = (y[j], y[j + h]);
                y[j] = a + b;
                y[j + h] = a - b;
            }
        }
        h *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::reverb_common::checks::{self, Verb};

    impl Verb for Hall {
        fn set(&mut self, p: &ReverbParams) {
            Hall::set(self, p)
        }
        fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
            Hall::process(self, l, r)
        }
    }

    fn hall_8() -> Box<dyn Verb> {
        Box::new(Hall::new(8))
    }

    fn hall_16() -> Box<dyn Verb> {
        Box::new(Hall::new(16))
    }

    #[test]
    fn line_lengths_are_distinct_primes() {
        for n in [8, 16] {
            for size in [0.0, 0.5, 1.0] {
                let mut h = Hall::new(n);
                h.set(&ReverbParams { size, ..ReverbParams::default() });
                let lens: Vec<usize> = h.lens[..n].iter().map(|g| g.value() as usize).collect();
                assert!(lens.iter().all(|&l| is_prime(l)), "{lens:?}");
                assert!(lens.windows(2).all(|w| w[0] < w[1]), "{lens:?}");
            }
        }
    }

    #[test]
    fn reverb_decays_at_its_set_time() {
        checks::decays_at_set_time(&hall_8, "hall 8", checks::plain(1.0));
        checks::decays_at_set_time(&hall_16, "hall 16", checks::plain(1.0));
    }

    #[test]
    fn longest_decay_stays_bounded() {
        let p = ReverbParams { modulation: 1.0, ..ReverbParams::default() };
        checks::longest_decay_stays_bounded(&hall_8, "hall 8", p);
        checks::longest_decay_stays_bounded(&hall_16, "hall 16", p);
    }

    #[test]
    fn freeze_off_resumes_decay() {
        checks::freeze_off_resumes_decay(&hall_8, "hall 8", ReverbParams::default());
        checks::freeze_off_resumes_decay(&hall_16, "hall 16", ReverbParams::default());
    }

    #[test]
    fn reverbs_are_deterministic() {
        let p = ReverbParams { modulation: 1.0, ..ReverbParams::default() };
        checks::deterministic(&hall_8, p);
        checks::deterministic(&hall_16, p);
    }
}
