//! The solid field: for every node of the liquid's grid, how far the nearest solid's face is
//! (pixels, below 0 inside the solid). The physics reads only this, so whatever makes a course
//! (carving in part 2b, pieces and pen walls in part 3) only has to turn itself into one.
//!
//! **Thin solids are thickened for the physics.** A hollow wall or a line is taken as at least one
//! cell thick. With that, a wall is always seen by the nodes round it, the distance's slope is a
//! usable normal wherever a drop can be, and no drop moving in sub-moves of under half a cell can
//! reach a wall's middle line. (At draft detail a pen wall thinner than a cell therefore holds
//! the water a little off itself; the editor's trouble marks say so, in a later part.)

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
        }
    }
}

/// How far `p` is from the segment `a` to `b`.
fn segment(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length()
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
        SolidField { layout, distance, wetting }
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
        SolidField { layout, distance: out, wetting }
    }

    /// The same field with every face `wetting` wet (0 to 1).
    pub fn with_wetting(mut self, wetting: f32) -> SolidField {
        let w = if wetting.is_finite() { wetting.clamp(0.0, 1.0) } else { DEFAULT_WETTING };
        self.wetting.iter_mut().for_each(|v| *v = w);
        self
    }

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
    fn a_field_reads_between_its_nodes() {
        let layout = Layout::cover(Vec2::new(-40.0, -40.0), Vec2::new(40.0, 40.0), 8.0);
        let field =
            SolidField::build(layout, &[Solid::rect(Vec2::new(-100.0, 20.0), Vec2::new(100.0, 100.0), true, 0.0)]);
        assert!((field.at(Vec2::new(3.0, 5.0)) - 15.0).abs() < 1e-3);
        assert!((field.at(Vec2::new(3.0, 23.0)) + 3.0).abs() < 1e-3);
    }
}
