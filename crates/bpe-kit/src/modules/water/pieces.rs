//! What a Piece's Shape and a Wall's Line are as solid (Water mode part 3): the water engine's
//! [`Solid`]s in the thing's own frame (pixels, its place at the middle, y down, not turned), which
//! the course turns and moves to the thing's place ([`placed`]) and lays over the carved rock.
//!
//! Each ready-made piece fills its box (its Size): solid pieces (a ramp, a step, a pillar, a gate,
//! a float, a drawn outline) are filled outlines; hollow ones (a basin, a funnel, a pipe, a sieve, a
//! bucket, a wheel) are walls `Thickness` thick, which the water takes as at least
//! [`bpe_water::drops::LEAST_WALL`] cells thick. With Cuts on a piece takes away its **cut**: a solid
//! piece its own shape, a hollow one only its inside (the open room inside its walls, whose walls then
//! go over the course as they are), so no sliver of open water is left beside its walls. Its **hull**
//! (its whole outline) is what other pieces snap to.

use bpe_core::ShapePath;
use bpe_core::maths::Maths;
use bpe_core::shape_path::FLATTEN;
use bpe_water::Solid;
use glam::Vec2;

use super::solids::{LineEnds, PieceKind, PieceShape, WallLine};

/// The palette's order on the Piece tool's strip.
pub const PALETTE: [PieceKind; 12] = [
    PieceKind::Ramp,
    PieceKind::Step,
    PieceKind::Pillar,
    PieceKind::Basin,
    PieceKind::Funnel,
    PieceKind::Pipe,
    PieceKind::Sieve,
    PieceKind::Bucket,
    PieceKind::Wheel,
    PieceKind::Gate,
    PieceKind::Float,
    PieceKind::Custom,
];

/// How big a piece of `kind` is when it is placed from the palette (pixels).
pub fn default_size(kind: PieceKind) -> Vec2 {
    let (w, h) = match kind {
        PieceKind::Ramp => (300.0, 150.0),
        PieceKind::Step => (300.0, 200.0),
        PieceKind::Pillar => (60.0, 300.0),
        PieceKind::Basin => (300.0, 160.0),
        PieceKind::Funnel => (300.0, 200.0),
        PieceKind::Pipe => (400.0, 80.0),
        PieceKind::Sieve => (320.0, 20.0),
        PieceKind::Bucket => (160.0, 180.0),
        PieceKind::Wheel => (200.0, 200.0),
        PieceKind::Gate => (20.0, 200.0),
        PieceKind::Float => (160.0, 80.0),
        PieceKind::Custom => (300.0, 200.0),
    };
    Vec2::new(w, h)
}

/// `solid` turned `turn` radians clockwise about the frame's middle, then moved to `at`.
pub fn placed(solid: &Solid, at: Vec2, turn: f32) -> Solid {
    let (s, c) = if turn.is_finite() { turn.det_sin_cos() } else { (0.0, 1.0) };
    let to = |p: Vec2| at + Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
    match solid {
        Solid::Polygon { points, filled, thickness } => {
            Solid::Polygon { points: points.iter().map(|p| to(*p)).collect(), filled: *filled, thickness: *thickness }
        }
        Solid::Circle { centre, radius, filled, thickness } => {
            Solid::Circle { centre: to(*centre), radius: *radius, filled: *filled, thickness: *thickness }
        }
        Solid::Line { a, b, thickness } => Solid::Line { a: to(*a), b: to(*b), thickness: *thickness },
        Solid::Polyline { points, thickness, square } => {
            Solid::Polyline { points: points.iter().map(|p| to(*p)).collect(), thickness: *thickness, square: *square }
        }
    }
}

/// The outlines of `solids` to draw (each filled part's outline closed, the first point again at
/// its end; each wall's middle line; each round part as 32 points), every solid with nothing to it
/// left out.
pub fn outlines(solids: &[Solid]) -> Vec<Vec<Vec2>> {
    let mut out = Vec::new();
    for s in solids.iter().filter(|s| s.is_sound()) {
        match s {
            Solid::Polygon { points, .. } => {
                let mut points = points.clone();
                points.push(points[0]);
                out.push(points);
            }
            Solid::Polyline { points, .. } => out.push(points.clone()),
            Solid::Line { a, b, .. } => out.push(vec![*a, *b]),
            Solid::Circle { centre, radius, .. } => out.push(
                (0..=32)
                    .map(|k| {
                        let (s, c) = (k as f32 * std::f32::consts::TAU / 32.0).det_sin_cos();
                        *centre + Vec2::new(c, s) * *radius
                    })
                    .collect(),
            ),
        }
    }
    out
}

