//! The solid field: for every node of the liquid's grid, how far the nearest solid's face is
//! (pixels, below 0 inside the solid). The physics reads only this, so whatever makes a course
//! (carving in part 2b, pieces and pen walls in part 3) only has to turn itself into one.
//!
//! **Thin solids are thickened for the physics.** A hollow wall or a line is taken as at least one
//! cell thick. With that, a wall is always seen by the nodes round it, the distance's slope is a
//! usable normal wherever a drop can be, and no drop moving in sub-moves of under half a cell can
//! reach a wall's middle line. (At draft detail a pen wall thinner than a cell therefore holds
//! the water a little off itself; the editor's trouble mark `THIN` says so.)
//!
//! **Pieces and walls** (part 3) are laid over a field one [`Part`] at a time, in the course's
//! order ([`SolidField::add_part`]): each adds solid, or first takes its hull away (Cuts), and every
//! face it leaves wears its own Wetting and is marked as its own ([`SolidField::owner`]) for the
//! drawing's colours.

use glam::Vec2;

/// Where the grid sits in the world: node `(x, y)` is at `origin + (x, y) * cell`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub origin: Vec2,
    /// Pixels a cell.
    pub cell: f32,
    /// Nodes across and down (one more than the cells).
    pub cols: u32,
    pub rows: u32,
}

impl Layout {
    /// A grid of `cell`-pixel cells covering the rectangle from `lo` to `hi` with [`Layout::MARGIN`]
    /// cells to spare all round.
    pub fn cover(lo: Vec2, hi: Vec2, cell: f32) -> Layout {
        let margin = Layout::MARGIN as f32;
        let size = ((hi - lo) / cell).ceil();
        Layout {
            origin: lo - Vec2::splat(margin * cell),
            cell,
            cols: size.x as u32 + 2 * Layout::MARGIN + 1,
            rows: size.y as u32 + 2 * Layout::MARGIN + 1,
        }
    }

    /// Cells kept free round what a layout covers (drops stop two cells in from the grid's edge).
    pub const MARGIN: u32 = 3;

    pub fn to_grid(&self, p: Vec2) -> Vec2 {
        (p - self.origin) / self.cell
    }

    pub fn to_world(&self, g: Vec2) -> Vec2 {
        self.origin + g * self.cell
    }

    pub fn nodes(&self) -> usize {
        self.cols as usize * self.rows as usize
    }
}

/// One solid of a course.
#[derive(Clone, Debug, PartialEq)]
pub enum Solid {
    /// A closed shape: filled, or hollow with a wall `thickness` pixels wide along its outline.
    Polygon {
        points: Vec<Vec2>,
        filled: bool,
        thickness: f32,
    },
    Circle {
        centre: Vec2,
        radius: f32,
        filled: bool,
        thickness: f32,
    },
    /// A straight wall with round ends.
    Line {
        a: Vec2,
        b: Vec2,
        thickness: f32,
    },
    /// A wall along `points` (a pen line, a piece's thin wall): round where it bends, and round or,
    /// with `square`, flat half its thickness past each end (as a drawn line's square cap).
    Polyline {
        points: Vec<Vec2>,
        thickness: f32,
        square: bool,
    },
}

