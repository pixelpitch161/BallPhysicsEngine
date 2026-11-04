//! The unison oscillator (up to 16 detuned copies of a wavetable, eight samples at a time in SIMD lanes)
//! and the sub oscillator.

use crate::BLOCK;
use crate::noise::Noise;
use crate::tables::{Reader, TableSet, mip_len, octaves};
use wide::{f32x4, f32x8, i32x4};

/// The most unison copies.
pub const MAX_UNISON: usize = 16;
const LANES: usize = 8;

/// What the unison oscillator plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnisonParams {
    pub freq_hz: f32,
    /// 1..=16 copies.
    pub unison: u8,
    /// How far apart the outermost copies are tuned, in cents.
    pub detune_cents: f32,
    /// 0..1: how far the copies spread across left and right.
    pub spread: f32,
    /// 0..1: the middle copy (0) against the others (1).
    pub blend: f32,
    /// 0..1: how far each copy's start is shuffled.
    pub random_phase: f32,
    /// Where in the wave a note starts, in degrees.
    pub phase_deg: f32,
    /// Restart the wave on each note (else it runs free).
    pub restart: bool,
}

impl Default for UnisonParams {
    fn default() -> Self {
        Self {
            freq_hz: 440.0,
            unison: 1,
            detune_cents: 20.0,
            spread: 0.5,
            blend: 0.5,
            random_phase: 0.0,
            phase_deg: 0.0,
            restart: true,
        }
    }
}

/// Where copy `i` of `n` sits among the others, -1..1, lowest pitch first.
fn place(i: usize, n: usize) -> f32 {
    if n <= 1 { 0.0 } else { -1.0 + 2.0 * i as f32 / (n - 1) as f32 }
}

/// Equal-power pan gains for `pan` (-1..1), each 1 in the middle.
fn pan_gains(pan: f32) -> (f32, f32) {
    let theta = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (theta.cos() * std::f32::consts::SQRT_2, theta.sin() * std::f32::consts::SQRT_2)
}

/// Up to 16 copies of one wavetable, detuned and spread.
#[derive(Clone, Debug)]
pub struct UnisonOsc {
    phase: [f64; MAX_UNISON],
    inc: [f64; MAX_UNISON],
    /// Each copy's tuning in octaves from the note.
    oct: [f64; MAX_UNISON],
    gl: [f32; MAX_UNISON],
    gr: [f32; MAX_UNISON],
    n: usize,
    params: UnisonParams,
    /// The tunings and pans follow `params`.
    ready: bool,
    rng: Noise,
}

impl Default for UnisonOsc {
    fn default() -> Self {
        Self::new()
    }
}

impl UnisonOsc {
    pub fn new() -> Self {
        let mut o = UnisonOsc {
            phase: [0.0; MAX_UNISON],
            inc: [0.0; MAX_UNISON],
            oct: [0.0; MAX_UNISON],
            gl: [0.0; MAX_UNISON],
            gr: [0.0; MAX_UNISON],
            n: 1,
            params: UnisonParams::default(),
            ready: false,
            rng: Noise::new(0),
        };
        o.set(&UnisonParams::default());
        o
    }

    /// Seeds the random phases (once per voice, from the scene's seed).
    pub fn prepare(&mut self, seed: u64) {
        self.rng.seed(seed);
    }

    pub fn params(&self) -> &UnisonParams {
        &self.params
    }