/// The corners of `solids` (the hull of a piece) that other things snap to: each polygon's points,
/// and each circle's top, bottom and sides.
pub fn corners(solids: &[Solid]) -> Vec<Vec2> {
    let mut out = Vec::new();
    for s in solids.iter().filter(|s| s.is_sound()) {
        match s {
            Solid::Polygon { points, .. } | Solid::Polyline { points, .. } => out.extend(points.iter().copied()),
            Solid::Line { a, b, .. } => out.extend([*a, *b]),
            Solid::Circle { centre, radius, .. } => {
                out.extend([Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y].map(|w| *centre + w * *radius));
            }
        }
    }
    out
}

/// `solid` mirrored left to right about the frame's middle.
fn mirrored(solid: Solid) -> Solid {
    let m = |p: Vec2| Vec2::new(-p.x, p.y);
    match solid {
        Solid::Polygon { points, filled, thickness } => {
            Solid::Polygon { points: points.into_iter().map(m).collect(), filled, thickness }
        }
        Solid::Circle { centre, radius, filled, thickness } => {
            Solid::Circle { centre: m(centre), radius, filled, thickness }
        }
        Solid::Line { a, b, thickness } => Solid::Line { a: m(a), b: m(b), thickness },
        Solid::Polyline { points, thickness, square } => {
            Solid::Polyline { points: points.into_iter().map(m).collect(), thickness, square }
        }
    }
}

fn filled(points: Vec<Vec2>) -> Solid {
    Solid::Polygon { points, filled: true, thickness: 0.0 }
}

fn shape_path(points: &[[f32; 2]]) -> ShapePath {
    ShapePath { points: points.to_vec(), bends: vec![None; points.len()] }
}

impl PieceShape {
    /// Its box's half size, or `None` when the Size is not a size (0, below 0, or not a number).
    fn half(&self) -> Option<Vec2> {
        let half = self.size / 2.0;
        (half.is_finite() && half.x > 0.0 && half.y > 0.0).then_some(half)
    }

    /// Its walls' thickness (pixels): the Thickness, kept to half its smaller side (a sieve's bars,
    /// which stand its whole height, to half its width).
    fn wall(&self, half: Vec2) -> f32 {
        let t = if self.thickness.is_finite() { self.thickness } else { 16.0 };
        let most = if self.piece == PieceKind::Sieve { half.x } else { half.x.min(half.y) };
        t.clamp(0.5, most.max(0.5))
    }

    /// Its Opening (pixels), kept to what its box holds.
    fn gap(&self, half: Vec2, t: f32) -> f32 {
        let g = if self.opening.is_finite() { self.opening } else { 40.0 };
        g.clamp(0.0, (2.0 * half.x - 2.0 * t).max(0.0))
    }

    /// Whether it is a hollow piece: walls round an inside, so Cuts leaves its walls in the hole.
    pub fn is_hollow(&self) -> bool {
        matches!(
            self.piece,
            PieceKind::Basin
                | PieceKind::Funnel
                | PieceKind::Pipe
                | PieceKind::Sieve
                | PieceKind::Bucket
                | PieceKind::Wheel
        )
    }

    /// Its one closed outline in shape units (each point times half its Size is pixels), for the
    /// kinds that have one: the solid ones, and a basin's or a bucket's walls as one outline. `None`
    /// for a funnel, a pipe, a sieve and a wheel, which are several parts.
    pub fn outline(&self) -> Option<ShapePath> {
        let third = 1.0 / 3.0;
        let half = self.half().unwrap_or(Vec2::ONE);
        let t = self.wall(half) / half;
        Some(match self.piece {
            PieceKind::Ramp => shape_path(&[[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0]]),
            PieceKind::Step => shape_path(&[
                [-1.0, 1.0],
                [1.0, 1.0],
                [1.0, -1.0],
                [third, -1.0],
                [third, -third],
                [-third, -third],
                [-third, third],
                [-1.0, third],
            ]),
            PieceKind::Pillar | PieceKind::Gate => shape_path(&[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]),
            PieceKind::Basin => shape_path(&[
                [-1.0, -1.0],
                [-1.0, 1.0],
                [1.0, 1.0],
                [1.0, -1.0],
                [1.0 - t.x, -1.0],
                [1.0 - t.x, 1.0 - t.y],
                [-1.0 + t.x, 1.0 - t.y],
                [-1.0 + t.x, -1.0],
            ]),
            PieceKind::Bucket => {
                let b = BUCKET_FOOT;
                shape_path(&[
                    [-1.0, -1.0],
                    [-b, 1.0],
                    [b, 1.0],
                    [1.0, -1.0],
                    [1.0 - t.x, -1.0],
                    [b - t.x, 1.0 - t.y],
                    [-b + t.x, 1.0 - t.y],
                    [-1.0 + t.x, -1.0],
                ])
            }
            PieceKind::Float => float_outline(half),
            PieceKind::Custom => self.path.clone(),
            PieceKind::Funnel | PieceKind::Pipe | PieceKind::Sieve | PieceKind::Wheel => return None,
        })
    }

