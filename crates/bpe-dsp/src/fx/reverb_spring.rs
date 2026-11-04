//! The spring reverb: up to three springs, each a loop of 64 first-order allpasses (which make
//! the lows arrive later than the highs, the spring's falling chirp) and a delay set by the
//! tension. The springs are tuned a little apart and spread across the stereo field; Drip brings
//! up the high part of each spring's sound, the splashy "drip".

pub use super::reverb_common::ReverbParams;
use super::reverb_common::{FreezeAmount, Glide, Line, PreDelay, Ramp, Svf, SvfCoefs, WetOut, flush, loop_gain};
use crate::{BLOCK, SAMPLE_RATE};

/// The allpasses in each spring.
const STAGES: usize = 64;
/// How long a spring's delay is, in ms, at no tension and at full tension.
const SLACK_MS: f32 = 55.0;
const TIGHT_MS: f32 = 22.0;
/// How each spring differs from the first: its length and its allpass coefficient.
const DETUNE: [f32; 3] = [1.0, 1.071, 0.937];
const COEF_DETUNE: [f32; 3] = [1.0, 0.96, 1.04];
/// Each spring's (left, right) level, for one, two and three springs.
const SPREAD: [[[f32; 2]; 3]; 3] = [
    [[1.0, 1.0], [0.0, 0.0], [0.0, 0.0]],
    [[1.0, 0.25], [0.25, 1.0], [0.0, 0.0]],
    [[0.85, 0.2], [0.2, 0.85], [0.5, 0.5]],
];

fn spring_len(tension: f32, s: usize) -> f32 {
    ((SLACK_MS + (TIGHT_MS - SLACK_MS) * tension.clamp(0.0, 1.0)) / 1_000.0 * SAMPLE_RATE * DETUNE[s]).round()
}

/// One spring's loop.
#[derive(Clone, Debug)]
struct OneSpring {
    xs: [f32; STAGES],
    ys: [f32; STAGES],
    line: Line,
    len: Glide,
    low: f32,
    drip: Svf,
}

impl OneSpring {
    fn new(longest: usize) -> Self {
        Self {
            xs: [0.0; STAGES],
            ys: [0.0; STAGES],
            line: Line::new(longest),
            len: Glide::new(150.0),
            low: 0.0,
            drip: Svf::default(),
        }
    }

    fn clear(&mut self) {
        self.xs = [0.0; STAGES];
        self.ys = [0.0; STAGES];
        self.line.clear();
        self.low = 0.0;
        self.drip = Svf::default();
    }

    /// One sample: `x` goes in, the spring's sound and its high part come out.
    #[inline]
    fn process(&mut self, x: f32, len: f32, gain: f32, coef: f32, damp: f32, drip: &SvfCoefs) -> (f32, f32) {
        let back = self.line.linear(len);
        self.low = flush((1.0 - damp) * back + damp * self.low);
        let mut u = x + gain * self.low;
        for (xs, ys) in self.xs.iter_mut().zip(self.ys.iter_mut()) {
            let y = coef * (u - *ys) + *xs;
            *xs = u;
            *ys = y;
            u = y;
        }
        self.line.push(u);
        let (_, high) = self.drip.run(back, drip);
        (back, high)
    }
}

/// A stereo spring reverb. Its output is the reverb only.
pub struct Spring {
    springs: [OneSpring; 3],
    pre: PreDelay,
    wet: WetOut,
    decay: Glide,
    damping: Glide,
    tension: Glide,
    freeze: FreezeAmount,
    input: Ramp,
    drip: Ramp,
    drip_target: f32,
    levels: [[Ramp; 2]; 3],
    count: usize,
    drip_coefs: SvfCoefs,
    fresh: bool,
}

impl Default for Spring {
    fn default() -> Self {
        Self::new()
    }
}

impl Spring {
    /// Three springs (however many are heard), allocated for the slackest tension.
    pub fn new() -> Self {
        let longest = spring_len(0.0, 1) as usize + 4;
        let mut s = Self {
            springs: std::array::from_fn(|_| OneSpring::new(longest)),
            pre: PreDelay::new(),
            wet: WetOut::new(),
            decay: Glide::new(50.0),
            damping: Glide::new(30.0),
            tension: Glide::new(150.0),
            freeze: FreezeAmount::default(),
            input: Ramp::default(),
            drip: Ramp::default(),
            drip_target: 0.0,
            levels: Default::default(),
            count: 2,
            drip_coefs: SvfCoefs::new(1_800.0, 2.0),
            fresh: true,
        };
        s.set(&ReverbParams::default());
        s.fresh = true;
        s
    }

    /// New settings, given once a block. They glide; the first ones after `new` or `reset`
    /// land at once.
    pub fn set(&mut self, p: &ReverbParams) {
        self.pre.set(p.pre_delay_samples);
        self.wet.set(p.low_cut_hz, p.high_cut_hz, p.width);
        let tension = p.tension.clamp(0.0, 1.0);
        self.tension.set(tension);
        for (i, s) in self.springs.iter_mut().enumerate() {
            // Whole-sample lengths at rest, so a frozen spring reads them exactly.
            s.len.set(spring_len(tension, i));
        }
        self.decay.set(p.decay_s.clamp(0.1, 20.0));
        self.damping.set(p.damping.clamp(0.0, 1.0));
        self.drip_target = p.drip.clamp(0.0, 1.0);
        self.count = p.springs.clamp(1, 3) as usize;
        self.freeze.target = if p.freeze { 1.0 } else { 0.0 };
        if self.fresh {
            self.snap();
            self.fresh = false;
        }
    }

