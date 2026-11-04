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
