//! The plate reverb: Jon Dattorro's 1997 plate. Four allpasses smear the input, then it circles a
//! figure-eight tank of two halves (a wobbling allpass, a delay, damping, an allpass and another
//! delay each), and fourteen taps on the tank make the left and right sound.

pub use super::reverb_common::ReverbParams;
use super::reverb_common::{Allpass, FreezeAmount, Glide, Line, PreDelay, Ramp, WetOut, flush, loop_gain};
use crate::{BLOCK, SAMPLE_RATE};

/// The rate Dattorro's lengths are given at.
const PLATE_RATE: f32 = 29_761.0;
/// The input diffusers' lengths.
const DIFFUSERS: [f32; 4] = [142.0, 107.0, 379.0, 277.0];
/// The tank's lengths, left half then right: the wobbling allpass, delay, allpass, delay.
const AP1: [f32; 2] = [672.0, 908.0];
const D1: [f32; 2] = [4453.0, 4217.0];
const AP2: [f32; 2] = [1800.0, 2656.0];
const D2: [f32; 2] = [3720.0, 3163.0];
/// The wobble's depth at full modulation, and its rates for the two halves (Hz).
const EXCURSION: f32 = 16.0;
const LFO_HZ: [f32; 2] = [1.0, 0.77];
/// The tank allpasses hold sound longer than their length (some notes stay in them several
/// times over), so the decay counts them this many times over, measured to land the RT60.
const AP_WEIGHT: f32 = 1.4;

/// Where an output tap reads: the first delay, the second allpass or the second delay of a half.
#[derive(Clone, Copy)]
enum Node {
    D1,
    Ap2,
    D2,
}

/// The output taps: (half, node, position, sign), left output then right.
const TAPS: [[(usize, Node, f32, f32); 7]; 2] = [
    [
        (1, Node::D1, 266.0, 1.0),
        (1, Node::D1, 2974.0, 1.0),
        (1, Node::Ap2, 1913.0, -1.0),
        (1, Node::D2, 1996.0, 1.0),
        (0, Node::D1, 1990.0, -1.0),
        (0, Node::Ap2, 187.0, -1.0),
        (0, Node::D2, 1066.0, -1.0),
    ],
    [
        (0, Node::D1, 353.0, 1.0),
        (0, Node::D1, 3627.0, 1.0),
        (0, Node::Ap2, 1228.0, -1.0),
        (0, Node::D2, 2673.0, 1.0),
        (1, Node::D1, 2111.0, -1.0),
        (1, Node::Ap2, 335.0, -1.0),
        (1, Node::D2, 121.0, -1.0),
    ],
];

/// How much longer than Dattorro's lengths the delays get at a size of 0 and of 1.
const SIZE_MIN: f32 = 0.4;
const SIZE_MAX: f32 = 1.1;

fn size_scale(size: f32) -> f32 {
    (SIZE_MIN + (SIZE_MAX - SIZE_MIN) * size.clamp(0.0, 1.0)) * SAMPLE_RATE / PLATE_RATE
}

fn max_len(base: f32) -> usize {
    (base * SIZE_MAX * SAMPLE_RATE / PLATE_RATE) as usize + EXCURSION as usize * 2 + 4
}

/// A stereo plate reverb. Its output is the reverb only.
pub struct Plate {
    pre: PreDelay,
    wet: WetOut,
    diffusers: [Allpass; 4],
    ap1: [Allpass; 2],
    d1: [Line; 2],
    ap2: [Allpass; 2],
    d2: [Line; 2],
    damp: [f32; 2],
    /// The size scale (for the diffusers and taps) and the tank's lengths, landing on whole samples.
    scale: Glide,
    lens: [[Glide; 4]; 2],
    decay: Glide,
    damping: Glide,
    modulation: Glide,
    diffusion: Glide,
    freeze: FreezeAmount,
    input: Ramp,
    phase: [f32; 2],
    fresh: bool,
}

impl Default for Plate {
    fn default() -> Self {
        Self::new()
    }
}

impl Plate {
    /// A plate with its delays allocated for the largest size.
    pub fn new() -> Self {
        let size = || Glide::new(150.0);
        let mut p = Self {
            pre: PreDelay::new(),
            wet: WetOut::new(),
            diffusers: DIFFUSERS.map(|d| Allpass::new(max_len(d))),
            ap1: AP1.map(|d| Allpass::new(max_len(d))),
            d1: D1.map(|d| Line::new(max_len(d))),
            ap2: AP2.map(|d| Allpass::new(max_len(d))),
            d2: D2.map(|d| Line::new(max_len(d))),
            damp: [0.0; 2],
            scale: size(),
            lens: std::array::from_fn(|_| std::array::from_fn(|_| size())),
            decay: Glide::new(50.0),
            damping: Glide::new(30.0),
            modulation: Glide::new(30.0),
            diffusion: Glide::new(30.0),
            freeze: FreezeAmount::default(),
            input: Ramp::default(),
            phase: [0.0, 1.3],
            fresh: true,
        };
        p.set(&ReverbParams::default());
        p.fresh = true;
        p
    }

