//! Shatter: a broken thing cut into pieces (a seeded Voronoi of what it draws) that fly apart from
//! the hit, fall, spin, bounce off the shapes and fade. Drawn only: each piece is the thing's own
//! instance again, moved and turned with the piece and clipped to its cell, so it wears exactly its
//! part of the picture. Pure in time: a piece is worked out afresh from the break at every moment.

use bpe_runtime::{Wall, keep_clear, reflect};
use glam::Vec2;

use crate::modules::look::layers::particles::emit::hash;
use crate::modules::look::layers::particles::sim::steps_for;
use crate::rules::breaks::LookSettings;

/// The outline of what a thing draws, in its own units.
#[derive(Clone, Copy, Debug)]
pub enum Edge<'a> {
    /// Corners, closed.
    Points(&'a [Vec2]),
    /// An ellipse round the middle with these half-axes (a circle is `Vec2::ONE`).
    Ellipse(Vec2),
}

/// What a thing is made of, in its own units: inside its edge, or a band along it.
#[derive(Clone, Copy, Debug)]
pub struct Material<'a> {
    pub edge: Edge<'a>,
    /// Half the band's thickness (0: filled).
    pub half: f32,
}

impl Material<'_> {
    /// A ball: the disc of radius 1.
    pub const DISC: Material<'static> = Material { edge: Edge::Ellipse(Vec2::ONE), half: 0.0 };

    /// How much of it there is, in its own units squared.
    pub fn area(&self) -> f32 {
        if self.half > 0.0 {
            return perimeter(self.edge) * 2.0 * self.half;
        }
        match self.edge {
            Edge::Points(ps) => polygon_area(ps).abs(),
            Edge::Ellipse(ab) => std::f32::consts::PI * ab.x.abs() * ab.y.abs(),
        }
    }
}

/// One piece: the place it turns about (a point of the material) and its cell (convex, corners
/// in order), both in the thing's own units.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub site: Vec2,
    pub cell: Vec<Vec2>,
}

/// Random numbers from one seed.
struct Rng(u32);

impl Rng {
    /// 0 to 1.
    fn unit(&mut self) -> f32 {
        self.0 = hash(&[self.0, 0x27d4_eb2f]);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }

