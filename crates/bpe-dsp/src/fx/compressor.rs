//! The bus compressor, with a sidechain that can listen to itself, another bus or hits.

use crate::fx::eq::Svf;
use crate::{Smoother, db_to_gain, gain_to_db};
use std::f64::consts::FRAC_1_SQRT_2;

/// The compressor's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompParams {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub knee_db: f32,
    pub makeup_db: f32,
    pub sc_low_cut_hz: f32,
    pub sc_high_cut_hz: f32,
}

impl Default for CompParams {
    fn default() -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            knee_db: 6.0,
            makeup_db: 0.0,
            sc_low_cut_hz: 20.0,
            sc_high_cut_hz: 20_000.0,
        }
    }
}

/// What the compressor listens to.
#[derive(Clone, Copy, Debug)]
pub enum Key<'a> {
    SelfInput,
    External(&'a [f32], &'a [f32]),
    Hits(&'a [(u32, f32)]),
}

/// Samples in the detector's RMS window (5 ms).
const RMS_LEN: usize = 240;
/// Samples a hit holds the detector up for (5 ms).
const HIT_HOLD: u32 = 240;
/// The quietest level the meters and the detector read, in dB.
const FLOOR_DB: f32 = -120.0;

/// The settings brought into their ranges.
fn clamped(p: &CompParams) -> CompParams {
    let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
    CompParams {
        threshold_db: finite(p.threshold_db, 0.0).clamp(-60.0, 0.0),
        ratio: finite(p.ratio, 1.0).clamp(1.0, 20.0),
        attack_ms: finite(p.attack_ms, 10.0).clamp(0.1, 100.0),
        release_ms: finite(p.release_ms, 100.0).clamp(5.0, 2_000.0),
        knee_db: finite(p.knee_db, 0.0).clamp(0.0, 24.0),
        makeup_db: finite(p.makeup_db, 0.0).clamp(0.0, 24.0),
        sc_low_cut_hz: finite(p.sc_low_cut_hz, 20.0).clamp(20.0, 20_000.0),
        sc_high_cut_hz: finite(p.sc_high_cut_hz, 20_000.0).clamp(20.0, 20_000.0),
    }
}

/// The gain computer: the level out for a level `x` in (all in dB), with the soft knee of
/// Giannoulis, Massberg and Reiss (2012). `slope` is 1 - 1/ratio.
#[inline]
fn curve(x: f32, threshold: f32, slope: f32, knee: f32) -> f32 {
    let over = x - threshold;
    if 2.0 * over <= -knee {
        x
    } else if 2.0 * over < knee {
        let into = over + 0.5 * knee;
        x - slope * into * into / (2.0 * knee)
    } else {
        x - slope * over
    }
}

/// A one-pole coefficient that gets 63 % of the way in `ms`.
fn one_pole(ms: f32) -> f32 {
    (-1.0 / (ms / 1_000.0 * crate::SAMPLE_RATE)).exp()
}

/// A 12 dB per octave filter on the key, for both channels.
#[derive(Clone, Debug, Default)]
struct KeyFilter {
    svf: Option<Svf>,
    state: [[f64; 2]; 2],
}

impl KeyFilter {
    /// Turns the filter on (`Some`) or off; turning it on starts with clear memories.
    fn set(&mut self, svf: Option<Svf>) {
        if self.svf.is_none() {
            self.state = [[0.0; 2]; 2];
        }
        self.svf = svf;
    }

    #[inline]
    fn run(&mut self, x: [f64; 2]) -> [f64; 2] {
        match &self.svf {
            Some(svf) => {
                let [s0, s1] = &mut self.state;
                [svf.tick(s0, x[0]), svf.tick(s1, x[1])]
            }
            None => x,
        }
    }

    fn flush(&mut self) {
        for v in self.state.iter_mut().flatten() {
            if v.abs() < 1e-25 {
                *v = 0.0;
            }
        }
    }
}

