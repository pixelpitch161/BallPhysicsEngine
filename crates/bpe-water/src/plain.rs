//! The plain drawing (the all-drops trial): every drop a soft round blob, the blobs summed and cut
//! at a level, so drops that touch merge and nothing is streaked. On the processor, from drops
//! already read back, the same code whichever engine made them: what the two engines' pictures
//! differ by is their physics.

// (A picture for people to look at, never physics: the platform's own maths is fine here.)
#![allow(clippy::disallowed_methods)]

use glam::Vec2;

use crate::sim::drop::Drop;
use crate::solid::SolidField;

/// A blob's radius, in cells (drops at rest are half a cell apart).
const RADIUS: f32 = 0.9;
/// The level the summed blobs are cut at (a body of liquid sums to 1).
const LEVEL: f32 = 0.2;

/// How the plain drawing is asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plain {
    /// The world point at the picture's top left.
    pub lo: Vec2,
    /// Picture pixels a world pixel.
    pub scale: f32,
    pub width: u32,
    pub height: u32,
    /// How far off a solid's face the engine's drops rest, in cells (the grid core's half a cell,
    /// the all-drops engine's quarter): the liquid is drawn up to the face either way.
    pub rest: f32,
}

fn srgb(linear: f32) -> f32 {
    let l = linear.clamp(0.0, 1.0);
    if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }
}

/// The share of a blob's weight further than `q` radii to one side of its middle (`q` from -1 to
/// 1), by table.
fn side_table() -> Vec<f32> {
    const N: usize = 129;
    // The blob (1 - r^2)^2 summed along a chord at `s`: (1 - s^2)^2.5.
    let f = |s: f32| (1.0 - s * s).max(0.0).powf(2.5);
    let mut run = vec![0.0f32; N];
    let step = 2.0 / (N - 1) as f32;
    for k in (0..N - 1).rev() {
        let (a, b) = (-1.0 + k as f32 * step, -1.0 + (k + 1) as f32 * step);
        run[k] = run[k + 1] + 0.5 * (f(a) + f(b)) * step;
    }
    let all = run[0];
    run.iter().map(|r| r / all).collect()
}

/// The plain picture of `drops` on `field`'s solids: rgba8.
pub fn draw(drops: &[Drop], field: &SolidField, plain: &Plain) -> Vec<u8> {
    let (w, h) = (plain.width as usize, plain.height as usize);
    let l = &field.layout;
    let scale = plain.scale;
    // (A blob is never under a picture pixel wide; one made wider is made that much fainter.)
    let true_r = RADIUS * l.cell * scale;
    let r = true_r.max(1.0);
    let weight = 3.0 / (4.0 * std::f32::consts::PI * RADIUS * RADIUS) * (true_r / r) * (true_r / r);
    let mut sum = vec![0.0f32; w * h];
    let mut tint = vec![[0.0f32; 3]; w * h];
    for d in drops {
        let p = (l.to_world(Vec2::from(d.x)) - plain.lo) * scale;
        if !p.is_finite() {
            continue;
        }
        let c = d.colour.to_le_bytes();
        let c = [f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0];
        let (x0, x1) = ((p.x - r).floor().max(0.0) as usize, ((p.x + r).ceil() as i64).min(w as i64 - 1));
        let (y0, y1) = ((p.y - r).floor().max(0.0) as usize, ((p.y + r).ceil() as i64).min(h as i64 - 1));
        if x1 < 0 || y1 < 0 {
            continue;
        }
        for y in y0..=y1 as usize {
            for x in x0..=x1 as usize {
                let off = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - p;
                let q2 = off.length_squared() / (r * r);
                if q2 < 1.0 {
                    let k = weight * (1.0 - q2) * (1.0 - q2);
                    let at = y * w + x;
                    sum[at] += k;
                    tint[at][0] += k * c[0];
                    tint[at][1] += k * c[1];
                    tint[at][2] += k * c[2];
                }
            }
        }
    }
    let table = side_table();
    let side = |q: f32| {
        let t = ((q.clamp(-1.0, 1.0) + 1.0) * 0.5 * (table.len() - 1) as f32).round() as usize;
        table[t.min(table.len() - 1)]
    };
    let back = [0.012f32, 0.014, 0.02];
    let wall = [0.58f32, 0.58, 0.6];
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let at = y * w + x;
            let world = plain.lo + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / scale;
            let far = field.at(world);
            let mut colour = back;
            let mut f = sum[at];
            if f > 0.0 {
                // Near a face no drop's middle can be: what is missing of the blobs there is made up,
                // so liquid that rests on a face is drawn up to it.
                let missing = side((far - plain.rest * l.cell) / (RADIUS * l.cell).max(1.0 / scale));
                f /= (1.0 - missing).max(0.2);
                let a = 0.04;
                let alpha = ((f - (LEVEL - a)) / (2.0 * a)).clamp(0.0, 1.0);
                let alpha = alpha * alpha * (3.0 - 2.0 * alpha);
                if alpha > 0.0 {
                    let own = tint[at].map(|c| c / sum[at]);
                    // Lighter where the liquid is thin (its edge, a film, a lone drop).
                    let deep = ((f - LEVEL) / (1.0 - LEVEL)).clamp(0.0, 1.0);
                    for k in 0..3 {
                        let light = own[k] * 1.6 + 0.1;
                        let c = light + (own[k] - light) * deep;
                        colour[k] += (c - colour[k]) * alpha;
                    }
                }
            }
            let cover = (0.5 - far * scale).clamp(0.0, 1.0);
            for k in 0..3 {
                colour[k] += (wall[k] - colour[k]) * cover;
                out[at * 4 + k] = (srgb(colour[k]) * 255.0 + 0.5) as u8;
            }
            out[at * 4 + 3] = 255;
        }
    }
    out
}

/// Pictures side by side (each `width` x `height`, rgba8) with a dark line between them.
pub fn beside(pictures: &[Vec<u8>], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let gap = 4usize;
    let (w, h) = (width as usize, height as usize);
    let wide = w * pictures.len() + gap * pictures.len().saturating_sub(1);
    let mut out = vec![0u8; wide * h * 4];
    for px in out.chunks_exact_mut(4) {
        px[3] = 255;
    }
    for (k, picture) in pictures.iter().enumerate() {
        for y in 0..h {
            let to = (y * wide + k * (w + gap)) * 4;
            out[to..to + w * 4].copy_from_slice(&picture[y * w * 4..(y + 1) * w * 4]);
        }
    }
    (out, wide as u32, height)
}
