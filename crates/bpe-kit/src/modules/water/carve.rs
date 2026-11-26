//! The Course's **Carving** card (Water mode part 2b) and the carved rock it makes.
//!
//! A Course starts as solid rock; the Carve tool's brush strokes dig it away (or, with Fill, put it
//! back). The card keeps the strokes, in world pixels with the course centred on the world's middle
//! (`Stroke.size` is the brush's radius, `softness` is 1 - Hardness, `erase` means Fill, `square` a
//! square tip). [`CarveMask`] turns them into the rock's signed distance on a grid of one sample every
//! [`STEP`] pixels (above 0 open, below 0 rock), which the water's physics and its drawing both read.

use std::sync::{Arc, Mutex};

use bpe_core::{Settings, Stroke, Strokes};
use glam::Vec2;

/// Pixels between two of the mask's samples (the water spec: "4 px per sample at 1080 wide with the
/// edge stored as a distance").
pub const STEP: f32 = 4.0;

/// The furthest the mask tells apart from a face, in pixels: the water reads the field only within
/// a reach of about two cells of a face, and this is four cells at the coarsest draft.
pub const BAND: f32 = 48.0;

/// The Course's Carving card.
#[derive(Clone, Debug, Default, PartialEq, Settings)]
pub struct CarveCard {
    /// Every stroke painted with the Carve tool, oldest first.
    #[setting(label = "Carved", simple, group = "Carving")]
    pub carved: Strokes,
}

/// The carved rock: its signed distance at one sample every [`STEP`] pixels over the course.
#[derive(Clone, Debug, PartialEq)]
pub struct CarveMask {
    /// The course's size, in pixels.
    pub size: Vec2,
    /// The first sample's place: the course's top left corner.
    pub lo: Vec2,
    pub cols: u32,
    pub rows: u32,
    /// The distance to the rock's face at each sample, row by row (the course's own edge left out:
    /// [`CarveMask::at`] adds it).
    pub open: Vec<f32>,
}

impl CarveMask {
    /// A course of `size` that is all rock.
    pub fn rock(size: Vec2) -> CarveMask {
        let size = size.max(Vec2::splat(STEP));
        let cols = (size.x / STEP).ceil() as u32 + 1;
        let rows = (size.y / STEP).ceil() as u32 + 1;
        CarveMask { size, lo: -size / 2.0, cols, rows, open: vec![-BAND; (cols * rows) as usize] }
    }

    /// The course of `size` with `strokes` carved into it, in order.
    pub fn build(size: Vec2, strokes: &Strokes) -> CarveMask {
        let mut mask = CarveMask::rock(size);
        for s in &strokes.list {
            mask.apply(s);
        }
        mask
    }

    /// Carves one more stroke in.
    pub fn apply(&mut self, stroke: &Stroke) {
        let Some(brush) = Brush::of(stroke) else { return };
        let Some(area) = self.area(stroke.points.iter().map(|p| Vec2::from(*p)), brush.reach()) else { return };
        let mut g = vec![f32::NEG_INFINITY; area.len()];
        for (a, b) in segments(&stroke.points) {
            let seg = Segment::new(a, b, &brush);
            if let Some(part) = self.area([a, b].into_iter(), brush.reach()) {
                for (x, y) in part.samples() {
                    let i = area.index(x, y);
                    g[i] = g[i].max(seg.inside(self.sample(x, y)));
                }
            }
        }
        for (x, y) in area.samples() {
            let i = (y * self.cols + x) as usize;
            self.open[i] = brush.combine(self.open[i], g[area.index(x, y)]);
        }
    }

    /// The signed distance at world point `p`: above 0 open, below 0 rock, and rock past the
    /// course's edge, which is a face like any other.
    pub fn at(&self, p: Vec2) -> f32 {
        let hi = self.lo + self.size;
        let edge = (p.x - self.lo.x).min(hi.x - p.x).min(p.y - self.lo.y).min(hi.y - p.y);
        if edge <= -BAND {
            return -BAND;
        }
        let g = ((p - self.lo) / STEP).clamp(Vec2::ZERO, Vec2::new(self.cols as f32 - 1.001, self.rows as f32 - 1.001));
        let (x, y) = (g.x.floor() as u32, g.y.floor() as u32);
        let f = g - Vec2::new(x as f32, y as f32);
        let at = |x: u32, y: u32| self.open[(y * self.cols + x) as usize];
        let top = at(x, y) + (at(x + 1, y) - at(x, y)) * f.x;
        let bottom = at(x, y + 1) + (at(x + 1, y + 1) - at(x, y + 1)) * f.x;
        (top + (bottom - top) * f.y).min(edge).clamp(-BAND, BAND)
    }

