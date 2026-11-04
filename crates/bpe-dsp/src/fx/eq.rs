//! The 8-band equaliser: low and high cuts (12, 24 or 48 dB per octave), shelves, bells and
//! notches, each on both channels or on the mid or side part only.

use std::f64::consts::FRAC_1_SQRT_2;

/// What a band does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BandShape {
    LowCut,
    LowShelf,
    #[default]
    Bell,
    Notch,
    HighShelf,
    HighCut,
}

/// Which part of the stereo sound a band works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Place {
    #[default]
    Both,
    Mid,
    Side,
}

/// One band's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandParams {
    pub on: bool,
    pub shape: BandShape,
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
    pub sections: u8,
    pub place: Place,
}

impl Default for BandParams {
    fn default() -> Self {
        Self {
            on: false,
            shape: BandShape::Bell,
            freq: 1_000.0,
            gain_db: 0.0,
            q: FRAC_1_SQRT_2 as f32,
            sections: 1,
            place: Place::Both,
        }
    }
}

/// Samples between coefficient updates while a band's settings glide.
const SUB: usize = 8;
/// How long a band takes to fade in or out when it is turned on or off or changes shape, in ms.
const FADE_MS: f64 = 10.0;
/// How long frequency, gain and Q take to get 99 % of the way to a new setting, in ms.
const GLIDE_MS: f64 = 15.0;
/// The most second-order sections a band runs (a 48 dB per octave cut).
const MAX_SECTIONS: usize = 4;

/// The coefficients of one state-variable filter section (Andrew Simper's trapezoidal SVF).
/// Its output mixes the input, band and low outputs by `m`; the shapes come from that mix.
/// Its coefficients can change on any sample without the filter blowing up or clicking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Svf {
    g: f64,
    k: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    m: [f64; 3],
}

impl Default for Svf {
    fn default() -> Self {
        Self::with(1.0, 1.0, [1.0, 0.0, 0.0])
    }
}

impl Svf {
    fn with(g: f64, k: f64, m: [f64; 3]) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self { g, k, a1, a2, a3: g * a2, m }
    }

    /// `tan` of a frequency's place: the filter's prewarped cutoff.
    fn warp(hz: f64) -> f64 {
        let fs = crate::SAMPLE_RATE as f64;
        (std::f64::consts::PI * hz.clamp(1.0, 0.49 * fs) / fs).tan()
    }

    /// A 12 dB per octave low pass at `hz`.
    pub(crate) fn lowpass(hz: f64, q: f64) -> Self {
        Self::with(Self::warp(hz), 1.0 / q, [0.0, 0.0, 1.0])
    }

    /// A 12 dB per octave high pass at `hz`.
    pub(crate) fn highpass(hz: f64, q: f64) -> Self {
        let k = 1.0 / q;
        Self::with(Self::warp(hz), k, [1.0, -k, -1.0])
    }

    /// A notch at `hz`, `q` wide.
    fn notch(hz: f64, q: f64) -> Self {
        let k = 1.0 / q;
        Self::with(Self::warp(hz), k, [1.0, -k, 0.0])
    }

    /// The RBJ peaking bell.
    fn bell(hz: f64, gain_db: f64, q: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let k = 1.0 / (q * a);
        Self::with(Self::warp(hz), k, [1.0, k * (a * a - 1.0), 0.0])
    }

    /// The RBJ low shelf, with its Q.
    fn low_shelf(hz: f64, gain_db: f64, q: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let k = 1.0 / q;
        Self::with(Self::warp(hz) / a.sqrt(), k, [1.0, k * (a - 1.0), a * a - 1.0])
    }

    /// The RBJ high shelf, with its Q.
    fn high_shelf(hz: f64, gain_db: f64, q: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let k = 1.0 / q;
        Self::with(Self::warp(hz) * a.sqrt(), k, [a * a, k * (1.0 - a) * a, 1.0 - a * a])
    }

    /// Runs one sample through, with `s` the filter's two memories.
    #[inline]
    pub(crate) fn tick(&self, s: &mut [f64; 2], v0: f64) -> f64 {
        let v3 = v0 - s[1];
        let v1 = self.a1 * s[0] + self.a2 * v3;
        let v2 = s[1] + self.a2 * s[0] + self.a3 * v3;
        s[0] = 2.0 * v1 - s[0];
        s[1] = 2.0 * v2 - s[1];
        self.m[0] * v0 + self.m[1] * v1 + self.m[2] * v2
    }

    /// The plain gain at `hz`: the analogue prototype at the prewarped frequency, which is
    /// exactly what the filter does to a steady sine.
    fn magnitude(&self, hz: f64) -> f64 {
        let x = Self::warp(hz) / self.g;
        let [m0, m1, m2] = self.m;
        let (nr, ni) = (m0 + m2 - m0 * x * x, (m0 * self.k + m1) * x);
        let (dr, di) = (1.0 - x * x, self.k * x);
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }
}

