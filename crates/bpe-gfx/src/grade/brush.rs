//! A Brush part's strokes as a grid of paint (colour grading part 5): worked out on the CPU over
//! the strokes' box, eight bits a texel, and carried in the grade's program. The same strokes
//! always give the same grid; grids are kept by the strokes' hash, and strokes that only add one
//! stroke to kept ones start from them, so a stroke being painted only draws itself.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use bpe_core::{Stroke, Strokes};

/// The grid's box is snapped out to this many pixels, so a growing stroke rarely moves it.
pub const GRID_SNAP: f32 = 64.0;
/// A texel is at least this many pixels of its place's units, and the grid's long side at most
/// [`MOST_TEXELS`] texels.
pub const LEAST_TEXEL: f32 = 2.0;
pub const MOST_TEXELS: f32 = 1024.0;

/// Strokes painted into a grid: texel `(x, y)` covers `origin + texel · (x, y)` to one texel
/// further, row by row from the top, 0 (none) to 255 (full).
#[derive(Clone, Debug, PartialEq)]
pub struct BrushGrid {
    pub origin: [f32; 2],
    pub texel: f32,
    pub size: [u32; 2],
    pub paint: Arc<[u8]>,
}

/// Where a grid sits, before it is painted.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    origin: [f32; 2],
    texel: f32,
    size: [u32; 2],
}

fn usable(s: &Stroke) -> bool {
    s.size.is_finite()
        && s.size > 0.0
        && s.softness.is_finite()
        && s.strength.is_finite()
        && s.density.is_finite()
        && !s.points.is_empty()
        && s.points.iter().all(|p| p[0].is_finite() && p[1].is_finite())
}

/// The grid's place for `strokes`: their points grown by their sizes, snapped out; `None` when no
/// stroke paints anything.
fn layout(strokes: &Strokes) -> Option<Layout> {
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for s in strokes.list.iter().filter(|s| usable(s) && !s.erase) {
        for p in &s.points {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i] - s.size);
                hi[i] = hi[i].max(p[i] + s.size);
            }
        }
    }
    if !(lo[0].is_finite() && hi[0].is_finite() && lo[1].is_finite() && hi[1].is_finite()) {
        return None;
    }
    let lo = lo.map(|v| (v / GRID_SNAP).floor() * GRID_SNAP);
    let hi = hi.map(|v| (v / GRID_SNAP).ceil() * GRID_SNAP);
    let span = [(hi[0] - lo[0]).max(GRID_SNAP), (hi[1] - lo[1]).max(GRID_SNAP)];
    let texel = (span[0].max(span[1]) / MOST_TEXELS).max(LEAST_TEXEL);
    let size = span.map(|v| ((v / texel).ceil() as u32).max(1));
    Some(Layout { origin: lo, texel, size })
}

/// How far `p` is from the segment `a`-`b`.
fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
    let (apx, apy) = (p[0] - a[0], p[1] - a[1]);
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0.0 { ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let (dx, dy) = (apx - abx * t, apy - aby * t);
    (dx * dx + dy * dy).sqrt()
}

/// How much a stroke paints at distance `d` from its path: full inside its hard middle, fading to
/// nothing at its size, times its strength (as the ball brush, `bpe-kit` `paint/brush.rs`).
pub fn stroke_cover(s: &Stroke, d: f32) -> f32 {
    let size = s.size.max(0.0);
    let inner = size * (1.0 - s.softness.clamp(0.0, 1.0));
    let t = if size - inner > 1e-6 { ((size - d) / (size - inner)).clamp(0.0, 1.0) } else { f32::from(d < size) };
    t * t * (3.0 - 2.0 * t) * s.strength.clamp(0.0, 1.0)
}

/// Paint `l` with a stroke of coverage `v` laid on it: erasing takes `v` of it away; painting adds
/// `v` of what is missing, up to the stroke's density, never taking paint away.
pub fn lay(l: f32, v: f32, s: &Stroke) -> f32 {
    if s.erase {
        l * (1.0 - v)
    } else {
        let density = if s.density.is_finite() { s.density.clamp(0.0, 1.0) } else { 1.0 };
        (l + v * (1.0 - l)).min(density.max(l))
    }
}