    /// Whether anything is carved at all.
    pub fn any_open(&self) -> bool {
        self.open.iter().any(|d| *d > 0.0)
    }

    /// The solid field the water's physics and its drawing read at `cell` pixels a cell.
    pub fn field(&self, cell: f32) -> bpe_water::SolidField {
        bpe_water::SolidField::from_distance(layout_for(self.size, cell), |p, _| self.at(p))
    }

    fn sample(&self, x: u32, y: u32) -> Vec2 {
        self.lo + Vec2::new(x as f32, y as f32) * STEP
    }

    /// The samples within `reach` of the box round `points`.
    fn area(&self, points: impl Iterator<Item = Vec2>, reach: f32) -> Option<Area> {
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for p in points {
            lo = lo.min(p);
            hi = hi.max(p);
        }
        if !(lo.is_finite() && hi.is_finite()) {
            return None;
        }
        let a = ((lo - Vec2::splat(reach) - self.lo) / STEP).floor().max(Vec2::ZERO);
        let b = ((hi + Vec2::splat(reach) - self.lo) / STEP).ceil();
        let b = b.min(Vec2::new(self.cols as f32 - 1.0, self.rows as f32 - 1.0));
        (a.x <= b.x && a.y <= b.y).then_some(Area { x0: a.x as u32, y0: a.y as u32, x1: b.x as u32, y1: b.y as u32 })
    }
}

/// The grid a course of `size` gets at `cell` pixels a cell: the course centred on the world's
/// middle, with the water's own margin round it.
pub fn layout_for(size: Vec2, cell: f32) -> bpe_water::Layout {
    bpe_water::Layout::cover(-size / 2.0, size / 2.0, cell)
}

/// One stroke being painted: the mask with the stroke so far carved in, kept up to date a segment at
/// a time, so painting shows exactly what letting go will give.
#[derive(Clone, Debug)]
pub struct CarvePaint {
    before: Arc<CarveMask>,
    stroke: Stroke,
    /// The stroke's own shape over the whole course (above 0 inside it).
    g: Vec<f32>,
    mask: CarveMask,
}

impl CarvePaint {
    /// Starts painting `stroke` (its points so far) onto `base`.
    pub fn start(base: Arc<CarveMask>, stroke: Stroke) -> CarvePaint {
        let mut paint = CarvePaint {
            g: vec![f32::NEG_INFINITY; base.open.len()],
            mask: (*base).clone(),
            before: base,
            stroke: Stroke { points: Vec::new(), ..stroke.clone() },
        };
        for p in stroke.points {
            paint.push(p);
        }
        paint
    }

    /// The stroke goes on to `p`.
    pub fn push(&mut self, p: [f32; 2]) {
        if !(p[0].is_finite() && p[1].is_finite()) {
            return;
        }
        let a = self.stroke.points.last().copied().unwrap_or(p);
        self.stroke.points.push(p);
        let Some(brush) = Brush::of(&self.stroke) else { return };
        let (a, b) = (Vec2::from(a), Vec2::from(p));
        let seg = Segment::new(a, b, &brush);
        let Some(area) = self.mask.area([a, b].into_iter(), brush.reach()) else { return };
        for (x, y) in area.samples() {
            let i = (y * self.mask.cols + x) as usize;
            self.g[i] = self.g[i].max(seg.inside(self.mask.sample(x, y)));
            self.mask.open[i] = brush.combine(self.before.open[i], self.g[i]);
        }
    }

    /// Starts the stroke over with `stroke` (a straight line's new end, a lazy brush's new path).
    pub fn reset(&mut self, stroke: Stroke) {
        *self = CarvePaint::start(self.before.clone(), stroke);
    }

