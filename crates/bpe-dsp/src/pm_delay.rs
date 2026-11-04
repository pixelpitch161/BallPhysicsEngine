//! Modulation of the sound reaching a chain layer by another sound (the source). FM and Phase
//! distortion are phase modulation done as a short moving delay of at most 2 ms, read with
//! cubic Hermite interpolation: first in a chain, on a sine, that is exactly FM of the
//! oscillator. AM and Ring scale the sound by the source.

use crate::SAMPLE_RATE;

/// The longest the delay moves, in seconds.
pub const MAX_DELAY_S: f32 = 0.002;
/// Every read is at least this many samples back, so the interpolation has a sample ahead.
pub const BASE_DELAY: f32 = 1.0;
const LEN: usize = 128;
const _: () = assert!(LEN as f32 > MAX_DELAY_S * SAMPLE_RATE + BASE_DELAY + 4.0);

/// One channel's delay line.
#[derive(Clone, Copy, Debug)]
pub struct PmDelay {
    buf: [f32; LEN],
    pos: usize,
}

impl Default for PmDelay {
    fn default() -> Self {
        PmDelay { buf: [0.0; LEN], pos: 0 }
    }
}

/// The depth at Amount 100 %, in samples (1 ms).
fn depth(amount: f32) -> f32 {
    amount.clamp(0.0, 1.0) * 0.5 * MAX_DELAY_S * SAMPLE_RATE
}

impl PmDelay {
    pub fn reset(&mut self) {
        *self = PmDelay::default();
    }

    /// Writes this sample's input.
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.pos = (self.pos + 1) & (LEN - 1);
        self.buf[self.pos] = x;
    }

    /// The input `delay` samples ago (fractional; kept within the line).
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let d = if delay.is_finite() { delay.clamp(BASE_DELAY, (LEN - 3) as f32) } else { BASE_DELAY };
        let i = d as usize;
        let t = d - i as f32;
        let at = |k: usize| self.buf[(self.pos + LEN - k) & (LEN - 1)];
        // Points one newer, at, one and two older than `i` samples back.
        let (ym1, y0, y1, y2) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let c1 = 0.5 * (y1 - ym1);
        let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
        let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
        ((c3 * t + c2) * t + c1) * t + y0
    }

    /// FM: pushes `x` and reads it back moved by `m` (-1..1) at `amount` (0..1). Returns the
    /// moved sound and the sound at the delay's middle (for Mix, so dry and wet line up).
    #[inline]
    pub fn fm(&mut self, x: f32, m: f32, amount: f32) -> (f32, f32) {
        self.push(x);
        let d = depth(amount);
        let m = if m.is_finite() { m.clamp(-1.0, 1.0) } else { 0.0 };
        (self.read(BASE_DELAY + d + d * m), self.read(BASE_DELAY + d))
    }

    /// Phase distortion: as [`PmDelay::fm`] but one-sided, the source rectified so the wave is
    /// only ever squeezed one way (Casio CZ style): up to 2 ms at Amount 100 %.
    #[inline]
    pub fn pd(&mut self, x: f32, m: f32, amount: f32) -> (f32, f32) {
        self.push(x);
        let d = 2.0 * depth(amount);
        let m = if m.is_finite() { m.clamp(0.0, 1.0) } else { 0.0 };
        // A rectified sine sits at 1/π on average.
        (self.read(BASE_DELAY + d * m), self.read(BASE_DELAY + d * std::f32::consts::FRAC_1_PI))
    }
}

/// AM: the source moves the level, from none (Amount 0) to all the way (the sound's level
/// kept at the top).
#[inline]
pub fn am(x: f32, m: f32, amount: f32) -> f32 {
    let a = amount.clamp(0.0, 1.0);
    x * (1.0 + a * m) / (1.0 + a)
}

/// Ring: the sound times the source, blended in by Amount.
#[inline]
pub fn ring(x: f32, m: f32, amount: f32) -> f32 {
    let a = amount.clamp(0.0, 1.0);
    x * (1.0 - a + a * m)
}