/// A feed-forward stereo compressor. Both channels share one gain from the louder of the two.
/// The detector reads the larger of the key's peak and its 5 ms RMS (scaled so a steady sine
/// reads its peak); the gain glides in dB with its attack and release times. The key is the
/// input itself, another stereo signal, or hits, each of which holds the detector at its
/// strength for 5 ms. The sidechain's low and high cuts filter only what it listens to.
pub struct Compressor {
    params: CompParams,
    threshold: Smoother,
    slope: Smoother,
    knee: Smoother,
    makeup: Smoother,
    attack: f32,
    release: f32,
    /// The gain change now, in dB (0 or below).
    gr: f32,
    squares: [f32; RMS_LEN],
    at: usize,
    sum: f64,
    low_cut: KeyFilter,
    high_cut: KeyFilter,
    /// The strength of the hit holding the detector up, and for how many more samples.
    pulse: f32,
    hold: u32,
    meter_gr: f32,
    meter_in: f32,
    meter_out: f32,
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compressor {
    /// A compressor with the default settings.
    pub fn new() -> Self {
        let mut c = Self {
            params: CompParams::default(),
            threshold: Smoother::new(Smoother::KNOB_MS),
            slope: Smoother::new(Smoother::KNOB_MS),
            knee: Smoother::new(Smoother::KNOB_MS),
            makeup: Smoother::new(Smoother::KNOB_MS),
            attack: 0.0,
            release: 0.0,
            gr: 0.0,
            squares: [0.0; RMS_LEN],
            at: 0,
            sum: 0.0,
            low_cut: KeyFilter::default(),
            high_cut: KeyFilter::default(),
            pulse: 0.0,
            hold: 0,
            meter_gr: 0.0,
            meter_in: FLOOR_DB,
            meter_out: FLOOR_DB,
        };
        c.set(&CompParams::default());
        c.reset();
        c
    }

    /// The settings. Threshold, ratio, knee and makeup glide there; the rest apply at once.
    pub fn set(&mut self, p: &CompParams) {
        let p = clamped(p);
        self.params = p;
        self.threshold.set_target(p.threshold_db);
        self.slope.set_target(1.0 - 1.0 / p.ratio);
        self.knee.set_target(p.knee_db);
        self.makeup.set_target(p.makeup_db);
        self.attack = one_pole(p.attack_ms);
        self.release = one_pole(p.release_ms);
        let low = (p.sc_low_cut_hz > 20.0).then(|| Svf::highpass(p.sc_low_cut_hz as f64, FRAC_1_SQRT_2));
        let high = (p.sc_high_cut_hz < 20_000.0).then(|| Svf::lowpass(p.sc_high_cut_hz as f64, FRAC_1_SQRT_2));
        self.low_cut.set(low);
        self.high_cut.set(high);
    }

    /// Forgets the sound so far and jumps the settings to where they are going.
    pub fn reset(&mut self) {
        for s in [&mut self.threshold, &mut self.slope, &mut self.knee, &mut self.makeup] {
            s.reset(s.target());
        }
        self.gr = 0.0;
        self.squares = [0.0; RMS_LEN];
        self.at = 0;
        self.sum = 0.0;
        self.low_cut.state = [[0.0; 2]; 2];
        self.high_cut.state = [[0.0; 2]; 2];
        self.pulse = 0.0;
        self.hold = 0;
        self.meter_gr = 0.0;
        self.meter_in = FLOOR_DB;
        self.meter_out = FLOOR_DB;
    }

    /// The detector's level for one key sample (left, right).
    #[inline]
    fn detect(&mut self, l: f32, r: f32) -> f32 {
        let [l, r] = self.high_cut.run(self.low_cut.run([l as f64, r as f64]));
        let peak = l.abs().max(r.abs()) as f32;
        let square = peak * peak;
        self.sum += (square - self.squares[self.at]) as f64;
        self.squares[self.at] = square;
        self.at += 1;
        if self.at == RMS_LEN {
            // Add it up afresh once a window so rounding never builds up.
            self.at = 0;
            self.sum = self.squares.iter().map(|&s| s as f64).sum();
        }
        let rms = ((2.0 * self.sum.max(0.0)) / RMS_LEN as f64).sqrt() as f32;
        peak.max(rms)
    }

    /// Compresses a stereo block in place (all wet), listening to `key`. External keys are as
    /// long as the block (missing samples count as silence); hits are (sample in this block,
    /// strength 0..1), in order, and a hit past the block's end counts on its last sample.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], key: Key) {
        let n = l.len().min(r.len());
        let mut hits = match key {
            Key::Hits(h) => h,
            _ => &[],
        };
        let (mut most_gr, mut most_in, mut most_out) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..n {
            let level = match key {
                Key::SelfInput => self.detect(l[i], r[i]),
                Key::External(kl, kr) => {
                    self.detect(kl.get(i).copied().unwrap_or(0.0), kr.get(i).copied().unwrap_or(0.0))
                }
                Key::Hits(_) => {
                    while let Some(&(at, strength)) = hits.first() {
                        if (at as usize).min(n - 1) > i {
                            break;
                        }
                        let strength = if strength.is_finite() { strength.clamp(0.0, 1.0) } else { 0.0 };
                        self.pulse = if self.hold > 0 { self.pulse.max(strength) } else { strength };
                        self.hold = HIT_HOLD;
                        hits = &hits[1..];
                    }
                    if self.hold > 0 {
                        self.hold -= 1;
                        self.pulse
                    } else {
                        0.0
                    }
                }
            };
            let x = 20.0 * level.max(1e-6).log10();
            let (threshold, slope, knee) = (self.threshold.next(), self.slope.next(), self.knee.next());
            let target = curve(x, threshold, slope, knee) - x;
            let c = if target < self.gr { self.attack } else { self.release };
            self.gr = target + c * (self.gr - target);
            if (self.gr - target).abs() < 1e-6 {
                self.gr = target;
            }
            let g = db_to_gain(self.gr + self.makeup.next());
            l[i] *= g;
            r[i] *= g;
            most_gr = most_gr.min(self.gr);
            most_in = most_in.max(level);
            most_out = most_out.max(l[i].abs()).max(r[i].abs());
        }
        self.low_cut.flush();
        self.high_cut.flush();
        self.meter_gr = most_gr;
        self.meter_in = gain_to_db(most_in).max(FLOOR_DB);
        self.meter_out = gain_to_db(most_out).max(FLOOR_DB);
    }

