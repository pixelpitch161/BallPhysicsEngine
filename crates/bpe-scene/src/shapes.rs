//! Custom shapes: outlines made of straight lines and curves that can be the arena itself or
//! an obstacle inside it.
//!
//! A shape is drawn on the preview, imported from an SVG file or traced from a picture. Its
//! points are stored in the shape's own units, centred on its middle with the farthest point
//! 1 away, so the same shape can be placed anywhere at any size. Arenas and obstacles refer to
//! a shape by name and say where it goes and how big it is.

use crate::Vec2;
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

/// Curves are flattened into straight pieces no further than this from the true curve, in
/// pixels at the size the shape is drawn.
pub const CURVE_TOLERANCE: f32 = 0.35;
/// Most straight pieces one curve is split into, however big it is drawn.
const MAX_CURVE_PIECES: u32 = 256;

/// A named outline made of one or more paths.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shape {
    pub name: String,
    pub paths: Vec<Path>,
}

/// A run of lines and curves starting at `start`. A closed path joins its end back to its start.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Path {
    pub closed: bool,
    pub start: Vec2,
    pub segments: Vec<Segment>,
}

/// One piece of a path, ending at its last point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Segment {
    Line(Vec2),
    /// A curve pulled towards one control point.
    Quad(Vec2, Vec2),
    /// A curve with two control points, as drawing programs make.
    Cubic(Vec2, Vec2, Vec2),
}

/// A path flattened into straight pieces between `points`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Polyline {
    pub closed: bool,
    pub points: Vec<Vec2>,
}

/// Where a shape goes in the world: each point times `size`, turned by `rotation` radians
/// clockwise, then moved to `center`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub center: Vec2,
    pub size: f32,
    pub rotation: f32,
}

impl Placement {
    pub fn apply(&self, point: Vec2) -> Vec2 {
        self.center + Vec2::from_angle(self.rotation).rotate(point * self.size)
    }

    /// The point in the shape's own units that lands on `world`.
    pub fn invert(&self, world: Vec2) -> Vec2 {
        Vec2::from_angle(-self.rotation).rotate(world - self.center) / self.size.max(1e-6)
    }
}

impl Segment {
    pub fn end(&self) -> Vec2 {
        match *self {
            Segment::Line(p) | Segment::Quad(_, p) | Segment::Cubic(_, _, p) => p,
        }
    }

    /// Calls `f` on every point of the segment, control points included.
    pub fn map_points(&mut self, mut f: impl FnMut(Vec2) -> Vec2) {
        match self {
            Segment::Line(p) => *p = f(*p),
            Segment::Quad(a, p) => {
                *a = f(*a);
                *p = f(*p);
            }
            Segment::Cubic(a, b, p) => {
                *a = f(*a);
                *b = f(*b);
                *p = f(*p);
            }
        }
    }

    /// Appends points along the segment from `from` to its end (not including `from`), close
    /// enough that no point of the curve is more than `tolerance` from the straight pieces.
    fn flatten(&self, from: Vec2, tolerance: f32, out: &mut Vec<Vec2>) {
        let tolerance = tolerance.max(1e-6);
        match *self {
            Segment::Line(to) => out.push(to),
            Segment::Quad(c, to) => {
                // A quadratic curve strays at most a quarter of its control polygon's bend.
                let bend = (from - 2.0 * c + to).length();
                let pieces = pieces_for(bend * 0.25, tolerance);
                for i in 1..=pieces {
                    let t = i as f32 / pieces as f32;
                    let u = 1.0 - t;
                    out.push(from * (u * u) + c * (2.0 * u * t) + to * (t * t));
                }
            }
            Segment::Cubic(c1, c2, to) => {
                let bend = (from - 2.0 * c1 + c2).length().max((c1 - 2.0 * c2 + to).length());
                let pieces = pieces_for(bend * 0.75, tolerance);
                for i in 1..=pieces {
                    let t = i as f32 / pieces as f32;
                    let u = 1.0 - t;
                    out.push(from * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + to * (t * t * t));
                }
            }
        }
    }
}

/// Straight pieces needed so a curve whose second difference is `bend` strays no more than
/// `tolerance`: the error of n even pieces is about bend / n².
fn pieces_for(bend: f32, tolerance: f32) -> u32 {
    let n = (bend / tolerance).sqrt().ceil();
    if n.is_finite() { (n as u32).clamp(1, MAX_CURVE_PIECES) } else { 1 }
}

impl Path {
    /// A path through `points`, joined by straight lines.
    pub fn polygon(points: &[Vec2], closed: bool) -> Self {
        let start = points.first().copied().unwrap_or(Vec2::ZERO);
        Self { closed, start, segments: points.iter().skip(1).map(|&p| Segment::Line(p)).collect() }
    }