impl Solid {
    /// A filled or hollow rectangle from `lo` to `hi`.
    pub fn rect(lo: Vec2, hi: Vec2, filled: bool, thickness: f32) -> Solid {
        Solid::Polygon { points: vec![lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)], filled, thickness }
    }

    /// How far `p` is from this solid's face (below 0 inside), with thin walls taken as at least
    /// `least` thick.
    pub fn distance(&self, p: Vec2, least: f32) -> f32 {
        match self {
            Solid::Polygon { points, filled, thickness } => {
                let (d, inside) = polygon(points, p);
                if *filled { if inside { -d } else { d } } else { d - 0.5 * thickness.max(least) }
            }
            Solid::Circle { centre, radius, filled, thickness } => {
                let d = (p - *centre).length() - radius;
                if *filled { d } else { d.abs() - 0.5 * thickness.max(least) }
            }
            Solid::Line { a, b, thickness } => segment(*a, *b, p) - 0.5 * thickness.max(least),
            Solid::Polyline { points, thickness, square } => polyline(points, *square, 0.5 * thickness.max(least), p),
        }
    }

    /// The box round this solid (thin walls at least `least` thick), or `None` when it has no finite
    /// point: a part touches the field only near its box.
    pub fn bounds(&self, least: f32) -> Option<(Vec2, Vec2)> {
        let (points, pad): (Vec<Vec2>, f32) = match self {
            Solid::Polygon { points, filled, thickness } => {
                (points.clone(), if *filled { 0.0 } else { 0.5 * thickness.max(least) })
            }
            Solid::Circle { centre, radius, filled, thickness } => {
                let r = radius.abs() + if *filled { 0.0 } else { 0.5 * thickness.max(least) };
                (vec![*centre - Vec2::splat(r), *centre + Vec2::splat(r)], 0.0)
            }
            Solid::Line { a, b, thickness } => (vec![*a, *b], 0.5 * thickness.max(least)),
            // (A square end's corners stand up to half a thickness times the root of two off its point.)
            Solid::Polyline { points, thickness, .. } => (points.clone(), thickness.max(least)),
        };
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for p in points.iter().filter(|p| p.is_finite()) {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        let pad = if pad.is_finite() { pad.max(0.0) } else { 0.0 };
        (lo.x <= hi.x && lo.y <= hi.y).then(|| (lo - Vec2::splat(pad), hi + Vec2::splat(pad)))
    }

    /// Whether this solid has something to it: enough finite points (three for a polygon, two for
    /// a wall) and a finite size. A part leaves out every solid that has not.
    pub fn is_sound(&self) -> bool {
        let finite = |points: &[Vec2]| points.iter().all(|p| p.is_finite());
        match self {
            Solid::Polygon { points, thickness, .. } => points.len() >= 3 && finite(points) && thickness.is_finite(),
            Solid::Circle { centre, radius, thickness, .. } => {
                centre.is_finite() && radius.is_finite() && *radius > 0.0 && thickness.is_finite()
            }
            Solid::Line { a, b, thickness } => a.is_finite() && b.is_finite() && thickness.is_finite(),
            Solid::Polyline { points, thickness, .. } => points.len() >= 2 && finite(points) && thickness.is_finite(),
        }
    }
}

/// One piece or pen wall laid over a field (part 3, [`SolidField::add_part`]): it first takes the
/// solids of `cut` away (Cuts on: a hollow piece's hull, a filled one's own shape), then adds
/// `solids`, and every face it leaves is `wetting` wet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Part {
    pub solids: Vec<Solid>,
    pub cut: Vec<Solid>,
    pub wetting: f32,
}

impl Part {
    /// The box round everything in the part, or `None` when it has nothing.
    pub fn bounds(&self, least: f32) -> Option<(Vec2, Vec2)> {
        self.solids
            .iter()
            .chain(&self.cut)
            .filter(|s| s.is_sound())
            .filter_map(|s| s.bounds(least))
            .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
    }
}

/// How far `p` is from the segment `a` to `b`.
fn segment(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length()
}

/// The signed distance from `p` to a wall `r` either side of `points` (at least two of them), with
/// round or square ends.
fn polyline(points: &[Vec2], square: bool, r: f32, p: Vec2) -> f32 {
    let n = points.len();
    if n == 0 {
        return SolidField::FAR;
    }
    if n == 1 {
        return (p - points[0]).length() - r;
    }
    let mut d = f32::MAX;
    for i in 0..n - 1 {
        let (mut a, mut b) = (points[i], points[i + 1]);
        let way = (b - a).normalize_or_zero();
        let first = i == 0;
        let last = i == n - 2;
        if square && way != Vec2::ZERO {
            // (A square end: the segment runs on half a thickness, and is cut off flat there.)
            if first {
                a -= way * r;
            }
            if last {
                b += way * r;
            }
        }
        let mut here = segment(a, b, p) - r;
        if square && way != Vec2::ZERO {
            if first {
                here = here.max((a - p).dot(way));
            }
            if last {
                here = here.max((p - b).dot(way));
            }
        }
        d = d.min(here);
    }
    d
}