/// A band's settings brought into their ranges, in the forms the filters use.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Target {
    on: bool,
    shape: BandShape,
    sections: usize,
    place: Place,
    log_hz: f64,
    gain_db: f64,
    ln_q: f64,
}

impl Target {
    fn of(b: &BandParams) -> Self {
        let sections = match b.shape {
            BandShape::LowCut | BandShape::HighCut => match b.sections {
                0 | 1 => 1,
                2 | 3 => 2,
                _ => 4,
            },
            _ => 1,
        };
        let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
        Self {
            on: b.on,
            shape: b.shape,
            sections,
            place: b.place,
            log_hz: (finite(b.freq, 1_000.0).clamp(20.0, 20_000.0) as f64).log2(),
            gain_db: finite(b.gain_db, 0.0).clamp(-18.0, 18.0) as f64,
            ln_q: (finite(b.q, FRAC_1_SQRT_2 as f32).clamp(0.1, 18.0) as f64).ln(),
        }
    }

    /// Whether `other` runs the same filters (so only frequency, gain and Q differ).
    fn same_kind(&self, other: &Self) -> bool {
        self.shape == other.shape && self.sections == other.sections && self.place == other.place
    }

    /// Fills `out` with this band's sections and gives how many there are.
    fn design(&self, out: &mut [Svf; MAX_SECTIONS]) -> usize {
        let (hz, q, gain_db) = (self.log_hz.exp2(), self.ln_q.exp(), self.gain_db);
        let shape = self.shape;
        match shape {
            BandShape::Bell => out[0] = Svf::bell(hz, gain_db, q),
            BandShape::LowShelf => out[0] = Svf::low_shelf(hz, gain_db, q),
            BandShape::HighShelf => out[0] = Svf::high_shelf(hz, gain_db, q),
            BandShape::Notch => out[0] = Svf::notch(hz, q),
            BandShape::LowCut | BandShape::HighCut => {
                // Butterworth section Qs, lowest first. The last (highest) one takes the band's Q
                // as a resonance, so at Q 0.7071 the cascade is an exact Butterworth.
                let n = self.sections;
                let order = (2 * n) as f64;
                for (j, section) in out.iter_mut().enumerate().take(n) {
                    let m = (n - j) as f64;
                    let mut sq = 1.0 / (2.0 * (std::f64::consts::PI * (2.0 * m - 1.0) / (2.0 * order)).sin());
                    if j == n - 1 {
                        sq *= q / FRAC_1_SQRT_2;
                    }
                    *section = if shape == BandShape::LowCut { Svf::highpass(hz, sq) } else { Svf::lowpass(hz, sq) };
                }
                return n;
            }
        }
        1
    }

    /// The analytic gain in dB at `f`, as if the band were on.
    fn response_db(&self, f: f32) -> f64 {
        let mut sections = [Svf::default(); MAX_SECTIONS];
        let n = self.design(&mut sections);
        let hz = (f as f64).max(1.0);
        let gain: f64 = sections[..n].iter().map(|s| s.magnitude(hz)).product();
        20.0 * gain.max(1e-15).log10()
    }
}

/// One band's analytic gain in dB at `f`: 0 for a band that is off. The editor draws with this.
pub fn band_response_db(b: &BandParams, f: f32) -> f32 {
    if b.on { Target::of(b).response_db(f) as f32 } else { 0.0 }
}

/// One band as it runs.
#[derive(Clone, Debug)]
struct Band {
    target: Target,
    /// The filters running now. While they differ in kind from the target's, the band fades out.
    running: Target,
    /// How much of the band's effect is heard, 0..1.
    mix: f64,
    sections: [Svf; MAX_SECTIONS],
    used: usize,
    /// Memories per channel (left and right, or the mid or side part in channel 0) and section.
    state: [[[f64; 2]; MAX_SECTIONS]; 2],
}

impl Band {
    fn new() -> Self {
        let target = Target::of(&BandParams::default());
        let mut band = Self {
            target,
            running: target,
            mix: 0.0,
            sections: [Svf::default(); MAX_SECTIONS],
            used: 1,
            state: [[[0.0; 2]; MAX_SECTIONS]; 2],
        };
        band.used = target.design(&mut band.sections);
        band
    }