    /// Calls `f` on every point, control points included.
    pub fn map_points(&mut self, mut f: impl FnMut(Vec2) -> Vec2) {
        self.start = f(self.start);
        for segment in &mut self.segments {
            segment.map_points(&mut f);
        }
    }

    /// The path as straight pieces. `tolerance` is in the path's own units.
    pub fn flatten(&self, tolerance: f32) -> Polyline {
        let mut points = vec![self.start];
        let mut at = self.start;
        for segment in &self.segments {
            segment.flatten(at, tolerance, &mut points);
            at = segment.end();
        }
        // Points on top of each other make zero-length walls, which have no direction.
        points.dedup_by(|a, b| a.distance_squared(*b) < 1e-12);
        if self.closed && points.len() > 1 && points[0].distance_squared(points[points.len() - 1]) < 1e-12 {
            points.pop();
        }
        Polyline { closed: self.closed && points.len() >= 3, points }
    }
}

impl Polyline {
    /// Each straight piece as (from, to), including the closing piece of a closed line.
    pub fn pieces(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        let n = self.points.len();
        let count = if self.closed { n } else { n.saturating_sub(1) };
        (0..count).map(move |i| (self.points[i], self.points[(i + 1) % n]))
    }

    /// Area enclosed, positive when the points go clockwise on screen (y down).
    pub fn signed_area(&self) -> f32 {
        let n = self.points.len();
        if n < 3 {
            return 0.0;
        }
        (0..n).map(|i| self.points[i].perp_dot(self.points[(i + 1) % n])).sum::<f32>() * 0.5
    }

    /// How many times the outline winds around `point` (0 means outside).
    pub fn winding(&self, point: Vec2) -> i32 {
        if !self.closed {
            return 0;
        }
        let mut winding = 0;
        for (a, b) in self.pieces() {
            let side = (b - a).perp_dot(point - a);
            if a.y <= point.y {
                if b.y > point.y && side > 0.0 {
                    winding += 1;
                }
            } else if b.y <= point.y && side < 0.0 {
                winding -= 1;
            }
        }
        winding
    }
}

/// Whether `point` is inside the closed lines, counting holes: inside an odd number of them.
pub fn inside(lines: &[Polyline], point: Vec2) -> bool {
    lines.iter().filter(|line| line.winding(point) != 0).count() % 2 == 1
}

/// Removes points that stray less than `tolerance` from the straight line between their
/// neighbours (Douglas-Peucker). Closed lines keep at least three points.
pub fn simplify(points: &[Vec2], tolerance: f32, closed: bool) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    if closed {
        // Split at the point farthest from the first, so both halves are open runs.
        let far = (1..points.len())
            .max_by(|&a, &b| points[0].distance_squared(points[a]).total_cmp(&points[0].distance_squared(points[b])))
            .unwrap_or(1);
        let mut first = simplify_open(&points[..=far], tolerance);
        let mut second: Vec<Vec2> = points[far..].to_vec();
        second.push(points[0]);
        let second = simplify_open(&second, tolerance);
        first.pop();
        first.extend_from_slice(&second[..second.len() - 1]);
        if first.len() < 3 {
            return points.to_vec();
        }
        return first;
    }
    simplify_open(points, tolerance)
}

fn simplify_open(points: &[Vec2], tolerance: f32) -> Vec<Vec2> {
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (from, to) = (points[a], points[b]);
        let (mut far, mut worst) = (a, -1.0);
        for (i, &p) in points.iter().enumerate().take(b).skip(a + 1) {
            let d = distance_to_segment(p, from, to);
            if d > worst {
                (far, worst) = (i, d);
            }
        }
        if worst > tolerance {
            keep[far] = true;
            stack.push((a, far));
            stack.push((far, b));
        }
    }
    points.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

/// Distance from `p` to the straight piece from `a` to `b`.
pub fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    let t = if d == Vec2::ZERO { 0.0 } else { ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0) };
    p.distance(a + d * t)
}

impl Shape {
    /// Every path as straight pieces, flattened for drawing at `size` pixels.
    pub fn flatten(&self, size: f32) -> Vec<Polyline> {
        let tolerance = CURVE_TOLERANCE / size.abs().max(1e-3);
        self.paths.iter().map(|p| p.flatten(tolerance)).filter(|p| p.points.len() >= 2).collect()
    }

    /// Calls `f` on every point, control points included.
    pub fn map_points(&mut self, mut f: impl FnMut(Vec2) -> Vec2) {
        for path in &mut self.paths {
            path.map_points(&mut f);
        }
    }