/// Paints stroke `s` onto `level` (laid out as `g`): its own coverage first (the nearest of its
/// segments), then laid on.
fn paint(level: &mut [f32], g: &Layout, s: &Stroke) {
    if !usable(s) {
        return;
    }
    let [w, h] = g.size;
    let texel_of = |v: f32, axis: usize| ((v - g.origin[axis]) / g.texel).floor();
    let window = |lo: [f32; 2], hi: [f32; 2]| {
        let x0 = texel_of(lo[0], 0).clamp(0.0, w as f32 - 1.0) as usize;
        let x1 = texel_of(hi[0], 0).clamp(0.0, w as f32 - 1.0) as usize;
        let y0 = texel_of(lo[1], 1).clamp(0.0, h as f32 - 1.0) as usize;
        let y1 = texel_of(hi[1], 1).clamp(0.0, h as f32 - 1.0) as usize;
        (x0, x1, y0, y1)
    };
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for p in &s.points {
        for i in 0..2 {
            lo[i] = lo[i].min(p[i] - s.size);
            hi[i] = hi[i].max(p[i] + s.size);
        }
    }
    let (sx0, sx1, sy0, sy1) = window(lo, hi);
    let sw = sx1 - sx0 + 1;
    let mut own = vec![0.0f32; sw * (sy1 - sy0 + 1)];
    let segments: Vec<([f32; 2], [f32; 2])> = match s.points.as_slice() {
        [one] => vec![(*one, *one)],
        points => points.windows(2).map(|w| (w[0], w[1])).collect(),
    };
    for (a, b) in segments {
        let lo = [a[0].min(b[0]) - s.size, a[1].min(b[1]) - s.size];
        let hi = [a[0].max(b[0]) + s.size, a[1].max(b[1]) + s.size];
        let (x0, x1, y0, y1) = window(lo, hi);
        for y in y0..=y1 {
            let cy = g.origin[1] + (y as f32 + 0.5) * g.texel;
            for x in x0..=x1 {
                let cx = g.origin[0] + (x as f32 + 0.5) * g.texel;
                let v = stroke_cover(s, segment_distance([cx, cy], a, b));
                let o = &mut own[(y - sy0) * sw + (x - sx0)];
                *o = o.max(v);
            }
        }
    }
    for y in sy0..=sy1 {
        for x in sx0..=sx1 {
            let v = own[(y - sy0) * sw + (x - sx0)];
            if v > 0.0 {
                let l = &mut level[y * w as usize + x];
                *l = lay(*l, v, s);
            }
        }
    }
}

/// The paint levels of `strokes` laid out as `g`, kept for the most recent few sets (see the
/// module's notes).
fn levels(strokes: &Strokes, g: &Layout) -> Arc<[f32]> {
    type Kept = HashMap<u64, (Layout, Arc<[f32]>)>;
    static LEVELS: OnceLock<Mutex<Kept>> = OnceLock::new();
    let lock = || LEVELS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let hash = strokes.hash64();
    if let Some((at, l)) = lock().get(&hash)
        && at == g
    {
        return l.clone();
    }
    let start = strokes.list.split_last().and_then(|(_, before)| {
        let before = Strokes { list: before.to_vec() };
        lock().get(&before.hash64()).filter(|(at, _)| at == g).map(|(_, l)| l.clone())
    });
    let mut level: Vec<f32> = match start {
        Some(l) => {
            let mut l = l.to_vec();
            paint(&mut l, g, strokes.list.last().expect("split above"));
            l
        }
        None => {
            let mut l = vec![0.0f32; g.size[0] as usize * g.size[1] as usize];
            for s in &strokes.list {
                paint(&mut l, g, s);
            }
            l
        }
    };
    level.iter_mut().for_each(|v| *v = v.clamp(0.0, 1.0));
    let level: Arc<[f32]> = level.into();
    let mut kept = lock();
    if kept.len() >= 32 {
        kept.clear();
    }
    kept.insert(hash, (*g, level.clone()));
    level
}

/// `strokes` as a grid, or `None` when they paint nothing. Kept by the strokes' hash.
pub fn grid(strokes: &Strokes) -> Option<Arc<BrushGrid>> {
    static MADE: OnceLock<Mutex<HashMap<u64, Arc<BrushGrid>>>> = OnceLock::new();
    let lock = || MADE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let g = layout(strokes)?;
    let hash = strokes.hash64();
    if let Some(made) = lock().get(&hash).filter(|m| m.origin == g.origin && m.texel == g.texel && m.size == g.size) {
        return Some(made.clone());
    }
    let paint: Arc<[u8]> = levels(strokes, &g).iter().map(|v| (v * 255.0).round() as u8).collect();
    let made = Arc::new(BrushGrid { origin: g.origin, texel: g.texel, size: g.size, paint });
    let mut kept = lock();
    if kept.len() >= 64 {
        kept.clear();
    }
    kept.insert(hash, made.clone());
    Some(made)
}