    /// The course with the stroke so far carved in.
    pub fn mask(&self) -> &CarveMask {
        &self.mask
    }

    /// The stroke so far.
    pub fn stroke(&self) -> &Stroke {
        &self.stroke
    }

    /// The course as it was before the stroke.
    pub fn before(&self) -> &Arc<CarveMask> {
        &self.before
    }
}

/// The carved course of `size` for `strokes`, from a cache of the last few, building on one whose
/// strokes start the same way (a stroke added on the end costs only that stroke).
pub fn mask_for(size: Vec2, strokes: &Strokes) -> Arc<CarveMask> {
    static KEPT: Mutex<MaskCache> = Mutex::new(MaskCache(Vec::new()));
    mask_with(&KEPT, size, strokes)
}

/// The masks [`mask_for`] keeps: the strokes' hash, the course's size, the strokes and their mask.
struct MaskCache(Vec<(u64, Vec2, Arc<Strokes>, Arc<CarveMask>)>);

impl MaskCache {
    const MOST: usize = 4;
}

fn mask_with(cache: &Mutex<MaskCache>, size: Vec2, strokes: &Strokes) -> Arc<CarveMask> {
    let hash = strokes.hash64();
    let base = {
        let kept = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(k) = kept.0.iter().find(|k| k.0 == hash && k.1 == size && *k.2 == *strokes) {
            return k.3.clone();
        }
        kept.0
            .iter()
            .filter(|k| k.1 == size && strokes.list.starts_with(&k.2.list))
            .max_by_key(|k| k.2.list.len())
            .map(|k| (k.2.list.len(), k.3.clone()))
    };
    // Built with the cache let go, so another thread asking for another course never waits on this one.
    let mask = match base {
        Some((done, mask)) => {
            let mut mask = (*mask).clone();
            for s in &strokes.list[done..] {
                mask.apply(s);
            }
            mask
        }
        None => CarveMask::build(size, strokes),
    };
    let mask = Arc::new(mask);
    let mut kept = cache.lock().unwrap_or_else(|e| e.into_inner());
    kept.0.retain(|k| !(k.0 == hash && k.1 == size));
    kept.0.push((hash, size, Arc::new(strokes.clone()), mask.clone()));
    if kept.0.len() > MaskCache::MOST {
        kept.0.remove(0);
    }
    mask
}

/// A stroke's segments, a dot first (so a stroke of one point carves too, and painting point by
/// point carves the same as building the whole stroke).
fn segments(points: &[[f32; 2]]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    let first = points.first().map(|p| (Vec2::from(*p), Vec2::from(*p)));
    first.into_iter().chain(points.windows(2).map(|w| (Vec2::from(w[0]), Vec2::from(w[1]))))
}

/// The brush a stroke was painted with.
struct Brush {
    radius: f32,
    /// How far the smooth join between this stroke and the rock reaches (0: a crisp corner).
    k: f32,
    fill: bool,
    square: bool,
}

impl Brush {
    fn of(s: &Stroke) -> Option<Brush> {
        if !(s.size.is_finite() && s.size > 0.0) {
            return None;
        }
        let k = s.softness.clamp(0.0, 1.0) * s.size;
        Some(Brush { radius: s.size, k: if k.is_finite() { k } else { 0.0 }, fill: s.erase, square: s.square })
    }

    /// How far past its path a stroke can change the mask.
    fn reach(&self) -> f32 {
        self.radius + self.k + BAND + STEP
    }

    /// The rock's distance `f` with a stroke whose own shape is `g` carved in.
    fn combine(&self, f: f32, g: f32) -> f32 {
        let d = if self.fill { smooth_min(f, -g, self.k) } else { -smooth_min(-f, -g, self.k) };
        d.clamp(-BAND, BAND)
    }
}

/// The smaller of `a` and `b`, rounded over `k` where they are close (k = 0: the plain minimum).
fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// One segment of a stroke: how far a point is inside the brush swept along it.
enum Segment {
    Round {
        a: Vec2,
        b: Vec2,
        radius: f32,
    },
    /// The square tip swept along the segment: a convex polygon (four to six corners, anticlockwise
    /// with y down taken as y up).
    Square(Vec<Vec2>),
}