    /// -1 to 1.
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

/// The seed of a break: the thing, its body row and the step it broke on.
pub fn seed_of(thing: u32, row: u32, step: u64) -> u32 {
    hash(&[thing, row, step as u32, (step >> 32) as u32, 0x5ba7])
}

fn polygon_area(ps: &[Vec2]) -> f32 {
    let n = ps.len();
    (0..n).map(|k| ps[k].perp_dot(ps[(k + 1) % n])).sum::<f32>() * 0.5
}

fn perimeter(edge: Edge) -> f32 {
    match edge {
        Edge::Points(ps) => (0..ps.len()).map(|k| ps[k].distance(ps[(k + 1) % ps.len()])).sum(),
        Edge::Ellipse(ab) => {
            // Ramanujan's.
            let (a, b) = (ab.x.abs(), ab.y.abs());
            std::f32::consts::PI * (3.0 * (a + b) - ((3.0 * a + b) * (a + 3.0 * b)).max(0.0).sqrt())
        }
    }
}

/// Whether `p` is inside `edge` (even-odd for corners).
fn inside(edge: Edge, p: Vec2) -> bool {
    match edge {
        Edge::Points(ps) => {
            let mut inside = false;
            let mut prev = ps[ps.len() - 1];
            for &q in ps {
                if (q.y > p.y) != (prev.y > p.y) {
                    let x = q.x + (p.y - q.y) / (prev.y - q.y) * (prev.x - q.x);
                    if p.x < x {
                        inside = !inside;
                    }
                }
                prev = q;
            }
            inside
        }
        Edge::Ellipse(ab) => (p / ab.abs().max(Vec2::splat(1e-6))).length_squared() < 1.0,
    }
}

/// A random point of `m`: inside it, or in its band.
fn sample(m: &Material, rng: &mut Rng) -> Vec2 {
    if m.half > 0.0 {
        let (at, normal) = along(m.edge, rng.unit());
        return at + normal * (rng.signed() * m.half);
    }
    match m.edge {
        Edge::Ellipse(ab) => {
            let (r, a) = (rng.unit().sqrt(), rng.unit() * std::f32::consts::TAU);
            Vec2::new(a.cos() * ab.x, a.sin() * ab.y) * r
        }
        Edge::Points(ps) => {
            let (lo, hi) =
                ps.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
            for _ in 0..32 {
                let p = lo + (hi - lo) * Vec2::new(rng.unit(), rng.unit());
                if inside(m.edge, p) {
                    return p;
                }
            }
            // A thin or tangled outline: somewhere between a corner and the corners' middle.
            let middle = ps.iter().copied().sum::<Vec2>() / ps.len() as f32;
            let corner = ps[((rng.unit() * ps.len() as f32) as usize).min(ps.len() - 1)];
            corner.lerp(middle, rng.unit())
        }
    }
}

/// The place `u` (0 to 1) of the way round `edge` and the way out of it there.
fn along(edge: Edge, u: f32) -> (Vec2, Vec2) {
    match edge {
        Edge::Ellipse(ab) => {
            let a = u * std::f32::consts::TAU;
            let at = Vec2::new(a.cos() * ab.x, a.sin() * ab.y);
            let n = Vec2::new(a.cos() / ab.x.max(1e-6), a.sin() / ab.y.max(1e-6)).normalize_or(Vec2::X);
            (at, n)
        }
        Edge::Points(ps) => {
            let n = ps.len();
            let lengths: Vec<f32> = (0..n).map(|k| ps[k].distance(ps[(k + 1) % n])).collect();
            let mut left = u * lengths.iter().sum::<f32>();
            for k in 0..n {
                let (a, b) = (ps[k], ps[(k + 1) % n]);
                if left <= lengths[k] || k + 1 == n {
                    let t = if lengths[k] > 0.0 { (left / lengths[k]).clamp(0.0, 1.0) } else { 0.0 };
                    return (a.lerp(b, t), (b - a).perp().normalize_or(Vec2::X));
                }
                left -= lengths[k];
            }
            (ps[0], Vec2::X)
        }
    }
}

/// `poly` (convex, corners in order) clipped to the side of the line through `at` that `toward`
/// points away from: the points `p` with `(p − at)·toward ≤ 0`.
fn clip(poly: &[Vec2], at: Vec2, toward: Vec2) -> Vec<Vec2> {
    let side = |p: Vec2| (p - at).dot(toward);
    let mut out = Vec::with_capacity(poly.len() + 1);
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        let (sa, sb) = (side(a), side(b));
        if sa <= 0.0 {
            out.push(a);
        }
        if (sa <= 0.0) != (sb <= 0.0) {
            out.push(a + (b - a) * (sa / (sa - sb)));
        }
    }
    out
}

/// `m` cut into `n` pieces: `n` seeded points of it, each with its Voronoi cell within the square
/// `bound` either way of the middle. The cells tile the square, so together the pieces are the whole
/// picture when `bound` takes in everything drawn.
pub fn cut(m: &Material, bound: f32, n: u32, seed: u32) -> Vec<Piece> {
    let n = n.clamp(1, 256) as usize;
    let bound = if bound.is_finite() { bound.max(1e-3) } else { 1.0 };
    let mut rng = Rng(seed);
    let sites: Vec<Vec2> = (0..n).map(|_| sample(m, &mut rng).clamp(Vec2::splat(-bound), Vec2::splat(bound))).collect();
    let square =
        [Vec2::new(-bound, -bound), Vec2::new(bound, -bound), Vec2::new(bound, bound), Vec2::new(-bound, bound)];
    let mut pieces = Vec::with_capacity(n);
    for (i, &s) in sites.iter().enumerate() {
        // A site on top of an earlier one is the same piece.
        if sites[..i].iter().any(|&o| o.distance_squared(s) < 1e-12) {
            continue;
        }
        let mut cell = square.to_vec();
        for (j, &o) in sites.iter().enumerate() {
            if j == i || o.distance_squared(s) < 1e-12 {
                continue;
            }
            cell = clip(&cell, (s + o) * 0.5, o - s);
            if cell.len() < 3 {
                break;
            }
        }
        if cell.len() >= 3 {
            pieces.push(Piece { site: s, cell });
        }
    }
    pieces
}

