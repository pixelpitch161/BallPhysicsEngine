//! What the three reverbs share: their settings, and the small pieces they (and the delay) are
//! built from: delay lines with fractional reads, allpasses, a state-variable filter, glides for
//! settings, the pre-delay and the wet output's cuts and width.

use crate::{BLOCK, SAMPLE_RATE};

/// The settings of a reverb. Each kind reads the ones it has and ignores the rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbParams {
    /// How late the reverb starts, in samples (0..24000, half a second).
    pub pre_delay_samples: f32,
    /// How long the tail takes to fall by 60 dB, in seconds (0.1..20).
    pub decay_s: f32,
    /// How big the space is (0..1): longer delays inside.
    pub size: f32,
    /// How much darker the tail gets as it falls (0..1).
    pub damping: f32,
    /// The wet sound's low cut in Hz; 20 or less is off.
    pub low_cut_hz: f32,
    /// The wet sound's high cut in Hz; 20000 or more is off.
    pub high_cut_hz: f32,
    /// How wide the wet sound is (0 mono, 1 full).
    pub width: f32,
    /// Plate: how much the input is smeared before the tank (0..1).
    pub diffusion: f32,
    /// Plate and hall: how much the delays inside wobble (0..1), for a lusher tail.
    pub modulation: f32,
    /// Hall: the level of the early reflections (0..1).
    pub early: f32,
    /// Spring: how many springs (1..3).
    pub springs: u8,
    /// Spring: how tight the springs are (0..1): tighter is shorter and brighter.
    pub tension: f32,
    /// Spring: how much of the springs' "drip" chirp comes through (0..1).
    pub drip: f32,
    /// Holds the tail as it is, forever: no new sound goes in and nothing fades.
    pub freeze: bool,
}

impl Default for ReverbParams {
    fn default() -> Self {
        Self {
            pre_delay_samples: 0.0,
            decay_s: 2.0,
            size: 0.5,
            damping: 0.3,
            low_cut_hz: 20.0,
            high_cut_hz: 20_000.0,
            width: 1.0,
            diffusion: 0.7,
            modulation: 0.3,
            early: 0.5,
            springs: 2,
            tension: 0.5,
            drip: 0.3,
            freeze: false,
        }
    }
}

/// The longest pre-delay, in samples (500 ms).
pub const MAX_PRE_DELAY: usize = 24_000;

/// Rounds numbers far too small to hear to zero, so a dying tail never turns into slow
/// denormal numbers. Anything above about 1e-25 passes unchanged.
#[inline]
pub(crate) fn flush(x: f32) -> f32 {
    (x + 1e-18) - 1e-18
}

/// The per-pass gain that makes a loop `len` samples long fall 60 dB in `decay_s` seconds.
#[inline]
pub(crate) fn loop_gain(len: f32, decay_s: f32) -> f32 {
    10f32.powf(-3.0 * len / (SAMPLE_RATE * decay_s.max(0.01)))
}

/// A delay line in a ring of a power-of-two size, read before it is written: a delay of `d`
/// gives the sample pushed `d` pushes ago.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    buf: Vec<f32>,
    mask: usize,
    w: usize,
}

impl Line {
    /// A line that can delay by up to `longest` samples (with room for the reads around it).
    pub fn new(longest: usize) -> Self {
        let n = (longest + 4).next_power_of_two();
        Self { buf: vec![0.0; n], mask: n - 1, w: 0 }
    }

    pub fn clear(&mut self) {
        self.buf.fill(0.0);
        self.w = 0;
    }

    #[inline]
    pub fn push(&mut self, x: f32) {
        self.buf[self.w] = flush(x);
        self.w = (self.w + 1) & self.mask;
    }

    /// The sample pushed `d` pushes ago (1 is the last one).
    #[inline]
    pub fn get(&self, d: usize) -> f32 {
        self.buf[self.w.wrapping_sub(d) & self.mask]
    }

    /// A fractional delay (at least 1), read with a straight line between samples.
    #[inline]
    pub fn linear(&self, d: f32) -> f32 {
        let i = d as usize;
        let f = d - i as f32;
        let a = self.get(i);
        a + f * (self.get(i + 1) - a)
    }