/// How far `p` is from the outline of the closed polygon `points`, and whether it is inside (even-odd).
fn polygon(points: &[Vec2], p: Vec2) -> (f32, bool) {
    let mut d = f32::MAX;
    let mut inside = false;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        d = d.min(segment(a, b, p));
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    (d, inside)
}

/// How wet a solid is when nothing says otherwise (Wetting, part 2c): see [`SolidField::wetting`].
/// Fully wet, as stone and clean glass are: on the owner's card it is the least Wetting at which the
/// course's top shelf drains to puddles at most 7 px tall at 6 px cells and 5.5 px at 3 px (part
/// 2c's plan, "As built and measured").
pub const DEFAULT_WETTING: f32 = 1.0;

/// The distances at a layout's nodes, row by row, in pixels, and how wet the nearest face is.
#[derive(Clone, Debug, PartialEq)]
pub struct SolidField {
    pub layout: Layout,
    pub distance: Vec<f32>,
    /// At each node, how wet its nearest face is (the material's Wetting, 0 to 1, part 2c): how much
    /// that solid counts as liquid to the liquid's skin. 0 is waxy (water beads up on it and rolls),
    /// 1 soaked (water spreads over it into a film).
    pub wetting: Vec<f32>,
    /// At each node, whose its nearest face is (part 3): 0 the field as it was made (the carved
    /// rock), `k` the `k`th part laid over it ([`SolidField::add_part`]). The drawing colours each
    /// solid by it.
    pub owner: Vec<u16>,
}

impl SolidField {
    /// The furthest a field says (pixels): past it a node is simply "far".
    pub const FAR: f32 = 1.0e4;

    /// The field of `solids` on `layout`, thin walls taken as a cell thick (as part 2a's grid core had them).
    pub fn build(layout: Layout, solids: &[Solid]) -> SolidField {
        SolidField::build_least(layout, solids, layout.cell)
    }

    /// The same with thin walls taken as at least `least` pixels thick (the all-drops engine's
    /// [`crate::drops::LEAST_WALL`] cells: see there).
    pub fn build_least(layout: Layout, solids: &[Solid], least: f32) -> SolidField {
        let mut distance = Vec::with_capacity(layout.nodes());
        for y in 0..layout.rows {
            for x in 0..layout.cols {
                let p = layout.to_world(Vec2::new(x as f32, y as f32));
                let d = solids.iter().map(|s| s.distance(p, least)).fold(SolidField::FAR, f32::min);
                distance.push(d);
            }
        }
        let wetting = vec![DEFAULT_WETTING; distance.len()];
        let owner = vec![0; distance.len()];
        SolidField { layout, distance, wetting, owner }
    }

    /// A field from any measure of the solids: `distance(p, least)` is how far world point `p` is
    /// from the nearest face (pixels, below 0 inside), with thin walls taken as at least `least`
    /// pixels thick (a cell: see the module's notes).
    pub fn from_distance(layout: Layout, distance: impl Fn(Vec2, f32) -> f32) -> SolidField {
        let mut out = Vec::with_capacity(layout.nodes());
        for y in 0..layout.rows {
            for x in 0..layout.cols {
                let d = distance(layout.to_world(Vec2::new(x as f32, y as f32)), layout.cell);
                out.push(if d.is_finite() { d.min(SolidField::FAR) } else { SolidField::FAR });
            }
        }
        let wetting = vec![DEFAULT_WETTING; out.len()];
        let owner = vec![0; out.len()];
        SolidField { layout, distance: out, wetting, owner }
    }

    /// The same field with every face `wetting` wet (0 to 1).
    pub fn with_wetting(mut self, wetting: f32) -> SolidField {
        let w = if wetting.is_finite() { wetting.clamp(0.0, 1.0) } else { DEFAULT_WETTING };
        self.wetting.iter_mut().for_each(|v| *v = w);
        self
    }