impl Segment {
    fn new(a: Vec2, b: Vec2, brush: &Brush) -> Segment {
        if !brush.square {
            return Segment::Round { a, b, radius: brush.radius };
        }
        let r = brush.radius;
        let corners = [Vec2::new(-r, -r), Vec2::new(r, -r), Vec2::new(r, r), Vec2::new(-r, r)];
        let points: Vec<Vec2> = corners.iter().flat_map(|c| [a + *c, b + *c]).collect();
        Segment::Square(hull(points))
    }

    /// Above 0 inside the brush's path (the distance to its edge), below 0 outside.
    fn inside(&self, p: Vec2) -> f32 {
        match self {
            Segment::Round { a, b, radius } => {
                let ab = *b - *a;
                let len = ab.length_squared();
                let t = if len > 0.0 { ((p - *a).dot(ab) / len).clamp(0.0, 1.0) } else { 0.0 };
                radius - (p - (*a + ab * t)).length()
            }
            Segment::Square(poly) => -polygon_distance(poly, p),
        }
    }
}

/// The convex hull of `points` (Andrew's monotone chain), anticlockwise.
fn hull(mut points: Vec<Vec2>) -> Vec<Vec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup();
    if points.len() < 3 {
        return points;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut out: Vec<Vec2> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let start = out.len();
        let iter: Box<dyn Iterator<Item = &Vec2>> =
            if pass == 0 { Box::new(points.iter()) } else { Box::new(points.iter().rev()) };
        for p in iter {
            while out.len() >= start + 2 && cross(out[out.len() - 2], out[out.len() - 1], *p) <= 0.0 {
                out.pop();
            }
            out.push(*p);
        }
        out.pop();
    }
    out
}

/// The signed distance from `p` to the polygon `poly` (below 0 inside).
fn polygon_distance(poly: &[Vec2], p: Vec2) -> f32 {
    let n = poly.len();
    if n == 0 {
        return f32::INFINITY;
    }
    if n < 3 {
        // A degenerate hull (a zero-size brush): the distance to its points.
        return poly.iter().map(|q| (p - *q).length()).fold(f32::INFINITY, f32::min);
    }
    let mut d = (p - poly[0]).length_squared();
    let mut sign = 1.0;
    let mut j = n - 1;
    for i in 0..n {
        let (vi, vj) = (poly[i], poly[j]);
        let e = vj - vi;
        let w = p - vi;
        let t = (w.dot(e) / e.length_squared()).clamp(0.0, 1.0);
        d = d.min((w - e * t).length_squared());
        let c = [p.y >= vi.y, p.y < vj.y, e.x * w.y > e.y * w.x];
        if c.iter().all(|x| *x) || c.iter().all(|x| !*x) {
            sign = -sign;
        }
        j = i;
    }
    sign * d.sqrt()
}

/// A box of samples.
struct Area {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl Area {
    fn len(&self) -> usize {
        ((self.x1 - self.x0 + 1) * (self.y1 - self.y0 + 1)) as usize
    }

    fn index(&self, x: u32, y: u32) -> usize {
        ((y - self.y0) * (self.x1 - self.x0 + 1) + (x - self.x0)) as usize
    }