    /// The most the gain was turned down in the last block, in dB (0 or below).
    pub fn gain_reduction_db(&self) -> f32 {
        self.meter_gr
    }

    /// The detector's highest level in the last block, in dB (-120 at the quietest).
    pub fn input_db(&self) -> f32 {
        self.meter_in
    }

    /// The output's peak in the last block, in dB (-120 at the quietest).
    pub fn output_db(&self) -> f32 {
        self.meter_out
    }

    /// The level out for a steady level in, in dB, makeup included. For the editor.
    pub fn static_curve_db(p: &CompParams, in_db: f32) -> f32 {
        let p = clamped(p);
        curve(in_db, p.threshold_db, 1.0 - 1.0 / p.ratio, p.knee_db) + p.makeup_db
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::sine;
    use crate::{BLOCK, SAMPLE_RATE};

    fn params(threshold_db: f32, ratio: f32, knee_db: f32, attack_ms: f32, release_ms: f32) -> CompParams {
        CompParams { threshold_db, ratio, knee_db, attack_ms, release_ms, ..CompParams::default() }
    }

    fn ready(p: &CompParams) -> Compressor {
        let mut c = Compressor::new();
        c.set(p);
        c.reset();
        c
    }

    /// Runs a mono signal (the same on both sides) through, listening to itself.
    fn run_self(c: &mut Compressor, x: &[f32]) -> Vec<f32> {
        let (mut l, mut r) = (x.to_vec(), x.to_vec());
        for (cl, cr) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
            c.process(cl, cr, Key::SelfInput);
        }
        assert_eq!(l, r);
        l
    }

    /// The gain in dB on each sample, for a steady positive input `x`.
    fn gains_db(x: &[f32], y: &[f32]) -> Vec<f32> {
        x.iter().zip(y).map(|(a, b)| 20.0 * (b / a).log10()).collect()
    }

