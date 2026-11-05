//! LFO waves, worked out in closed form at a phase: the sound engine and the picture's Follow
//! sound read the same numbers.

/// An LFO's wave.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LfoWave {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
    /// A new random value each cycle, held.
    Random,
    /// A random value each cycle, glided between with a cosine.
    SmoothRandom,
    /// A shape drawn by hand (see [`curve_at`]).
    Curve,
}

/// A well-mixed 64-bit hash (splitmix64's finaliser).
#[inline]
pub fn hash(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// A random number 0..1 for cycle `step` of the wave seeded `seed`.
#[inline]
pub fn random_at(seed: u64, step: i64) -> f32 {
    (hash(seed ^ hash(step as u64)) >> 40) as f32 / (1u64 << 24) as f32
}

/// The wave's value, 0..1, at `phase` cycles (any number; whole cycles repeat). Sine and
/// Triangle start in the middle going up, Saw starts at 0 going up, Square starts high. Random
/// and Smooth random take a new number each whole cycle from `seed`.
pub fn shape_at(wave: LfoWave, phase: f64, seed: u64) -> f32 {
    if !phase.is_finite() {
        return 0.0;
    }
    let step = phase.floor();
    let p = (phase - step) as f32;
    let step = step as i64;
    match wave {
        LfoWave::Sine => 0.5 + 0.5 * (std::f32::consts::TAU * p).sin(),
        LfoWave::Triangle => {
            let t = if p < 0.25 {
                p * 4.0
            } else if p < 0.75 {
                2.0 - p * 4.0
            } else {
                p * 4.0 - 4.0
            };
            0.5 + 0.5 * t
        }
        LfoWave::Saw => p,
        LfoWave::Square => {
            if p < 0.5 {
                1.0
            } else {
                0.0
            }
        }
        LfoWave::Random => random_at(seed, step),
        LfoWave::SmoothRandom => {
            let (a, b) = (random_at(seed, step), random_at(seed, step + 1));
            let w = 0.5 - 0.5 * (std::f32::consts::PI * p).cos();
            a + (b - a) * w
        }
        // Without its points a drawn shape sits in the middle (see [`wave_at`]).
        LfoWave::Curve => 0.5,
    }
}

/// The wave's value, 0..1, at `phase` cycles: a drawn shape reads `curve`, the others as
/// [`shape_at`].
pub fn wave_at(wave: LfoWave, curve: &[[f32; 3]], phase: f64, seed: u64) -> f32 {
    match wave {
        LfoWave::Curve => curve_at(curve, phase),
        w => shape_at(w, phase, seed),
    }
}

/// Where a wave played once comes to rest: its value just before `phase + 1` cycles (where it
/// started, one cycle on).
pub fn wave_end(wave: LfoWave, curve: &[[f32; 3]], phase: f64, seed: u64) -> f32 {
    match wave {
        LfoWave::Curve => {
            let p = (phase - phase.floor()) as f32;
            curve_inside(curve, if p <= 0.0 { 1.0 } else { p }, true)
        }
        w => shape_at(w, phase + 1.0 - 1e-9, seed),
    }
}

/// How far along a part of a drawn shape is, `t` (0..1) bowed by `bend` (-1..1): straight at
/// 0; above 0 it rises fast and levels off, below 0 it starts slow.
#[inline]
pub fn bend_at(t: f32, bend: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if bend.abs() < 1e-4 { t } else { t.powf(16f32.powf(-bend.clamp(-1.0, 1.0))) }
}

/// The bend that puts a part's middle `r` (0..1) of the way from its start to its end: what
/// dragging the part's middle dot sets.
pub fn bend_for_middle(r: f32) -> f32 {
    let r = r.clamp(1e-4, 1.0 - 1e-4);
    // 0.5^e = r, e = 16^-bend.
    let e = r.ln() / 0.5f32.ln();
    (-e.ln() / 16f32.ln()).clamp(-1.0, 1.0)
}

/// A drawn shape's height (0..1) at `phase` cycles (whole cycles repeat). `points` are
/// `[x, y, bend]` with x in order from 0 to 1; where two share an x the shape jumps, and the
/// height there is the one after the jump. No points read 0.5, one point its height.
pub fn curve_at(points: &[[f32; 3]], phase: f64) -> f32 {
    if !phase.is_finite() {
        return 0.0;
    }
    let p = (phase - phase.floor()) as f32;
    curve_inside(points, p, false)
}

/// The height at the very end of one cycle of a drawn shape, coming from the left (where a
/// shape played once comes to rest).
pub fn curve_end(points: &[[f32; 3]]) -> f32 {
    curve_inside(points, 1.0, true)
}

/// The height at `p` (0..=1). `left`: at a jump, the height before it.
fn curve_inside(points: &[[f32; 3]], p: f32, left: bool) -> f32 {
    match points {
        [] => 0.5,
        [only] => only[1],
        _ => {
            // The part p falls in: the last point at or before it (or strictly before it, from
            // the left), then the next.
            let i = if left {
                points.iter().rposition(|q| q[0] < p).unwrap_or(0)
            } else {
                points.iter().rposition(|q| q[0] <= p).unwrap_or(0)
            };
            let a = points[i];
            let Some(b) = points.get(i + 1) else { return a[1] };
            let w = b[0] - a[0];
            if w <= 0.0 {
                return b[1];
            }
            a[1] + (b[1] - a[1]) * bend_at((p - a[0]) / w, a[2])
        }
    }
}

/// How much of the wave shows `since` seconds after it started: nothing during `delay`, then
/// rising in a straight line over `fade_in`, then all of it.
pub fn fade_at(since: f64, delay: f64, fade_in: f64) -> f32 {
    let t = since - delay.max(0.0);
    if t < 0.0 {
        0.0
    } else if fade_in <= 0.0 {
        1.0
    } else {
        (t / fade_in).min(1.0) as f32
    }
}

/// The one-pole coefficient that follows a change over `seconds`, for steps `dt` long (1 is
/// no smoothing).
pub fn smooth_coefficient(seconds: f64, dt: f64) -> f32 {
    if seconds <= 0.0 { 1.0 } else { (1.0 - (-dt / seconds).exp()) as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawn_shape_reads_its_parts() {
        let tri = [[0.0, 0.0, 0.0], [0.5, 1.0, 0.0], [1.0, 0.0, 0.0]];
        assert!((curve_at(&tri, 0.25) - 0.5).abs() < 1e-6);
        assert!((curve_at(&tri, 0.5) - 1.0).abs() < 1e-6);
        assert!((curve_at(&tri, 1.75) - 0.5).abs() < 1e-6, "whole cycles repeat");
        assert!((curve_at(&tri, -0.25) - 0.5).abs() < 1e-6);
        assert_eq!(curve_end(&tri), 0.0);
        // A jump reads the height after it, and from the left the one before.
        let square = [[0.0, 1.0, 0.0], [0.5, 1.0, 0.0], [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]];
        assert_eq!(curve_at(&square, 0.5), 0.0);
        assert_eq!(curve_at(&square, 0.49), 1.0);
        assert_eq!(curve_at(&square, 0.0), 1.0);
        assert_eq!(curve_end(&square), 0.0);
        assert_eq!(curve_at(&[], 0.3), 0.5);
        assert_eq!(wave_end(LfoWave::Curve, &square, 0.0, 0), 0.0);
        assert_eq!(wave_end(LfoWave::Curve, &square, 0.5, 0), 1.0, "from halfway it ends just before the drop");
        assert_eq!(wave_at(LfoWave::Curve, &tri, 0.5, 0), 1.0);
        assert_eq!(curve_at(&[[0.0, 0.2, 0.0]], 0.3), 0.2);
    }

    #[test]
    fn a_bend_bows_the_part_and_its_middle_dot_finds_it_again() {
        assert_eq!(bend_at(0.5, 0.0), 0.5);
        assert!(bend_at(0.5, 0.5) > 0.5, "bowed up");
        assert!(bend_at(0.5, -0.5) < 0.5);
        assert_eq!(bend_at(0.0, 0.7), 0.0);
        assert_eq!(bend_at(1.0, -0.7), 1.0);
        for b in [-0.9, -0.3, 0.0, 0.25, 0.8] {
            let back = bend_for_middle(bend_at(0.5, b));
            assert!((back - b).abs() < 1e-3, "{b} -> {back}");
        }
    }

    #[test]
    fn waves_start_where_they_say() {
        assert!((shape_at(LfoWave::Sine, 0.0, 0) - 0.5).abs() < 1e-6);
        assert!((shape_at(LfoWave::Sine, 0.25, 0) - 1.0).abs() < 1e-6);
        assert!((shape_at(LfoWave::Triangle, 0.25, 0) - 1.0).abs() < 1e-6);
        assert!((shape_at(LfoWave::Triangle, 0.75, 0)).abs() < 1e-6);
        assert!((shape_at(LfoWave::Saw, 0.5, 0) - 0.5).abs() < 1e-6);
        assert_eq!(shape_at(LfoWave::Square, 0.1, 0), 1.0);
        assert_eq!(shape_at(LfoWave::Square, 0.6, 0), 0.0);
        assert_eq!(shape_at(LfoWave::Saw, 3.5, 0), shape_at(LfoWave::Saw, 0.5, 0));
    }

    #[test]
    fn random_holds_a_cycle_and_smooth_random_glides_between() {
        let a = shape_at(LfoWave::Random, 2.1, 7);
        assert_eq!(a, shape_at(LfoWave::Random, 2.9, 7), "held for the cycle");
        assert_ne!(a, shape_at(LfoWave::Random, 3.1, 7), "a new one next cycle");
        assert_eq!(shape_at(LfoWave::SmoothRandom, 2.0, 7), a, "starts on the cycle's number");
        let next = shape_at(LfoWave::Random, 3.0, 7);
        assert!((shape_at(LfoWave::SmoothRandom, 2.999_999, 7) - next).abs() < 1e-3, "ends on the next");
        for i in 0..1000 {
            let v = shape_at(LfoWave::Random, i as f64, 1);
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn fade_waits_then_rises() {
        assert_eq!(fade_at(0.5, 1.0, 2.0), 0.0);
        assert!((fade_at(2.0, 1.0, 2.0) - 0.5).abs() < 1e-6);
        assert_eq!(fade_at(9.0, 1.0, 2.0), 1.0);
        assert_eq!(fade_at(0.0, 0.0, 0.0), 1.0);
    }
}
