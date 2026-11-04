//! Band-limited wavetables. A [`TableSet`] holds one wave bent by one Warp mode at
//! [`WARP_STEPS`] amounts, each as [`MIPS`] copies with fewer and fewer harmonics (one per
//! octave). A note reads the two copies whose harmonics stay under half the sample rate at its
//! pitch, crossfaded, and the two Warp steps either side of its Warp, crossfaded, so nothing
//! folds back however high it plays.
//!
//! Sets are baked on first use (off the audio thread) and kept in a small shared bank: baking
//! every wave and mode up front would take hundreds of megabytes.

use crate::warp::{WarpModeId, WaveShape, warped};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};

/// Samples one cycle is drawn at before it is band-limited.
pub const FRAME: usize = 8192;
/// How many Warp amounts a set holds, from 0 to 100 %.
pub const WARP_STEPS: usize = 64;
/// Band-limited copies per step, one per octave: copy `m` keeps `1024 >> m` harmonics.
pub const MIPS: usize = 11;
/// Harmonics in the richest copy.
pub const TOP_HARMONICS: usize = 1024;
/// Samples each copy holds: at least 16 per harmonic, 32 to 64 in the copies a narrow pulse or a
/// full Sync fills to the top, so reading between samples adds nothing that folds back above
/// -90 dB even when the top harmonic is the loudest. A set is about 12 MB.
const LENS: [usize; MIPS] = [16384, 8192, 4096, 4096, 2048, 2048, 1024, 1024, 1024, 1024, 1024];
/// The highest a harmonic may play, in Hz (half the sample rate).
const NYQUIST: f64 = crate::SAMPLE_RATE as f64 / 2.0;
/// Sets the bank keeps.
pub const BANK_SIZE: usize = 12;

/// Harmonics copy `m` keeps.
pub fn harmonics(m: usize) -> usize {
    (TOP_HARMONICS >> m).max(1)
}

/// Samples copy `m` holds per step.
pub fn mip_len(m: usize) -> usize {
    LENS[m.min(MIPS - 1)]
}

/// One octave's copies of every step: step `s` is `data[s * (len + 3)..]`, the cycle with one
/// sample before it and two after (wrapped) so a 4-point read never wraps.
struct Mip {
    len: usize,
    data: Vec<f32>,
}

impl Mip {
    #[inline]
    fn frame(&self, step: usize) -> &[f32] {
        let stride = self.len + 3;
        &self.data[step * stride..(step + 1) * stride]
    }
}

/// One wave and Warp mode, baked.
pub struct TableSet {
    pub wave: WaveShape,
    pub mode: WarpModeId,
    mips: Vec<Mip>,
}

/// In-place iterative radix-2 complex FFT. `inverse` leaves out the 1/n.
pub fn fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let ang = sign * std::f64::consts::TAU / len as f64;
        let (ws, wc) = ang.sin_cos();
        for start in (0..n).step_by(len) {
            let (mut c, mut s) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (xr, xi) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
                let nc = c * wc - s * ws;
                s = c * ws + s * wc;
                c = nc;
            }
        }
        len <<= 1;
    }
}

impl TableSet {
    /// Bakes every step and copy of `wave` bent by `mode`. Allocates and takes a while (tens of
    /// milliseconds): never on the audio thread.
    pub fn bake(wave: WaveShape, mode: WarpModeId) -> TableSet {
        let mut mips: Vec<Mip> = (0..MIPS)
            .map(|m| Mip { len: mip_len(m), data: Vec::with_capacity((mip_len(m) + 3) * WARP_STEPS) })
            .collect();
        let (mut re, mut im) = (vec![0.0f64; FRAME], vec![0.0f64; FRAME]);
        for step in 0..WARP_STEPS {
            let amount = step as f32 / (WARP_STEPS - 1) as f32;
            for (i, (r, j)) in re.iter_mut().zip(im.iter_mut()).enumerate() {
                *r = warped(wave, mode, amount, i as f32 / FRAME as f32) as f64;
                *j = 0.0;
            }
            fft(&mut re, &mut im, false);
            for (m, mip) in mips.iter_mut().enumerate() {
                let (n, h) = (mip.len, harmonics(m));
                let (mut r2, mut i2) = (vec![0.0f64; n], vec![0.0f64; n]);
                // Harmonics 1..=h (no DC), scaled to the copy's length.
                for k in 1..=h.min(n / 2 - 1) {
                    let s = 1.0 / FRAME as f64;
                    r2[k] = re[k] * s;
                    i2[k] = im[k] * s;
                    r2[n - k] = re[FRAME - k] * s;
                    i2[n - k] = im[FRAME - k] * s;
                }
                fft(&mut r2, &mut i2, true);
                mip.data.push(r2[n - 1] as f32);
                mip.data.extend(r2.iter().map(|v| *v as f32));
                mip.data.push(r2[0] as f32);
                mip.data.push(r2[1] as f32);
            }
        }
        TableSet { wave, mode, mips }
    }