    /// Its outline's polygon in pixels (its own frame, not flipped), if it has a sound one.
    fn outline_polygon(&self, half: Vec2) -> Option<Vec<Vec2>> {
        let path = self.outline()?;
        if !path.is_valid() || path.is_empty() {
            return None;
        }
        Some(path.flatten(FLATTEN).into_iter().map(|p| p * half).collect())
    }

    /// What it adds to the course, in its own frame (pixels, Flip applied): nothing when its Size
    /// or its outline is broken.
    pub fn parts(&self) -> Vec<Solid> {
        let Some(half) = self.half() else { return Vec::new() };
        let t = self.wall(half);
        let (w, h) = (half.x, half.y);
        let line = |points: Vec<Vec2>, square: bool| Solid::Polyline { points, thickness: t, square };
        let parts = match self.piece {
            PieceKind::Basin => {
                let (x, top, foot) = (w - t / 2.0, -h + t / 2.0, h - t / 2.0);
                vec![line(vec![Vec2::new(-x, top), Vec2::new(-x, foot), Vec2::new(x, foot), Vec2::new(x, top)], true)]
            }
            PieceKind::Bucket => {
                let (x, b, top, foot) = (w - t / 2.0, BUCKET_FOOT * w - t / 2.0, -h + t / 2.0, h - t / 2.0);
                // (Round at its rim: a square end on a slanting wall would stand out of its box.)
                vec![line(vec![Vec2::new(-x, top), Vec2::new(-b, foot), Vec2::new(b, foot), Vec2::new(x, top)], false)]
            }
            PieceKind::Funnel => {
                let g = self.gap(half, t);
                let (x, mouth, top, foot) = (w - t / 2.0, g / 2.0 + t / 2.0, -h + t / 2.0, h - t / 2.0);
                vec![
                    line(vec![Vec2::new(-x, top), Vec2::new(-mouth, foot)], false),
                    line(vec![Vec2::new(x, top), Vec2::new(mouth, foot)], false),
                ]
            }
            PieceKind::Pipe => {
                let (x, y) = (w - t / 2.0, h - t / 2.0);
                vec![
                    line(vec![Vec2::new(-x, -y), Vec2::new(x, -y)], true),
                    line(vec![Vec2::new(-x, y), Vec2::new(x, y)], true),
                ]
            }
            PieceKind::Sieve => sieve_bars(half, t, self.gap(half, t)),
            PieceKind::Wheel => {
                let r = w.min(h);
                let hub =
                    Solid::Circle { centre: Vec2::ZERO, radius: WHEEL_HUB * 2.0 * r, filled: true, thickness: 0.0 };
                let reach = (r - t / 2.0).max(0.0);
                let mut parts = vec![hub];
                for k in 0..WHEEL_PADDLES {
                    let (s, c) = (k as f32 * std::f32::consts::TAU / WHEEL_PADDLES as f32).det_sin_cos();
                    parts.push(Solid::Line { a: Vec2::ZERO, b: Vec2::new(c, s) * reach, thickness: t });
                }
                parts
            }
            PieceKind::Ramp
            | PieceKind::Step
            | PieceKind::Pillar
            | PieceKind::Gate
            | PieceKind::Float
            | PieceKind::Custom => self.outline_polygon(half).map(filled).into_iter().collect(),
        };
        self.flipped(parts)
    }