    /// The smallest box around every point (control points included), as (min, max).
    pub fn bounds(&self) -> Option<(Vec2, Vec2)> {
        let mut bounds: Option<(Vec2, Vec2)> = None;
        let mut grow = |p: Vec2| {
            bounds = Some(match bounds {
                Some((lo, hi)) => (lo.min(p), hi.max(p)),
                None => (p, p),
            });
            p
        };
        self.clone().map_points(&mut grow);
        bounds
    }

    /// Moves and scales the shape so the middle of its bounding box is at the origin and its
    /// farthest flattened point is 1 away. Returns where the old points were: the old middle
    /// and the old farthest distance, so `Placement { center, size, .. }` puts it back.
    pub fn normalize(&mut self) -> Option<(Vec2, f32)> {
        let (lo, hi) = self.bounds()?;
        let middle = (lo + hi) * 0.5;
        let reach = self
            .flatten(1e4 / (hi - lo).max_element().max(1e-6))
            .iter()
            .flat_map(|line| line.points.iter())
            .map(|p| p.distance(middle))
            .fold(0.0f32, f32::max);
        if !reach.is_finite() || reach <= 1e-6 {
            return None;
        }
        self.map_points(|p| (p - middle) / reach);
        Some((middle, reach))
    }

    /// How far the shape's farthest point or curve handle is from its origin, in its own
    /// units. Curves never leave their handles' outline, so nothing drawn reaches farther. 1 or
    /// less for shapes made in the editor, which are normalized, but anything for one written
    /// by hand.
    pub fn reach(&self) -> f32 {
        let mut far = 0.0f32;
        self.clone().map_points(|p| {
            far = far.max(p.length());
            p
        });
        far
    }

    /// Whether the shape has at least one closed path with some area, so it can hold balls.
    pub fn is_closed(&self) -> bool {
        self.flatten(100.0).iter().any(|line| line.closed && line.signed_area().abs() > 1e-6)
    }

    /// For a closed shape around its middle, the distance from the middle to the nearest point
    /// of the outline: the biggest circle that fits inside, centred there. 0 if the middle is
    /// outside the shape.
    pub fn inner_radius(&self) -> f32 {
        let lines = self.flatten(1000.0);
        if !inside(&lines, Vec2::ZERO) {
            return 0.0;
        }
        lines
            .iter()
            .flat_map(|line| line.pieces())
            .map(|(a, b)| distance_to_segment(Vec2::ZERO, a, b))
            .fold(f32::MAX, f32::min)
            .min(1.0)
    }

    /// A heart, pointing down, with its farthest point 1 from the middle.
    pub fn heart() -> Self {
        let p = |x: f32, y: f32| Vec2::new(x, y);
        let path = Path {
            closed: true,
            start: p(0.0, -0.45),
            segments: vec![
                Segment::Cubic(p(0.2, -0.95), p(1.05, -0.85), p(0.95, -0.2)),
                Segment::Cubic(p(0.88, 0.3), p(0.3, 0.6), p(0.0, 1.0)),
                Segment::Cubic(p(-0.3, 0.6), p(-0.88, 0.3), p(-0.95, -0.2)),
                Segment::Cubic(p(-1.05, -0.85), p(-0.2, -0.95), p(0.0, -0.45)),
            ],
        };
        let mut shape = Self { name: "Heart".into(), paths: vec![path] };
        shape.normalize();
        shape
    }

    /// A star with `points` tips, the inner corners `inner` of the way out.
    pub fn star(points: u32, inner: f32) -> Self {
        let n = points.max(2) * 2;
        let corners: Vec<Vec2> = (0..n)
            .map(|i| {
                let angle = i as f32 / n as f32 * TAU - PI * 0.5;
                Vec2::from_angle(angle) * if i % 2 == 0 { 1.0 } else { inner }
            })
            .collect();
        Self { name: "Star".into(), paths: vec![Path::polygon(&corners, true)] }
    }