    /// Bytes it holds.
    pub fn memory(&self) -> usize {
        self.mips.iter().map(|m| m.data.len() * 4).sum()
    }

    /// How to read it for a note at `freq_hz` with Warp at `warp` (0..1). Cheap; no allocation.
    #[inline]
    pub fn reader(&self, freq_hz: f32) -> Reader<'_> {
        // The copy whose top harmonic is under half the sample rate at this pitch, crossfaded
        // toward the next one as the pitch rises through its octave.
        let f = (freq_hz.abs() as f64).max(1e-3);
        let mu = (f * TOP_HARMONICS as f64 / NYQUIST).log2();
        let (m0, t) = if mu <= 0.0 {
            (0usize, 0.0f32)
        } else {
            let a = mu.ceil();
            (a as usize, (mu - (a - 1.0)) as f32)
        };
        let m0 = m0.min(MIPS - 1);
        let m1 = (m0 + 1).min(MIPS - 1);
        let t = if m0 == m1 { 0.0 } else { t.clamp(0.0, 1.0) };
        Reader { set: self, m0, m1, t }
    }

    /// One sample at `phase` (cycles, wrapped), Warp step position `step` (0..1) for a note at
    /// `freq_hz`.
    pub fn sample(&self, phase: f64, step: f32, freq_hz: f32) -> f32 {
        self.reader(freq_hz).read(phase, step)
    }
}

/// A [`TableSet`] read at one pitch: the two copies and how much of the second.
#[derive(Clone, Copy)]
pub struct Reader<'a> {
    set: &'a TableSet,
    m0: usize,
    m1: usize,
    t: f32,
}

/// 4-point, third-order Lagrange interpolation at `f` (0..1) between `y[1]` and `y[2]`.
#[inline(always)]
fn lagrange(y: &[f32], i: usize, f: f32) -> f32 {
    let (y0, y1, y2, y3) = (y[i], y[i + 1], y[i + 2], y[i + 3]);
    let c0 = -f * (f - 1.0) * (f - 2.0) * (1.0 / 6.0);
    let c1 = (f + 1.0) * (f - 1.0) * (f - 2.0) * 0.5;
    let c2 = -(f + 1.0) * f * (f - 2.0) * 0.5;
    let c3 = (f + 1.0) * f * (f - 1.0) * (1.0 / 6.0);
    c0 * y0 + c1 * y1 + c2 * y2 + c3 * y3
}

impl Reader<'_> {
    /// The two Warp steps either side of `step` (0..1) and how much of the upper one.
    #[inline(always)]
    pub fn steps(step: f32) -> (usize, usize, f32) {
        let x = step.clamp(0.0, 1.0) * (WARP_STEPS - 1) as f32;
        let s0 = (x as usize).min(WARP_STEPS - 1);
        let s1 = (s0 + 1).min(WARP_STEPS - 1);
        (s0, s1, x - s0 as f32)
    }

    #[inline(always)]
    fn read_mip(&self, m: usize, phase: f64, s0: usize, s1: usize, sf: f32) -> f32 {
        let mip = &self.set.mips[m];
        let x = phase * mip.len as f64;
        let i = x as usize;
        let f = (x - i as f64) as f32;
        let i = i.min(mip.len - 1);
        let a = lagrange(mip.frame(s0), i, f);
        if sf <= 1e-6 || s0 == s1 {
            return a;
        }
        let b = lagrange(mip.frame(s1), i, f);
        a + (b - a) * sf
    }

    /// The sample at `phase` (cycles; any value, wrapped) and Warp position `step` (0..1).
    #[inline(always)]
    pub fn read(&self, phase: f64, step: f32) -> f32 {
        let (s0, s1, sf) = Self::steps(step);
        self.read_steps(phase, s0, s1, sf)
    }

    /// [`Reader::read`] with the steps already worked out.
    #[inline(always)]
    pub fn read_steps(&self, phase: f64, s0: usize, s1: usize, sf: f32) -> f32 {
        let phase = phase - phase.floor();
        let a = self.read_mip(self.m0, phase, s0, s1, sf);
        if self.t <= 1e-6 {
            return a;
        }
        let b = self.read_mip(self.m1, phase, s0, s1, sf);
        a + (b - a) * self.t
    }
}

