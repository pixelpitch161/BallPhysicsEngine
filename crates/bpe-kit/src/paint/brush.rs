//! Brush strokes as a picture: a Strokes value drawn into a small two-channel tile the renderer
//! reads: how painted each pixel is, and how far it is from the painted edge (so a brush Cut has
//! an edge for later sub-layers to follow). The same strokes always give the same bytes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use bpe_core::{Stroke, Strokes};

/// A tile's width and height, in pixels. It covers the ball's square, -1 to 1 both ways.
pub const TILE: usize = bpe_gfx::MASK_TILE;

/// How far `p` is from the segment `a`-`b`.
fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
    let (apx, apy) = (p[0] - a[0], p[1] - a[1]);
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0.0 { ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (dx, dy) = (apx - abx * t, apy - aby * t);
    (dx * dx + dy * dy).sqrt()
}

/// How far `p` is from the stroke's path (a single point is a dot).
fn stroke_distance(p: [f32; 2], s: &Stroke) -> f32 {
    match s.points.as_slice() {
        [] => f32::INFINITY,
        [one] => segment_distance(p, *one, *one),
        points => points.windows(2).map(|w| segment_distance(p, w[0], w[1])).fold(f32::INFINITY, f32::min),
    }
}

/// How much one stroke paints at distance `d` from its path: full inside the hard middle, fading
/// to nothing at its size.
fn coverage(s: &Stroke, d: f32) -> f32 {
    let size = s.size.max(0.0);
    let inner = size * (1.0 - s.softness.clamp(0.0, 1.0));
    let t = if size - inner > 1e-6 { ((size - d) / (size - inner)).clamp(0.0, 1.0) } else { f32::from(d < size) };
    t * t * (3.0 - 2.0 * t) * s.strength.clamp(0.0, 1.0)
}

/// Paints one stroke onto `level` (one value per tile pixel, 0 to 1).
fn apply(level: &mut [f32], s: &Stroke) {
    if s.points.is_empty() {
        return;
    }
    // Only the pixels the stroke can reach.
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for p in &s.points {
        for i in 0..2 {
            lo[i] = lo[i].min(p[i] - s.size);
            hi[i] = hi[i].max(p[i] + s.size);
        }
    }
    let px = |v: f32| (((v + 1.0) * 0.5 * TILE as f32).floor().max(0.0) as usize).min(TILE - 1);
    for y in px(lo[1])..=px(hi[1]) {
        for x in px(lo[0])..=px(hi[0]) {
            let q = [((x as f32 + 0.5) / TILE as f32) * 2.0 - 1.0, ((y as f32 + 0.5) / TILE as f32) * 2.0 - 1.0];
            let v = coverage(s, stroke_distance(q, s));
            let l = &mut level[y * TILE + x];
            *l = if s.erase { *l * (1.0 - v) } else { *l + v * (1.0 - *l) };
        }
    }
}

/// Ball radii per tile pixel: the tile covers two radii.
const RADII_PER_PIXEL: f32 = 2.0 / TILE as f32;

/// How far each pixel of `inside` is from the nearest pixel that isn't (in pixels, times 3): a
/// two-pass 3-4 chamfer distance. Every pixel is far when no pixel is outside.
fn chamfer(inside: &[bool]) -> Vec<u32> {
    const FAR: u32 = u32::MAX / 2;
    let mut d: Vec<u32> = inside.iter().map(|&i| if i { FAR } else { 0 }).collect();
    let n = TILE as i32;
    let at = |x: i32, y: i32| (y * n + x) as usize;
    let forward = [(-1, 0, 3), (-1, -1, 4), (0, -1, 3), (1, -1, 4)];
    for y in 0..n {
        for x in 0..n {
            for (dx, dy, w) in forward {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && nx < n && ny >= 0 {
                    d[at(x, y)] = d[at(x, y)].min(d[at(nx, ny)] + w);
                }
            }
        }
    }
    for y in (0..n).rev() {
        for x in (0..n).rev() {
            for (dx, dy, w) in forward {
                let (nx, ny) = (x - dx, y - dy);
                if nx >= 0 && nx < n && ny < n {
                    d[at(x, y)] = d[at(x, y)].min(d[at(nx, ny)] + w);
                }
            }
        }
    }
    d
}