    fn peak_db(v: &[f32]) -> f32 {
        let rms = (v.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
        (20.0 * (rms * std::f64::consts::SQRT_2).log10()) as f32
    }

    fn dc(level: f32, n: usize) -> Vec<f32> {
        vec![level; n]
    }

    #[test]
    fn static_curve_has_its_knee() {
        let p = CompParams { threshold_db: -20.0, ratio: 4.0, knee_db: 10.0, makeup_db: 2.0, ..Default::default() };
        let at = |x| Compressor::static_curve_db(&p, x);
        assert_eq!(at(-40.0), -38.0);
        assert_eq!(at(-25.0), -23.0);
        assert!((at(0.0) - (-20.0 + 20.0 / 4.0 + 2.0)).abs() < 1e-4);
        assert!((at(-15.0) - (-20.0 + 5.0 / 4.0 + 2.0)).abs() < 1e-4);
        // Inside the knee it bends smoothly between the two lines.
        assert!((at(-20.0) - (-20.0 - 0.75 * 25.0 / 20.0 + 2.0)).abs() < 1e-4);
        let hard = CompParams { knee_db: 0.0, ..p };
        assert_eq!(Compressor::static_curve_db(&hard, -20.0), -18.0);
    }

    #[test]
    fn compressor_static_curve() {
        let settings = [
            CompParams { makeup_db: 0.0, ..params(-20.0, 4.0, 0.0, 5.0, 100.0) },
            CompParams { makeup_db: 3.0, ..params(-24.0, 2.0, 12.0, 5.0, 100.0) },
            CompParams { makeup_db: 0.0, ..params(-10.0, 10.0, 6.0, 5.0, 100.0) },
            CompParams { makeup_db: 6.0, ..params(-30.0, 20.0, 24.0, 5.0, 100.0) },
        ];
        for p in settings {
            for level in [-30.0f32, -20.0, -10.0, 0.0] {
                let mut c = ready(&p);
                let x = sine(1_000.0, crate::db_to_gain(level) as f64, 0.0, 24_000);
                let y = run_self(&mut c, &x);
                let got = peak_db(&y[12_000..]);
                let want = Compressor::static_curve_db(&p, level);
                assert!((got - want).abs() < 0.5, "{p:?} at {level} dB: got {got}, want {want}");
            }
        }
    }

    #[test]
    fn compressor_attack_and_release_times() {
        let p = params(-20.0, 4.0, 0.0, 20.0, 200.0);
        let mut c = ready(&p);
        let fs = SAMPLE_RATE;
        let (quiet, loud) = (4_800, 24_000);
        let mut x = dc(0.01, quiet);
        x.extend(dc(1.0, loud));
        x.extend(dc(0.01, 48_000));
        let g = gains_db(&x, &run_self(&mut c, &x));
        assert!(g[quiet - 1].abs() < 1e-3, "quiet is untouched: {}", g[quiet - 1]);
        let full = g[quiet + loud - 1];
        assert!(full < -14.0, "loud is turned down: {full}");
        let attack = g[quiet..].iter().position(|&v| v <= 0.632 * full).unwrap() as f32 / fs * 1_000.0;
        assert!((attack - 20.0).abs() < 2.0, "attack {attack} ms");
        let after = &g[quiet + loud..];
        let release = after.iter().position(|&v| v >= (1.0 - 0.632) * full).unwrap() as f32 / fs * 1_000.0;
        assert!((release - 200.0).abs() < 20.0, "release {release} ms");
        // Five release times on, under 1 % of the reduction is left.
        assert!(after.last().unwrap().abs() < 0.01 * -full, "{}", after.last().unwrap());
    }

    #[test]
    fn hits_sidechain_ducks_on_each_hit() {
        let p = params(-18.0, 4.0, 0.0, 0.5, 40.0);
        let mut c = ready(&p);
        let hits: [(usize, u32, f32); 3] = [(10, 17, 1.0), (200, 0, 1.0), (400, 63, 0.5)];
        let x = dc(0.03, 600 * BLOCK);
        let (mut l, mut r) = (x.clone(), x.clone());
        for (b, (cl, cr)) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).enumerate() {
            let list: Vec<(u32, f32)> = hits.iter().filter(|h| h.0 == b).map(|h| (h.1, h.2)).collect();
            c.process(cl, cr, Key::Hits(&list));
        }
        let g = gains_db(&x, &l);
        for (block, offset, strength) in hits {
            let at = block * BLOCK + offset as usize;
            let want = (20.0 * strength.log10() + 18.0) * 0.75;
            assert!(g[at - 1].abs() < 0.1, "nothing before the hit at {at}: {}", g[at - 1]);
            let deepest = g[at..at + 480].iter().copied().fold(0.0, f32::min);
            assert!((deepest + want).abs() < 1.0, "hit at {at} ducks {deepest}, want -{want}");
            assert!(g[at + 48] < -0.6 * want, "ducks quickly after the hit at {at}: {}", g[at + 48]);
            assert!(g[at + 9_600] > -0.2, "recovers after the hit at {at}: {}", g[at + 9_600]);
        }
    }