    /// A gentle S-shaped open line, as an obstacle to bounce off.
    pub fn wave() -> Self {
        let p = |x: f32, y: f32| Vec2::new(x, y);
        let path = Path {
            closed: false,
            start: p(-1.0, 0.0),
            segments: vec![Segment::Cubic(p(-0.5, -0.6), p(0.5, 0.6), p(1.0, 0.0))],
        };
        Self { name: "Wave".into(), paths: vec![path] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Polyline {
        let p = |x, y| Vec2::new(x, y);
        Polyline { closed: true, points: vec![p(-1.0, -1.0), p(1.0, -1.0), p(1.0, 1.0), p(-1.0, 1.0)] }
    }

    #[test]
    fn winding_tells_inside_from_outside() {
        let s = square();
        assert_eq!(s.winding(Vec2::ZERO).abs(), 1);
        assert_eq!(s.winding(Vec2::new(2.0, 0.0)), 0);
        assert_eq!(s.winding(Vec2::new(0.0, -1.5)), 0);
        // Open lines hold nothing.
        let open = Polyline { closed: false, ..s.clone() };
        assert_eq!(open.winding(Vec2::ZERO), 0);
    }

    #[test]
    fn a_hole_counts_as_outside() {
        let outer = square();
        let mut hole = square();
        hole.points.iter_mut().for_each(|p| *p *= 0.5);
        let lines = [outer, hole];
        assert!(!inside(&lines, Vec2::ZERO));
        assert!(inside(&lines, Vec2::new(0.75, 0.0)));
    }

    #[test]
    fn curves_flatten_within_tolerance() {
        let path = Path {
            closed: false,
            start: Vec2::new(0.0, 0.0),
            segments: vec![Segment::Quad(Vec2::new(50.0, 100.0), Vec2::new(100.0, 0.0))],
        };
        let line = path.flatten(0.1);
        assert!(line.points.len() > 10);
        // Every point of the true curve is within the tolerance of the straight pieces.
        for i in 0..=100 {
            let t = i as f32 / 100.0;
            let u = 1.0 - t;
            let p = Vec2::new(50.0, 100.0) * (2.0 * u * t) + Vec2::new(100.0, 0.0) * (t * t);
            let best = line.pieces().map(|(a, b)| distance_to_segment(p, a, b)).fold(f32::MAX, f32::min);
            assert!(best <= 0.1 + 1e-3, "{best} at t = {t}");
        }
        // A straight "curve" stays one piece.
        let flat = Path { segments: vec![Segment::Quad(Vec2::new(50.0, 0.0), Vec2::new(100.0, 0.0))], ..path };
        assert_eq!(flat.flatten(0.1).points.len(), 2);
    }

    #[test]
    fn degenerate_paths_flatten_safely() {
        // Empty, a single point, and a closed path whose points all sit on one spot.
        let empty = Path::default();
        assert_eq!(empty.flatten(0.1).points.len(), 1);
        let dot = Path::polygon(&[Vec2::ONE, Vec2::ONE, Vec2::ONE], true);
        let line = dot.flatten(0.1);
        assert!(!line.closed);
        assert_eq!(line.pieces().count(), 0);
        let mut shape = Shape { name: "dot".into(), paths: vec![dot] };
        assert!(shape.normalize().is_none());
        assert!(!shape.is_closed());
        assert!(Shape::default().flatten(100.0).is_empty());
        assert!(Shape::default().bounds().is_none());
    }

    #[test]
    fn huge_curves_are_capped() {
        let path = Path {
            closed: false,
            start: Vec2::ZERO,
            segments: vec![Segment::Cubic(Vec2::new(1e9, 1e9), Vec2::new(-1e9, 1e9), Vec2::new(1.0, 0.0))],
        };
        assert!(path.flatten(1e-6).points.len() <= MAX_CURVE_PIECES as usize + 1);
        let nan = Path { segments: vec![Segment::Quad(Vec2::NAN, Vec2::ONE)], ..path };
        assert!(nan.flatten(0.1).points.len() <= MAX_CURVE_PIECES as usize + 1);
    }

    #[test]
    fn heart_is_normalized_and_closed() {
        let heart = Shape::heart();
        assert!(heart.is_closed());
        let lines = heart.flatten(500.0);
        let reach = lines.iter().flat_map(|l| l.points.iter()).map(|p| p.length()).fold(0.0f32, f32::max);
        assert!((reach - 1.0).abs() < 1e-3, "reach {reach}");
        assert!(inside(&lines, Vec2::new(0.0, 0.1)));
        // The dip at the top of the heart is outside it.
        assert!(!inside(&lines, Vec2::new(0.0, -0.75)));
    }

    #[test]
    fn simplify_drops_points_on_a_straight_line() {
        let points: Vec<Vec2> = (0..=10).map(|i| Vec2::new(i as f32, 0.0)).collect();
        assert_eq!(simplify(&points, 0.01, false), vec![points[0], points[10]]);
        let mut square = square().points;
        // Extra points along the square's sides vanish, the corners stay.
        square.insert(1, Vec2::new(0.0, -1.0));
        square.insert(3, Vec2::new(1.0, 0.0));
        let simple = simplify(&square, 0.01, true);
        assert_eq!(simple.len(), 4);
    }

    #[test]
    fn placement_round_trips() {
        let place = Placement { center: Vec2::new(10.0, -20.0), size: 300.0, rotation: 0.7 };
        let p = Vec2::new(0.3, -0.4);
        assert!(place.invert(place.apply(p)).distance(p) < 1e-5);
    }

    #[test]
    fn shapes_save_and_load() {
        let shape = Shape::heart();
        let text = ron::to_string(&shape).unwrap();
        assert_eq!(ron::from_str::<Shape>(&text).unwrap(), shape);
    }
}
