//! Per-instance colour variation: each ball (or other instance) turns its hue, saturation and
//! brightness by its own amount, fixed or wobbling slowly over time. Every value here is a pure
//! function of (seed, time), so scrubbing backwards gives the same colours as playing forwards.
//! The hashing uses only 32-bit integer steps so a shader can do the same.

use super::Paint;
use bpe_core::ThingId;
pub use bpe_gfx::NO_VARY;

/// One step of the PCG hash: a well mixed 32-bit value for `x`.
fn pcg(x: u32) -> u32 {
    let state = x.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277_803_737);
    (word >> 22) ^ word
}

/// A stable seed for instance `index` of `thing`. Different for every index of one thing.
pub fn seed_of(thing: ThingId, index: usize) -> u32 {
    // `index` goes through the hash on its own first, so neighbouring indices land far apart,
    // and the outer hash of a bijective mix keeps them distinct for one thing.
    pcg(pcg(thing.0).wrapping_add(0x9e37_79b9) ^ pcg(index as u32))
}

/// A value in -1 to 1 for (seed, which of the three values, lattice point).
fn lattice(seed: u32, k: u32, n: u32) -> f32 {
    let h = pcg(seed ^ pcg(k.wrapping_mul(0x9e37_79b9) ^ pcg(n)));
    (h >> 8) as f32 / 16_777_215.0 * 2.0 - 1.0
}

/// Value `k` (0, 1 or 2) of an instance, -1 to 1: fixed when `wobble` is 0, else smooth noise
/// over `time * wobble`, offset per instance so they don't all turn together.
fn value(seed: u32, k: u32, time: f32, wobble: f32) -> f32 {
    if wobble <= 0.0 || !wobble.is_finite() || !time.is_finite() {
        return lattice(seed, k, 0);
    }
    let phase = (lattice(seed, k + 3, 0) + 1.0) * 0.5;
    let x = time * wobble + phase;
    let n = x.floor();
    let f = x - n;
    let n = n as i32 as u32;
    let s = f * f * (3.0 - 2.0 * f);
    let (a, b) = (lattice(seed, k, n), lattice(seed, k, n.wrapping_add(1)));
    a + (b - a) * s
}

/// `x` if it is a real number, else 0.
fn finite(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// The variation for instance `seed` at simulation time `time`: `[hue turn in degrees,
/// saturation added, brightness multiplier]`. `None` when the paint varies nothing.
pub fn vary_offsets(p: &Paint, seed: u32, time: f32) -> Option<[f32; 3]> {
    let (hue, sat, bright) = (finite(p.vary_hue), finite(p.vary_sat), finite(p.vary_bright));
    if hue == 0.0 && sat == 0.0 && bright == 0.0 {
        return None;
    }
    let wobble = finite(p.wobble);
    let r = |k| value(seed, k, time, wobble);
    Some([hue * r(0), sat * r(1), (1.0 + bright * r(2)).max(0.0)])
}

/// [`vary_offsets`] as a shader reads it (`PaintGpu::vary`): the hue turn in turns rather than
/// degrees, the saturation added, the brightness multiplier, and 0. [`NO_VARY`] when the paint
/// varies nothing.
pub fn vary_gpu(p: &Paint, seed: u32, time: f32) -> [f32; 4] {
    vary_offsets(p, seed, time).map_or(NO_VARY, |[hue, sat, bright]| [hue / 360.0, sat, bright, 0.0])
}

/// Linear RGB to hue (0 to 1), saturation and value. Value may be above 1 (HDR).
fn to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let delta = max - r.min(g).min(b);
    let hue = if delta <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    (hue, if max <= 0.0 { 0.0 } else { delta / max }, max)
}