    /// A fractional delay (at least 2), read with a cubic Hermite curve through four samples.
    /// A whole-number delay gives the stored sample exactly.
    #[inline]
    pub fn hermite(&self, d: f32) -> f32 {
        let i = d as usize;
        let f = d - i as f32;
        let (xm1, x0, x1, x2) = (self.get(i - 1), self.get(i), self.get(i + 1), self.get(i + 2));
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * f + c2) * f + c1) * f + x0
    }
}

/// A Schroeder allpass around a delay line: it smears sound in time without colouring it.
#[derive(Clone, Debug)]
pub(crate) struct Allpass {
    pub line: Line,
}

impl Allpass {
    pub fn new(longest: usize) -> Self {
        Self { line: Line::new(longest) }
    }

    /// One sample through, with the delay `d` (at least 1) and coefficient `c` (-1..1).
    #[inline]
    pub fn process(&mut self, x: f32, d: f32, c: f32) -> f32 {
        let delayed = self.line.linear(d);
        let w = x - c * delayed;
        self.line.push(w);
        c * w + delayed
    }
}

/// The coefficients of a [`Svf`] for one cutoff and Q.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SvfCoefs {
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl SvfCoefs {
    pub fn new(hz: f32, q: f32) -> Self {
        let g = (std::f32::consts::PI * hz.clamp(5.0, 0.45 * SAMPLE_RATE) / SAMPLE_RATE).tan();
        Self::from_g(g, 1.0 / q.max(0.1))
    }

    /// From the warped cutoff `g` (tan(pi fc / fs)) and damping `k` (1 / Q).
    pub fn from_g(g: f32, k: f32) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self { k, a1, a2, a3: g * a2 }
    }
}

/// A state-variable filter (Simper's trapezoidal form): stable however fast it is swept.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Svf {
    ic1: f32,
    ic2: f32,
}

impl Svf {
    /// One sample through: (lowpass, highpass).
    #[inline]
    pub fn run(&mut self, x: f32, c: &SvfCoefs) -> (f32, f32) {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        (v2, x - c.k * v1 - v2)
    }
}

/// A one-pole glide for a setting, moved once per block of up to [`BLOCK`] samples: it gets
/// 99 % of the way in its time and then lands exactly on the target.
#[derive(Clone, Debug)]
pub(crate) struct Glide {
    k: f32,
    value: f32,
    target: f32,
}

impl Glide {
    pub fn new(time_ms: f32) -> Self {
        let blocks = (time_ms / 1_000.0 * SAMPLE_RATE / BLOCK as f32).max(1.0);
        Self { k: 1.0 - 0.01f32.powf(1.0 / blocks), value: 0.0, target: 0.0 }
    }

    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    pub fn snap(&mut self) {
        self.value = self.target;
    }

    /// One block on; the new value.
    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.k;
        if (self.target - self.value).abs() <= 1e-5 * self.target.abs().max(1.0) {
            self.value = self.target;
        }
        self.value
    }

    pub fn value(&self) -> f32 {
        self.value
    }
}

/// A value that moves in a straight line across one block, sample by sample, and ends the
/// block exactly on its target.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ramp {
    value: f32,
    step: f32,
    end: f32,
}

impl Ramp {
    /// Heads for `target` over the next `n` samples.
    pub fn to(&mut self, target: f32, n: usize) {
        self.value = self.end;
        self.end = target;
        self.step = (target - self.value) / n.max(1) as f32;
    }

    pub fn snap(&mut self, v: f32) {
        self.value = v;
        self.end = v;
        self.step = 0.0;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        self.value += self.step;
        self.value
    }

    /// Lands on the target at the end of the block.
    pub fn finish(&mut self) {
        self.value = self.end;
        self.step = 0.0;
    }
}

/// How far into Freeze a reverb is (0 off, 1 frozen): a straight 30 ms move, landing exactly.
#[derive(Clone, Debug, Default)]
pub(crate) struct FreezeAmount {
    pub value: f32,
    pub target: f32,
}

impl FreezeAmount {
    const STEP: f32 = BLOCK as f32 / (0.03 * SAMPLE_RATE);

    #[inline]
    pub fn tick(&mut self) -> f32 {
        let d = self.target - self.value;
        self.value = if d.abs() <= Self::STEP { self.target } else { self.value + Self::STEP * d.signum() };
        self.value
    }
}