    /// New settings; the copies' tunings and pans follow at once (the caller smooths).
    pub fn set(&mut self, p: &UnisonParams) {
        // Called every block: the tunings and pans only need working out when something moved.
        if *p == self.params && self.ready {
            return;
        }
        self.ready = true;
        self.params = *p;
        let n = (p.unison as usize).clamp(1, MAX_UNISON);
        self.n = n;
        let rate = crate::SAMPLE_RATE as f64;
        // Copies tuned symmetrically, the inner ones closer (a curve like the classic unisons).
        let half = p.detune_cents.max(0.0) * 0.5;
        // The middle copy, or the two middle ones, against the rest.
        let centre = if n == 1 {
            1
        } else if n % 2 == 1 {
            1
        } else {
            2
        };
        let sides = n - centre;
        let blend = p.blend.clamp(0.0, 1.0);
        let (centre_g, side_g) = if sides == 0 {
            ((1.0 / centre as f32).sqrt(), 0.0)
        } else {
            (((1.0 - blend) / centre as f32).sqrt(), (blend / sides as f32).sqrt())
        };
        for i in 0..MAX_UNISON {
            if i >= n {
                self.inc[i] = 0.0;
                self.oct[i] = 0.0;
                self.gl[i] = 0.0;
                self.gr[i] = 0.0;
                continue;
            }
            let x = place(i, n);
            let cents = x.signum() * x.abs().powf(1.5) * half;
            self.oct[i] = cents as f64 / 1200.0;
            self.inc[i] = p.freq_hz.max(0.0) as f64 * 2f64.powf(self.oct[i]) / rate;
            let is_centre = if n % 2 == 1 { i == n / 2 } else { i == n / 2 || i + 1 == n / 2 };
            let g = if is_centre { centre_g } else { side_g };
            // Alternately left and right, further out the further the copy is from the middle.
            let side = if i % 2 == 0 { -1.0 } else { 1.0 };
            let (l, r) = pan_gains(side * x.abs() * p.spread.clamp(0.0, 1.0));
            self.gl[i] = g * l;
            self.gr[i] = g * r;
        }
    }

    /// A note starts: with Restart every copy goes to Phase, plus its share of Random phase.
    pub fn note_on(&mut self) {
        if !self.params.restart {
            return;
        }
        let start = (self.params.phase_deg as f64 / 360.0).rem_euclid(1.0);
        let random = self.params.random_phase.clamp(0.0, 1.0) as f64;
        for p in &mut self.phase {
            let r = self.rng.unit() as f64;
            *p = (start + random * r).rem_euclid(1.0);
        }
    }

    /// Moves every copy back `samples` samples (a note starting part way into a block reads
    /// its start phase on its first sample).
    pub fn rewind(&mut self, samples: u32) {
        for v in 0..MAX_UNISON {
            self.phase[v] = (self.phase[v] - samples as f64 * self.inc[v]).rem_euclid(1.0);
        }
    }

    /// Takes over another oscillator's phases (a patch swap keeps ringing notes in step).
    pub fn take_phases(&mut self, other: &UnisonOsc) {
        self.phase = other.phase;
    }

    /// Adds one block into `l` and `r`: the table read at each copy's pitch, Warp at
    /// `warp[i]` (0..1) and gain `gain[i]` on each sample.
    ///
    /// Each copy runs on its own, eight samples at a time in SIMD lanes, its phase a 32-bit
    /// fixed-point count for the block (the tables are powers of two long, so the index is
    /// the top bits and the fraction the rest); the f64 phase is the one kept between blocks.
    pub fn render(
        &mut self,
        set: &TableSet,
        warp: &[f32; BLOCK],
        gain: &[f32; BLOCK],
        l: &mut [f32; BLOCK],
        r: &mut [f32; BLOCK],
    ) {
        // The Warp steps of each sample, shared by every copy.
        let mut s0 = [0u32; BLOCK];
        let mut s1 = [0u32; BLOCK];
        let mut sf = [0.0f32; BLOCK];
        let mut cross = [false; BLOCK / 4];
        let step_of = |w: f32| {
            let (a, b, f) = Reader::steps(w);
            (a as u32, b as u32, if a == b { 0.0 } else { f })
        };
        if warp.iter().all(|w| *w == warp[0]) {
            let (a, b, f) = step_of(warp[0]);
            s0 = [a; BLOCK];
            s1 = [b; BLOCK];
            sf = [f; BLOCK];
            cross = [f > 1e-6; BLOCK / 4];
        } else {
            for i in 0..BLOCK {
                (s0[i], s1[i], sf[i]) = step_of(warp[i]);
                cross[i / 4] |= sf[i] > 1e-6;
            }
        }
        let even: [bool; BLOCK / 4] = std::array::from_fn(|c| s0[4 * c..4 * c + 4].iter().all(|s| *s == s0[4 * c]));
        let steps = Steps { s0: &s0, s1: &s1, sf: &sf, even: &even, cross: &cross };
        let (mut sl, mut sr) = ([0.0f32; BLOCK], [0.0f32; BLOCK]);
        let mu = octaves(self.params.freq_hz);
        for v in 0..self.n {
            let (m0, m1, t) = set.mips_at(mu + self.oct[v]);
            let phase = (self.phase[v] * TWO_32) as u64 as u32;
            let inc = (self.inc[v] * TWO_32).round() as u32;
            let (gl, gr) = (self.gl[v], self.gr[v]);
            let mut out = Out { l: &mut sl, r: &mut sr };
            if t > 1e-6 {
                read_copy(set, m0, phase, inc, &steps, (gl * (1.0 - t), gr * (1.0 - t)), &mut out);
                read_copy(set, m1, phase, inc, &steps, (gl * t, gr * t), &mut out);
            } else {
                read_copy(set, m0, phase, inc, &steps, (gl, gr), &mut out);
            }
            self.phase[v] = (self.phase[v] + BLOCK as f64 * self.inc[v]).rem_euclid(1.0);
        }
        for c in 0..BLOCK / LANES {
            let at = c * LANES;
            let g = f32x8::from(lanes(gain, at));
            let a = f32x8::from(lanes(l, at)) + f32x8::from(lanes(&sl, at)) * g;
            let b = f32x8::from(lanes(r, at)) + f32x8::from(lanes(&sr, at)) * g;
            put(l, at, a);
            put(r, at, b);
        }
    }
}

