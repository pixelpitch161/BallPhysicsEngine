//! The position-based (XPBD) solver one soft body runs on: points tied by links, a ring that
//! keeps its area, points that push each other apart, and a grid to find what is close.
//! Everything runs in index order on the CPU, so a run is the same every time.

use std::ops::Range;

use bpe_core::ThingId;
use bpe_runtime::{Bodies, Party};
use glam::Vec2;

use super::SoftShape;
use super::sim::Impact;

/// One point of a soft body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub position: Vec2,
    pub velocity: Vec2,
    /// Where it was at the start of the current solver pass.
    pub(crate) prev: Vec2,
    /// 0 for pinned points, which never move.
    pub inverse_mass: f32,
    pub radius: f32,
}

impl Point {
    pub fn pinned(&self) -> bool {
        self.inverse_mass == 0.0
    }
}

/// Two points kept a set distance apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Link {
    pub a: u32,
    pub b: u32,
    pub rest: f32,
    /// How much it gives, as 1 / (2π × its springiness in Hz)², in seconds squared: 0 is rigid,
    /// and a link with give `g` wobbles like a spring at 1 / (2π √g) times a second.
    pub give: f32,
    /// Cut links hold nothing.
    pub cut: bool,
}

/// A closed ring of points that keeps its area, like a balloon.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ring {
    pub points: Range<u32>,
    pub area: f32,
    /// How much it gives, like [`Link::give`].
    pub give: f32,
}

/// One soft body's points and what holds them together.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoftWorld {
    pub points: Vec<Point>,
    pub(crate) links: Vec<Link>,
    pub(crate) rings: Vec<Ring>,
    /// Points far apart in the body close enough to touch this step.
    pairs: Vec<(u32, u32)>,
    grid: Grid,
    /// Scratch for queries.
    nearby: Vec<u32>,
    /// Solver passes per step.
    pub passes: u32,
    pub shape: SoftShape,
    /// Cloth only: points across and down, stored row by row.
    pub columns: u32,
    pub rows: u32,
    /// Rope only: how thick it is drawn.
    pub thickness: f32,
}

/// How fast a jelly ball's wobble (its points' speed relative to the whole) dies away, per
/// second.
const JELLY_DAMPING: f32 = 2.0;

/// The give (see [`Link::give`]) of a link that wobbles `hz` times a second. 0 Hz or less is
/// as loose as a link can be, and infinity is rigid.
pub fn give(hz: f32) -> f32 {
    let w = std::f32::consts::TAU * hz.max(0.05);
    1.0 / (w * w)
}