    /// New settings, given once a block. They glide; the first ones after `new` or `reset`
    /// land at once.
    pub fn set(&mut self, p: &ReverbParams) {
        self.pre.set(p.pre_delay_samples);
        self.wet.set(p.low_cut_hz, p.high_cut_hz, p.width);
        let scale = size_scale(p.size);
        self.scale.set(scale);
        for (side, lens) in self.lens.iter_mut().enumerate() {
            for (len, base) in lens.iter_mut().zip([AP1[side], D1[side], AP2[side], D2[side]]) {
                // The loop's lengths rest on whole samples, so a frozen tank reads them exactly.
                len.set((base * scale).round());
            }
        }
        self.decay.set(p.decay_s.clamp(0.1, 20.0));
        self.damping.set(p.damping.clamp(0.0, 1.0));
        self.modulation.set(p.modulation.clamp(0.0, 1.0));
        self.diffusion.set(p.diffusion.clamp(0.0, 1.0));
        self.freeze.target = if p.freeze { 1.0 } else { 0.0 };
        if self.fresh {
            self.snap();
            self.fresh = false;
        }
    }

    fn snap(&mut self) {
        self.pre.snap();
        self.wet.snap();
        for g in [&mut self.scale, &mut self.decay, &mut self.damping, &mut self.modulation, &mut self.diffusion]
            .into_iter()
            .chain(self.lens.iter_mut().flatten())
        {
            g.snap();
        }
        self.freeze.value = self.freeze.target;
        self.input.snap(1.0 - self.freeze.value);
    }

    /// Back to silence, keeping the settings.
    pub fn reset(&mut self) {
        self.pre.clear();
        self.wet.clear();
        for ap in self.diffusers.iter_mut().chain(&mut self.ap1).chain(&mut self.ap2) {
            ap.line.clear();
        }
        for l in self.d1.iter_mut().chain(&mut self.d2) {
            l.clear();
        }
        self.damp = [0.0; 2];
        self.phase = [0.0, 1.3];
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
        // The settings move once a block.
        let frozen = self.freeze.tick();
        let thaw = 1.0 - frozen;
        let scale = self.scale.tick();
        let lens = self.lens.each_mut().map(|half| half.each_mut().map(|g| g.tick()));
        let decay = self.decay.tick();
        let hold = |g: f32| g + (1.0 - g) * frozen;
        let g1 = [0, 1].map(|s| hold(loop_gain(AP_WEIGHT * lens[s][0] + lens[s][1], decay)));
        let g2 = [0, 1].map(|s| hold(loop_gain(AP_WEIGHT * lens[s][2] + lens[s][3], decay)));
        let damp = 0.75 * self.damping.tick() * thaw;
        let excursion = EXCURSION * SAMPLE_RATE / PLATE_RATE * self.modulation.tick() * thaw;
        let diffusion = self.diffusion.tick();
        let diff_c = [0.75 * diffusion, 0.75 * diffusion, 0.625 * diffusion, 0.625 * diffusion];
        let diff_len = DIFFUSERS.map(|d| d * scale);
        let lfo_step = LFO_HZ.map(|hz| std::f32::consts::TAU * hz / SAMPLE_RATE);
        self.input.to(thaw, n);
        self.wet.tick(n);

        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let (pl, pr) = self.pre.process(*a, *b);
            let mut x = 0.5 * (pl + pr) * self.input.next();
            for ((d, len), c) in self.diffusers.iter_mut().zip(diff_len).zip(diff_c) {
                x = d.process(x, len, c);
            }
            let ends = [self.d2[0].linear(lens[0][3]), self.d2[1].linear(lens[1][3])];
            for s in 0..2 {
                let into = x + g2[1 - s] * ends[1 - s];
                let wobble = excursion * self.phase[s].sin();
                self.phase[s] = (self.phase[s] + lfo_step[s]) % std::f32::consts::TAU;
                let smeared = self.ap1[s].process(into, lens[s][0] + wobble, -0.7);
                let late = self.d1[s].linear(lens[s][1]);
                self.d1[s].push(smeared);
                self.damp[s] = flush((1.0 - damp) * late + damp * self.damp[s]);
                let again = self.ap2[s].process(self.damp[s] * g1[s], lens[s][2], 0.5);
                self.d2[s].push(again);
            }
            let mut out = [0.0f32; 2];
            for (o, taps) in out.iter_mut().zip(&TAPS) {
                for &(half, node, at, sign) in taps {
                    let line = match node {
                        Node::D1 => &self.d1[half],
                        Node::Ap2 => &self.ap2[half].line,
                        Node::D2 => &self.d2[half],
                    };
                    *o += sign * line.linear(at * scale);
                }
            }
            (*a, *b) = self.wet.process(0.6 * out[0], 0.6 * out[1]);
        }
        self.input.finish();
        self.wet.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::reverb_common::checks::{self, Verb};

    impl Verb for Plate {
        fn set(&mut self, p: &ReverbParams) {
            Plate::set(self, p)
        }
        fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
            Plate::process(self, l, r)
        }
    }

    fn plate() -> Box<dyn Verb> {
        Box::new(Plate::new())
    }

    #[test]
    fn reverb_decays_at_its_set_time() {
        for size in [0.0, 0.5, 1.0] {
            checks::decays_at_set_time(
                &plate,
                &format!("plate size {size}"),
                ReverbParams { size, ..checks::plain(1.0) },
            );
        }
    }

    #[test]
    fn longest_decay_stays_bounded() {
        checks::longest_decay_stays_bounded(
            &plate,
            "plate",
            ReverbParams { modulation: 1.0, ..ReverbParams::default() },
        );
    }

    #[test]
    fn freeze_off_resumes_decay() {
        checks::freeze_off_resumes_decay(&plate, "plate", ReverbParams::default());
    }

    #[test]
    fn reverbs_are_deterministic() {
        checks::deterministic(&plate, ReverbParams { modulation: 1.0, ..ReverbParams::default() });
    }
}