    /// Takes the target as it is, with clear memories.
    fn snap(&mut self) {
        self.running = self.target;
        self.mix = if self.target.on { 1.0 } else { 0.0 };
        self.state = [[[0.0; 2]; MAX_SECTIONS]; 2];
        self.used = self.running.design(&mut self.sections);
    }

    /// Moves frequency, gain and Q one step of `k` towards the target.
    fn glide(&mut self, k: f64) {
        let t = self.target;
        let r = &mut self.running;
        let mut moved = false;
        for (v, to, close) in
            [(&mut r.log_hz, t.log_hz, 1e-5), (&mut r.gain_db, t.gain_db, 1e-4), (&mut r.ln_q, t.ln_q, 1e-5)]
        {
            if *v != to {
                *v += (to - *v) * k;
                if (to - *v).abs() < close {
                    *v = to;
                }
                moved = true;
            }
        }
        if moved {
            self.used = self.running.design(&mut self.sections);
        }
    }

    /// Runs a few samples (at most [`SUB`]) through the band.
    fn run(&mut self, l: &mut [f32], r: &mut [f32], glide: f64, fade: f64) {
        if self.mix == 0.0 {
            if !self.target.on {
                return;
            }
            // Silent and wanted: start afresh from the target, faded out.
            self.snap();
            self.mix = 0.0;
        } else {
            self.glide(glide);
        }
        let want = if self.target.on && self.target.same_kind(&self.running) { 1.0 } else { 0.0 };
        let sections = &self.sections[..self.used];
        let [s0, s1] = &mut self.state;
        let mut mix = self.mix;
        let mut step = || {
            if mix != want {
                mix = if want > mix { (mix + fade).min(want) } else { (mix - fade).max(want) };
            }
            mix
        };
        let cascade = |st: &mut [[f64; 2]; MAX_SECTIONS], x: f64| {
            sections.iter().zip(st.iter_mut()).fold(x, |y, (c, s)| c.tick(s, y))
        };
        match self.running.place {
            Place::Both => {
                for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                    let m = step();
                    let (x, y) = (*a as f64, *b as f64);
                    *a = (x + m * (cascade(s0, x) - x)) as f32;
                    *b = (y + m * (cascade(s1, y) - y)) as f32;
                }
            }
            Place::Mid | Place::Side => {
                let on_mid = self.running.place == Place::Mid;
                for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                    let m = step();
                    let (x, y) = (*a as f64, *b as f64);
                    let (mut mid, mut side) = (0.5 * (x + y), 0.5 * (x - y));
                    let part = if on_mid { &mut mid } else { &mut side };
                    *part += m * (cascade(s0, *part) - *part);
                    *a = (mid + side) as f32;
                    *b = (mid - side) as f32;
                }
            }
        }
        self.mix = mix;
    }

    /// Clears memories small enough to drift into denormals.
    fn flush(&mut self) {
        for v in self.state.iter_mut().flatten().flatten() {
            if v.abs() < 1e-25 {
                *v = 0.0;
            }
        }
    }
}

/// The 8-band equaliser: Simper state-variable filters whose gains match the RBJ cookbook
/// biquads, so frequency, gain and Q can glide every few samples. A band that is off costs
/// nothing. Turning a band on or off, or changing its shape, slope or place, fades its effect
/// out and back in over 10 ms.
pub struct Eq8 {
    bands: [Band; 8],
    output: crate::Smoother,
    output_db: f32,
    glide: f64,
    fade: f64,
}

impl Default for Eq8 {
    fn default() -> Self {
        Self::new()
    }
}

impl Eq8 {
    /// An equaliser with every band off and the output at 0 dB.
    pub fn new() -> Self {
        let fs = crate::SAMPLE_RATE as f64;
        let mut output = crate::Smoother::new(crate::Smoother::KNOB_MS);
        output.reset(1.0);
        Self {
            bands: std::array::from_fn(|_| Band::new()),
            output,
            output_db: 0.0,
            glide: 1.0 - 0.01f64.powf(SUB as f64 / (GLIDE_MS / 1_000.0 * fs)),
            fade: 1.0 / (FADE_MS / 1_000.0 * fs),
        }
    }

    /// The bands' settings, glided to from where they are.
    pub fn set(&mut self, bands: &[BandParams; 8]) {
        for (band, p) in self.bands.iter_mut().zip(bands) {
            band.target = Target::of(p);
        }
    }