/// `level` as tile bytes: per pixel how painted it is, then its distance to the painted edge (the
/// 0.5 level) in radii, positive inside, as [`bpe_gfx::tile_distance`] stores it.
fn bytes(level: &[f32]) -> Vec<u8> {
    let inside: Vec<bool> = level.iter().map(|&l| l >= 0.5).collect();
    let outside: Vec<bool> = inside.iter().map(|i| !i).collect();
    let (to_out, to_in) = (chamfer(&inside), chamfer(&outside));
    let mut out = Vec::with_capacity(2 * level.len());
    for (i, l) in level.iter().enumerate() {
        // The edge runs between pixels: half a pixel from the nearest one across it.
        let px = if inside[i] { to_out[i] as f32 / 3.0 - 0.5 } else { 0.5 - to_in[i] as f32 / 3.0 };
        out.push((l.clamp(0.0, 1.0) * 255.0).round() as u8);
        out.push(bpe_gfx::tile_distance(px * RADII_PER_PIXEL));
    }
    out
}

/// The strokes as a `TILE` × `TILE` tile, row by row from the top, two bytes a pixel: how
/// painted it is, 0 to 255, and its distance to the painted edge. Strokes paint in order;
/// erasing ones take paint away.
pub fn rasterise(strokes: &Strokes) -> Vec<u8> {
    let mut level = vec![0.0f32; TILE * TILE];
    for s in &strokes.list {
        apply(&mut level, s);
    }
    bytes(&level)
}

/// The strokes' paint levels, kept for the most recent few sets. Strokes that add one stroke to
/// a kept set start from it, so a stroke being painted only draws itself each frame; the
/// levels come out the same either way, as each stroke paints the same sums in the same order.
fn level_of(strokes: &Strokes, hash: u64) -> Arc<[f32]> {
    static LEVELS: OnceLock<Mutex<HashMap<u64, Arc<[f32]>>>> = OnceLock::new();
    let mut levels = LEVELS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(l) = levels.get(&hash) {
        return l.clone();
    }
    let (last, before) = match strokes.list.split_last() {
        Some((last, before)) => (Some(last), Strokes { list: before.to_vec() }),
        None => (None, Strokes::default()),
    };
    let start = levels.get(&before.hash64()).cloned();
    let mut level: Vec<f32> = match (start, last) {
        (Some(l), Some(_)) => l.to_vec(),
        _ => {
            let mut l = vec![0.0f32; TILE * TILE];
            for s in &before.list {
                apply(&mut l, s);
            }
            l
        }
    };
    if let Some(last) = last {
        apply(&mut level, last);
    }
    if levels.len() >= 32 {
        levels.clear();
    }
    let level: Arc<[f32]> = level.into();
    levels.insert(hash, level.clone());
    level
}

