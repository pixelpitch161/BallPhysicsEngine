//! Brush strokes as a picture: a Strokes value drawn into a small greyscale tile the renderer
//! reads, white where painted. The same strokes always give the same bytes.

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

/// `level` as tile bytes.
fn bytes(level: &[f32]) -> Vec<u8> {
    level.iter().map(|l| (l.clamp(0.0, 1.0) * 255.0).round() as u8).collect()
}

/// The strokes as a `TILE` × `TILE` tile, row by row from the top: how painted each pixel is, 0
/// to 255. Strokes paint in order; erasing ones take paint away.
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

    /// The tile's value at ball point `q`.
    fn at(tile: &[u8], q: [f32; 2]) -> u8 {
        let px = |v: f32| ((v + 1.0) * 0.5 * TILE as f32) as usize;
        tile[px(q[1]) * TILE + px(q[0])]
    }

    #[test]
    fn a_stroke_paints_along_its_path_and_nowhere_else() {
        let tile = rasterise(&Strokes { list: vec![line(false)] });
        assert_eq!(tile.len(), TILE * TILE);
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
        assert!(rasterise(&Strokes::default()).iter().all(|&b| b == 0));
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