    /// Lays `part` over the field as its `owner` (1 on), with thin walls taken as at least `least`
    /// pixels thick: its cut solids first take the solid away (`max(d, -cut)`), then its solids add
    /// (`min(d, solid)`). A node whose nearest face is now the part's (open water nearer it, the
    /// inside of what it adds, and solid within [`SolidField::LINING`] cells of a face it cut) is
    /// marked as the part's and wears its Wetting. Only nodes within [`SolidField::BAND`] pixels of the
    /// part's box change: past that only a node's sign matters, and the part cannot change it.
    pub fn add_part(&mut self, part: &Part, owner: u16, least: f32) {
        let cut: Vec<&Solid> = part.cut.iter().filter(|s| s.is_sound()).collect();
        let solids: Vec<&Solid> = part.solids.iter().filter(|s| s.is_sound()).collect();
        let Some((lo, hi)) = part.bounds(least) else { return };
        let l = self.layout;
        let a = l.to_grid(lo - Vec2::splat(SolidField::BAND)).floor().max(Vec2::ZERO);
        let b = l
            .to_grid(hi + Vec2::splat(SolidField::BAND))
            .ceil()
            .min(Vec2::new(l.cols as f32 - 1.0, l.rows as f32 - 1.0));
        if !(a.x <= b.x && a.y <= b.y) {
            return;
        }
        let wet = if part.wetting.is_finite() { part.wetting.clamp(0.0, 1.0) } else { DEFAULT_WETTING };
        let lining = SolidField::LINING * l.cell;
        for y in a.y as u32..=b.y as u32 {
            for x in a.x as u32..=b.x as u32 {
                let i = (y * l.cols + x) as usize;
                let p = l.to_world(Vec2::new(x as f32, y as f32));
                let mut d = self.distance[i];
                let mut mine = false;
                if !cut.is_empty() {
                    let open = -cut.iter().map(|s| s.distance(p, least)).fold(SolidField::FAR, f32::min);
                    if open > d {
                        d = open;
                        mine = d >= -lining;
                    }
                }
                if !solids.is_empty() {
                    let here = solids.iter().map(|s| s.distance(p, least)).fold(SolidField::FAR, f32::min);
                    if here < d || here < 0.0 {
                        mine = true;
                    }
                    d = d.min(here);
                }
                self.distance[i] =
                    if d.is_finite() { d.clamp(-SolidField::FAR, SolidField::FAR) } else { SolidField::FAR };
                if mine {
                    self.owner[i] = owner;
                    self.wetting[i] = wet;
                }
            }
        }
    }

    /// How far round a part's box (pixels) [`SolidField::add_part`] looks: the carved rock's own
    /// reach (`bpe_kit`'s carving band), past which the water and the drawing read only a node's sign.
    pub const BAND: f32 = 48.0;

    /// How deep into the solid round a cut (cells) its faces wear the cutting part's Wetting and
    /// colour: the water reads a face's Wetting from the nodes either side of it.
    pub const LINING: f32 = 2.0;

    /// How wet the face nearest world point `p` is (between the nodes round it).
    pub fn wetting_at(&self, p: Vec2) -> f32 {
        self.between(&self.wetting, p)
    }

    /// The distance at world point `p` (between the nodes round it), in pixels.
    pub fn at(&self, p: Vec2) -> f32 {
        self.between(&self.distance, p)
    }

    /// `values` (one a node) at world point `p`, between the nodes round it.
    fn between(&self, values: &[f32], p: Vec2) -> f32 {
        let l = &self.layout;
        let g = l.to_grid(p).clamp(Vec2::ZERO, Vec2::new(l.cols as f32 - 1.001, l.rows as f32 - 1.001));
        let (x, y) = (g.x.floor() as usize, g.y.floor() as usize);
        let f = g - Vec2::new(x as f32, y as f32);
        let at = |x: usize, y: usize| values[y * l.cols as usize + x];
        let top = at(x, y) + (at(x + 1, y) - at(x, y)) * f.x;
        let bottom = at(x, y + 1) + (at(x + 1, y + 1) - at(x, y + 1)) * f.x;
        top + (bottom - top) * f.y
    }