/// 2^32: a phase of one whole cycle in fixed point.
const TWO_32: f64 = 4_294_967_296.0;

/// Where the copies add up, left and right.
struct Out<'a> {
    l: &'a mut [f32; BLOCK],
    r: &'a mut [f32; BLOCK],
}

/// The Warp steps of a block's samples.
struct Steps<'a> {
    s0: &'a [u32; BLOCK],
    s1: &'a [u32; BLOCK],
    sf: &'a [f32; BLOCK],
    /// Whether each four samples share one step.
    even: &'a [bool; BLOCK / 4],
    /// Whether any sample of each four crossfades to a second step.
    cross: &'a [bool; BLOCK / 4],
}

/// Adds one copy's block, read from copy `m` of the tables, into `out` (times `weight`, left
/// and right).
#[inline(always)]
fn read_copy(set: &TableSet, m: usize, phase: u32, inc: u32, steps: &Steps, weight: (f32, f32), out: &mut Out) {
    let data = set.mip_data(m);
    // The length as a constant lets the compiler see every tap is inside its frame.
    match mip_len(m) {
        16384 => read_len::<18>(data, phase, inc, steps, weight, out),
        8192 => read_len::<19>(data, phase, inc, steps, weight, out),
        4096 => read_len::<20>(data, phase, inc, steps, weight, out),
        2048 => read_len::<21>(data, phase, inc, steps, weight, out),
        1024 => read_len::<22>(data, phase, inc, steps, weight, out),
        other => unreachable!("no copy is {other} long"),
    }
}

/// [`read_copy`] for a copy `1 << (32 - SHIFT)` samples long, four samples at a time: each
/// sample's four taps are one load, turned into a tap per lane with a 4x4 transpose.
#[inline(always)]
fn read_len<const SHIFT: u32>(data: &[f32], phase: u32, inc: u32, steps: &Steps, weight: (f32, f32), out: &mut Out) {
    let len = 1usize << (32 - SHIFT);
    let stride = len + 3;
    let low = i32x4::splat(((1u32 << (32 - SHIFT)) - 1) as i32);
    let mask = i32x4::splat(((1u32 << SHIFT) - 1) as i32);
    let scale = f32x4::splat(1.0 / (1u64 << SHIFT) as f32);
    let (wl, wr) = (f32x4::splat(weight.0), f32x4::splat(weight.1));
    let ramp: [i32; 4] = std::array::from_fn(|k| phase.wrapping_add(inc.wrapping_mul(k as u32)) as i32);
    let mut p = i32x4::from(ramp);
    let jump = i32x4::splat(inc.wrapping_mul(4) as i32);
    for c in 0..BLOCK / 4 {
        let at = c * 4;
        // Logical shift: the arithmetic one, with the sign bits masked off.
        let idx = ((p >> SHIFT) & low).to_array();
        let f = (p & mask).round_float() * scale;
        p += jump;
        let mut a = if steps.even[c] {
            let start = steps.s0[at] as usize * stride;
            lagrange4::<SHIFT>(&data[start..start + stride], [0; 4], &idx, f)
        } else {
            lagrange_steps::<SHIFT>(data, &steps.s0[at..at + 4], &idx, f)
        };
        if steps.cross[c] {
            let b = lagrange_steps::<SHIFT>(data, &steps.s1[at..at + 4], &idx, f);
            a += (b - a) * f32x4::from(quad(steps.sf, at));
        }
        let l = f32x4::from(quad(out.l, at)) + a * wl;
        let r = f32x4::from(quad(out.r, at)) + a * wr;
        out.l[at..at + 4].copy_from_slice(&l.to_array());
        out.r[at..at + 4].copy_from_slice(&r.to_array());
    }
}