    /// What it takes away with Cuts on, in its own frame (Flip applied): a solid piece's own shape, a
    /// hollow one's inside: the open room its walls hold (a basin's, a bucket's or a funnel's out to
    /// its top, a funnel's to its foot, a pipe's bore end to end, a sieve's gaps, a wheel's round). It
    /// meets the walls only at their inner faces, so laying the walls over the cut leaves no open
    /// water outside them.
    pub fn cut(&self) -> Vec<Solid> {
        let Some(half) = self.half() else { return Vec::new() };
        let t = self.wall(half);
        let (w, h) = (half.x, half.y);
        let corners = |lo: Vec2, hi: Vec2| filled(vec![lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)]);
        let inside = match self.piece {
            PieceKind::Basin => vec![corners(Vec2::new(-w + t, -h), Vec2::new(w - t, h - t))],
            PieceKind::Pipe => vec![corners(Vec2::new(-w, -h + t), Vec2::new(w, h - t))],
            PieceKind::Sieve => {
                let bars = sieve_xs(half, t, self.gap(half, t));
                bars.windows(2).map(|x| corners(Vec2::new(x[0] + t, -h), Vec2::new(x[1], h))).collect()
            }
            PieceKind::Bucket => {
                let (x, b, top, foot) = (w - t / 2.0, BUCKET_FOOT * w - t / 2.0, -h + t / 2.0, h - t / 2.0);
                within(Vec2::new(-x, top), Vec2::new(-b, foot), t / 2.0, -h, h - t).into_iter().collect()
            }
            PieceKind::Funnel => {
                let g = self.gap(half, t);
                let (x, mouth, top, foot) = (w - t / 2.0, g / 2.0 + t / 2.0, -h + t / 2.0, h - t / 2.0);
                within(Vec2::new(-x, top), Vec2::new(-mouth, foot), t / 2.0, -h, h).into_iter().collect()
            }
            PieceKind::Wheel => {
                vec![Solid::Circle { centre: Vec2::ZERO, radius: w.min(h), filled: true, thickness: 0.0 }]
            }
            _ => return self.parts(),
        };
        self.flipped(inside)
    }

    /// Its whole outline, in its own frame (Flip applied): a solid piece's own shape, a hollow one's
    /// outline. Other pieces' corners and pen points snap to its corners.
    pub fn hull(&self) -> Vec<Solid> {
        let Some(half) = self.half() else { return Vec::new() };
        let t = self.wall(half);
        let (w, h) = (half.x, half.y);
        let corners = |lo: Vec2, hi: Vec2| filled(vec![lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)]);
        let hull = match self.piece {
            PieceKind::Basin | PieceKind::Pipe | PieceKind::Sieve => vec![corners(-half, half)],
            // (A slanting wall's rim stands straight up for a wall's thickness, so the hull does too.)
            PieceKind::Bucket => {
                let b = BUCKET_FOOT * w + t / 2.0;
                vec![slanting(w, h, b, t)]
            }
            PieceKind::Funnel => vec![slanting(w, h, self.gap(half, t) / 2.0 + t, t)],
            PieceKind::Wheel => {
                vec![Solid::Circle { centre: Vec2::ZERO, radius: w.min(h), filled: true, thickness: 0.0 }]
            }
            _ => return self.parts(),
        };
        self.flipped(hull)
    }

    fn flipped(&self, solids: Vec<Solid>) -> Vec<Solid> {
        if self.flip { solids.into_iter().map(mirrored).collect() } else { solids }
    }

    /// A point of its Outline (shape units) in its own frame (pixels, Flip applied).
    pub fn frame_of(&self, shape: Vec2) -> Vec2 {
        let p = shape * self.size / 2.0;
        if self.flip { Vec2::new(-p.x, p.y) } else { p }
    }

    /// A point of its own frame as a point of its Outline (shape units); `None` for a Size that is
    /// not a size.
    pub fn shape_of(&self, frame: Vec2) -> Option<Vec2> {
        let half = self.half()?;
        let p = if self.flip { Vec2::new(-frame.x, frame.y) } else { frame };
        Some(p / half)
    }

    /// Its thinnest part (pixels): a hollow piece's walls (or a sieve's bars, if shorter), a solid
    /// one's smaller side.
    pub fn thinnest(&self) -> f32 {
        let Some(half) = self.half() else { return 0.0 };
        let t = self.wall(half);
        match self.piece {
            PieceKind::Sieve => t.min(self.size.y),
            _ if self.is_hollow() => t,
            _ => self.size.x.min(self.size.y),
        }
    }
}

/// How wide a bucket's foot is, a share of its top.
const BUCKET_FOOT: f32 = 0.7;
/// A wheel's paddles.
const WHEEL_PADDLES: u32 = 8;
/// A wheel's hub's radius, a share of its smaller side.
const WHEEL_HUB: f32 = 0.15;

/// A box `2w` wide at its top that narrows to `2foot` at its bottom, straight for `t` below its top.
fn slanting(w: f32, h: f32, foot: f32, t: f32) -> Solid {
    let rim = (-h + t).min(h);
    filled(vec![
        Vec2::new(-w, -h),
        Vec2::new(w, -h),
        Vec2::new(w, rim),
        Vec2::new(foot, h),
        Vec2::new(-foot, h),
        Vec2::new(-w, rim),
    ])
}

