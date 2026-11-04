//! A Shape's moulded outline: corners, and sides that are straight or cubic curves.
//!
//! Everything here is in **shape units**: the shape's own frame, x right, y the same way as world
//! y, the origin at the shape's middle, not turned. The even polygon's corners lie on the unit
//! circle; the Shape's Size multiplies.

use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// How many straight pieces a curved side is cut into for collision and drawing.
pub const FLATTEN: usize = 16;

/// A point that sits this close to the up-down middle line (in shape units) counts as on it.
pub const ON_THE_LINE: f32 = 0.005;

/// How far (shape units) a mirror partner may be from the dragged point's mirror image.
pub const MIRROR_REACH: f32 = 0.25;

/// A closed outline in shape units. Empty means "not moulded": the shape is its even polygon or
/// its circle.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShapePath {
    /// The corners, in order around the shape.
    pub points: Vec<[f32; 2]>,
    /// One per side, the side from point i to point i + 1 (the last closes to point 0):
    /// None = straight, Some([c1, c2]) = a cubic Bezier with those two bend handles.
    pub bends: Vec<Option<[[f32; 2]; 2]>>,
}

fn v(p: [f32; 2]) -> Vec2 {
    Vec2::from(p)
}

fn a(p: Vec2) -> [f32; 2] {
    p.into()
}