/// 4-point Lagrange in each lane `k` between samples `idx[k] + 1` and `idx[k] + 2` of the frame
/// at `frames[k]` in `data`.
#[inline(always)]
fn lagrange4<const SHIFT: u32>(data: &[f32], frames: [usize; 4], idx: &[i32; 4], f: f32x4) -> f32x4 {
    let len = 1usize << (32 - SHIFT);
    let rows: [f32x4; 4] = std::array::from_fn(|k| {
        let frame = &data[frames[k]..frames[k] + len + 3];
        let i = idx[k] as usize & (len - 1);
        let t: [f32; 4] = frame[i..i + 4].try_into().expect("four taps");
        f32x4::from(t)
    });
    let [y0, y1, y2, y3] = f32x4::transpose(rows);
    let one = f32x4::splat(1.0);
    let two = f32x4::splat(2.0);
    let (fm1, fm2, fp1) = (f - one, f - two, f + one);
    let c0 = -f * fm1 * fm2 * f32x4::splat(1.0 / 6.0);
    let c1 = fp1 * fm1 * fm2 * f32x4::splat(0.5);
    let c2 = -fp1 * f * fm2 * f32x4::splat(0.5);
    let c3 = fp1 * f * fm1 * f32x4::splat(1.0 / 6.0);
    c0 * y0 + c1 * y1 + c2 * y2 + c3 * y3
}

/// [`lagrange4`] with each lane on its own Warp step.
#[inline(always)]
fn lagrange_steps<const SHIFT: u32>(data: &[f32], steps: &[u32], idx: &[i32; 4], f: f32x4) -> f32x4 {
    let stride = (1usize << (32 - SHIFT)) + 3;
    lagrange4::<SHIFT>(data, std::array::from_fn(|k| steps[k] as usize * stride), idx, f)
}

/// Samples `at .. at + 4` of a block.
#[inline(always)]
fn quad(a: &[f32; BLOCK], at: usize) -> [f32; 4] {
    a[at..at + 4].try_into().expect("four lanes")
}

/// Samples `at .. at + 8` of a block.
#[inline(always)]
fn lanes(a: &[f32; BLOCK], at: usize) -> [f32; LANES] {
    a[at..at + LANES].try_into().expect("eight lanes")
}

#[inline(always)]
fn put(a: &mut [f32; BLOCK], at: usize, v: f32x8) {
    a[at..at + LANES].copy_from_slice(&v.to_array());
}

/// The sub oscillator's wave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubWaveId {
    #[default]
    Off,
    Sine,
    Square,
}

/// A plain wave one or two octaves under the oscillator, in the middle.
#[derive(Clone, Debug, Default)]
pub struct SubOsc {
    phase: f64,
    inc: f64,
    wave: SubWaveId,
    level: f32,
}

impl SubOsc {
    /// Plays `wave` at `freq_hz` (the oscillator's pitch) `octave` (1 or 2) octaves down, at
    /// gain `level`.
    pub fn set(&mut self, freq_hz: f32, wave: SubWaveId, octave: u8, level: f32) {
        self.inc = freq_hz.max(0.0) as f64 / (1u32 << octave.clamp(1, 2)) as f64 / crate::SAMPLE_RATE as f64;
        self.wave = wave;
        self.level = level;
    }