impl SoftWorld {
    /// An empty body of `shape` that solves in `passes` passes a step.
    pub fn new(shape: SoftShape, passes: u32) -> Self {
        Self { passes: passes.clamp(1, 64), shape, ..Self::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub(crate) fn point(&mut self, position: Vec2, velocity: Vec2, radius: f32, mass: f32) {
        let inverse_mass = if mass > 0.0 { 1.0 / mass } else { 0.0 };
        self.points.push(Point { position, velocity, prev: position, inverse_mass, radius });
    }

    pub(crate) fn link(&mut self, a: u32, b: u32, give: f32) {
        let rest = self.points[a as usize].position.distance(self.points[b as usize].position);
        self.links.push(Link { a, b, rest, give, cut: false });
    }

    /// Starts one solver pass of `dt` seconds: moves every free point by gravity and its speed.
    pub fn predict(&mut self, dt: f32, gravity: Vec2, drag: f32) {
        for p in &mut self.points {
            p.prev = p.position;
            if p.inverse_mass > 0.0 {
                p.velocity = (p.velocity + gravity * dt) * (1.0 - drag * dt).max(0.0);
                p.position += p.velocity * dt;
            }
        }
    }

    /// Pulls points back into shape: links, the ring's area and touching points. Then sets each
    /// point's speed from how far it moved this pass.
    pub fn solve(&mut self, dt: f32) {
        // XPBD: a link's give counts for more the shorter the pass.
        let per_pass = 1.0 / (dt * dt).max(1e-12);
        for link in &self.links {
            if link.cut {
                continue;
            }
            let (a, b) = (self.points[link.a as usize], self.points[link.b as usize]);
            let w = a.inverse_mass + b.inverse_mass;
            if w <= 0.0 {
                continue;
            }
            let d = b.position - a.position;
            let length = d.length();
            if length < 1e-6 {
                continue;
            }
            let lambda = -(length - link.rest) / (w * (1.0 + link.give * per_pass));
            let n = d / length;
            self.points[link.a as usize].position -= n * (lambda * a.inverse_mass);
            self.points[link.b as usize].position += n * (lambda * b.inverse_mass);
        }
        for ring in &self.rings {
            solve_area(&mut self.points, ring, per_pass);
        }
        self.solve_pairs();
        for p in &mut self.points {
            if p.inverse_mass > 0.0 {
                p.velocity = (p.position - p.prev) / dt;
            }
        }
        if self.shape == SoftShape::Jelly {
            self.damp_wobble(dt);
        }
    }

    /// Takes a little out of the points' speeds relative to the body as a whole, so a jelly
    /// ball's wobble dies away instead of bouncing it for ever. Ropes and cloth swing freely.
    fn damp_wobble(&mut self, dt: f32) {
        let (mut momentum, mut mass) = (Vec2::ZERO, 0.0);
        for p in &self.points {
            if p.inverse_mass > 0.0 {
                let m = 1.0 / p.inverse_mass;
                momentum += p.velocity * m;
                mass += m;
            }
        }
        if mass <= 0.0 {
            return;
        }
        let mean = momentum / mass;
        let keep = (1.0 - JELLY_DAMPING * dt).max(0.0);
        for p in &mut self.points {
            if p.inverse_mass > 0.0 {
                p.velocity = mean + (p.velocity - mean) * keep;
            }
        }
    }

    /// Sorts the points into the grid and, for a jelly ball, finds the points of its skin close
    /// enough to touch this step (a rope's or cloth's points never touch each other).
    pub fn find_pairs(&mut self) {
        self.pairs.clear();
        let reach = self.points.iter().map(|p| p.radius).fold(0.0f32, f32::max) * 2.0;
        // Also stops on a NaN radius.
        if self.points.is_empty() || reach.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            self.grid.rebuild(1.0, std::iter::empty());
            return;
        }
        // A little extra room, for points that move closer during the step.
        self.grid.rebuild(reach * 1.25, self.points.iter().map(|p| p.position));
        if self.shape != SoftShape::Jelly {
            return;
        }
        let n = self.points.len() as u32;
        for i in 0..self.points.len() {
            let (cx, cy) = self.grid.cell_of(self.points[i].position);
            self.nearby.clear();
            self.nearby.extend(self.grid.around(cx, cy).filter(|&j| j as usize > i));
            self.nearby.sort_unstable();
            for &j in &self.nearby {
                // Points next to each other, or one apart, are held apart by their links.
                if ring_neighbours(n, i as u32, j) {
                    continue;
                }
                let (a, b) = (&self.points[i], &self.points[j as usize]);
                let limit = (a.radius + b.radius) * 1.25;
                if a.position.distance_squared(b.position) < limit * limit {
                    self.pairs.push((i as u32, j));
                }
            }
        }
    }

    /// Pushes apart touching points.
    fn solve_pairs(&mut self) {
        for &(i, j) in &self.pairs {
            let (a, b) = (self.points[i as usize], self.points[j as usize]);
            let w = a.inverse_mass + b.inverse_mass;
            if w <= 0.0 {
                continue;
            }
            let d = b.position - a.position;
            let distance = d.length();
            let overlap = a.radius + b.radius - distance;
            if overlap <= 0.0 {
                continue;
            }
            let n = if distance > 1e-6 { d / distance } else { Vec2::Y };
            self.points[i as usize].position -= n * (overlap * a.inverse_mass / w);
            self.points[j as usize].position += n * (overlap * b.inverse_mass / w);
        }
    }

    /// Touches between this body's points and another thing's balls: both are pushed apart by
    /// their weights and bounce a little. Returns the hardest impact, naming the ball hit.
    pub fn collide_balls(&mut self, other: ThingId, balls: &mut Bodies, restitution: f32) -> Option<Impact> {
        if balls.is_empty() || self.points.is_empty() || self.grid.is_empty() {
            return None;
        }
        let mut hardest: Option<Impact> = None;
        let cell = self.grid.cell_size();
        for index in 0..balls.len() {
            if !balls.alive[index] {
                continue;
            }
            let (mut position, mut velocity, radius) =
                (balls.position[index], balls.velocity[index], balls.radius[index]);
            let reach = radius + cell;
            let (x0, y0) = self.grid.cell_of(position - Vec2::splat(reach));
            let (x1, y1) = self.grid.cell_of(position + Vec2::splat(reach));
            self.nearby.clear();
            // A ball much bigger than a cell checks a block of cells, but never more than every point.
            if ((x1 - x0 + 1) as i64) * ((y1 - y0 + 1) as i64) > self.points.len() as i64 {
                self.nearby.extend(0..self.points.len() as u32);
            } else {
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        self.nearby.extend(self.grid.cell(x, y));
                    }
                }
                self.nearby.sort_unstable();
            }
            let ball_inverse = 1.0 / (radius * radius).max(1e-6);
            for k in 0..self.nearby.len() {
                let i = self.nearby[k] as usize;
                let p = self.points[i];
                let d = p.position - position;
                let distance = d.length();
                let overlap = radius + p.radius - distance;
                if overlap <= 0.0 {
                    continue;
                }
                let n = if distance > 1e-6 { d / distance } else { Vec2::NEG_Y };
                let w = ball_inverse + p.inverse_mass;
                position -= n * (overlap * ball_inverse / w);
                self.points[i].position += n * (overlap * p.inverse_mass / w);
                let closing = (velocity - p.velocity).dot(n);
                if closing > 0.0 {
                    let impulse = closing * (1.0 + restitution) / w;
                    velocity -= n * (impulse * ball_inverse);
                    self.points[i].velocity += n * (impulse * p.inverse_mass);
                    let impact = Impact {
                        point: i as u32,
                        at: position + n * radius,
                        normal: -n,
                        speed: closing,
                        other: Some(Party { thing: other, item: index as u32 }),
                    };
                    if hardest.is_none_or(|h| impact.speed > h.speed) {
                        hardest = Some(impact);
                    }
                }
            }
            balls.position[index] = position;
            balls.velocity[index] = velocity;
        }
        hardest
    }

    /// The links that hold, as point index pairs, for drawing ropes and cloth.
    pub fn links_of(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.links.iter().filter(|l| !l.cut).map(|l| (l.a, l.b))
    }

    /// A jelly ball's area now compared to its starting area; 1 for anything else.
    pub fn squash(&self) -> f32 {
        match self.rings.first() {
            Some(ring) => area(&self.points, &ring.points) / ring.area.max(1e-6),
            None => 1.0,
        }
    }

    /// The middle of all the points.
    pub fn middle(&self) -> Vec2 {
        if self.points.is_empty() {
            return Vec2::ZERO;
        }
        self.points.iter().map(|p| p.position).sum::<Vec2>() / self.points.len() as f32
    }
}