    /// The output gain in dB, glided to.
    pub fn set_output_db(&mut self, db: f32) {
        self.output_db = if db.is_finite() { db.clamp(-48.0, 24.0) } else { 0.0 };
        self.output.set_target(crate::db_to_gain(self.output_db));
    }

    /// Clears the filters' memories and jumps every setting to where it is going.
    pub fn reset(&mut self) {
        for band in &mut self.bands {
            band.snap();
        }
        self.output.reset(self.output.target());
    }

    /// Runs a stereo block through, in place.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let (l, r) = (&mut l[..n], &mut r[..n]);
        for (cl, cr) in l.chunks_mut(SUB).zip(r.chunks_mut(SUB)) {
            for band in &mut self.bands {
                band.run(cl, cr, self.glide, self.fade);
            }
        }
        if !(self.output.settled() && self.output.value() == 1.0) {
            for (a, b) in l.iter_mut().zip(r.iter_mut()) {
                let g = self.output.next();
                *a *= g;
                *b *= g;
            }
        }
        for band in &mut self.bands {
            band.flush();
        }
    }

    /// The curve in dB at `f` for the settings given (not where they have glided to so far):
    /// the bands that are on and work on Both or Mid, plus the output gain. For the editor.
    pub fn response_db(&self, f: f32) -> f32 {
        let bands = self.bands.iter().map(|b| &b.target).filter(|t| t.on && t.place != Place::Side);
        (self.output_db as f64 + bands.map(|t| t.response_db(f)).sum::<f64>()) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Noise, sine};
    use crate::{BLOCK, SAMPLE_RATE};

    /// The Q of a plain 12 dB per octave Butterworth section.
    const BUTTERWORTH: f32 = std::f32::consts::FRAC_1_SQRT_2;

    fn band(shape: BandShape, freq: f32, gain_db: f32, q: f32, sections: u8) -> BandParams {
        BandParams { on: true, shape, freq, gain_db, q, sections, place: Place::Both }
    }

    fn with(list: &[BandParams]) -> [BandParams; 8] {
        let mut bands = [BandParams::default(); 8];
        bands[..list.len()].copy_from_slice(list);
        bands
    }

    fn run(eq: &mut Eq8, l: &mut [f32], r: &mut [f32]) {
        for (cl, cr) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
            eq.process(cl, cr);
        }
    }

    fn rms(v: &[f32]) -> f64 {
        (v.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    }

    /// The steady gain in dB of a sine at `hz` through `eq`, the same on both sides.
    fn sine_gain_db(eq: &mut Eq8, hz: f64) -> f32 {
        eq.reset();
        let fs = SAMPLE_RATE as f64;
        let settle = (0.4 * fs) as usize;
        let periods = (hz * 0.25).ceil().max(1.0);
        let len = (periods * fs / hz).round() as usize;
        let x = sine(hz, 0.5, 0.0, settle + len);
        let (mut l, mut r) = (x.clone(), x.clone());
        run(eq, &mut l, &mut r);
        (20.0 * (rms(&l[settle..]) / rms(&x[settle..])).log10()) as f32
    }

    /// The largest jump from one sample to the next.
    fn biggest_step(v: &[f32]) -> f32 {
        v.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn eq_curve_by_sine_sweep_matches_response() {
        use BandShape::*;
        let side_bell = BandParams { place: Place::Side, ..band(Bell, 700.0, 12.0, 1.0, 1) };
        let mid_bell = BandParams { place: Place::Mid, ..band(Bell, 12_000.0, 5.0, 0.5, 1) };
        let sets: [(Vec<BandParams>, f32); 4] = [
            (
                vec![
                    band(LowCut, 80.0, 0.0, BUTTERWORTH, 1),
                    band(Bell, 1_000.0, 6.0, 1.5, 1),
                    band(Notch, 3_000.0, 0.0, 2.0, 1),
                    band(HighShelf, 8_000.0, -4.0, BUTTERWORTH, 1),
                    band(LowShelf, 200.0, 3.0, 0.7, 1),
                ],
                2.0,
            ),
            (
                vec![
                    band(LowCut, 150.0, 0.0, BUTTERWORTH, 2),
                    band(HighCut, 6_000.0, 0.0, 1.0, 4),
                    band(Bell, 400.0, -9.0, 4.0, 1),
                ],
                0.0,
            ),
            (
                vec![
                    band(LowCut, 60.0, 0.0, 1.5, 4),
                    band(HighCut, 2_000.0, 0.0, 2.0, 1),
                    band(LowShelf, 100.0, -6.0, BUTTERWORTH, 1),
                    band(HighShelf, 4_000.0, 8.0, 1.2, 1),
                    mid_bell,
                    side_bell,
                ],
                -3.0,
            ),
            (vec![band(HighCut, 3_000.0, 0.0, BUTTERWORTH, 2), band(Bell, 90.0, 18.0, 18.0, 1)], 0.0),
        ];
        for (list, out) in sets {
            let mut eq = Eq8::new();
            eq.set(&with(&list));
            eq.set_output_db(out);
            for i in 0..30 {
                let hz = 30.0 * (18_000.0f64 / 30.0).powf(i as f64 / 29.0);
                let want = eq.response_db(hz as f32);
                let got = sine_gain_db(&mut eq, hz);
                if want > -60.0 {
                    assert!((got - want).abs() < 0.3, "{list:?} at {hz:.1} Hz: got {got:.3}, want {want:.3}");
                } else {
                    assert!(got < -50.0, "{list:?} at {hz:.1} Hz: got {got:.3}, want {want:.3}");
                }
            }
        }
    }

    #[test]
    fn cuts_have_their_slopes() {
        // A Butterworth cut is 3 dB down at its frequency and, well past it, falls 12 dB an octave per section.
        for (sections, slope) in [(1u8, 12.0f32), (2, 24.0), (4, 48.0)] {
            let lo = band(BandShape::LowCut, 1_000.0, 0.0, BUTTERWORTH, sections);
            let hi = band(BandShape::HighCut, 1_000.0, 0.0, BUTTERWORTH, sections);
            let at_cut = band_response_db(&lo, 1_000.0);
            assert!((at_cut + 3.0103).abs() < 0.05, "{sections}: {at_cut}");
            let d = band_response_db(&lo, 100.0) - band_response_db(&lo, 50.0);
            assert!((d - slope).abs() < 0.5, "low cut {sections}: {d}");
            let d = band_response_db(&hi, 2_000.0) - band_response_db(&hi, 4_000.0);
            assert!((d - slope).abs() < 1.5, "high cut {sections}: {d}");
        }
        assert_eq!(band_response_db(&BandParams::default(), 1_000.0), 0.0);
    }

    #[test]
    fn mid_side_bands_affect_only_their_part() {
        let s = sine(1_000.0, 0.5, 0.0, 9_600);
        let neg: Vec<f32> = s.iter().map(|v| -v).collect();
        for place in [Place::Mid, Place::Side] {
            let mut eq = Eq8::new();
            eq.set(&with(&[BandParams { place, ..band(BandShape::Bell, 1_000.0, 12.0, 1.0, 1) }]));
            eq.reset();
            // The part the band does not work on goes through untouched.
            let (mut l, mut r) = if place == Place::Mid { (s.clone(), neg.clone()) } else { (s.clone(), s.clone()) };
            let (l0, r0) = (l.clone(), r.clone());
            run(&mut eq, &mut l, &mut r);
            let diff = l.iter().zip(&l0).chain(r.iter().zip(&r0)).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(diff < 1e-6, "{place:?} touched the other part: {diff}");
            // Its own part is boosted.
            eq.reset();
            let (mut l, mut r) = if place == Place::Mid { (s.clone(), s.clone()) } else { (s.clone(), neg.clone()) };
            run(&mut eq, &mut l, &mut r);
            let gain = 20.0 * (rms(&l[4_800..]) / rms(&s[4_800..])).log10();
            assert!((gain - 12.0).abs() < 0.3, "{place:?} own part: {gain}");
            assert!((rms(&r[4_800..]) - rms(&l[4_800..])).abs() < 1e-4);
        }
    }

    #[test]
    fn bell_at_zero_db_is_transparent() {
        let mut eq = Eq8::new();
        let mut list = Vec::new();
        for (i, hz) in [40.0, 120.0, 500.0, 1_500.0, 4_000.0, 9_000.0, 15_000.0, 19_000.0].into_iter().enumerate() {
            let place = [Place::Both, Place::Mid, Place::Side][i % 3];
            list.push(BandParams { place, ..band(BandShape::Bell, hz, 0.0, 0.1 + i as f32 * 2.5, 1) });
        }
        eq.set(&with(&list));
        eq.reset();
        let mut noise = Noise(7);
        let mut l: Vec<f32> = (0..9_600).map(|_| noise.next()).collect();
        let mut r: Vec<f32> = (0..9_600).map(|_| noise.next()).collect();
        let (l0, r0) = (l.clone(), r.clone());
        run(&mut eq, &mut l, &mut r);
        let diff = l.iter().zip(&l0).chain(r.iter().zip(&r0)).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(diff < 1e-6, "{diff}");
        for hz in [20.0, 100.0, 1_000.0, 10_000.0, 20_000.0] {
            assert!(eq.response_db(hz).abs() < 1e-6);
        }
    }

    #[test]
    fn fast_freq_changes_do_not_click() {
        let x = sine(1_000.0, 0.25, 0.0, 48_000 * 2);
        let (mut l, mut r) = (x.clone(), x.clone());
        let mut eq = Eq8::new();
        let blocks = x.len() / BLOCK;
        let mut bell = band(BandShape::Bell, 100.0, 12.0, 2.0, 1);
        eq.set(&with(&[bell]));
        eq.reset();
        for (b, (cl, cr)) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).enumerate() {
            // Up from 100 Hz to 10 kHz over a second, then a jump straight back to 100 Hz.
            let t = b as f32 / (blocks / 2) as f32;
            bell.freq = if t < 1.0 { 100.0 * 100f32.powf(t) } else { 100.0 };
            eq.set(&with(&[bell]));
            eq.process(cl, cr);
        }
        let bound = biggest_step(&x) * 4.0 * 1.5;
        let step = biggest_step(&l);
        assert!(step < bound, "a jump of {step} (bound {bound})");
        // Gain jumping from one end to the other glides too.
        let (mut l, mut r) = (x.clone(), x.clone());
        for (b, (cl, cr)) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).enumerate() {
            let gain_db = if (b / 50) % 2 == 0 { -18.0 } else { 18.0 };
            eq.set(&with(&[band(BandShape::Bell, 1_000.0, gain_db, 1.0, 1)]));
            eq.process(cl, cr);
        }
        let bound = biggest_step(&x) * 8.0 * 1.5;
        let step = biggest_step(&l);
        assert!(step < bound, "a jump of {step} (bound {bound})");
    }

    #[test]
    fn turning_bands_on_and_off_does_not_click() {
        let x = sine(100.0, 0.5, 0.0, 48_000 * 2);
        let (mut l, mut r) = (x.clone(), x.clone());
        let mut eq = Eq8::new();
        for (b, (cl, cr)) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).enumerate() {
            let on = (b / 20) % 2 == 1;
            let shape = if (b / 40) % 2 == 0 { BandShape::HighCut } else { BandShape::Notch };
            let place = if (b / 80) % 2 == 0 { Place::Both } else { Place::Mid };
            eq.set(&with(&[BandParams { on, place, ..band(shape, 100.0, 0.0, BUTTERWORTH, 4) }]));
            eq.process(cl, cr);
        }
        let bound = biggest_step(&x) * 2.0;
        let step = biggest_step(&l).max(biggest_step(&r));
        assert!(step < bound, "a jump of {step} (bound {bound})");
        // It did turn on: some of the sound was cut.
        assert!(rms(&l) < rms(&x) * 0.9);
    }

    #[test]
    fn stays_finite() {
        use BandShape::*;
        let mut eq = Eq8::new();
        let mut rnd = Noise(99);
        let mut u = move || rnd.next() * 0.5 + 0.5;
        let mut noise = Noise(3);
        let mut l = [0.0f32; BLOCK];
        let mut r = [0.0f32; BLOCK];
        for _ in 0..3_000 {
            let mut bands = [BandParams::default(); 8];
            for b in &mut bands {
                *b = BandParams {
                    on: u() > 0.3,
                    shape: [LowCut, LowShelf, Bell, Notch, HighShelf, HighCut][(u() * 5.99) as usize],
                    freq: 20.0 * 1_000f32.powf(u()),
                    gain_db: u() * 36.0 - 18.0,
                    q: 0.1 * 180f32.powf(u()),
                    sections: [1, 2, 4][(u() * 2.99) as usize],
                    place: [Place::Both, Place::Mid, Place::Side][(u() * 2.99) as usize],
                };
            }
            eq.set(&bands);
            eq.set_output_db(u() * 36.0 - 18.0);
            for (a, b) in l.iter_mut().zip(&mut r) {
                *a = noise.next();
                *b = noise.next();
            }
            eq.process(&mut l, &mut r);
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 1e4), "{l:?}");
            assert!(eq.response_db(1_000.0).is_finite());
        }
    }
}