    /// A note starts the sub at the start of its wave.
    pub fn note_on(&mut self) {
        self.phase = 0.0;
    }

    /// Moves back `samples` samples (see [`UnisonOsc::rewind`]).
    pub fn rewind(&mut self, samples: u32) {
        self.phase = (self.phase - samples as f64 * self.inc).rem_euclid(1.0);
    }

    pub fn is_off(&self) -> bool {
        self.wave == SubWaveId::Off || self.level == 0.0
    }

    /// Adds one block into `l` and `r`, `gain[i]` on each sample. `square` is the band-limited
    /// square table.
    pub fn render(
        &mut self,
        square: &TableSet,
        gain: &[f32; BLOCK],
        l: &mut [f32; BLOCK],
        r: &mut [f32; BLOCK],
    ) {
        if self.is_off() {
            return;
        }
        let reader = square.reader(self.inc as f32 * crate::SAMPLE_RATE);
        for i in 0..BLOCK {
            let v = match self.wave {
                SubWaveId::Sine => (std::f64::consts::TAU * self.phase).sin() as f32,
                SubWaveId::Square => reader.read_steps(self.phase, 0, 0, 0.0),
                SubWaveId::Off => 0.0,
            } * self.level
                * gain[i];
            l[i] += v;
            r[i] += v;
            self.phase += self.inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::WaveBank;
    use crate::warp::{WarpModeId, WaveShape};

    fn saw() -> std::sync::Arc<TableSet> {
        WaveBank::get(WaveShape::Saw, WarpModeId::Sync)
    }

    fn run(osc: &mut UnisonOsc, set: &TableSet, blocks: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut ls, mut rs) = (Vec::new(), Vec::new());
        let warp = [0.0; BLOCK];
        let gain = [1.0; BLOCK];
        for _ in 0..blocks {
            let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
            osc.render(set, &warp, &gain, &mut l, &mut r);
            ls.extend(l);
            rs.extend(r);
        }
        (ls, rs)
    }

    /// The window of a 7-term Blackman-Harris (sidelobes under -180 dB).
    fn bh7(i: usize, n: usize) -> f64 {
        let a = [
            0.271_051_400_693_42,
            0.433_297_939_234_48,
            0.218_122_999_543_11,
            0.065_925_446_388_03,
            0.010_811_742_098_37,
            0.000_776_584_825_22,
            0.000_013_887_217_35,
        ];
        let t = std::f64::consts::TAU * i as f64 / n as f64;
        a.iter().enumerate().map(|(k, c)| if k % 2 == 0 { *c } else { -*c } * (k as f64 * t).cos()).sum()
    }

    #[test]
    fn unison_16_at_max_detune_is_alias_free_at_c8() {
        let set = saw();
        let mut osc = UnisonOsc::new();
        let hz = 4186.0f32;
        let p = UnisonParams { freq_hz: hz, unison: 16, detune_cents: 100.0, spread: 1.0, ..UnisonParams::default() };
        osc.set(&p);
        osc.note_on();
        let n = 1 << 16;
        let (l, _) = run(&mut osc, &set, n / BLOCK);
        let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
        for i in 0..n {
            re[i] = l[i] as f64 * bh7(i, n);
        }
        crate::tables::fft(&mut re, &mut im, false);
        let power: Vec<f64> = (0..n / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect();
        // Every copy's harmonics under half the sample rate, and the bins near them.
        let bin_hz = crate::SAMPLE_RATE as f64 / n as f64;
        let mut near = vec![false; n / 2];
        for v in 0..16 {
            let f = osc.inc[v] * crate::SAMPLE_RATE as f64;
            let mut h = 1.0;
            while h * f < crate::SAMPLE_RATE as f64 / 2.0 {
                let c = (h * f / bin_hz).round() as i64;
                for k in (c - 12).max(0)..=(c + 12).min(n as i64 / 2 - 1) {
                    near[k as usize] = true;
                }
                h += 1.0;
            }
        }
        let loudest = power.iter().fold(0.0f64, |m, v| m.max(*v));
        let worst = power.iter().zip(&near).filter(|(_, n)| !**n).map(|(p, _)| *p).fold(0.0f64, f64::max);
        let db = 10.0 * (worst / loudest).max(1e-30).log10();
        assert!(db < -90.0, "folded components at {db:.1} dB");
    }

    #[test]
    fn unison_1_equals_a_single_table_read() {
        let set = saw();
        let mut osc = UnisonOsc::new();
        osc.set(&UnisonParams { freq_hz: 330.0, unison: 1, ..UnisonParams::default() });
        osc.note_on();
        let (l, r) = run(&mut osc, &set, 4);
        let reader = set.reader(330.0);
        let inc = 330.0f64 / crate::SAMPLE_RATE as f64;
        let mut phase = 0.0f64;
        for i in 0..l.len() {
            let want = reader.read(phase, 0.0);
            assert!((l[i] - want).abs() < 1e-5 && (r[i] - want).abs() < 1e-5, "{i}: {} {} {want}", l[i], r[i]);
            phase += inc;
            if phase >= 1.0 {
                phase -= 1.0;
            }
        }
    }

    #[test]
    fn spread_zero_is_mono() {
        let set = saw();
        let mut osc = UnisonOsc::new();
        osc.set(&UnisonParams { freq_hz: 220.0, unison: 7, spread: 0.0, ..UnisonParams::default() });
        osc.note_on();
        let (l, r) = run(&mut osc, &set, 8);
        assert_eq!(l, r);
        osc.set(&UnisonParams { freq_hz: 220.0, unison: 7, spread: 1.0, ..UnisonParams::default() });
        let (l, r) = run(&mut osc, &set, 8);
        assert_ne!(l, r);
    }

    #[test]
    fn restart_and_random_phase() {
        let mut osc = UnisonOsc::new();
        osc.prepare(9);
        osc.set(&UnisonParams { phase_deg: 90.0, unison: 4, ..UnisonParams::default() });
        osc.note_on();
        assert!(osc.phase[..4].iter().all(|p| (*p - 0.25).abs() < 1e-12));
        osc.set(&UnisonParams { random_phase: 1.0, unison: 4, ..UnisonParams::default() });
        osc.note_on();
        assert!(osc.phase[0] != osc.phase[1]);
        let kept = osc.phase;
        osc.set(&UnisonParams { restart: false, ..UnisonParams::default() });
        osc.note_on();
        assert_eq!(osc.phase, kept, "free phases run on");
    }

    #[test]
    fn no_denormals_after_silence() {
        let _guard = crate::FlushGuard::new();
        let set = saw();
        let mut osc = UnisonOsc::new();
        osc.set(&UnisonParams { freq_hz: 110.0, unison: 16, ..UnisonParams::default() });
        osc.note_on();
        let warp = [0.0; BLOCK];
        let gain = [0.0; BLOCK];
        let mut times = Vec::new();
        let blocks = 10 * 750;
        for b in 0..blocks {
            let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
            let t = std::time::Instant::now();
            osc.render(&set, &warp, &gain, &mut l, &mut r);
            if b < 750 || b >= blocks - 750 {
                times.push(t.elapsed().as_nanos() as f64);
            }
            assert!(l.iter().all(|v| *v == 0.0));
        }
        let median = |v: &mut [f64]| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        let (first, last) = times.split_at_mut(750);
        let (a, b) = (median(first), median(last));
        assert!(b < a * 1.5 + 2_000.0, "first second {a} ns a block, last {b}");
    }

    #[test]
    fn the_sub_plays_an_octave_down() {
        let set = WaveBank::get(WaveShape::Square, WarpModeId::Sync);
        let mut sub = SubOsc::default();
        sub.set(440.0, SubWaveId::Sine, 1, 1.0);
        sub.note_on();
        let gain = [1.0; BLOCK];
        let mut out = Vec::new();
        for _ in 0..30 {
            let (mut l, mut r) = ([0.0; BLOCK], [0.0; BLOCK]);
            sub.render(&set, &gain, &mut l, &mut r);
            out.extend(l);
        }
        // 220 Hz: a zero crossing upward every 48000 / 220 samples.
        let ups = out.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert_eq!(ups, (out.len() as f64 * 220.0 / 48_000.0) as usize);
        let mut off = SubOsc::default();
        off.set(440.0, SubWaveId::Off, 1, 1.0);
        assert!(off.is_off());
    }
}