/// The FM index (peak phase swing, radians) Amount gives a carrier at `carrier_hz`, for the
/// editor's readout.
pub fn fm_index_of(amount: f32, carrier_hz: f32) -> f32 {
    std::f32::consts::TAU * carrier_hz * depth(amount) / SAMPLE_RATE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    /// The level at `hz` of `y` (a whole number of cycles long).
    fn level(y: &[f32], hz: f64) -> f64 {
        let (mut c, mut s) = (0.0, 0.0);
        for (i, v) in y.iter().enumerate() {
            let ph = TAU * hz * i as f64 / SAMPLE_RATE as f64;
            c += *v as f64 * ph.cos();
            s += *v as f64 * ph.sin();
        }
        2.0 * (c * c + s * s).sqrt() / y.len() as f64
    }

    /// The Bessel function of the first kind, order `n`, by its series.
    fn bessel(n: i32, x: f64) -> f64 {
        let mut sum = 0.0;
        let mut fact_k = 1.0;
        for k in 0..30 {
            if k > 0 {
                fact_k *= k as f64;
            }
            let fact_kn: f64 = (1..=(k + n)).map(|v| v as f64).product();
            sum += (-1f64).powi(k) / (fact_k * fact_kn) * (x / 2.0).powi(2 * k + n);
        }
        sum
    }

    #[test]
    fn fm_first_in_chain_matches_analytic_fm() {
        let (fc, fm) = (440.0, 110.0);
        for beta in [0.3f32, 0.8, 1.5] {
            let amount = beta / fm_index_of(1.0, fc as f32);
            let mut d = PmDelay::default();
            let n = 48_000;
            let y: Vec<f32> = (0..2 * n)
                .map(|i| {
                    let t = i as f64 / SAMPLE_RATE as f64;
                    let x = (TAU * fc * t).sin() as f32;
                    let m = (TAU * fm * t).sin() as f32;
                    d.fm(x, m, amount).0
                })
                .collect();
            let y = &y[n..];
            for k in 0..3 {
                let want = bessel(k, beta as f64).abs();
                for side in [-1.0, 1.0] {
                    let got = level(y, fc + side * k as f64 * fm);
                    let db = 20.0 * (got / want).log10();
                    assert!(db.abs() < 1.0, "index {beta}, sideband {k}·{side}: {got:.4} vs Bessel {want:.4}");
                    if k == 0 {
                        break;
                    }
                }
            }
        }
    }

    #[test]
    fn pd_is_one_sided() {
        // An impulse read back: with the source below zero, PD leaves the delay at its base,
        // where FM moves it earlier; above zero both move it later.
        let delay_of = |pd: bool, m: f32| {
            let mut d = PmDelay::default();
            let mut out = Vec::new();
            for i in 0..100 {
                let x = if i == 10 { 1.0 } else { 0.0 };
                out.push(if pd { d.pd(x, m, 1.0).0 } else { d.fm(x, m, 1.0).0 });
            }
            out.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0 as f32 - 10.0
        };
        assert_eq!(delay_of(true, -0.5), BASE_DELAY);
        assert_eq!(delay_of(true, -1.0), BASE_DELAY);
        assert_eq!(delay_of(true, 0.5), BASE_DELAY + 48.0);
        assert_eq!(delay_of(false, -0.5), BASE_DELAY + 24.0);
        assert_eq!(delay_of(false, 0.5), BASE_DELAY + 72.0);
    }

    #[test]
    fn am_and_ring_levels() {
        for a in [0.0, 0.5, 1.0] {
            assert_eq!(am(1.0, 1.0, a), 1.0, "AM keeps the top at the sound's level");
            assert!((am(1.0, -1.0, a) - (1.0 - a) / (1.0 + a)).abs() < 1e-6);
            assert_eq!(ring(0.5, 1.0, a), 0.5);
            assert!((ring(0.5, -1.0, a) - 0.5 * (1.0 - 2.0 * a)).abs() < 1e-6);
            assert!((ring(0.5, 0.0, a) - 0.5 * (1.0 - a)).abs() < 1e-6);
        }
        assert_eq!(ring(0.7, 0.3, 0.0), 0.7);
        assert_eq!(am(0.7, 0.3, 0.0), 0.7);
    }
}
