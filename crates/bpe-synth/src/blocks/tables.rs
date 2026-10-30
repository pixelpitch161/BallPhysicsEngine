//! Band-limited wave tables for the oscillator. Each wave has one table per half octave, holding
//! only the harmonics that stay under half the sample rate at the top of that half octave, so
//! saw, square and pulse don't alias.

use std::sync::OnceLock;

pub const SIZE: usize = 4096;
const BANDS: usize = 24;
/// The phase step (cycles per frame) at the top of the lowest band: 20 Hz at 48 kHz.
const LOWEST: f64 = 20.0 / 48_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Saw,
    Square,
    Triangle,
}

pub struct Tables {
    /// [shape][band], each SIZE + 1 samples (the last repeats the first).
    tables: [Vec<Vec<f32>>; 3],
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| Tables { tables: [build(Shape::Saw), build(Shape::Square), build(Shape::Triangle)] })
}

/// The table to use for a phase step of `inc` cycles per frame: the first band whose top is at
/// or above it, the bands' tops rising by half an octave from [`LOWEST`].
fn band(inc: f32) -> usize {
    if inc as f64 <= LOWEST {
        return 0;
    }
    let b = (2.0 * (inc as f64 / LOWEST).log2()).ceil() as usize;
    b.min(BANDS - 1)
}

/// The table for `shape` at a phase step of `inc` cycles per frame, to [`read`] from.
#[inline]
pub fn table(shape: Shape, inc: f32) -> &'static [f32] {
    &tables().tables[shape as usize][band(inc.abs())]
}

/// A table's wave at `phase` (0 to 1).
#[inline]
pub fn read(t: &[f32], phase: f32) -> f32 {
    let x = phase.rem_euclid(1.0) * SIZE as f32;
    let i = (x as usize).min(SIZE - 1);
    let f = x - i as f32;
    t[i] + (t[i + 1] - t[i]) * f
}

/// The wave at `phase` (0 to 1) for an oscillator stepping `inc` cycles per frame.
#[inline]
pub fn lookup(shape: Shape, inc: f32, phase: f32) -> f32 {
    read(table(shape, inc), phase)
}

fn build(shape: Shape) -> Vec<Vec<f32>> {
    let mut top = LOWEST;
    (0..BANDS)
        .map(|_| {
            let harmonics = ((0.5 / top) as usize).clamp(1, SIZE / 2 - 1);
            top *= std::f64::consts::SQRT_2;
            // Sine amplitude of harmonic k.
            let amp = |k: usize| -> f64 {
                let k_f = k as f64;
                match shape {
                    Shape::Saw => -2.0 / (std::f64::consts::PI * k_f),
                    Shape::Square if k % 2 == 1 => 4.0 / (std::f64::consts::PI * k_f),
                    Shape::Triangle if k % 2 == 1 => {
                        let sign = if (k / 2).is_multiple_of(2) { 1.0 } else { -1.0 };
                        sign * 8.0 / (std::f64::consts::PI * std::f64::consts::PI * k_f * k_f)
                    }
                    _ => 0.0,
                }
            };
            let mut re = vec![0.0f64; SIZE];
            let mut im = vec![0.0f64; SIZE];
            for k in 1..=harmonics {
                // x[n] = sum a_k sin(2 pi k n / N) = sum over +-k of complex coefficients.
                let a = amp(k);
                im[k] = -a / 2.0;
                im[SIZE - k] = a / 2.0;
            }
            fft(&mut re, &mut im, true);
            let mut t: Vec<f32> = re.iter().map(|&v| v as f32).collect();
            t.push(t[0]);
            t
        })
        .collect()
}

/// In-place radix-2 FFT; `inverse` leaves out the 1/N, which the coefficients above already
/// account for.
fn fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
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
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (xr, xi) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_picks_the_first_top_at_or_above() {
        let mut top = LOWEST;
        for b in 0..BANDS {
            assert_eq!(band((top * 0.999) as f32), b, "just under band {b}'s top");
            top *= std::f64::consts::SQRT_2;
        }
        assert_eq!(band(0.0), 0);
    }

    #[test]
    fn a_low_saw_table_is_a_ramp() {
        // Half way through a cycle a rising saw crosses zero; a quarter in it is near -0.5.
        assert!(lookup(Shape::Saw, 0.0001, 0.5).abs() < 0.01);
        assert!((lookup(Shape::Saw, 0.0001, 0.25) + 0.5).abs() < 0.02);
        assert!((lookup(Shape::Square, 0.0001, 0.25) - 1.0).abs() < 0.05);
        assert!((lookup(Shape::Triangle, 0.0001, 0.25) - 1.0).abs() < 0.01);
    }
}