/// The stereo pre-delay in front of a reverb (its time glides, so a change never clicks).
#[derive(Clone, Debug)]
pub(crate) struct PreDelay {
    lines: [Line; 2],
    time: crate::Smoother,
}

impl PreDelay {
    pub fn new() -> Self {
        Self { lines: [Line::new(MAX_PRE_DELAY + 2), Line::new(MAX_PRE_DELAY + 2)], time: crate::Smoother::new(80.0) }
    }

    pub fn set(&mut self, samples: f32) {
        self.time.set_target(samples.clamp(0.0, MAX_PRE_DELAY as f32) + 1.0);
    }

    pub fn snap(&mut self) {
        self.time.reset(self.time.target());
    }

    pub fn clear(&mut self) {
        for l in &mut self.lines {
            l.clear();
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let d = self.time.next();
        let out = (self.lines[0].linear(d), self.lines[1].linear(d));
        self.lines[0].push(l);
        self.lines[1].push(r);
        out
    }
}

/// The wet output's low and high cuts (12 dB an octave, off exactly at their ends) and width.
#[derive(Clone, Debug)]
pub(crate) struct WetOut {
    low: Glide,
    high: Glide,
    low_on: Ramp,
    high_on: Ramp,
    low_on_target: f32,
    high_on_target: f32,
    width: Ramp,
    width_target: f32,
    low_c: SvfCoefs,
    high_c: SvfCoefs,
    filters: [[Svf; 2]; 2],
}

impl WetOut {
    pub fn new() -> Self {
        Self {
            low: Glide::new(30.0),
            high: Glide::new(30.0),
            low_on: Ramp::default(),
            high_on: Ramp::default(),
            low_on_target: 0.0,
            high_on_target: 0.0,
            width: Ramp::default(),
            width_target: 1.0,
            low_c: SvfCoefs::new(20.0, std::f32::consts::FRAC_1_SQRT_2),
            high_c: SvfCoefs::new(20_000.0, std::f32::consts::FRAC_1_SQRT_2),
            filters: [[Svf::default(); 2]; 2],
        }
    }

    pub fn set(&mut self, low_cut_hz: f32, high_cut_hz: f32, width: f32) {
        // An off cut keeps its last frequency, so it fades out rather than jumping.
        self.low_on_target = if low_cut_hz > 20.0 { 1.0 } else { 0.0 };
        if low_cut_hz > 20.0 {
            self.low.set(low_cut_hz.min(20_000.0));
        } else if self.low.target == 0.0 {
            self.low.set(20.0);
        }
        self.high_on_target = if high_cut_hz < 20_000.0 { 1.0 } else { 0.0 };
        if high_cut_hz < 20_000.0 {
            self.high.set(high_cut_hz.max(20.0));
        } else if self.high.target == 0.0 {
            self.high.set(20_000.0);
        }
        self.width_target = width.clamp(0.0, 1.0);
    }

    pub fn snap(&mut self) {
        self.low.snap();
        self.high.snap();
        self.low_on.snap(self.low_on_target);
        self.high_on.snap(self.high_on_target);
        self.width.snap(self.width_target);
        self.update_coefs();
    }

    pub fn clear(&mut self) {
        self.filters = [[Svf::default(); 2]; 2];
    }

    fn update_coefs(&mut self) {
        let q = std::f32::consts::FRAC_1_SQRT_2;
        self.low_c = SvfCoefs::new(self.low.value(), q);
        self.high_c = SvfCoefs::new(self.high.value(), q);
    }

    /// Moves the settings on at the start of a block of `n` samples.
    pub fn tick(&mut self, n: usize) {
        self.low.tick();
        self.high.tick();
        self.update_coefs();
        self.low_on.to(self.low_on_target, n);
        self.high_on.to(self.high_on_target, n);
        self.width.to(self.width_target, n);
    }

    /// Lands the ramps at the end of the block.
    pub fn finish(&mut self) {
        self.low_on.finish();
        self.high_on.finish();
        self.width.finish();
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let (lo, hi, w) = (self.low_on.next(), self.high_on.next(), self.width.next());
        let mut out = [l, r];
        for (x, f) in out.iter_mut().zip(self.filters.iter_mut()) {
            let (_, hp) = f[0].run(*x, &self.low_c);
            *x += lo * (hp - *x);
            let (lp, _) = f[1].run(*x, &self.high_c);
            *x += hi * (lp - *x);
        }
        let mid = 0.5 * (out[0] + out[1]);
        let side = 0.5 * (out[0] - out[1]) * w;
        (mid + side, mid - side)
    }
}

/// The checks every reverb must pass, shared by their tests.
#[cfg(test)]
pub(crate) mod checks {
    use super::ReverbParams;
    use crate::testing::Noise;