    fn snap(&mut self) {
        self.pre.snap();
        self.wet.snap();
        for g in [&mut self.decay, &mut self.damping, &mut self.tension]
            .into_iter()
            .chain(self.springs.iter_mut().map(|s| &mut s.len))
        {
            g.snap();
        }
        self.freeze.value = self.freeze.target;
        self.input.snap(1.0 - self.freeze.value);
        self.drip.snap(self.drip_target);
        for (levels, spread) in self.levels.iter_mut().zip(&SPREAD[self.count - 1]) {
            for (level, v) in levels.iter_mut().zip(spread) {
                level.snap(*v);
            }
        }
    }

    /// Back to silence, keeping the settings.
    pub fn reset(&mut self) {
        self.pre.clear();
        self.wet.clear();
        for s in &mut self.springs {
            s.clear();
        }
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
        let n = l.len();
        let frozen = self.freeze.tick();
        let thaw = 1.0 - frozen;
        let decay = self.decay.tick();
        let damp = 0.6 * self.damping.tick() * thaw;
        let tension = self.tension.tick();
        let mut lens = [0.0f32; 3];
        let mut gains = [0.0f32; 3];
        let mut coefs = [0.0f32; 3];
        for (i, s) in self.springs.iter_mut().enumerate() {
            lens[i] = s.len.tick();
            // The allpasses hold the sound one sample each on average, so they count in the loop.
            let g = loop_gain(lens[i] + STAGES as f32, decay);
            gains[i] = g + (1.0 - g) * frozen;
            // Negative, so the lows come later: the chirp falls.
            coefs[i] = -(0.5 + 0.2 * tension) * COEF_DETUNE[i];
        }
        self.input.to(thaw, n);
        self.drip.to(self.drip_target, n);
        for (levels, spread) in self.levels.iter_mut().zip(&SPREAD[self.count - 1]) {
            for (level, v) in levels.iter_mut().zip(spread) {
                level.to(*v, n);
            }
        }
        self.wet.tick(n);

        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let (pl, pr) = self.pre.process(*a, *b);
            let x = 0.5 * (pl + pr) * self.input.next();
            let drip = 2.0 * self.drip.next();
            let mut out = [0.0f32; 2];
            for (i, s) in self.springs.iter_mut().enumerate() {
                let (sound, high) = s.process(x, lens[i], gains[i], coefs[i], damp, &self.drip_coefs);
                let v = sound + drip * high;
                out[0] += self.levels[i][0].next() * v;
                out[1] += self.levels[i][1].next() * v;
            }
            (*a, *b) = self.wet.process(0.5 * out[0], 0.5 * out[1]);
        }
        self.input.finish();
        self.drip.finish();
        for level in self.levels.iter_mut().flatten() {
            level.finish();
        }
        self.wet.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::reverb_common::checks::{self, Verb};

    impl Verb for Spring {
        fn set(&mut self, p: &ReverbParams) {
            Spring::set(self, p)
        }
        fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
            Spring::process(self, l, r)
        }
    }

    fn spring() -> Box<dyn Verb> {
        Box::new(Spring::new())
    }

    #[test]
    fn reverb_decays_at_its_set_time() {
        for springs in [1, 3] {
            for tension in [0.0, 1.0] {
                let p = ReverbParams { springs, tension, ..checks::plain(1.0) };
                checks::decays_at_set_time(&spring, &format!("springs {springs} tension {tension}"), p);
            }
        }
    }

    #[test]
    fn longest_decay_stays_bounded() {
        for springs in [1, 3] {
            let p = ReverbParams { springs, drip: 1.0, ..ReverbParams::default() };
            checks::longest_decay_stays_bounded(&spring, &format!("springs {springs}"), p);
        }
    }

    #[test]
    fn freeze_off_resumes_decay() {
        for springs in [1, 3] {
            let p = ReverbParams { springs, ..ReverbParams::default() };
            checks::freeze_off_resumes_decay(&spring, &format!("springs {springs}"), p);
        }
    }

    #[test]
    fn reverbs_are_deterministic() {
        checks::deterministic(&spring, ReverbParams { springs: 3, drip: 1.0, ..ReverbParams::default() });
    }

    #[test]
    fn the_chirp_falls() {
        // A click through one spring: in its first echo the highs arrive before the lows.
        let mut s = Spring::new();
        s.set(&ReverbParams { springs: 1, drip: 0.0, ..checks::plain(2.0) });
        let mut l = vec![0.0; 6_000];
        l[0] = 1.0;
        let mut r = l.clone();
        s.process(&mut l, &mut r);
        // The first echo comes after the delay plus 16 (highs) to 256 (lows) samples of allpasses.
        let at = spring_len(0.5, 0) as usize;
        let first = &l[at + 8..at + 264];
        let crossings = |w: &[f32]| w.windows(2).filter(|p| (p[0] < 0.0) != (p[1] < 0.0)).count();
        let (early, late) = first.split_at(first.len() / 2);
        assert!(crossings(early) > crossings(late), "{} then {}", crossings(early), crossings(late));
    }
}