/// A float's outline: a rectangle whose corners round off by a quarter of its smaller side, in shape
/// units for a box of half size `half`.
fn float_outline(half: Vec2) -> ShapePath {
    let r = 0.5 * half.x.min(half.y);
    let (rx, ry) = (r / half.x, r / half.y);
    // (A quarter circle's cubic: handles this share of the radius along the tangents.)
    let k = 0.552_284_8;
    let mut path = ShapePath::default();
    // Clockwise from the top edge's left end, a straight side then a round corner.
    let corners = [(1.0f32, -1.0f32), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)];
    for (cx, cy) in corners {
        // The straight side ends where the corner starts; the corner turns a quarter clockwise.
        let (start, end, h1, h2) = if cx * cy < 0.0 {
            // Top right or bottom left: from along x to along y.
            let start = [cx * (1.0 - rx), cy];
            let end = [cx, cy * (1.0 - ry)];
            let h1 = [cx * (1.0 - rx + k * rx), cy];
            let h2 = [cx, cy * (1.0 - ry + k * ry)];
            (start, end, h1, h2)
        } else {
            // Bottom right or top left: from along y to along x.
            let start = [cx, cy * (1.0 - ry)];
            let end = [cx * (1.0 - rx), cy];
            let h1 = [cx, cy * (1.0 - ry + k * ry)];
            let h2 = [cx * (1.0 - rx + k * rx), cy];
            (start, end, h1, h2)
        };
        path.points.push(start);
        path.bends.push(Some([h1, h2]));
        path.points.push(end);
        path.bends.push(None);
    }
    path
}

/// Where a sieve's bars start (their left sides): as many `t` wide as fit with gaps of at least `g`
/// (two at least), spread evenly across the box.
fn sieve_xs(half: Vec2, t: f32, g: f32) -> Vec<f32> {
    let w = 2.0 * half.x;
    let n = (((w + g) / (t + g).max(1e-3)).floor() as u32).clamp(2, 512);
    let step = if n > 1 { (w - t) / (n - 1) as f32 } else { 0.0 };
    (0..n).map(|k| -half.x + k as f32 * step).collect()
}

/// A sieve's bars ([`sieve_xs`]), each as tall as its box.
fn sieve_bars(half: Vec2, t: f32, g: f32) -> Vec<Solid> {
    let bar = |x: f32| {
        filled(vec![Vec2::new(x, -half.y), Vec2::new(x + t, -half.y), Vec2::new(x + t, half.y), Vec2::new(x, half.y)])
    };
    sieve_xs(half, t, g).into_iter().map(bar).collect()
}

/// The room between a left wall running from `a` to `b` (its middle line) and its mirror image,
/// each wall `off` from its middle to its face, from `top` down to `bottom`: the polygon through the
/// walls' inner faces, carried on straight past their ends. `None` when the walls' faces cross
/// (walls too thick for the room between them).
fn within(a: Vec2, b: Vec2, off: f32, top: f32, bottom: f32) -> Option<Solid> {
    let way = b - a;
    if way.y.abs() < 1e-6 || !way.is_finite() {
        return None;
    }
    // The inner face: the middle line moved `off` toward the middle (+x side of a left wall).
    let normal = Vec2::new(way.y, -way.x).normalize_or_zero();
    let normal = if normal.x < 0.0 { -normal } else { normal };
    let (p, d) = (a + normal * off, way);
    let at = |y: f32| p.x + d.x * (y - p.y) / d.y;
    let (xt, xb) = (at(top), at(bottom));
    (xt < 0.0 && xb < 0.0)
        .then(|| filled(vec![Vec2::new(xt, top), Vec2::new(-xt, top), Vec2::new(-xb, bottom), Vec2::new(xb, bottom)]))
}

/// Pieces a curved wall's span between two of its points is cut into.
pub const CURVE_PIECES: usize = 8;