/// Keeps the sets baked so far (the last [`BANK_SIZE`] used) and hands them out shared.
#[derive(Default)]
pub struct WaveBank {
    sets: VecDeque<Arc<TableSet>>,
}

impl WaveBank {
    pub fn new() -> Self {
        Self::default()
    }

    /// The program's one bank.
    pub fn shared() -> &'static Mutex<WaveBank> {
        static BANK: OnceLock<Mutex<WaveBank>> = OnceLock::new();
        BANK.get_or_init(|| Mutex::new(WaveBank::new()))
    }

    /// The set for `wave` bent by `mode`, baked now if the bank doesn't have it.
    pub fn prepare(&mut self, wave: WaveShape, mode: WarpModeId) -> Arc<TableSet> {
        if let Some(i) = self.sets.iter().position(|s| s.wave == wave && s.mode == mode) {
            let set = self.sets.remove(i).expect("just found");
            self.sets.push_back(set.clone());
            return set;
        }
        let set = Arc::new(TableSet::bake(wave, mode));
        self.sets.push_back(set.clone());
        while self.sets.len() > BANK_SIZE {
            self.sets.pop_front();
        }
        set
    }

    /// From the shared bank.
    pub fn get(wave: WaveShape, mode: WarpModeId) -> Arc<TableSet> {
        Self::shared().lock().unwrap_or_else(|e| e.into_inner()).prepare(wave, mode)
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }
}

#[cfg(test)]
pub(crate) mod spectrum {
    use super::fft;