    /// What the checks need from a reverb.
    pub trait Verb {
        fn set(&mut self, p: &ReverbParams);
        fn process(&mut self, l: &mut [f32], r: &mut [f32]);
    }

    /// Runs `input` through in blocks, giving the wet output.
    pub fn run(v: &mut dyn Verb, l: &[f32], r: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let (mut a, mut b) = (l.to_vec(), r.to_vec());
        for (ca, cb) in a.chunks_mut(crate::BLOCK).zip(b.chunks_mut(crate::BLOCK)) {
            v.process(ca, cb);
        }
        (a, b)
    }

    pub fn noise(seed: u64, n: usize, amp: f32) -> (Vec<f32>, Vec<f32>) {
        let mut ns = Noise(seed);
        let l = (0..n).map(|_| ns.next() * amp).collect();
        let r = (0..n).map(|_| ns.next() * amp).collect();
        (l, r)
    }

    /// Settings for measuring: nothing that colours or wobbles the tail.
    pub fn plain(decay_s: f32) -> ReverbParams {
        ReverbParams { decay_s, damping: 0.0, modulation: 0.0, ..ReverbParams::default() }
    }

    /// The RT60 of the wet output after a short noise burst, by Schroeder's backward
    /// integration: a straight line fitted between -5 and -35 dB, carried on to -60.
    pub fn measured_rt60(v: &mut dyn Verb, p: &ReverbParams) -> f32 {
        v.set(p);
        let n = ((2.5 * p.decay_s + 0.5) * crate::SAMPLE_RATE) as usize;
        let (mut l, mut r) = noise(11, n, 0.0);
        let (bl, br) = noise(5, 480, 0.5);
        l[..480].copy_from_slice(&bl);
        r[..480].copy_from_slice(&br);
        let (a, b) = run(v, &l, &r);
        let mut tail = vec![0.0f64; n + 1];
        for i in (0..n).rev() {
            tail[i] = tail[i + 1] + (a[i] as f64).powi(2) + (b[i] as f64).powi(2);
        }
        let db: Vec<f64> = tail[..n].iter().map(|e| 10.0 * (e / tail[0]).log10()).collect();
        let from = db.iter().position(|&d| d <= -5.0).expect("falls 5 dB");
        let to = db.iter().position(|&d| d <= -35.0).expect("falls 35 dB");
        let (mut sx, mut sy, mut sxx, mut sxy) = (0.0, 0.0, 0.0, 0.0);
        let m = (to - from) as f64;
        for (i, &d) in db.iter().enumerate().take(to).skip(from) {
            let t = i as f64 / crate::SAMPLE_RATE as f64;
            sx += t;
            sy += d;
            sxx += t * t;
            sxy += t * d;
        }
        let slope = (m * sxy - sx * sy) / (m * sxx - sx * sx);
        (-60.0 / slope) as f32
    }

    /// Checks the RT60 is within 15 % of each set decay.
    pub fn decays_at_set_time(make: &dyn Fn() -> Box<dyn Verb>, name: &str, extra: ReverbParams) {
        for decay in [1.0, 4.0] {
            let p = ReverbParams { decay_s: decay, ..extra };
            let rt = measured_rt60(make().as_mut(), &p);
            println!("{name}: set {decay} s, measured RT60 {rt:.3} s");
            assert!((rt / decay - 1.0).abs() < 0.15, "{name}: set {decay} s, measured {rt:.3} s");
        }
    }

    fn peak(x: &[f32], y: &[f32]) -> f32 {
        x.iter().chain(y).fold(0.0f32, |m, v| m.max(v.abs()))
    }

    fn rms_db(x: &[f32], y: &[f32]) -> f32 {
        let e: f64 = x.iter().chain(y).map(|v| (*v as f64).powi(2)).sum::<f64>() / (2 * x.len()) as f64;
        (10.0 * e.log10()) as f32
    }