/// The strokes' tile and its hash, drawn once per distinct set of strokes and kept (the most
/// recent few hundred), so a frame only draws strokes that changed.
pub fn tile(strokes: &Strokes) -> (u64, Arc<[u8]>) {
    static MADE: OnceLock<Mutex<HashMap<u64, Arc<[u8]>>>> = OnceLock::new();
    let hash = strokes.hash64();
    if let Some(t) = MADE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).get(&hash) {
        return (hash, t.clone());
    }
    let t: Arc<[u8]> = bytes(&level_of(strokes, hash)).into();
    let mut made = MADE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if made.len() >= 512 {
        made.clear();
    }
    made.insert(hash, t.clone());
    (hash, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(erase: bool) -> Stroke {
        Stroke { points: vec![[-0.5, 0.0], [0.5, 0.0]], size: 0.1, softness: 0.5, strength: 1.0, erase }
    }

    /// The tile's paint at ball point `q`.
    fn at(tile: &[u8], q: [f32; 2]) -> u8 {
        tile[2 * pixel(q)]
    }

    /// The tile's distance byte at ball point `q`.
    fn dist(tile: &[u8], q: [f32; 2]) -> u8 {
        tile[2 * pixel(q) + 1]
    }

    fn pixel(q: [f32; 2]) -> usize {
        let px = |v: f32| ((v + 1.0) * 0.5 * TILE as f32) as usize;
        px(q[1]) * TILE + px(q[0])
    }

    #[test]
    fn a_painted_disc_knows_how_far_its_edge_is() {
        let disc = Stroke { points: vec![[0.0, 0.0]], size: 0.4, softness: 0.0, strength: 1.0, erase: false };
        let tile = rasterise(&Strokes { list: vec![disc] });
        assert_eq!(tile.len(), 2 * TILE * TILE);
        let edge = dist(&tile, [0.4, 0.0]);
        assert!((120..=136).contains(&edge), "about half on the edge: {edge}");
        assert_eq!(dist(&tile, [0.0, 0.0]), 255, "deep inside is clamped high");
        assert_eq!(dist(&tile, [0.9, 0.9]), 0, "far outside is clamped low");
        let (inside, outside) = (dist(&tile, [0.3, 0.0]), dist(&tile, [0.5, 0.0]));
        assert!(inside > 140 && outside < 116, "{inside} {outside}");
        // Symmetric about the edge, within a step or two.
        let (a, b) = (inside as i32 - 128, 128 - outside as i32);
        assert!((a - b).abs() <= 6, "{a} {b}");
        // Same both sides of the disc.
        assert!(dist(&tile, [-0.3, 0.0]).abs_diff(inside) <= 3);
    }

    #[test]
    fn the_paint_channel_is_what_it_was() {
        // The paint bytes alone, as a one-channel tile was before the distance channel.
        let strokes = Strokes { list: vec![line(false)] };
        let tile = rasterise(&strokes);
        let paint: Vec<u8> = tile.iter().step_by(2).copied().collect();
        let mut level = vec![0.0f32; TILE * TILE];
        apply(&mut level, &strokes.list[0]);
        let old: Vec<u8> = level.iter().map(|l| (l.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
        assert_eq!(paint, old);
    }

    #[test]
    fn a_stroke_paints_along_its_path_and_nowhere_else() {
        let tile = rasterise(&Strokes { list: vec![line(false)] });
        assert_eq!(tile.len(), 2 * TILE * TILE);
        assert!(at(&tile, [0.0, 0.0]) > 240, "{}", at(&tile, [0.0, 0.0]));
        assert!(at(&tile, [-0.45, 0.0]) > 240);
        assert_eq!(at(&tile, [0.0, 0.5]), 0);
        assert_eq!(at(&tile, [0.8, 0.0]), 0);
        let edge = at(&tile, [0.0, 0.08]);
        assert!(edge > 0 && edge < 200, "the soft edge fades: {edge}");
    }

    #[test]
    fn erasing_takes_the_paint_away_and_the_same_strokes_give_the_same_bytes() {
        let painted = Strokes { list: vec![line(false)] };
        let erased = Strokes { list: vec![line(false), line(true)] };
        assert_eq!(at(&rasterise(&erased), [0.0, 0.0]), 0);
        assert_eq!(rasterise(&painted), rasterise(&painted.clone()));
        assert!(rasterise(&Strokes::default()).iter().step_by(2).all(|&b| b == 0));
        let dot = Strokes { list: vec![Stroke { points: vec![[0.2, -0.2]], ..line(false) }] };
        assert!(at(&rasterise(&dot), [0.2, -0.2]) > 240);
    }

    #[test]
    fn a_stroke_added_to_kept_strokes_draws_the_same_bytes_as_drawing_them_all() {
        let mut strokes = Strokes { list: vec![line(false), line(true)] };
        tile(&strokes);
        let mut dab = line(false);
        dab.points = vec![[0.1, 0.3], [0.2, 0.35], [0.3, 0.2]];
        strokes.list.push(dab);
        assert_eq!(&*tile(&strokes).1, rasterise(&strokes).as_slice());
        strokes.list[2].points.push([0.4, 0.1]);
        assert_eq!(&*tile(&strokes).1, rasterise(&strokes).as_slice(), "and again as the stroke grows");
    }
}