/// Whether points `i` and `j` of a ring of `n` are next to each other or one apart, which their
/// links already keep apart.
fn ring_neighbours(n: u32, i: u32, j: u32) -> bool {
    let gap = i.abs_diff(j).min(n - i.abs_diff(j));
    gap <= 2
}

/// Area inside a ring of points, positive when they go clockwise on screen.
pub(crate) fn area(points: &[Point], ring: &Range<u32>) -> f32 {
    let n = ring.len();
    let mut total = 0.0;
    for k in 0..n {
        let a = points[ring.start as usize + k].position;
        let b = points[ring.start as usize + (k + 1) % n].position;
        total += a.perp_dot(b);
    }
    total * 0.5
}

/// Pushes a ring's points in or out along the outline's normals so it keeps its area.
fn solve_area(points: &mut [Point], ring: &Ring, per_pass: f32) {
    let n = ring.points.len();
    if n < 3 {
        return;
    }
    let start = ring.points.start as usize;
    let c = area(points, &ring.points) - ring.area;
    // The area changes by half the cross product of each point's neighbours' gap.
    let gradient = |k: usize, points: &[Point]| {
        let prev = points[start + (k + n - 1) % n].position;
        let next = points[start + (k + 1) % n].position;
        let d = next - prev;
        Vec2::new(d.y, -d.x) * 0.5
    };
    let mut sum = 0.0;
    for k in 0..n {
        sum += points[start + k].inverse_mass * gradient(k, points).length_squared();
    }
    if sum <= 0.0 {
        return;
    }
    let lambda = -c / (sum * (1.0 + ring.give * per_pass));
    for k in 0..n {
        let g = gradient(k, points);
        let w = points[start + k].inverse_mass;
        points[start + k].position += g * (lambda * w);
    }
}

/// A uniform grid for finding points close to each other. It keeps its memory between steps,
/// and cells are kept sorted by (column, row) with items in index order, so every pass over
/// the pairs visits them in the same order on every run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    cell: f32,
    /// (cell key, item index), sorted.
    entries: Vec<(u64, u32)>,
    /// Each occupied cell: its key and its run in `entries`.
    cells: Vec<(u64, u32, u32)>,
}