    /// Power of each bin of `x` under a 4-term Blackman-Harris window.
    pub fn power(x: &[f32]) -> Vec<f64> {
        let n = x.len();
        let a = [0.35875, 0.48829, 0.14128, 0.01168];
        let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
        for (i, v) in x.iter().enumerate() {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            let w = a[0] - a[1] * t.cos() + a[2] * (2.0 * t).cos() - a[3] * (3.0 * t).cos();
            re[i] = *v as f64 * w;
        }
        fft(&mut re, &mut im, false);
        (0..n / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect()
    }

    /// The loudest bin more than 3 bins from every harmonic of `bin`, in dB against `bin`.
    pub fn worst_off_harmonic_db(power: &[f64], bin: usize) -> f64 {
        let fund = power[bin].max(1e-300);
        let worst = (0..power.len())
            .filter(|&k| {
                let r = k % bin;
                r > 3 && bin - r > 3
            })
            .map(|k| power[k])
            .fold(0.0f64, f64::max);
        10.0 * (worst / fund).max(1e-30).log10()
    }

    /// The window length and bin for a steady tone near `hz`: the tone sits exactly on a bin
    /// and its harmonics at least 16 bins apart.
    pub fn plan(hz: f64) -> (usize, usize) {
        let rate = crate::SAMPLE_RATE as f64;
        let mut n = 8192;
        while (n as f64) * hz / rate < 16.0 {
            n *= 2;
        }
        (n, (hz * n as f64 / rate).round() as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::spectrum::*;
    use super::*;

    /// Test pitches from 20 Hz to C8 (4186 Hz, the scientific naming; FL's C8 is 2093 Hz),
    /// about a half octave apart, with some in between.
    pub(crate) const PITCHES: [f64; 17] = [
        20.0, 31.0, 47.0, 62.0, 93.0, 130.0, 185.0, 260.0, 370.0, 520.0, 740.0, 1047.0, 1480.0, 2093.0, 2960.0, 3520.0,
        4186.0,
    ];

    fn render(set: &TableSet, step: f32, bin: usize, n: usize) -> Vec<f32> {
        let hz = bin as f64 * crate::SAMPLE_RATE as f64 / n as f64;
        let r = set.reader(hz as f32);
        (0..n).map(|i| r.read(hz * i as f64 / crate::SAMPLE_RATE as f64, step)).collect()
    }

    #[test]
    fn every_wave_and_mode_is_alias_free_to_c8() {
        let mut bank = WaveBank::new();
        let mut worst = (f64::MIN, String::new());
        for wave in WaveShape::ALL {
            for mode in WarpModeId::ALL {
                let set = bank.prepare(wave, mode);
                for warp in [0.0, 0.5, 1.0] {
                    for hz in PITCHES {
                        let (n, bin) = plan(hz);
                        let x = render(&set, warp, bin, n);
                        let p = power(&x);
                        // Against the loudest harmonic: a full Sync moves the energy up off the
                        // fundamental.
                        let loudest = (1..=p.len() / bin).filter(|h| h * bin < p.len()).map(|h| p[h * bin]).fold(0.0, f64::max);
                        // A warp that leaves (almost) nothing, such as a square mirrored onto
                        // itself, has nothing to fold.
                        let full_scale = (n as f64 * 0.35875 / 2.0).powi(2);
                        if loudest < full_scale * 1e-6 {
                            continue;
                        }
                        let mut q = p.clone();
                        q[bin] = loudest;
                        let db = worst_off_harmonic_db(&q, bin);
                        if db > worst.0 {
                            worst = (db, format!("{wave:?} {mode:?} warp {warp} at {hz} Hz"));
                        }
                    }
                }
            }
        }
        assert!(worst.0 < -90.0, "folded components reach {:.1} dB ({})", worst.0, worst.1);
    }

    #[test]
    fn table_sample_is_continuous_across_mips() {
        let set = TableSet::bake(WaveShape::Saw, WarpModeId::Sync);
        // A glide from 1 kHz to 3 kHz over a second.
        let rate = crate::SAMPLE_RATE as f64;
        let n = rate as usize;
        let mut phase = 0.0f64;
        let mut prev = 0.0f32;
        let mut worst_jump = 0.0f32;
        let mut own = 0.0f32;
        for i in 0..n {
            let hz = 1000.0 * 3f64.powf(i as f64 / n as f64);
            if i % 64 == 0 {
                // The wave's own steepest sample-to-sample step at this pitch.
                let r = set.reader(hz as f32);
                own = (0..512)
                    .map(|k| {
                        let p = k as f64 / 512.0;
                        (r.read(p + hz / rate, 0.0) - r.read(p, 0.0)).abs()
                    })
                    .fold(0.0f32, f32::max);
            }
            let v = set.sample(phase, 0.0, hz as f32);
            if i > 0 {
                worst_jump = worst_jump.max((v - prev).abs() / own);
            }
            prev = v;
            phase += hz / rate;
        }
        assert!(worst_jump <= 1.5, "a step {worst_jump:.2} times the wave's own steepest");
    }

    #[test]
    fn draw_matches_the_table_shape() {
        // Read at a low pitch, where the table keeps the most harmonics.
        for wave in WaveShape::ALL {
            for mode in [WarpModeId::Sync, WarpModeId::BendUp, WarpModeId::Pwm] {
                let set = TableSet::bake(wave, mode);
                // On a step: between steps a Sync crossfades two different ratios.
                let amount = 32.0 / (WARP_STEPS - 1) as f32;
                let drawn = crate::warp::draw(wave, mode, amount, 256);
                let r = set.reader(30.0);
                let table: Vec<f32> = (0..256).map(|i| r.read(i as f64 / 256.0, amount)).collect();
                let corr = correlation(&drawn, &table);
                assert!(corr > 0.98, "{wave:?} {mode:?}: {corr}");
            }
        }
    }

    fn correlation(a: &[f32], b: &[f32]) -> f64 {
        let mean = |x: &[f32]| x.iter().map(|v| *v as f64).sum::<f64>() / x.len() as f64;
        let (ma, mb) = (mean(a), mean(b));
        let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
        for (x, y) in a.iter().zip(b) {
            let (x, y) = (*x as f64 - ma, *y as f64 - mb);
            ab += x * y;
            aa += x * x;
            bb += y * y;
        }
        ab / (aa * bb).sqrt()
    }

    #[test]
    fn the_bank_keeps_twelve_and_shares_them() {
        let mut bank = WaveBank::new();
        let a = bank.prepare(WaveShape::Sine, WarpModeId::Sync);
        let b = bank.prepare(WaveShape::Sine, WarpModeId::Sync);
        assert!(Arc::ptr_eq(&a, &b));
        for mode in WarpModeId::ALL {
            bank.prepare(WaveShape::Saw, mode);
        }
        bank.prepare(WaveShape::Square, WarpModeId::Flip);
        bank.prepare(WaveShape::Square, WarpModeId::Pwm);
        assert_eq!(bank.len(), BANK_SIZE);
        let c = bank.prepare(WaveShape::Sine, WarpModeId::Sync);
        assert!(!Arc::ptr_eq(&a, &c), "the oldest was let go and baked again");
        assert!(a.memory() < 13 << 20, "{} bytes", a.memory());
    }
}