impl WallLine {
    /// The line it runs along, in its own frame (pixels): its points, each repeat and each point
    /// that is not a number left out, and with Curved on a smooth curve through them (centripetal
    /// Catmull-Rom, [`CURVE_PIECES`] a span). Empty when fewer than two points are left.
    pub fn path(&self) -> Vec<Vec2> {
        let mut points: Vec<Vec2> = Vec::with_capacity(self.points.len());
        for p in self.points.iter().map(|p| Vec2::from(*p)).filter(|p| p.is_finite()) {
            if points.last().is_none_or(|q| q.distance_squared(p) > 1e-6) {
                points.push(p);
            }
        }
        if points.len() < 2 {
            return Vec::new();
        }
        if !self.curved || points.len() < 3 {
            return points;
        }
        let n = points.len();
        let at = |i: isize| -> Vec2 {
            if i < 0 {
                2.0 * points[0] - points[1]
            } else if i as usize >= n {
                2.0 * points[n - 1] - points[n - 2]
            } else {
                points[i as usize]
            }
        };
        let mut out = vec![points[0]];
        for i in 0..n - 1 {
            let i = i as isize;
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let knot = |a: Vec2, b: Vec2| a.distance(b).sqrt().max(1e-4);
            let (t0, t1) = (0.0, knot(p0, p1));
            let t2 = t1 + knot(p1, p2);
            let t3 = t2 + knot(p2, p3);
            for k in 1..=CURVE_PIECES {
                let t = t1 + (t2 - t1) * k as f32 / CURVE_PIECES as f32;
                let a1 = p0 * ((t1 - t) / (t1 - t0)) + p1 * ((t - t0) / (t1 - t0));
                let a2 = p1 * ((t2 - t) / (t2 - t1)) + p2 * ((t - t1) / (t2 - t1));
                let a3 = p2 * ((t3 - t) / (t3 - t2)) + p3 * ((t - t2) / (t3 - t2));
                let b1 = a1 * ((t2 - t) / (t2 - t0)) + a2 * ((t - t0) / (t2 - t0));
                let b2 = a2 * ((t3 - t) / (t3 - t1)) + a3 * ((t - t1) / (t3 - t1));
                let c = b1 * ((t2 - t) / (t2 - t1)) + b2 * ((t - t1) / (t2 - t1));
                out.push(if c.is_finite() { c } else { p2 });
            }
        }
        out
    }

    /// It as solid, in its own frame: a wall along [`WallLine::path`] `Thickness` thick with its
    /// Ends, or `None` with fewer than two points.
    pub fn solid(&self) -> Option<Solid> {
        let points = self.path();
        let thickness = if self.thickness.is_finite() { self.thickness.max(0.5) } else { 24.0 };
        (points.len() >= 2).then(|| Solid::Polyline { points, thickness, square: self.ends == LineEnds::Square })
    }