impl BrushGrid {
    /// The paint at grid texel `(x, y)`, 0..1; none outside.
    pub fn texel_at(&self, x: i64, y: i64) -> f64 {
        let [w, h] = self.size;
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            return 0.0;
        }
        self.paint[(y as usize) * w as usize + x as usize] as f64 / 255.0
    }

    /// The paint at point `q` in the place's units, read between the four nearest texels (as the
    /// shader does).
    pub fn sample(&self, q: [f64; 2]) -> f64 {
        let t = [0, 1].map(|i| (q[i] - self.origin[i] as f64) / self.texel as f64 - 0.5);
        let (x0, y0) = (t[0].floor(), t[1].floor());
        let (fx, fy) = (t[0] - x0, t[1] - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let top = self.texel_at(x0, y0) * (1.0 - fx) + self.texel_at(x0 + 1, y0) * fx;
        let bottom = self.texel_at(x0, y0 + 1) * (1.0 - fx) + self.texel_at(x0 + 1, y0 + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }

    /// How far its furthest corner is from its place (the place's units).
    pub fn reach(&self) -> f32 {
        let far =
            [self.origin[0] + self.texel * self.size[0] as f32, self.origin[1] + self.texel * self.size[1] as f32];
        let x = self.origin[0].abs().max(far[0].abs());
        let y = self.origin[1].abs().max(far[1].abs());
        x.hypot(y)
    }

    /// The grid as program words: three bytes in each lane as a whole number (`b0 + 256 b1 +
    /// 65536 b2`, exact in an f32), so twelve texels a word.
    pub fn words(&self) -> Vec<[f32; 4]> {
        let lanes: Vec<f32> = self
            .paint
            .chunks(3)
            .map(|c| {
                let b = |i: usize| c.get(i).copied().unwrap_or(0) as u32;
                (b(0) | (b(1) << 8) | (b(2) << 16)) as f32
            })
            .collect();
        lanes.chunks(4).map(|c| [0, 1, 2, 3].map(|i| c.get(i).copied().unwrap_or(0.0))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(size: f32, softness: f32) -> Stroke {
        Stroke { points: vec![[-200.0, 0.0], [200.0, 0.0]], size, softness, strength: 1.0, ..Default::default() }
    }

    #[test]
    fn a_stroke_paints_its_middle_full_and_nothing_past_its_size() {
        let g = grid(&Strokes { list: vec![line(40.0, 0.5)] }).unwrap();
        assert!(g.sample([0.0, 0.0]) > 0.99, "{}", g.sample([0.0, 0.0]));
        assert!(g.sample([0.0, 15.0]) > 0.99, "inside the hard middle");
        assert!(g.sample([0.0, 30.0]) > 0.05 && g.sample([0.0, 30.0]) < 0.95, "in the feather");
        assert!(g.sample([0.0, 45.0]) < 0.01, "past its size");
        assert!(g.sample([300.0, 0.0]) < 0.01, "past its end");
        assert!(g.texel >= LEAST_TEXEL);
        assert_eq!(g.origin, [-256.0, -64.0], "snapped out");
    }

    #[test]
    fn erasing_density_and_no_strokes() {
        let erased = Strokes { list: vec![line(40.0, 0.0), Stroke { erase: true, ..line(20.0, 0.0) }] };
        let g = grid(&erased).unwrap();
        assert!(g.sample([0.0, 0.0]) < 0.01 && g.sample([0.0, 30.0]) > 0.99, "only its edges are left");
        let thin = Stroke { density: 0.5, ..line(40.0, 0.0) };
        let g = grid(&Strokes { list: vec![thin.clone(), thin] }).unwrap();
        assert!((g.sample([0.0, 0.0]) - 0.5).abs() < 0.01, "{}", g.sample([0.0, 0.0]));
        assert!(grid(&Strokes::default()).is_none());
        assert!(grid(&Strokes { list: vec![Stroke { erase: true, ..line(40.0, 0.0) }] }).is_none());
    }

    #[test]
    fn a_stroke_added_to_kept_ones_paints_the_same_grid_as_painting_them_all() {
        let mut strokes = Strokes { list: vec![line(40.0, 0.5)] };
        grid(&strokes);
        strokes.list.push(Stroke { points: vec![[-50.0, 10.0], [0.0, 20.0], [40.0, -5.0]], ..line(20.0, 0.3) });
        let quick = grid(&strokes).unwrap();
        let g = layout(&strokes).unwrap();
        let mut whole = vec![0.0f32; (g.size[0] * g.size[1]) as usize];
        for s in &strokes.list {
            paint(&mut whole, &g, s);
        }
        let whole: Vec<u8> = whole.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
        assert_eq!(&*quick.paint, whole.as_slice());
    }

    #[test]
    fn a_big_box_has_bigger_texels_and_words_pack_three_bytes_a_lane() {
        let wide = Stroke { points: vec![[-5000.0, 0.0], [5000.0, 0.0]], ..line(10.0, 0.0) };
        let g = grid(&Strokes { list: vec![wide] }).unwrap();
        assert!(g.size[0] <= MOST_TEXELS as u32 + 1 && g.texel > LEAST_TEXEL, "{:?} {}", g.size, g.texel);
        let small = BrushGrid { origin: [0.0; 2], texel: 2.0, size: [5, 1], paint: vec![1, 2, 3, 4, 255].into() };
        let words = small.words();
        assert_eq!(words, vec![[(1 + 2 * 256 + 3 * 65536) as f32, (4 + 255 * 256) as f32, 0.0, 0.0]]);
        assert!((small.reach() - 10.0f32.hypot(2.0)).abs() < 1e-4);
    }
}