fn cubic(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    p0 * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

/// The nearest point to `q` on the segment `a`..`b`: (t, point, distance).
pub fn nearest_on_segment(a: Vec2, b: Vec2, q: Vec2) -> (f32, Vec2, f32) {
    let d = b - a;
    let len2 = d.length_squared();
    let t = if len2 > 0.0 { ((q - a).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
    let p = a + d * t;
    (t, p, (q - p).length())
}

/// Whether `q` is inside the closed polygon `points`, counting crossings (even-odd), so a
/// self-crossing outline still has an inside.
pub fn inside_polygon(points: &[Vec2], q: Vec2) -> bool {
    let mut inside = false;
    let n = points.len();
    for i in 0..n {
        let (p, r) = (points[i], points[(i + 1) % n]);
        if (p.y > q.y) != (r.y > q.y) {
            let x = p.x + (q.y - p.y) / (r.y - p.y) * (r.x - p.x);
            if q.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

impl ShapePath {
    /// Whether every number in it is finite.
    pub fn is_finite(&self) -> bool {
        self.points.iter().flatten().all(|x| x.is_finite())
            && self.bends.iter().flatten().flatten().flatten().all(|x| x.is_finite())
    }

    /// Empty, or at least 3 corners with one bend per side and every number finite.
    pub fn is_valid(&self) -> bool {
        (self.points.is_empty() && self.bends.is_empty())
            || (self.points.len() >= 3 && self.bends.len() == self.points.len() && self.is_finite())
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The even polygon of `sides` corners on the unit circle, all sides straight. Its corners are
    /// the Arena polygon's (`Outline::Ring` turned a quarter, which the runtime's tests pin): a
    /// corner at the top when the count is odd, a flat top and bottom when it is even. The first
    /// corner is the top-most one (the left of two), and they go clockwise.
    pub fn even(sides: u32) -> ShapePath {
        let n = sides.max(3);
        let offset = if n % 2 == 0 { PI / n as f32 } else { 0.0 };
        let points = (0..n)
            .map(|k| {
                let angle = -FRAC_PI_2 - offset + k as f32 * TAU / n as f32;
                [angle.cos(), angle.sin()]
            })
            .collect();
        ShapePath { points, bends: vec![None; n as usize] }
    }

    /// The unit circle as 8 curved sides, starting at the top: each side a cubic whose handles are
    /// tangent at the corners, which traces the circle to within 0.01%.
    pub fn circle() -> ShapePath {
        let n = 8;
        let handle = 4.0 / 3.0 * (TAU / 32.0).tan();
        let corner = |k: usize| {
            let angle = -FRAC_PI_2 + k as f32 * TAU / n as f32;
            (Vec2::new(angle.cos(), angle.sin()), Vec2::new(-angle.sin(), angle.cos()))
        };
        let mut path = ShapePath::default();
        for k in 0..n {
            let (p, tangent) = corner(k);
            let (q, next_tangent) = corner((k + 1) % n);
            path.points.push(a(p));
            path.bends.push(Some([a(p + tangent * handle), a(q - next_tangent * handle)]));
        }
        path
    }

    pub fn side_count(&self) -> usize {
        self.points.len()
    }

    pub fn curve_count(&self) -> usize {
        self.bends.iter().filter(|b| b.is_some()).count()
    }

    /// How the outline reads in a settings row: "7 points, 1 curve".
    pub fn words(&self) -> String {
        let n = self.points.len();
        let points = if n == 1 { "1 point".to_owned() } else { format!("{n} points") };
        match self.curve_count() {
            0 => points,
            1 => format!("{points}, 1 curve"),
            c => format!("{points}, {c} curves"),
        }
    }

    pub fn point(&self, i: usize) -> Vec2 {
        v(self.points[i])
    }

    /// The side's two corners, its own and the next.
    pub fn side_ends(&self, side: usize) -> (Vec2, Vec2) {
        (self.point(side), self.point((side + 1) % self.points.len()))
    }

    /// The point `t` (0 to 1) along a side.
    pub fn side_point(&self, side: usize, t: f32) -> Vec2 {
        let (p0, p3) = self.side_ends(side);
        match self.bends[side] {
            None => p0.lerp(p3, t),
            Some([c1, c2]) => cubic(p0, v(c1), v(c2), p3, t),
        }
    }

    /// The nearest point to `q` on a side: (t, point, distance). Exact for a straight side; a
    /// curve is sampled at 32 t, then refined around the best by ternary search.
    pub fn nearest_on_side(&self, side: usize, q: Vec2) -> (f32, Vec2, f32) {
        let (p0, p3) = self.side_ends(side);
        if self.bends[side].is_none() {
            return nearest_on_segment(p0, p3, q);
        }
        let dist = |t: f32| (self.side_point(side, t) - q).length();
        let mut best = 0.0;
        let mut best_d = f32::INFINITY;
        for k in 0..=32 {
            let t = k as f32 / 32.0;
            let d = dist(t);
            if d < best_d {
                (best, best_d) = (t, d);
            }
        }
        let (mut lo, mut hi) = ((best - 1.0 / 32.0).max(0.0), (best + 1.0 / 32.0).min(1.0));
        for _ in 0..24 {
            let m1 = lo + (hi - lo) / 3.0;
            let m2 = hi - (hi - lo) / 3.0;
            if dist(m1) < dist(m2) {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        let t = (lo + hi) * 0.5;
        let (t, d) = if dist(t) < best_d { (t, dist(t)) } else { (best, best_d) };
        (t, self.side_point(side, t), d)
    }

    /// The side nearest `q`: (side, t, distance).
    pub fn nearest_side(&self, q: Vec2) -> Option<(usize, f32, f32)> {
        (0..self.points.len())
            .map(|s| {
                let (t, _, d) = self.nearest_on_side(s, q);
                (s, t, d)
            })
            .min_by(|x, y| x.2.total_cmp(&y.2))
    }

    /// The outline as a closed list of points: each corner, then for a curved side
    /// `per_curve - 1` points inside it at equal t.
    pub fn flatten(&self, per_curve: usize) -> Vec<Vec2> {
        let mut out = Vec::with_capacity(self.points.len() * 2);
        for side in 0..self.points.len() {
            out.push(self.point(side));
            if self.bends[side].is_some() {
                for k in 1..per_curve.max(1) {
                    out.push(self.side_point(side, k as f32 / per_curve as f32));
                }
            }
        }
        out
    }

    /// A new corner `t` along a side. A curved side is split there so the outline is unchanged.
    pub fn add_point(&mut self, side: usize, t: f32) -> bool {
        if side >= self.points.len() || !t.is_finite() {
            return false;
        }
        let t = t.clamp(0.0, 1.0);
        let (p0, p3) = self.side_ends(side);
        match self.bends[side] {
            None => {
                self.points.insert(side + 1, a(p0.lerp(p3, t)));
                self.bends.insert(side + 1, None);
            }
            Some([c1, c2]) => {
                let (c1, c2) = (v(c1), v(c2));
                let (q0, q1, q2) = (p0.lerp(c1, t), c1.lerp(c2, t), c2.lerp(p3, t));
                let (r0, r1) = (q0.lerp(q1, t), q1.lerp(q2, t));
                let s = r0.lerp(r1, t);
                self.bends[side] = Some([a(q0), a(r0)]);
                self.points.insert(side + 1, a(s));
                self.bends.insert(side + 1, Some([a(r1), a(q2)]));
            }
        }
        true
    }

    /// Removes a corner (never below 3). The new side keeps the outer handles of the two sides it
    /// replaces when either was curved (a straight one gives its outer corner), else it is straight.
    pub fn delete_point(&mut self, i: usize) -> bool {
        let n = self.points.len();
        if n <= 3 || i >= n {
            return false;
        }
        let prev = (i + n - 1) % n;
        let next_corner = self.point((i + 1) % n);
        let prev_corner = self.point(prev);
        let bend = match (self.bends[prev], self.bends[i]) {
            (None, None) => None,
            (b1, b2) => {
                let c1 = b1.map_or(prev_corner, |b| v(b[0]));
                let c2 = b2.map_or(next_corner, |b| v(b[1]));
                Some([a(c1), a(c2)])
            }
        };
        self.points.remove(i);
        self.bends.remove(i);
        let prev = if prev > i { prev - 1 } else { prev };
        self.bends[prev] = bend;
        true
    }

    /// Bends a straight side: handles at 1/3 and 2/3 along it, pushed away from the shape's middle
    /// by a quarter of its length.
    pub fn create_curve(&mut self, side: usize) -> bool {
        if side >= self.points.len() || self.bends[side].is_some() {
            return false;
        }
        let (p0, p3) = self.side_ends(side);
        let d = p3 - p0;
        let len = d.length();
        if len <= 0.0 {
            return false;
        }
        let mut n = d.perp() / len;
        let middle_side = n.dot(-p0);
        if middle_side > 1e-6 * len {
            n = -n;
        }
        let push = n * 0.25 * len;
        self.bends[side] = Some([a(p0 + d / 3.0 + push), a(p0 + d * (2.0 / 3.0) + push)]);
        true
    }

    /// Makes a curved side straight.
    pub fn straighten(&mut self, side: usize) -> bool {
        side < self.bends.len() && self.bends[side].take().is_some()
    }

    pub fn move_point(&mut self, i: usize, to: Vec2) -> bool {
        if i >= self.points.len() || !to.is_finite() || self.point(i) == to {
            return false;
        }
        self.points[i] = a(to);
        true
    }

    /// Moves both corners of a side (and its own bend handles) by `by`. The handles of the sides
    /// next to it stay where they are.
    pub fn move_side(&mut self, side: usize, by: Vec2) -> bool {
        let n = self.points.len();
        if side >= n || !by.is_finite() || by == Vec2::ZERO {
            return false;
        }
        for i in [side, (side + 1) % n] {
            self.points[i] = a(self.point(i) + by);
        }
        if let Some(b) = &mut self.bends[side] {
            for h in b.iter_mut() {
                *h = a(v(*h) + by);
            }
        }
        true
    }

    /// Moves one bend handle (0 or 1) of a curved side.
    pub fn move_bend(&mut self, side: usize, which: usize, to: Vec2) -> bool {
        if which > 1 || !to.is_finite() {
            return false;
        }
        match self.bends.get_mut(side) {
            Some(Some(b)) if v(b[which]) != to => {
                b[which] = a(to);
                true
            }
            _ => false,
        }
    }

    /// The other corner nearest the mirror image (x → −x) of corner `i`, within [`MIRROR_REACH`].
    /// None when corner `i` is on the middle line or nothing is near its image.
    pub fn mirror_partner_point(&self, i: usize) -> Option<usize> {
        let p = self.point(i);
        if p.x.abs() < ON_THE_LINE {
            return None;
        }
        let image = Vec2::new(-p.x, p.y);
        (0..self.points.len())
            .filter(|&j| j != i && self.point(j).x.abs() >= ON_THE_LINE)
            .map(|j| (j, (self.point(j) - image).length()))
            .filter(|&(_, d)| d <= MIRROR_REACH)
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(j, _)| j)
    }

    /// The other bend handle nearest the mirror image of handle `which` of `side`, as
    /// (side, which), within [`MIRROR_REACH`].
    pub fn mirror_partner_bend(&self, side: usize, which: usize) -> Option<(usize, usize)> {
        let h = v(self.bends.get(side).copied().flatten()?[which.min(1)]);
        if h.x.abs() < ON_THE_LINE {
            return None;
        }
        let image = Vec2::new(-h.x, h.y);
        self.bends
            .iter()
            .enumerate()
            .filter_map(|(s, b)| b.map(|b| (s, b)))
            .flat_map(|(s, b)| [(s, 0, v(b[0])), (s, 1, v(b[1]))])
            .filter(|&(s, w, _)| (s, w) != (side, which))
            .map(|(s, w, q)| ((s, w), (q - image).length()))
            .filter(|&(_, d)| d <= MIRROR_REACH)
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(sw, _)| sw)
    }

    /// The half size when the outline is 4 straight sides making a rectangle lined up with the
    /// shape's own axes, centred on its middle.
    pub fn is_axis_rectangle(&self) -> Option<Vec2> {
        if self.points.len() != 4 || self.bends.iter().any(|b| b.is_some()) {
            return None;
        }
        let half = v(self.points[0]).abs();
        let tol = 1e-6 * half.max_element().max(1e-12);
        if half.min_element() <= tol {
            return None;
        }
        let mut signs = [false; 4];
        let mut quadrant = [0usize; 4];
        for (i, p) in self.points.iter().enumerate() {
            let p = v(*p);
            if (p.x.abs() - half.x).abs() > tol || (p.y.abs() - half.y).abs() > tol {
                return None;
            }
            let q = (p.x > 0.0) as usize + 2 * (p.y > 0.0) as usize;
            signs[q] = true;
            quadrant[i] = q;
        }
        // Every corner once, and each side changes one sign only (no bow tie).
        let all = signs.iter().all(|s| *s);
        let sides_ok = (0..4).all(|i| (quadrant[i] ^ quadrant[(i + 1) % 4]).count_ones() == 1);
        (all && sides_ok).then_some(half)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hexagon() -> ShapePath {
        ShapePath::even(6)
    }

    /// The outline sampled at `per` points per side.
    fn trace(p: &ShapePath, per: usize) -> Vec<Vec2> {
        (0..p.side_count()).flat_map(|s| (0..per).map(move |k| p.side_point(s, k as f32 / per as f32))).collect()
    }

    fn distance_to(p: &ShapePath, q: Vec2) -> f32 {
        p.nearest_side(q).unwrap().2
    }

    #[test]
    fn even_polygons_have_their_corners_on_the_unit_circle() {
        for n in 3..=12 {
            let p = ShapePath::even(n);
            assert_eq!(p.side_count(), n as usize);
            assert!(p.is_valid());
            assert!(p.points.iter().all(|c| (v(*c).length() - 1.0).abs() < 1e-6));
        }
        // Odd counts put a corner at the top.
        assert!((ShapePath::even(3).point(0) - Vec2::new(0.0, -1.0)).length() < 1e-6);
        // Even counts sit flat on their top, with corners at left and right for a hexagon.
        let h = hexagon();
        assert!((h.point(0).y - h.point(1).y).abs() < 1e-6);
        assert!(h.points.iter().any(|c| (v(*c) - Vec2::X).length() < 1e-6));
    }

    #[test]
    fn the_circle_traces_the_unit_circle() {
        let c = ShapePath::circle();
        assert_eq!((c.side_count(), c.curve_count()), (8, 8));
        assert!((c.point(0) - Vec2::new(0.0, -1.0)).length() < 1e-6);
        for p in c.flatten(64) {
            assert!((p.length() - 1.0).abs() < 1e-4, "{p} at {}", p.length());
        }
    }

    #[test]
    fn adding_a_point_on_a_curve_keeps_the_outline() {
        let mut c = ShapePath::circle();
        let before = trace(&c, 64);
        assert!(c.add_point(2, 0.3));
        assert_eq!((c.side_count(), c.curve_count()), (9, 9));
        for q in &before {
            assert!(distance_to(&c, *q) < 1e-4, "{q}");
        }
        for q in trace(&c, 64) {
            assert!((q.length() - 1.0).abs() < 1e-4);
        }
        // On a straight side the point is on the line.
        let mut h = hexagon();
        let (p0, p1) = h.side_ends(5);
        assert!(h.add_point(5, 0.5));
        assert_eq!(h.side_count(), 7);
        assert!((h.point(6) - (p0 + p1) * 0.5).length() < 1e-6);
        assert_eq!(h.point(0), p1);
    }

    #[test]
    fn deleting_a_point_refuses_at_three_and_keeps_outer_handles() {
        let mut t = ShapePath::even(3);
        assert!(!t.delete_point(0));
        let mut sq = ShapePath::even(4);
        sq.create_curve(0);
        let outer = sq.bends[0].unwrap()[0];
        assert!(sq.delete_point(1));
        assert_eq!(sq.side_count(), 3);
        // The new side 0 runs from old corner 0 to old corner 2, keeping side 0's first handle;
        // the straight side 1 gives its outer corner as the second handle.
        let b = sq.bends[0].unwrap();
        assert_eq!(b[0], outer);
        assert_eq!(b[1], sq.points[1]);
        // Two straight sides make a straight one.
        let mut h = hexagon();
        assert!(h.delete_point(0));
        assert_eq!(h.curve_count(), 0);
        assert!(h.is_valid());
        // Deleting the first corner when the last side is curved bends the new closing side.
        let mut h = hexagon();
        h.create_curve(5);
        let first = h.bends[5].unwrap()[0];
        assert!(h.delete_point(0));
        assert_eq!(h.bends[4].unwrap()[0], first);
        assert!(h.is_valid());
    }

    #[test]
    fn a_new_curve_bulges_away_from_the_middle() {
        let mut h = hexagon();
        assert!(h.create_curve(1));
        assert!(!h.create_curve(1), "already curved");
        let (p0, p1) = h.side_ends(1);
        let mid_line = (p0 + p1) * 0.5;
        let bulge = h.side_point(1, 0.5);
        assert!(bulge.length() > mid_line.length() + 0.1, "{bulge} vs {mid_line}");
        assert!(h.straighten(1));
        assert!(!h.straighten(1));
        assert_eq!(h.curve_count(), 0);
        // A side through the middle bulges to its left.
        let mut odd = ShapePath {
            points: vec![[-1.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            bends: vec![None; 3],
        };
        assert!(odd.create_curve(0));
        assert!(odd.side_point(0, 0.5).dot(Vec2::X.perp()) > 0.0);
    }

    #[test]
    fn moving_a_side_moves_both_corners_and_its_own_handles() {
        let mut h = hexagon();
        h.create_curve(1);
        h.create_curve(2);
        let before = h.clone();
        let by = Vec2::new(0.1, -0.2);
        assert!(h.move_side(1, by));
        for i in 0..6 {
            let expected = if i == 1 || i == 2 { before.point(i) + by } else { before.point(i) };
            assert!((h.point(i) - expected).length() < 1e-6, "corner {i}");
        }
        let (b, a2) = (before.bends[1].unwrap(), h.bends[1].unwrap());
        assert!((v(a2[0]) - v(b[0]) - by).length() < 1e-6);
        assert_eq!(h.bends[2], before.bends[2], "the next side's handles stay");
        assert!(!h.move_side(1, Vec2::ZERO));
        assert!(h.move_point(0, Vec2::new(0.0, -2.0)));
        assert_eq!(h.points[0], [0.0, -2.0]);
        assert!(h.move_bend(2, 1, Vec2::new(3.0, 3.0)));
        assert_eq!(h.bends[2].unwrap()[1], [3.0, 3.0]);
        assert!(!h.move_bend(0, 0, Vec2::ONE), "a straight side has no handles");
    }

    #[test]
    fn mirror_partners_on_a_symmetric_shape() {
        let h = hexagon();
        for i in 0..6 {
            let j = h.mirror_partner_point(i).expect("every hexagon corner has a partner");
            let (p, q) = (h.point(i), h.point(j));
            assert!((p.x + q.x).abs() < 1e-5 && (p.y - q.y).abs() < 1e-5);
        }
        // A triangle's top corner is on the line.
        let t = ShapePath::even(3);
        assert_eq!(t.mirror_partner_point(0), None);
        assert_eq!(t.mirror_partner_point(1), Some(2));
        // Nothing near the image: no partner.
        let mut lop = t.clone();
        lop.move_point(2, Vec2::new(-0.2, 0.0));
        assert_eq!(lop.mirror_partner_point(1), None);
        // Handles of the circle mirror across.
        let c = ShapePath::circle();
        let (s, w) = c.mirror_partner_bend(0, 0).unwrap();
        let (h0, h1) = (v(c.bends[0].unwrap()[0]), v(c.bends[s].unwrap()[w]));
        assert!((h0.x + h1.x).abs() < 1e-5 && (h0.y - h1.y).abs() < 1e-5, "{h0} {h1}");
    }

    #[test]
    fn axis_rectangles_are_found() {
        let r = ShapePath {
            points: vec![[-0.5, -0.8], [0.5, -0.8], [0.5, 0.8], [-0.5, 0.8]],
            bends: vec![None; 4],
        };
        assert_eq!(r.is_axis_rectangle(), Some(Vec2::new(0.5, 0.8)));
        assert_eq!(ShapePath::even(4).is_axis_rectangle().map(|h| (h.x - h.y).abs() < 1e-6), Some(true));
        let mut bow = r.clone();
        bow.points.swap(2, 3);
        assert_eq!(bow.is_axis_rectangle(), None);
        let mut curved = r.clone();
        curved.create_curve(0);
        assert_eq!(curved.is_axis_rectangle(), None);
        assert_eq!(hexagon().is_axis_rectangle(), None);
        let mut off = r;
        off.move_point(0, Vec2::new(-0.5, -0.7));
        assert_eq!(off.is_axis_rectangle(), None);
    }

    #[test]
    fn nearest_side_and_inside() {
        let h = hexagon();
        let (side, _, d) = h.nearest_side(Vec2::new(0.0, -2.0)).unwrap();
        assert_eq!(side, 0, "the flat top");
        assert!((d - (2.0 - (PI / 6.0).cos())).abs() < 1e-5);
        let flat = h.flatten(FLATTEN);
        assert!(inside_polygon(&flat, Vec2::ZERO));
        assert!(!inside_polygon(&flat, Vec2::new(1.1, 0.0)));
        let c = ShapePath::circle();
        assert_eq!(c.flatten(FLATTEN).len(), 8 * FLATTEN);
        let (_, p, d) = c.nearest_on_side(3, Vec2::new(2.0, 2.0));
        assert!((p.length() - 1.0).abs() < 1e-3 && (d - (8f32.sqrt() - 1.0)).abs() < 1e-3);
    }

    #[test]
    fn validity() {
        assert!(ShapePath::default().is_valid());
        let two = ShapePath { points: vec![[0.0, 0.0], [1.0, 0.0]], bends: vec![None; 2] };
        assert!(!two.is_valid());
        let mut nan = hexagon();
        nan.points[2][0] = f32::NAN;
        assert!(!nan.is_valid());
        let mut short = hexagon();
        short.bends.pop();
        assert!(!short.is_valid());
        assert_eq!(hexagon().words(), "6 points");
        let mut one = hexagon();
        one.create_curve(0);
        one.add_point(3, 0.5);
        assert_eq!(one.words(), "7 points, 1 curve");
        assert_eq!(ShapePath::circle().words(), "8 points, 8 curves");
    }
}