/// Packs a cell's column and row so that sorting keys sorts by column, then row.
fn key(cx: i32, cy: i32) -> u64 {
    ((cx as u32 ^ 0x8000_0000) as u64) << 32 | (cy as u32 ^ 0x8000_0000) as u64
}

impl Grid {
    /// Cell size in world units.
    pub fn cell_size(&self) -> f32 {
        self.cell
    }

    pub fn cell_of(&self, p: Vec2) -> (i32, i32) {
        // Far-off or broken positions land in the outermost cells instead of wrapping around.
        let c = |v: f32| (v / self.cell).floor().clamp(-1e9, 1e9) as i32;
        (c(p.x), c(p.y))
    }

    /// Sorts `points` into cells of size `cell`.
    pub fn rebuild(&mut self, cell: f32, points: impl Iterator<Item = Vec2>) {
        self.cell = if cell.is_finite() && cell > 0.0 { cell } else { 1.0 };
        self.entries.clear();
        for (i, p) in points.enumerate() {
            let (cx, cy) = self.cell_of(p);
            self.entries.push((key(cx, cy), i as u32));
        }
        self.entries.sort_unstable();
        self.cells.clear();
        let mut start = 0;
        for i in 1..=self.entries.len() {
            if i == self.entries.len() || self.entries[i].0 != self.entries[start].0 {
                self.cells.push((self.entries[start].0, start as u32, i as u32));
                start = i;
            }
        }
    }

    /// The items in cell (cx, cy), in index order.
    pub fn cell(&self, cx: i32, cy: i32) -> impl Iterator<Item = u32> + '_ {
        let k = key(cx, cy);
        let run = match self.cells.binary_search_by_key(&k, |c| c.0) {
            Ok(i) => self.cells[i].1 as usize..self.cells[i].2 as usize,
            Err(_) => 0..0,
        };
        self.entries[run].iter().map(|e| e.1)
    }

    /// The items in the 3 x 3 cells around cell (cx, cy), column by column.
    pub fn around(&self, cx: i32, cy: i32) -> impl Iterator<Item = u32> + '_ {
        (-1..=1).flat_map(move |dx| self.column(cx + dx, cy - 1, cy + 1))
    }

    /// Items in the cells of column `cx` from row `low` to `high`, which sit next to each other.
    fn column(&self, cx: i32, low: i32, high: i32) -> impl Iterator<Item = u32> + '_ {
        let (from, to) = (key(cx, low), key(cx, high));
        let first = self.cells.partition_point(|c| c.0 < from);
        let last = first + self.cells[first..].partition_point(|c| c.0 <= to);
        let run = match (self.cells.get(first), last > first) {
            (Some(c), true) => c.1 as usize..self.cells[last - 1].2 as usize,
            _ => 0..0,
        };
        self.entries[run].iter().map(|e| e.1)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours_cover_the_surrounding_cells() {
        let points = [Vec2::new(-0.5, -0.5), Vec2::new(0.5, 0.5), Vec2::new(1.5, -0.5), Vec2::new(5.0, 5.0)];
        let mut grid = Grid::default();
        grid.rebuild(1.0, points.iter().copied());
        let near: Vec<u32> = grid.around(0, 0).collect();
        assert_eq!(near, vec![0, 1, 2]);
        assert_eq!(grid.cell(5, 5).collect::<Vec<_>>(), vec![3]);
        grid.rebuild(0.0, [Vec2::NAN, Vec2::splat(1e30), Vec2::splat(-1e30)].into_iter());
        assert_eq!(grid.cell_size(), 1.0);
        grid.rebuild(1.0, std::iter::empty());
        assert!(grid.is_empty());
    }

    #[test]
    fn a_rigid_link_holds_its_length_and_a_ring_keeps_its_area() {
        let mut w = SoftWorld::new(SoftShape::Rope, 1);
        w.point(Vec2::ZERO, Vec2::ZERO, 1.0, 0.0);
        w.point(Vec2::new(10.0, 0.0), Vec2::ZERO, 1.0, 1.0);
        w.link(0, 1, 0.0);
        let mut lowest = 0.0f32;
        for _ in 0..240 {
            w.predict(1.0 / 240.0, Vec2::new(0.0, 500.0), 0.0);
            w.solve(1.0 / 240.0);
            let length = w.points[1].position.length();
            assert!((length - 10.0).abs() < 0.05, "{length}");
            lowest = lowest.max(w.points[1].position.y);
        }
        assert!(lowest > 9.0, "the free end swings down: {lowest}");
        assert_eq!(w.points[0].position, Vec2::ZERO, "the pinned end stays");
        assert_eq!(w.squash(), 1.0, "no ring, no squash");
    }
}
