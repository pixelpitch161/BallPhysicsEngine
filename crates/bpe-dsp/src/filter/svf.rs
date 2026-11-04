//! Andrew Simper's (Cytomic) trapezoidal state-variable filter. One structure gives every
//! second-order type: the output is a mix `m0·input + m1·band + m2·low` of its taps.

/// How a stage mixes its taps, and how it is tuned.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SvfCoefs {
    /// `tan(π·cutoff/rate)` (scaled for the shelves).
    pub g: f32,
    /// One over Q.
    pub k: f32,
    pub m0: f32,
    pub m1: f32,
    pub m2: f32,
}

/// The second-order types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvfMode {
    Low,
    High,
    /// Band pass with 0 dB at its peak.
    Band,
    Notch,
    Bell,
    LowShelf,
    HighShelf,
}

impl SvfCoefs {
    /// A stage of `mode` at `tan_w = tan(π·cutoff/rate)`, damping `k`, and `a = 10^(dB/40)` for
    /// the bell and shelves.
    pub fn new(mode: SvfMode, tan_w: f64, k: f64, a: f64) -> Self {
        let (g, k, m0, m1, m2) = match mode {
            SvfMode::Low => (tan_w, k, 0.0, 0.0, 1.0),
            SvfMode::High => (tan_w, k, 1.0, -k, -1.0),
            SvfMode::Band => (tan_w, k, 0.0, k, 0.0),
            SvfMode::Notch => (tan_w, k, 1.0, -k, 0.0),
            SvfMode::Bell => {
                let k = k / a;
                (tan_w, k, 1.0, k * (a * a - 1.0), 0.0)
            }
            SvfMode::LowShelf => (tan_w / a.sqrt(), k, 1.0, k * (a - 1.0), a * a - 1.0),
            SvfMode::HighShelf => (tan_w * a.sqrt(), k, a * a, k * (1.0 - a) * a, 1.0 - a * a),
        };
        SvfCoefs { g: g as f32, k: k as f32, m0: m0 as f32, m1: m1 as f32, m2: m2 as f32 }
    }

    /// The stage's response at `tan_f = tan(π·f/rate)`, as (re, im).
    pub fn response(&self, tan_f: f64) -> (f64, f64) {
        // The analog prototype at s = j·tan_f/g: low = 1/(s²+ks+1), band = s/(s²+ks+1).
        let w = tan_f / self.g as f64;
        let k = self.k as f64;
        // den = 1 - w² + j k w
        let (dr, di) = (1.0 - w * w, k * w);
        let d2 = dr * dr + di * di;
        // low = 1/den, band = j w / den
        let (lr, li) = (dr / d2, -di / d2);
        let (br, bi) = (w * di / d2, w * dr / d2);
        let (m0, m1, m2) = (self.m0 as f64, self.m1 as f64, self.m2 as f64);
        (m0 + m1 * br + m2 * lr, m1 * bi + m2 * li)
    }
}

/// Where a state starts to be soft-limited. Well above any level a sound reaches (+12 dB of Gain
/// on a full-scale wave is 4), so only a screaming resonance meets it, never a loud clean sound.
const KNEE: f32 = 8.0;

/// Soft-limits a state past [`KNEE`] towards twice it, so a screaming resonance stays bounded.
#[inline]
pub(crate) fn knee(x: f32) -> f32 {
    let a = x.abs();
    if a <= KNEE { x } else { x.signum() * (KNEE + KNEE * ((a - KNEE) / KNEE).tanh()) }
}

/// Zero for a value too small to hear (no denormals where flushing is off).
#[inline]
pub(crate) fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// One stage's memory, one channel.
#[derive(Clone, Copy, Debug, Default)]
pub struct SvfState {
    ic1: f32,
    ic2: f32,
}

impl SvfState {
    pub fn reset(&mut self) {
        *self = SvfState::default();
    }

    #[inline]
    pub fn tick(&mut self, c: &SvfCoefs, v0: f32) -> f32 {
        let a1 = 1.0 / (1.0 + c.g * (c.g + c.k));
        let a2 = c.g * a1;
        let a3 = c.g * a2;
        let v3 = v0 - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = flush(knee(2.0 * v1 - self.ic1));
        self.ic2 = flush(knee(2.0 * v2 - self.ic2));
        c.m0 * v0 + c.m1 * v1 + c.m2 * v2
    }
}