    /// Its thickness (pixels).
    pub fn thinnest(&self) -> f32 {
        if self.thickness.is_finite() { self.thickness.max(0.5) } else { 24.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Settings;

    fn piece(kind: PieceKind) -> PieceShape {
        PieceShape { piece: kind, size: default_size(kind), ..PieceShape::default() }
    }

    /// The box round the inside of `solids`, sampled every pixel to 4 px past a box of `size`.
    fn reach(solids: &[Solid], size: Vec2) -> (Vec2, Vec2) {
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        let n = (size / 2.0 + Vec2::splat(4.0)).ceil();
        for y in -n.y as i32..=n.y as i32 {
            for x in -n.x as i32..=n.x as i32 {
                let p = Vec2::new(x as f32, y as f32);
                if solids.iter().any(|s| s.distance(p, 0.0) < 0.0) {
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
        }
        (lo, hi)
    }

    #[test]
    fn every_piece_fills_its_box() {
        for kind in PALETTE {
            let p = piece(kind);
            let parts = p.parts();
            assert!(!parts.is_empty(), "{kind:?}");
            let half = p.size / 2.0;
            let (lo, hi) = reach(&parts, p.size);
            // Within its box (a pixel's sampling to spare), and out to its four sides.
            assert!(lo.x >= -half.x - 1.0 && lo.y >= -half.y - 1.0, "{kind:?}: {lo} past {half}");
            assert!(hi.x <= half.x + 1.0 && hi.y <= half.y + 1.0, "{kind:?}: {hi} past {half}");
            // (A ramp's tips are too thin for a pixel's sampling right at their ends.)
            assert!(lo.x <= -half.x + 4.0 && hi.x >= half.x - 4.0, "{kind:?} is not as wide as its box: {lo} {hi}");
            assert!(lo.y <= -half.y + 4.0 && hi.y >= half.y - 4.0, "{kind:?} is not as tall as its box: {lo} {hi}");
        }
    }

    #[test]
    fn the_hull_holds_the_parts() {
        for kind in PALETTE {
            let p = piece(kind);
            let (parts, hull) = (p.parts(), p.hull());
            let n = p.size.max_element() as i32;
            for y in (-n..=n).step_by(3) {
                for x in (-n..=n).step_by(3) {
                    let q = Vec2::new(x as f32, y as f32);
                    if parts.iter().any(|s| s.distance(q, 0.0) < -1.0) {
                        assert!(hull.iter().any(|s| s.distance(q, 0.0) < 0.5), "{kind:?}: {q} is out of its hull");
                    }
                }
            }
        }
    }

    /// A hollow piece that cuts opens only the room its walls hold: no open water lies beside a wall
    /// with rock straight past it (a sliver between a wall and the rock).
    #[test]
    fn a_cutting_hollow_piece_leaves_no_room_beside_its_walls() {
        // (Which way its walls stand across: rows scanned along x, or columns along y.)
        for (kind, across) in [
            (PieceKind::Basin, Vec2::X),
            (PieceKind::Bucket, Vec2::X),
            (PieceKind::Funnel, Vec2::X),
            (PieceKind::Pipe, Vec2::Y),
            (PieceKind::Sieve, Vec2::X),
        ] {
            for flip in [false, true] {
                let p = PieceShape { flip, cuts: true, ..piece(kind) };
                let (parts, cut) = (p.parts(), p.cut());
                assert!(!cut.is_empty(), "{kind:?}");
                let d = |set: &[Solid], q: Vec2| set.iter().map(|s| s.distance(q, 0.0)).fold(f32::MAX, f32::min);
                let open = |q: Vec2| d(&cut, q) < -0.5 && d(&parts, q) > 0.5;
                let rock = |q: Vec2| d(&cut, q) > 0.5 && d(&parts, q) > 0.5;
                let half = p.size / 2.0;
                // Along the walls, clear of their ends by a wall's thickness (a sieve's bars stand
                // its whole height).
                let end = if kind == PieceKind::Sieve { 1.0 } else { p.thickness };
                let along = Vec2::ONE - across;
                let span = (half * along).max_element() - end;
                for a in (-span as i32)..=(span as i32) {
                    for b in -(half * across).max_element() as i32..=(half * across).max_element() as i32 {
                        let q = along * a as f32 + across * b as f32;
                        if open(q) {
                            for side in [q + across * 1.5, q - across * 1.5] {
                                assert!(!rock(side), "{kind:?} (flip {flip}): open water at {q} beside rock");
                            }
                        }
                    }
                }
            }
        }
    }

    /// A sieve's bars take their Thickness (up to half its width), not its height: a flat sieve of
    /// 16 px bars is not THIN at the 8 px draft.
    #[test]
    fn a_sieves_bars_take_their_thickness() {
        let sieve = piece(PieceKind::Sieve);
        assert_eq!((sieve.size, sieve.thickness), (Vec2::new(320.0, 20.0), 16.0));
        assert_eq!(sieve.thinnest(), 16.0);
        let bar = &sieve.parts()[0];
        assert!(bar.distance(Vec2::new(-160.0 + 15.0, 0.0), 0.0) < 0.0, "16 px wide");
        assert!(bar.distance(Vec2::new(-160.0 + 17.0, 0.0), 0.0) > 0.0);
        assert_eq!(PieceShape { thickness: 1e6, ..sieve }.thinnest(), 20.0, "kept to its height when thinner");
    }

    #[test]
    fn flip_mirrors_a_piece() {
        for kind in [PieceKind::Ramp, PieceKind::Step, PieceKind::Funnel, PieceKind::Bucket] {
            let p = piece(kind);
            let f = PieceShape { flip: true, ..p.clone() };
            for q in [Vec2::new(100.0, 50.0), Vec2::new(-60.0, 20.0), Vec2::new(130.0, -70.0)] {
                let d = |s: &PieceShape, q: Vec2| s.parts().iter().map(|s| s.distance(q, 0.0)).fold(f32::MAX, f32::min);
                assert!((d(&p, q) - d(&f, Vec2::new(-q.x, q.y))).abs() < 1e-3, "{kind:?} at {q}");
            }
        }
        // A ramp rises to the right, and flipped to the left.
        let ramp = piece(PieceKind::Ramp);
        let up_right = Vec2::new(140.0, -60.0);
        assert!(ramp.parts()[0].distance(up_right, 0.0) < 0.0);
        assert!(PieceShape { flip: true, ..ramp }.parts()[0].distance(up_right, 0.0) > 0.0);
    }

    #[test]
    fn a_piece_turns_about_its_place() {
        let block = Solid::rect(Vec2::new(0.0, -5.0), Vec2::new(100.0, 5.0), true, 0.0);
        let turned = placed(&block, Vec2::new(500.0, 300.0), std::f32::consts::FRAC_PI_2);
        // A quarter turn clockwise (y down): what ran right now runs down.
        assert!(turned.distance(Vec2::new(500.0, 380.0), 0.0) < 0.0);
        assert!(turned.distance(Vec2::new(580.0, 300.0), 0.0) > 0.0);
        assert!((turned.distance(Vec2::new(500.0, 300.0), 0.0) + 0.0).abs() < 1e-3);
    }

    #[test]
    fn a_curved_wall_passes_through_its_points() {
        let points = vec![[-200.0, 0.0], [0.0, -100.0], [150.0, 50.0], [300.0, 0.0]];
        let wall = WallLine { points: points.clone(), curved: true, ..WallLine::default() };
        let path = wall.path();
        assert_eq!(path.len(), 1 + 3 * CURVE_PIECES);
        for (k, p) in points.iter().enumerate() {
            assert!(path[k * CURVE_PIECES].distance(Vec2::from(*p)) < 1e-3, "point {k}");
        }
        // Between its points it bends (it is not the straight line).
        let mid = path[CURVE_PIECES / 2];
        let straight = (Vec2::new(-200.0, 0.0) + Vec2::new(0.0, -100.0)) / 2.0;
        assert!(mid.distance(straight) > 2.0);
        // Straight, it is its points.
        let straight = WallLine { points, ..WallLine::default() };
        assert_eq!(straight.path().len(), 4);
        assert!(matches!(straight.solid(), Some(Solid::Polyline { square: false, .. })));
        let square = WallLine { ends: LineEnds::Square, ..straight };
        assert!(matches!(square.solid(), Some(Solid::Polyline { square: true, thickness, .. }) if thickness == 24.0));
    }

    #[test]
    fn make_custom_keeps_the_outline() {
        for kind in [PieceKind::Ramp, PieceKind::Step, PieceKind::Pillar, PieceKind::Gate, PieceKind::Float] {
            let p = piece(kind);
            let custom = PieceShape { piece: PieceKind::Custom, path: p.outline().unwrap(), ..p.clone() };
            assert_eq!(custom.parts(), p.parts(), "{kind:?}");
        }
        // A basin's walls as one outline: the same solid but at its outer corners, which the walls round.
        let basin = piece(PieceKind::Basin);
        let custom = PieceShape { piece: PieceKind::Custom, path: basin.outline().unwrap(), ..basin.clone() };
        for q in [Vec2::new(-145.0, 0.0), Vec2::new(0.0, 75.0), Vec2::new(0.0, 0.0), Vec2::new(145.0, -70.0)] {
            let inside = |s: &PieceShape| s.parts().iter().any(|s| s.distance(q, 0.0) < 0.0);
            assert_eq!(inside(&basin), inside(&custom), "{q}");
        }
        for kind in [PieceKind::Funnel, PieceKind::Pipe, PieceKind::Sieve, PieceKind::Wheel] {
            assert!(piece(kind).outline().is_none(), "{kind:?}");
        }
    }

    #[test]
    fn broken_shapes_add_nothing() {
        for size in [Vec2::ZERO, Vec2::new(-10.0, 50.0), Vec2::new(f32::NAN, 50.0), Vec2::new(50.0, f32::INFINITY)] {
            for kind in PALETTE {
                let p = PieceShape { piece: kind, size, ..PieceShape::default() };
                assert!(p.parts().is_empty() && p.hull().is_empty(), "{kind:?} at {size}");
                assert_eq!(p.thinnest(), 0.0);
            }
        }
        let two = ShapePath { points: vec![[0.0, 0.0], [1.0, 0.0]], bends: vec![None, None] };
        assert!(PieceShape { piece: PieceKind::Custom, path: two, ..PieceShape::default() }.parts().is_empty());
        let nan = ShapePath { points: vec![[0.0, 0.0], [1.0, f32::NAN], [0.0, 1.0]], bends: vec![None; 3] };
        assert!(PieceShape { piece: PieceKind::Custom, path: nan, ..PieceShape::default() }.parts().is_empty());
        for points in [vec![], vec![[0.0, 0.0]], vec![[5.0, 5.0], [5.0, 5.0]], vec![[f32::NAN, 0.0], [1.0, 1.0]]] {
            assert!(WallLine { points, ..WallLine::default() }.solid().is_none());
        }
        // Thickness or Opening that is not a number, or past the box, still gives a sound piece.
        for kind in PALETTE {
            for (thickness, opening) in [(f32::NAN, f32::NAN), (1e9, 1e9), (0.0, -5.0)] {
                let p = PieceShape { thickness, opening, ..piece(kind) };
                assert!(p.parts().iter().all(Solid::is_sound), "{kind:?}");
            }
        }
    }

    #[test]
    fn the_shape_card_shows_thickness_and_opening_only_where_they_count() {
        let defs = PieceShape::defs();
        let when = |key: &str| defs.iter().find(|d| d.key == key).unwrap().when;
        assert!(when("thickness").contains("piece=pipe") && !when("thickness").contains("piece=ramp"));
        assert_eq!(when("opening"), "piece=funnel or piece=sieve");
    }
}
