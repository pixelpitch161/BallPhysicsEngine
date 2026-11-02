//! The chip shapes in plain Rust: a line-for-line twin of the signed distances in
//! `canvas_stamp.wgsl`, so the simulation's grid can take away exactly the shape the picture does.
//! A negative distance is inside the shape. `q` is in the shape's own turned frame.

use glam::Vec2;

const TAU: f32 = 6.283185307;

/// A well-mixed number from 0 to 1 for a variant, an index and a salt (PCG).
fn hash(variant: u32, k: u32, salt: u32) -> f32 {
    let mut x = variant
        .wrapping_mul(747796405)
        .wrapping_add(k.wrapping_mul(2891336453))
        .wrapping_add(salt.wrapping_mul(277803737))
        .wrapping_add(1);
    x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    x = (x >> 22) ^ x;
    x as f32 / 4294967295.0
}

fn segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = (pa.dot(ba) / ba.dot(ba).max(1e-8)).clamp(0.0, 1.0);
    (pa - ba * h).length()
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

fn splat(q: Vec2, r: f32, variant: u32) -> f32 {
    let v = variant & 7;
    let squash = ((variant >> 3) & 3) as f32;
    let flat = 1.0 + squash / 6.0;
    let wide = 1.0 + squash * 0.12;
    let p = Vec2::new(q.x * flat, q.y / wide);
    let scale = 0.5 * (1.0 / flat + wide);
    let k = 0.25 * r;
    let mut d = 1e6_f32;
    let discs = 3 + (hash(v, 0, 11) * 2.999) as u32;
    for i in 0..discs {
        let a = hash(v, i, 12) * TAU;
        let off = hash(v, i, 13) * 0.25 * r;
        let radius = (0.4 + 0.15 * hash(v, i, 14)) * r;
        d = smin(d, (p - Vec2::new(a.cos(), a.sin()) * off).length() - radius, k);
    }
    let reach = (0.55 + 0.25 * hash(v, 0, 15)) * r;
    d = smin(d, segment(p, Vec2::new(-0.2 * r, 0.0), Vec2::new(reach, 0.0)) - 0.2 * r, k);
    let drops = 4 + (hash(v, 0, 16) * 5.999) as u32;
    for i in 0..drops {
        let a = (hash(v, i, 17) - 0.5) * 1.7453293;
        let far = 0.9 + 0.7 * hash(v, i, 18);
        let radius = (0.16 - 0.09 * (far - 0.9) / 0.7) * r * (0.8 + 0.4 * hash(v, i, 19));
        d = d.min((p - Vec2::new(a.cos(), a.sin()) * far * r).length() - radius);
    }
    d * scale
}

fn ring(q: Vec2, r: f32) -> f32 {
    (q.length() - 0.85 * r).abs() - 0.15 * r
}

fn star(q: Vec2, r: f32) -> f32 {
    let k1 = Vec2::new(0.809016994, -0.587785252);
    let k2 = Vec2::new(-k1.x, k1.y);
    let mut p = Vec2::new(q.x.abs(), -q.y);
    p -= 2.0 * k1.dot(p).max(0.0) * k1;
    p -= 2.0 * k2.dot(p).max(0.0) * k2;
    p.x = p.x.abs();
    p.y -= r;
    let ba = 0.45 * Vec2::new(-k1.y, k1.x) - Vec2::new(0.0, 1.0);
    let h = (p.dot(ba) / ba.dot(ba)).clamp(0.0, r);
    (p - ba * h).length() * (p.y * ba.x - p.x * ba.y).signum_wgsl()
}

/// WGSL `sign`: -1, 0 or 1 (Rust's `signum` gives 1 for zero).
trait SignWgsl {
    fn signum_wgsl(self) -> f32;
}

impl SignWgsl for f32 {
    fn signum_wgsl(self) -> f32 {
        if self > 0.0 {
            1.0
        } else if self < 0.0 {
            -1.0
        } else {
            0.0
        }
    }
}