/// Where a broken thing was and how its pieces set off.
#[derive(Clone, Copy, Debug)]
pub struct Start {
    /// Its middle in the world, its turn (radians), and world pixels per unit of its own.
    pub centre: Vec2,
    pub turn: f32,
    pub scale: f32,
    /// How it was moving.
    pub velocity: Vec2,
    /// Where the pieces fly away from (world).
    pub from: Vec2,
    /// The world's gravity when it broke.
    pub gravity: Vec2,
    /// Each piece's size for bouncing (world pixels).
    pub radius: f32,
    pub seed: u32,
}

impl Start {
    /// Where the pieces fly from: the hit, when it lies within twice `reach` (world pixels) of
    /// the middle (a break on a time or beat has no hit there), else the middle.
    pub fn from_of(centre: Vec2, point: Vec2, reach: f32) -> Vec2 {
        if point.is_finite() && point.distance(centre) <= 2.0 * reach.max(0.0) { point } else { centre }
    }

    /// The world place of `p` (the thing's own units) as it stood.
    pub fn world_of(&self, p: Vec2) -> Vec2 {
        self.centre + Vec2::from_angle(self.turn).rotate(p * self.scale)
    }
}

/// A piece at a moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceNow {
    /// Where its site is (world) and the turn of the whole piece (radians, its start turn and its
    /// spin).
    pub at: Vec2,
    pub turn: f32,
}

impl PieceNow {
    /// Where the thing's middle would be for the piece to sit here: the instance's centre.
    pub fn centre(&self, site: Vec2, scale: f32) -> Vec2 {
        self.at - Vec2::from_angle(self.turn).rotate(site * scale)
    }
}

/// Piece `k` (`piece`) of a break that started as `start`, `age` seconds after it, bouncing off
/// `walls` when the look hits shapes.
pub fn piece_at(piece: &Piece, k: u32, start: &Start, look: &LookSettings, age: f32, walls: &[&Wall]) -> PieceNow {
    let mut rng = Rng(hash(&[start.seed, k, 0x9ece]));
    let p0 = start.world_of(piece.site);
    let away =
        (p0 - start.from).try_normalize().unwrap_or_else(|| Vec2::from_angle(rng.unit() * std::f32::consts::TAU));
    let away = Vec2::from_angle(rng.signed() * 25f32.to_radians()).rotate(away);
    let spread = if look.spread.is_finite() { look.spread.max(0.0) } else { 0.0 };
    let v0 = start.velocity + away * spread * (0.6 + 0.8 * rng.unit());
    let spin = if look.spin.is_finite() { look.spin } else { 0.0 };
    let spin = spin.to_radians() * (0.5 + rng.unit()) * if rng.unit() < 0.5 { -1.0 } else { 1.0 };
    let g = start.gravity * if look.gravity.is_finite() { look.gravity } else { 0.0 };
    let age = if age.is_finite() { age.max(0.0) } else { 0.0 };
    let turn = start.turn + spin * age;
    if !look.hit_shapes || walls.is_empty() {
        // Nothing in the way: exact.
        return PieceNow { at: p0 + v0 * age + g * (0.5 * age * age), turn };
    }
    let bounce = if look.bounce.is_finite() { look.bounce.clamp(0.0, 1.0) } else { 0.0 };
    let radius = start.radius.max(0.5);
    // Each piece starts on its own side of every wall (its site is where it was drawn), and
    // `keep_clear` keeps it there.
    let (mut p, mut v) = (p0, v0);
    let (steps, step) = steps_for(age);
    for n in 0..steps {
        let dt = if n + 1 == steps { age - n as f32 * step } else { step };
        v += g * dt;
        let before = p;
        p += v * dt;
        for wall in walls {
            if let Some(c) = keep_clear(&mut p, before, v, radius, wall) {
                v = reflect(v, &c, bounce);
            }
        }
    }
    PieceNow { at: p, turn }
}