    /// Decay 20 s fed full-scale noise for a long time, then silence: the tail's peak (per
    /// 100 ms) never climbs more than 1 dB over its level when the input stopped.
    pub fn longest_decay_stays_bounded(make: &dyn Fn() -> Box<dyn Verb>, name: &str, extra: ReverbParams) {
        let secs = if cfg!(debug_assertions) { 8 } else { 60 };
        let tail_secs = if cfg!(debug_assertions) { 4 } else { 20 };
        let mut v = make();
        v.set(&ReverbParams { decay_s: 20.0, ..extra });
        let sr = crate::SAMPLE_RATE as usize;
        let (l, r) = noise(3, secs * sr, 1.0);
        let (a, b) = run(v.as_mut(), &l, &r);
        assert!(a.iter().chain(&b).all(|x| x.is_finite()), "{name}: finite while fed");
        let win = sr / 10;
        let at_stop = peak(&a[a.len() - 5 * win..], &b[b.len() - 5 * win..]);
        let (z, _) = noise(1, tail_secs * sr, 0.0);
        let (ta, tb) = run(v.as_mut(), &z, &z);
        assert!(ta.iter().chain(&tb).all(|x| x.is_finite()), "{name}: finite in the tail");
        for (i, (wa, wb)) in ta.chunks(win).zip(tb.chunks(win)).enumerate() {
            let p = peak(wa, wb);
            assert!(
                crate::gain_to_db(p) <= crate::gain_to_db(at_stop) + 1.0,
                "{name}: window {i} peaks at {p}, over {at_stop} at the stop"
            );
        }
    }

    /// Freeze holds the tail level (within 1.5 dB, never growing); off again, it falls.
    pub fn freeze_off_resumes_decay(make: &dyn Fn() -> Box<dyn Verb>, name: &str, extra: ReverbParams) {
        let sr = crate::SAMPLE_RATE as usize;
        let mut v = make();
        let p = ReverbParams { decay_s: 2.0, ..extra };
        v.set(&p);
        let (l, r) = noise(9, 2 * sr, 0.5);
        run(v.as_mut(), &l, &r);
        v.set(&ReverbParams { freeze: true, ..p });
        let (z, _) = noise(1, 3 * sr, 0.0);
        // Freeze also mutes what comes in: noise here must not reach the tail.
        let (nl, nr) = noise(4, 3 * sr, 0.5);
        let (fa, fb) = run(v.as_mut(), &nl, &nr);
        let win = sr / 4;
        let reference = rms_db(&fa[win..2 * win], &fb[win..2 * win]);
        for (i, (wa, wb)) in fa.chunks(win).zip(fb.chunks(win)).enumerate().skip(1) {
            let lvl = rms_db(wa, wb);
            println!("{name}: frozen window {i}: {lvl:.2} dB (reference {reference:.2})");
            assert!((lvl - reference).abs() <= 1.5, "{name}: frozen level moved to {lvl:.2} from {reference:.2} dB");
        }
        let frozen = rms_db(&fa[fa.len() - win..], &fb[fb.len() - win..]);
        v.set(&p);
        let (da, db) = run(v.as_mut(), &z[..2 * sr], &z[..2 * sr]);
        let mut last = frozen;
        for (i, (wa, wb)) in da.chunks(win).zip(db.chunks(win)).enumerate() {
            let lvl = rms_db(wa, wb);
            assert!(lvl <= last + 0.5, "{name}: thawed window {i} grew to {lvl:.2} from {last:.2} dB");
            assert!(lvl <= frozen + 1.0, "{name}: thawed window {i} at {lvl:.2}, over the frozen {frozen:.2} dB");
            last = lvl;
        }
        let end = rms_db(&da[da.len() - win..], &db[db.len() - win..]);
        assert!(end < frozen - 20.0, "{name}: after Freeze the tail fell only to {end:.2} from {frozen:.2} dB");
    }

    /// The same input from new gives the same samples.
    pub fn deterministic(make: &dyn Fn() -> Box<dyn Verb>, extra: ReverbParams) {
        let take = || {
            let mut v = make();
            v.set(&extra);
            let (l, r) = noise(2, 48_000, 0.5);
            run(v.as_mut(), &l, &r)
        };
        assert_eq!(take(), take());
    }
}