    /// The distances in cells, as the card takes them.
    pub fn in_cells(&self) -> Vec<f32> {
        self.distance.iter().map(|d| (d / self.layout.cell).clamp(-64.0, 64.0)).collect()
    }

    /// The field as the all-drops engine's card takes it: the distances in cells, then the
    /// wettings (0 to 1).
    pub fn for_card(&self) -> Vec<f32> {
        let mut out = self.in_cells();
        out.extend(self.wetting.iter().map(|w| if w.is_finite() { w.clamp(0.0, 1.0) } else { 0.0 }));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thin_wall_is_a_cell_thick_to_the_physics() {
        let line = Solid::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(100.0, 0.0), thickness: 3.0 };
        // At 8 px cells its face is 4 px off its middle, not 1.5.
        assert!((line.distance(Vec2::new(50.0, 10.0), 8.0) - 6.0).abs() < 1e-4);
        // A thick wall keeps its own thickness.
        let thick = Solid::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(100.0, 0.0), thickness: 20.0 };
        assert!((thick.distance(Vec2::new(50.0, 14.0), 8.0) - 4.0).abs() < 1e-4);
    }

    #[test]
    fn a_filled_shape_is_negative_inside() {
        let block = Solid::rect(Vec2::new(-10.0, -10.0), Vec2::new(10.0, 10.0), true, 0.0);
        assert!((block.distance(Vec2::ZERO, 8.0) + 10.0).abs() < 1e-4);
        assert!((block.distance(Vec2::new(15.0, 0.0), 8.0) - 5.0).abs() < 1e-4);
        let disc = Solid::Circle { centre: Vec2::ZERO, radius: 10.0, filled: true, thickness: 0.0 };
        assert!((disc.distance(Vec2::new(0.0, 4.0), 8.0) + 6.0).abs() < 1e-4);
    }

    #[test]
    fn a_field_carries_its_faces_wetting() {
        let layout = Layout::cover(Vec2::new(-40.0, -40.0), Vec2::new(40.0, 40.0), 8.0);
        let field = SolidField::build(layout, &[]);
        assert!(field.wetting.iter().all(|w| *w == DEFAULT_WETTING));
        assert_eq!(field.wetting.len(), layout.nodes());
        let wet = field.clone().with_wetting(0.7);
        assert!((wet.wetting_at(Vec2::new(3.0, 5.0)) - 0.7).abs() < 1e-6);
        assert_eq!(field.clone().with_wetting(3.0).wetting[0], 1.0);
        assert_eq!(field.clone().with_wetting(f32::NAN).wetting[0], DEFAULT_WETTING);
        let card = wet.for_card();
        assert_eq!(card.len(), 2 * layout.nodes());
        assert_eq!(&card[..layout.nodes()], &wet.in_cells()[..]);
        assert!(card[layout.nodes()..].iter().all(|w| (*w - 0.7).abs() < 1e-6));
    }

    #[test]
    fn a_polyline_is_round_or_square_at_its_ends() {
        let points = vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)];
        let round = Solid::Polyline { points: points.clone(), thickness: 20.0, square: false };
        let square = Solid::Polyline { points, thickness: 20.0, square: true };
        let near = |a: f32, b: f32| assert!((a - b).abs() < 1e-3, "{a} is not {b}");
        // Along the middle of a run: half the thickness off its line.
        near(round.distance(Vec2::new(50.0, 25.0), 1.0), 15.0);
        near(square.distance(Vec2::new(50.0, 25.0), 1.0), 15.0);
        // Past the first point: a round end is a half disc, a square one flat half a thickness on.
        near(round.distance(Vec2::new(-20.0, 0.0), 1.0), 10.0);
        near(square.distance(Vec2::new(-20.0, 0.0), 1.0), 10.0);
        near(round.distance(Vec2::new(-10.0, 10.0), 1.0), 200f32.sqrt() - 10.0);
        near(square.distance(Vec2::new(-10.0, 10.0), 1.0), 0.0);
        near(square.distance(Vec2::new(-5.0, 0.0), 1.0), -5.0);
        // The last end the same, and the bend round outside.
        near(square.distance(Vec2::new(100.0, 125.0), 1.0), 15.0);
        near(round.distance(Vec2::new(120.0, -20.0), 1.0), 800f32.sqrt() - 10.0);
        // Inside: below 0.
        assert!(square.distance(Vec2::new(100.0, 50.0), 1.0) < -9.0);
        // Thin ones are taken as `least` thick.
        near(
            Solid::Polyline { points: vec![Vec2::ZERO, Vec2::X * 50.0], thickness: 2.0, square: false }
                .distance(Vec2::new(25.0, 10.0), 8.0),
            6.0,
        );
        let (lo, hi) = square.bounds(1.0).unwrap();
        assert!(lo.x <= -10.0 && lo.y <= -10.0 && hi.x >= 110.0 && hi.y >= 110.0);
    }

    fn open_field() -> SolidField {
        let layout = Layout::cover(Vec2::new(-200.0, -200.0), Vec2::new(200.0, 200.0), 4.0);
        SolidField::build(layout, &[]).with_wetting(1.0)
    }

    fn node(field: &SolidField, p: Vec2) -> usize {
        let g = field.layout.to_grid(p).round();
        g.y as usize * field.layout.cols as usize + g.x as usize
    }

    #[test]
    fn a_part_adds_its_solid_and_its_wetting() {
        let mut field = open_field();
        let block = Part {
            solids: vec![Solid::rect(Vec2::new(-20.0, -20.0), Vec2::new(20.0, 20.0), true, 0.0)],
            wetting: 0.0,
            ..Part::default()
        };
        field.add_part(&block, 1, 4.0);
        let at = |p: Vec2| node(&field, p);
        assert!((field.distance[at(Vec2::ZERO)] + 20.0).abs() < 1e-3);
        assert_eq!(field.owner[at(Vec2::ZERO)], 1);
        assert_eq!(field.wetting[at(Vec2::ZERO)], 0.0);
        // Open water nearer it than anything else is its, and wears its Wetting.
        let beside = at(Vec2::new(28.0, 0.0));
        assert!((field.distance[beside] - 8.0).abs() < 1e-3);
        assert_eq!((field.owner[beside], field.wetting[beside]), (1, 0.0));
        // Far off nothing changed.
        let far = at(Vec2::new(-180.0, -180.0));
        assert_eq!((field.owner[far], field.wetting[far], field.distance[far]), (0, 1.0, SolidField::FAR));
    }

    #[test]
    fn a_cutting_part_opens_the_solid_and_lines_it() {
        let layout = Layout::cover(Vec2::new(-200.0, -200.0), Vec2::new(200.0, 200.0), 4.0);
        let rock = Solid::rect(Vec2::new(-300.0, -300.0), Vec2::new(300.0, 300.0), true, 0.0);
        let mut field = SolidField::build(layout, &[rock]).with_wetting(1.0);
        let tunnel = Part {
            cut: vec![Solid::rect(Vec2::new(-100.0, -10.0), Vec2::new(100.0, 10.0), true, 0.0)],
            wetting: 0.25,
            ..Part::default()
        };
        field.add_part(&tunnel, 3, 4.0);
        let at = |p: Vec2| node(&field, p);
        // Open in the tunnel, its walls the cutter's.
        assert!((field.distance[at(Vec2::ZERO)] - 10.0).abs() < 1e-3);
        assert_eq!((field.owner[at(Vec2::ZERO)], field.wetting[at(Vec2::ZERO)]), (3, 0.25));
        let wall = at(Vec2::new(0.0, 16.0));
        assert!((field.distance[wall] + 6.0).abs() < 1e-3);
        assert_eq!((field.owner[wall], field.wetting[wall]), (3, 0.25));
        // Deeper in the rock than the lining, the rock's own.
        let deep = at(Vec2::new(0.0, 40.0));
        assert!(field.distance[deep] < 0.0);
        assert_eq!((field.owner[deep], field.wetting[deep]), (0, 1.0));
        // A later part adds solid back over the cut.
        let plug = Part {
            solids: vec![Solid::Circle { centre: Vec2::ZERO, radius: 5.0, filled: true, thickness: 0.0 }],
            wetting: 0.5,
            ..Part::default()
        };
        let middle = at(Vec2::ZERO);
        field.add_part(&plug, 4, 4.0);
        assert!((field.distance[middle] + 5.0).abs() < 1e-3);
        assert_eq!(field.owner[middle], 4);
    }

    #[test]
    fn a_part_touches_only_its_band() {
        let layout = Layout::cover(Vec2::new(-400.0, -400.0), Vec2::new(400.0, 400.0), 4.0);
        let before =
            SolidField::build(layout, &[Solid::rect(Vec2::new(-500.0, 0.0), Vec2::new(500.0, 500.0), true, 0.0)]);
        let mut after = before.clone();
        let line = Part {
            solids: vec![Solid::Polyline {
                points: vec![Vec2::new(-10.0, -100.0), Vec2::new(10.0, -100.0)],
                thickness: 8.0,
                square: true,
            }],
            wetting: 0.0,
            ..Part::default()
        };
        after.add_part(&line, 1, 6.0);
        let mut changed = 0;
        for y in 0..layout.rows {
            for x in 0..layout.cols {
                let i = (y * layout.cols + x) as usize;
                let p = layout.to_world(Vec2::new(x as f32, y as f32));
                let off = (p - Vec2::new(0.0, -100.0)).abs()
                    - Vec2::new(10.0 + 8.0 + SolidField::BAND + 4.0, 8.0 + SolidField::BAND + 4.0);
                if off.x > 0.0 || off.y > 0.0 {
                    assert_eq!(after.distance[i].to_bits(), before.distance[i].to_bits(), "node {x}, {y} changed");
                    assert_eq!((after.owner[i], after.wetting[i]), (before.owner[i], before.wetting[i]));
                } else if after.distance[i] != before.distance[i] {
                    changed += 1;
                }
            }
        }
        assert!(changed > 100);
    }

    #[test]
    fn a_part_with_nothing_in_it_changes_nothing() {
        let before = open_field();
        let mut after = before.clone();
        let broken = Part {
            solids: vec![
                Solid::Polygon { points: vec![Vec2::ZERO, Vec2::X], filled: true, thickness: 0.0 },
                Solid::Polyline { points: vec![Vec2::ZERO], thickness: 5.0, square: false },
                Solid::Line { a: Vec2::new(f32::NAN, 0.0), b: Vec2::X, thickness: 5.0 },
                Solid::Circle { centre: Vec2::ZERO, radius: 0.0, filled: true, thickness: 0.0 },
            ],
            cut: vec![Solid::Polygon { points: vec![], filled: true, thickness: 0.0 }],
            wetting: f32::NAN,
        };
        after.add_part(&broken, 1, 4.0);
        after.add_part(&Part::default(), 2, 4.0);
        assert_eq!(after, before);
        // Far past the layout, or huge, it is kept to the layout.
        let huge = Part {
            solids: vec![Solid::rect(Vec2::splat(-1e6), Vec2::splat(1e6), true, 0.0)],
            wetting: 0.5,
            ..Part::default()
        };
        after.add_part(&huge, 1, 4.0);
        assert!(after.distance.iter().all(|d| *d < 0.0));
        let mut far = before.clone();
        far.add_part(
            &Part {
                solids: vec![Solid::rect(Vec2::splat(1e5), Vec2::splat(1e5 + 10.0), true, 0.0)],
                ..Part::default()
            },
            1,
            4.0,
        );
        assert_eq!(far, before);
    }

    #[test]
    fn a_field_reads_between_its_nodes() {
        let layout = Layout::cover(Vec2::new(-40.0, -40.0), Vec2::new(40.0, 40.0), 8.0);
        let field =
            SolidField::build(layout, &[Solid::rect(Vec2::new(-100.0, 20.0), Vec2::new(100.0, 100.0), true, 0.0)]);
        assert!((field.at(Vec2::new(3.0, 5.0)) - 15.0).abs() < 1e-3);
        assert!((field.at(Vec2::new(3.0, 23.0)) + 3.0).abs() < 1e-3);
    }
}