fn crack(q: Vec2, r: f32, v: u32) -> f32 {
    let mut d = q.length() - 0.12 * r;
    for k in 0..5u32 {
        let a = TAU * k as f32 / 5.0 + (hash(v, k, 3) - 0.5) * 0.8;
        let reach = r * (0.6 + 0.4 * hash(v, k, 4));
        let along = Vec2::new(a.cos(), a.sin());
        let across = Vec2::new(-along.y, along.x);
        let p1 = along * reach / 3.0 + across * (hash(v, k, 5) - 0.5) * 0.25 * reach;
        let p2 = along * reach * 2.0 / 3.0 + across * (hash(v, k, 6) - 0.5) * 0.25 * reach;
        let p3 = along * reach;
        let w = 0.05 * r;
        d = d.min(segment(q, Vec2::ZERO, p1) - w);
        d = d.min(segment(q, p1, p2) - w * 0.7);
        d = d.min(segment(q, p2, p3) - w * 0.4);
    }
    d
}

fn burst(q: Vec2, r: f32) -> f32 {
    let sector = TAU / 12.0;
    let a = q.y.atan2(q.x);
    let folded = a - sector * (a / sector).round_ties_even();
    let p = q.length() * Vec2::new(folded.cos(), folded.sin());
    segment(p, Vec2::new(0.3 * r, 0.0), Vec2::new(r, 0.0)) - 0.04 * r
}

fn dots(q: Vec2, r: f32, v: u32) -> f32 {
    let mut d = 1e9_f32;
    for k in 0..6u32 {
        let a = TAU * hash(v, k, 7);
        let centre = Vec2::new(a.cos(), a.sin()) * r * (0.15 + 0.6 * hash(v, k, 8));
        d = d.min((q - centre).length() - r * (0.12 + 0.13 * hash(v, k, 9)));
    }
    d
}

/// The signed distance (canvas px, negative inside) from `q` to a chip of `shape` and radius `r`.
/// Shape 0 splat (variant bits 0-4), 1 ring, 2 star, 3 crack (variant & 7), 4 burst, 5 dots
/// (variant & 7); anything else is a disc. `q` is in the chip's own turned frame, the same one
/// the stamp shader computes.
pub fn chip_distance(shape: u32, q: Vec2, r: f32, variant: u32) -> f32 {
    match shape {
        0 => splat(q, r, variant & 31),
        1 => ring(q, r),
        2 => star(q, r),
        3 => crack(q, r, variant & 7),
        4 => burst(q, r),
        5 => dots(q, r, variant & 7),
        _ => q.length() - r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_the_formula() {
        // Worked out separately from the WGSL formula: variant 3, k 5, salt 12.
        assert_eq!(hash(3, 5, 12), 824183528_u32 as f32 / 4294967295.0);
        let h = hash(3, 5, 12);
        assert!((0.0..=1.0).contains(&h));
        assert_ne!(hash(3, 5, 12), hash(3, 5, 13));
    }

    #[test]
    fn ring_is_inside_on_the_line() {
        assert!(chip_distance(1, Vec2::new(0.85 * 60.0, 0.0), 60.0, 0) < 0.0);
        assert!(chip_distance(1, Vec2::ZERO, 60.0, 0) > 0.0);
    }

    #[test]
    fn splat_has_its_middle_inside() {
        for v in 0..32 {
            assert!(chip_distance(0, Vec2::ZERO, 50.0, v) < 0.0, "variant {v}");
        }
    }

    #[test]
    fn nothing_reaches_past_one_point_nine_r() {
        let r = 50.0;
        for shape in 0..6 {
            for variant in 0..32 {
                for k in 0..64 {
                    let a = TAU * k as f32 / 64.0;
                    let q = Vec2::new(a.cos(), a.sin()) * 1.9 * r;
                    assert!(chip_distance(shape, q, r, variant) > 0.0, "shape {shape} variant {variant} angle {k}");
                }
            }
        }
    }
}