    #[test]
    fn external_key_ducks() {
        let p = params(-20.0, 4.0, 0.0, 1.0, 50.0);
        let x = dc(0.03, 24_000);
        let key = sine(1_000.0, 1.0, 0.0, x.len());
        let mut c = ready(&p);
        let (mut l, mut r) = (x.clone(), x.clone());
        for ((cl, cr), k) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).zip(key.chunks(BLOCK)) {
            c.process(cl, cr, Key::External(k, k));
        }
        let g = gains_db(&x, &l);
        let late = g[12_000..].iter().copied().fold(f32::MIN, f32::max);
        assert!((late + 15.0).abs() < 1.0, "ducked to {late}");
        assert!((c.gain_reduction_db() + 15.0).abs() < 1.0, "meter {}", c.gain_reduction_db());
        assert!(c.input_db().abs() < 0.5, "key level {}", c.input_db());
        assert!((c.output_db() - (-30.46 - 15.0)).abs() < 1.0, "output {}", c.output_db());
        // Listening to itself, the same quiet input is left alone.
        let mut c = ready(&p);
        let y = run_self(&mut c, &x);
        assert_eq!(y, x);
        assert_eq!(c.gain_reduction_db(), 0.0);
    }

    #[test]
    fn sidechain_filter_ignores_lows() {
        let x = dc(0.03, 24_000);
        let key = sine(40.0, 1.0, 0.0, x.len());
        let duck = |low_cut: f32| {
            let p = CompParams { sc_low_cut_hz: low_cut, ..params(-20.0, 4.0, 0.0, 1.0, 50.0) };
            let mut c = ready(&p);
            let (mut l, mut r) = (x.clone(), x.clone());
            for ((cl, cr), k) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).zip(key.chunks(BLOCK)) {
                c.process(cl, cr, Key::External(k, k));
            }
            gains_db(&x[12_000..], &l[12_000..]).into_iter().fold(0.0, f32::min)
        };
        let (off, on) = (duck(20.0), duck(200.0));
        assert!(off < -12.0, "a 40 Hz key ducks with no low cut: {off}");
        assert!(on > -3.0, "the low cut takes it out: {on}");
        // A high cut below the key does the same.
        let p = CompParams { sc_high_cut_hz: 2_000.0, ..params(-20.0, 4.0, 0.0, 1.0, 50.0) };
        let mut c = ready(&p);
        let x = dc(0.03, 9_600);
        let key = sine(12_000.0, 1.0, 0.0, x.len());
        let (mut l, mut r) = (x.clone(), x.clone());
        for ((cl, cr), k) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).zip(key.chunks(BLOCK)) {
            c.process(cl, cr, Key::External(k, k));
        }
        assert!(c.gain_reduction_db() > -3.0, "{}", c.gain_reduction_db());
    }

    #[test]
    fn settings_changes_do_not_click() {
        let x = sine(200.0, 0.5, 0.0, 48_000);
        let (mut l, mut r) = (x.clone(), x.clone());
        let mut c = ready(&params(-20.0, 4.0, 0.0, 0.1, 5.0));
        for (b, (cl, cr)) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).enumerate() {
            let jump = (b / 50) % 2 == 0;
            let p = CompParams {
                threshold_db: if jump { -40.0 } else { 0.0 },
                makeup_db: if jump { 18.0 } else { 0.0 },
                ..params(0.0, if jump { 20.0 } else { 1.0 }, 0.0, 0.1, 5.0)
            };
            c.set(&p);
            c.process(cl, cr, Key::SelfInput);
        }
        let step = |v: &[f32]| v.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        let bound = step(&x) * 3.0;
        assert!(step(&l) < bound, "{} against {bound}", step(&l));
        assert!(l.iter().all(|v| v.is_finite()));
    }
}