    fn samples(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        (self.y0..=self.y1).flat_map(move |y| (self.x0..=self.x1).map(move |x| (x, y)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::water::SCREEN;
    use bpe_core::maths::Maths;

    fn dot(at: [f32; 2], radius: f32) -> Stroke {
        Stroke { points: vec![at], size: radius, strength: 1.0, ..Default::default() }
    }

    fn line(points: Vec<[f32; 2]>, radius: f32) -> Stroke {
        Stroke { points, size: radius, strength: 1.0, ..Default::default() }
    }

    fn carved(list: Vec<Stroke>) -> CarveMask {
        CarveMask::build(SCREEN, &Strokes { list })
    }

    fn near(got: f32, want: f32, by: f32) {
        assert!((got - want).abs() <= by, "{got} is not within {by} of {want}");
    }

    #[test]
    fn nothing_carved_is_all_rock() {
        let mask = carved(Vec::new());
        assert_eq!(mask.at(Vec2::ZERO), -BAND);
        assert!(!mask.any_open());
        let field = mask.field(8.0);
        assert!(field.distance.iter().all(|d| *d < 0.0));
        assert_eq!(field.layout, layout_for(SCREEN, 8.0));
    }

    #[test]
    fn a_dig_opens_a_round_hole_with_its_distance() {
        let mask = carved(vec![dot([0.0, 0.0], 100.0)]);
        near(mask.at(Vec2::ZERO), BAND, 0.0);
        near(mask.at(Vec2::new(70.0, 0.0)), 30.0, 0.5);
        near(mask.at(Vec2::new(130.0, 0.0)), -30.0, 0.5);
        near(mask.at(Vec2::new(0.0, -100.0)), 0.0, 0.6);
        near(mask.at(Vec2::new(-60.0, 80.0)), 0.0, 0.6);
        assert_eq!(mask.at(Vec2::new(300.0, 0.0)), -BAND);
    }

    #[test]
    fn a_fill_after_a_dig_puts_rock_back() {
        let fill = Stroke { erase: true, ..dot([0.0, 0.0], 40.0) };
        let mask = carved(vec![dot([0.0, 0.0], 100.0), fill]);
        near(mask.at(Vec2::ZERO), -40.0, 0.5);
        near(mask.at(Vec2::new(80.0, 0.0)), 20.0, 0.5);
        near(mask.at(Vec2::new(40.0, 0.0)), 0.0, 0.6);
    }

    #[test]
    fn strokes_count_in_order() {
        let fill = Stroke { erase: true, ..dot([0.0, 0.0], 60.0) };
        assert!(carved(vec![fill.clone(), dot([0.0, 0.0], 60.0)]).at(Vec2::ZERO) > 0.0);
        assert!(carved(vec![dot([0.0, 0.0], 60.0), fill]).at(Vec2::ZERO) < 0.0);
    }

    #[test]
    fn a_square_tip_digs_a_square() {
        let mask = carved(vec![Stroke { square: true, ..dot([0.0, 0.0], 50.0) }]);
        assert!(mask.at(Vec2::new(45.0, 45.0)) > 0.0);
        near(mask.at(Vec2::new(45.0, 0.0)), 5.0, 0.5);
        near(mask.at(Vec2::new(0.0, 60.0)), -10.0, 0.5);
        near(mask.at(Vec2::new(60.0, 60.0)), -(200.0f32).sqrt(), 0.6);
        // Swept along a slant it is a six-sided shape: inside the corners the round tip leaves out.
        let slant = Stroke { square: true, ..line(vec![[-100.0, -100.0], [100.0, 100.0]], 30.0) };
        let round = line(vec![[-100.0, -100.0], [100.0, 100.0]], 30.0);
        let (s, r) = (carved(vec![slant]), carved(vec![round]));
        let corner = Vec2::new(-128.0, -128.0);
        assert!(s.at(corner) > 0.0 && r.at(corner) < 0.0, "{} {}", s.at(corner), r.at(corner));
        // Its long sides run 30 px either side of the path along each axis: 30 x sqrt 2 across it.
        near(s.at(Vec2::ZERO), 60.0 / 2.0f32.sqrt(), 0.5);
        near(r.at(Vec2::ZERO), 30.0, 0.5);
    }

    #[test]
    fn hardness_rounds_the_corner_where_strokes_meet() {
        let across = line(vec![[-200.0, 0.0], [200.0, 0.0]], 30.0);
        let down = |softness| Stroke { softness, ..line(vec![[0.0, -200.0], [0.0, 200.0]], 30.0) };
        let hard = carved(vec![across.clone(), down(0.0)]);
        let soft = carved(vec![across, down(1.0)]);
        let corner = Vec2::new(40.0, 40.0);
        assert!(hard.at(corner) < 0.0);
        assert!(soft.at(corner) > hard.at(corner), "{} {}", soft.at(corner), hard.at(corner));
        // Far from the corner the soft stroke digs the same.
        near(soft.at(Vec2::new(0.0, 150.0)), hard.at(Vec2::new(0.0, 150.0)), 0.01);
    }

    #[test]
    fn past_the_course_is_rock() {
        let mask = carved(vec![Stroke { square: true, ..dot([0.0, 0.0], 2000.0) }]);
        assert_eq!(mask.at(Vec2::new(600.0, 0.0)), -BAND);
        near(mask.at(Vec2::new(530.0, 0.0)), 10.0, 0.01);
        near(mask.at(Vec2::new(540.0, 0.0)), 0.0, 0.01);
        near(mask.at(Vec2::new(0.0, -950.0)), 10.0, 0.01);
    }

    #[test]
    fn painting_point_by_point_is_the_same_as_building_whole() {
        let base =
            carved(vec![dot([0.0, 0.0], 100.0), Stroke { erase: true, softness: 0.5, ..dot([50.0, 0.0], 30.0) }]);
        let points: Vec<[f32; 2]> =
            (0..30).map(|i| [-300.0 + i as f32 * 20.0, 200.0 * (i as f32 * 0.3).det_sin()]).collect();
        for (square, erase, softness) in [(false, false, 0.0), (true, false, 0.4), (false, true, 1.0)] {
            let stroke = Stroke { square, erase, softness, ..line(points.clone(), 45.0) };
            let mut paint = CarvePaint::start(Arc::new(base.clone()), Stroke { points: vec![], ..stroke.clone() });
            for p in &points {
                paint.push(*p);
            }
            let mut whole = base.clone();
            whole.apply(&stroke);
            assert_eq!(paint.stroke(), &stroke);
            for (a, b) in paint.mask().open.iter().zip(&whole.open) {
                assert!((a - b).abs() <= 1e-4, "{a} {b} ({square} {erase} {softness})");
            }
        }
    }

    #[test]
    fn a_long_stroke_is_quick_to_paint_and_to_build() {
        let points: Vec<[f32; 2]> = (0..400).map(|i| [-540.0 + i as f32 * 2.7, (i % 40) as f32 * 10.0]).collect();
        let strokes = Strokes { list: vec![line(points, 150.0)] };
        let start = std::time::Instant::now();
        let mask = CarveMask::build(SCREEN, &strokes);
        let took = start.elapsed().as_secs_f32();
        assert!(mask.any_open());
        assert!(took < 1.0, "{took} s to build one long stroke");
    }

    #[test]
    fn the_physics_field_follows_the_mask() {
        let mask = carved(vec![line(vec![[-200.0, -300.0], [150.0, 400.0]], 90.0), dot([300.0, -500.0], 70.0)]);
        let field = mask.field(8.0);
        for i in 0..20 {
            let p = Vec2::new(-150.0 + i as f32 * 4.0, -300.0 + i as f32 * 31.0) + Vec2::new(90.0, 0.0);
            let (f, m) = (field.at(p), mask.at(p));
            if m.abs() < BAND - 10.0 {
                near(f, m, 1.0);
            }
        }
    }

    #[test]
    fn a_stroke_added_on_the_end_builds_on_the_kept_mask() {
        let cache = Mutex::new(MaskCache(Vec::new()));
        let one = Strokes { list: vec![dot([0.0, 0.0], 80.0)] };
        let two = Strokes { list: vec![dot([0.0, 0.0], 80.0), Stroke { softness: 0.5, ..dot([90.0, 0.0], 50.0) }] };
        let a = mask_with(&cache, SCREEN, &one);
        assert!(Arc::ptr_eq(&a, &mask_with(&cache, SCREEN, &one)), "the same strokes come from the cache");
        let b = mask_with(&cache, SCREEN, &two);
        assert_eq!(*b, CarveMask::build(SCREEN, &two));
        assert!(Arc::ptr_eq(&a, &mask_with(&cache, SCREEN, &one)));
        assert_eq!(
            *mask_with(&cache, Vec2::new(2160.0, 1920.0), &one),
            CarveMask::build(Vec2::new(2160.0, 1920.0), &one)
        );
        assert_eq!(*mask_for(SCREEN, &two), *b);
    }

    #[test]
    fn the_card_holds_the_strokes() {
        let card = CarveCard::default();
        assert!(card.carved.list.is_empty());
        let defs = CarveCard::defs();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].ty, bpe_core::ValueType::Strokes);
    }
}