/// Hue, saturation and value back to linear RGB. Value is not clamped, so HDR stays HDR.
fn from_hsv(hue: f32, s: f32, v: f32) -> [f32; 3] {
    let h = hue.rem_euclid(1.0) * 6.0;
    // A hue a hair below 0 wraps to a hair below 1, which can round to exactly 1: red, not the
    // sector after magenta.
    let h = if h >= 6.0 { 0.0 } else { h };
    let f = h - h.floor();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match h as u32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// `rgba` (linear light) turned by `v` from [`vary_offsets`]. Alpha is untouched.
pub fn apply_vary(rgba: [f32; 4], v: [f32; 3]) -> [f32; 4] {
    let [r, g, b, a] = rgba.map(finite);
    let (h, s, val) = to_hsv(r.max(0.0), g.max(0.0), b.max(0.0));
    let h = h + finite(v[0]) / 360.0;
    let s = (s + finite(v[1])).clamp(0.0, 1.0);
    let val = val * finite(v[2]).max(0.0);
    let [r, g, b] = from_hsv(h, s, val).map(|c| finite(c).max(0.0));
    [r, g, b, a]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::{Follow, Inputs, colour_at};
    use bpe_core::{Color, Gradient};

    fn varied(hue: f32, sat: f32, bright: f32, wobble: f32) -> Paint {
        Paint { vary_hue: hue, vary_sat: sat, vary_bright: bright, wobble, ..Paint::solid(Color::rgb(200, 60, 30)) }
    }

    #[test]
    fn zero_variation_is_an_exact_no_op() {
        let paints = [
            Paint::solid(Color::rgb(10, 200, 90)),
            Paint { colours: Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]), ..Paint::default() },
            Paint { follow: Follow::Time, scroll: 0.3, ..Paint::solid(Color::additive(255, 128, 0)) },
            Paint { wobble: 3.0, ..Paint::default() },
        ];
        for p in &paints {
            assert_eq!(vary_offsets(p, 1234, 2.5), None);
            for seed in [0, 1, 99_999] {
                let i = Inputs { seed, time: 1.7, place: glam::Vec2::new(400.0, 300.0), ..Inputs::default() };
                let plain = Inputs { seed: 0, ..i };
                let got = colour_at(p, &i);
                let want = p.colours.sample(crate::paint::offset(p, &plain));
                assert_eq!(got.map(f32::to_bits), want.map(f32::to_bits));
            }
        }
    }

    #[test]
    fn different_seeds_give_different_hues_within_the_range() {
        let p = varied(30.0, 0.0, 0.0, 0.0);
        let mut seen = Vec::new();
        for i in 0..50 {
            let v = vary_offsets(&p, seed_of(ThingId(7), i), 0.0).unwrap();
            assert!((-30.0..=30.0).contains(&v[0]), "{v:?}");
            assert_eq!(v[1], 0.0);
            assert_eq!(v[2], 1.0);
            if !seen.iter().any(|s: &f32| (s - v[0]).abs() < 1e-4) {
                seen.push(v[0]);
            }
        }
        assert!(seen.len() >= 40, "only {} distinct hues", seen.len());
    }

    #[test]
    fn neighbouring_instances_have_different_seeds() {
        for t in 0..200 {
            assert_ne!(seed_of(ThingId(t), 0), seed_of(ThingId(t), 1));
        }
    }

    #[test]
    fn fixed_offsets_do_not_change_with_time_when_wobble_is_zero() {
        let p = varied(40.0, 0.5, 0.5, 0.0);
        let s = seed_of(ThingId(1), 3);
        let first = vary_offsets(&p, s, 0.0).unwrap();
        for t in [0.5, 1.0, 17.25, 300.0, -2.0] {
            assert_eq!(vary_offsets(&p, s, t), Some(first));
        }
    }

    #[test]
    fn wobble_changes_smoothly_and_is_a_pure_function_of_time() {
        let p = varied(180.0, 1.0, 1.0, 5.0);
        let range = [360.0, 2.0, 2.0];
        let mut moved = false;
        for i in 0..20 {
            let s = seed_of(ThingId(2), i);
            for k in 0..400 {
                let t = k as f32 * 0.037;
                let (a, b) = (vary_offsets(&p, s, t).unwrap(), vary_offsets(&p, s, t + 0.001).unwrap());
                for c in 0..3 {
                    assert!((a[c] - b[c]).abs() < 0.05 * range[c], "seed {s} t {t}: {a:?} vs {b:?}");
                }
                moved |= a != vary_offsets(&p, s, t + 0.5).unwrap();
            }
            let at5 = vary_offsets(&p, s, 5.0);
            let _ = vary_offsets(&p, s, 1.0);
            assert_eq!(vary_offsets(&p, s, 5.0), at5);
        }
        assert!(moved, "wobble moves the colours over time");
        // colour_at too: same inputs, same colour, in any order.
        let i = |time| Inputs { seed: 77, time, ..Inputs::default() };
        let c5 = colour_at(&p, &i(5.0));
        let c1 = colour_at(&p, &i(1.0));
        assert_ne!(c5, c1);
        assert_eq!(colour_at(&p, &i(5.0)), c5);
    }

    #[test]
    fn saturation_and_brightness_are_clamped() {
        let colours = [[0.8, 0.1, 0.05, 1.0], [0.0, 0.0, 0.0, 1.0], [3.0, 1.5, 0.2, 0.0], [0.5, 0.5, 0.5, 0.3]];
        for c in colours {
            for v in [[0.0, 1.0, 1.0], [90.0, -1.0, 0.0], [-170.0, 2.0, 3.0], [45.0, 0.5, -1.0]] {
                let out = apply_vary(c, v);
                assert!(out.iter().all(|x| x.is_finite() && *x >= 0.0), "{c:?} {v:?} -> {out:?}");
                assert_eq!(out[3], c[3], "alpha untouched");
                let (_, s, _) = to_hsv(out[0], out[1], out[2]);
                assert!((0.0..=1.0).contains(&s));
            }
        }
        // Full desaturation gives grey; negative brightness gives black, never below.
        let grey = apply_vary([0.8, 0.1, 0.05, 1.0], [0.0, -1.0, 1.0]);
        assert!((grey[0] - grey[1]).abs() < 1e-6 && (grey[1] - grey[2]).abs() < 1e-6);
        assert_eq!(apply_vary([0.8, 0.1, 0.05, 1.0], [0.0, 0.0, 0.0]), [0.0, 0.0, 0.0, 1.0]);
        // Through vary_offsets, brightness multipliers are never negative.
        let p = varied(0.0, 1.0, 1.0, 0.0);
        for i in 0..100 {
            let v = vary_offsets(&p, seed_of(ThingId(4), i), 0.0).unwrap();
            assert!(v[2] >= 0.0 && (-1.0..=1.0).contains(&v[1]));
        }
    }

    #[test]
    fn hdr_values_keep_their_scale() {
        let c = [4.0, 2.0, 1.0, 1.0];
        let same = apply_vary(c, [0.0, 0.0, 1.0]);
        for k in 0..3 {
            assert!((same[k] - c[k]).abs() < 1e-5, "{same:?}");
        }
        let turned = apply_vary(c, [120.0, 0.0, 1.0]);
        assert!((turned.iter().take(3).cloned().fold(0.0, f32::max) - 4.0).abs() < 1e-5, "{turned:?}");
        assert!((turned[1] - 4.0).abs() < 1e-5, "red turned a third round is green: {turned:?}");
    }

    #[test]
    fn a_tiny_turn_back_from_red_stays_red() {
        // -1e-5 degrees wraps to a hue a hair below 1, which rounds to exactly 1 in f32.
        for c in [[1.0, 0.0, 0.0, 1.0], [0.8, 0.1, 0.1, 1.0]] {
            let out = apply_vary(c, [-1e-5, 0.0, 1.0]);
            for k in 0..3 {
                assert!((out[k] - c[k]).abs() < 1e-4, "{c:?} turned to {out:?}");
            }
        }
    }

    #[test]
    fn the_gpu_gets_the_same_variation_in_turns() {
        let p = varied(90.0, 0.5, 0.5, 0.0);
        let s = seed_of(ThingId(3), 4);
        let v = vary_offsets(&p, s, 1.0).unwrap();
        assert_eq!(vary_gpu(&p, s, 1.0), [v[0] / 360.0, v[1], v[2], 0.0]);
        assert_eq!(vary_gpu(&Paint::default(), s, 1.0), NO_VARY);
    }
}
