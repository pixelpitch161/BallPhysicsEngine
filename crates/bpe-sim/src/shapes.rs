//! Collisions with custom shapes: a closed shape as the arena, and shapes of any kind as
//! obstacles. Each shape is flattened once into straight pieces in its own units and sorted
//! into a fixed grid, so a ball only checks the few pieces near it however detailed the
//! shape is. The shape's placement (where, how big, how turned) can change every step.

use crate::Ball;
use crate::arena::{self, Piece};
use bpe_scene::shapes::{Polyline, distance_to_segment};
use bpe_scene::{Placement, Shape, Vec2};

/// Most grid cells along one side of a shape's grid.
const MAX_CELLS: usize = 96;

/// A shape's outline in its own units, ready for collisions.
#[derive(Clone, Debug, Default)]
pub struct ShapeWalls {
    lines: Vec<Polyline>,
    /// Every straight piece, in the shape's own units.
    pieces: Vec<(Vec2, Vec2)>,
    /// Grid over the shape's bounds: cell (x, y) holds `items[starts[c]..starts[c + 1]]`.
    origin: Vec2,
    cell: f32,
    columns: usize,
    rows: usize,
    starts: Vec<u32>,
    items: Vec<u32>,
    /// Whether some outline is closed, so the shape has an inside.
    closed: bool,
    /// Per piece, the last query that looked at it, so a piece in several cells is checked once.
    seen: Vec<u32>,
    query: u32,
    /// Pieces near the ball being checked, kept to save allocating every time.
    scratch: Vec<(Vec2, Vec2)>,
}

/// A hit on a shape: where the ball touched and how fast it was moving into the wall.
pub type Hit = (Vec2, f32);

impl ShapeWalls {
    /// Flattens `shape` for drawing at about `size` pixels.
    pub fn new(shape: &Shape, size: f32) -> Self {
        let lines = shape.flatten(size.max(1.0));
        let pieces: Vec<(Vec2, Vec2)> = lines.iter().flat_map(|l| l.pieces()).collect();
        let closed = lines.iter().any(|l| l.closed);
        let mut walls = Self { lines, closed, seen: vec![0; pieces.len()], pieces, ..Self::default() };
        walls.build_grid();
        walls
    }

    fn build_grid(&mut self) {
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        for &(a, b) in &self.pieces {
            lo = lo.min(a).min(b);
            hi = hi.max(a).max(b);
        }
        if self.pieces.is_empty() || !lo.is_finite() || !hi.is_finite() {
            self.columns = 0;
            self.rows = 0;
            return;
        }
        let span = (hi - lo).max(Vec2::splat(1e-3));
        let average = self.pieces.iter().map(|(a, b)| a.distance(*b)).sum::<f32>() / self.pieces.len() as f32;
        let cell = average.max(span.max_element() / MAX_CELLS as f32).max(1e-4);
        self.origin = lo;
        self.cell = cell;
        self.columns = ((span.x / cell).ceil() as usize).clamp(1, MAX_CELLS);
        self.rows = ((span.y / cell).ceil() as usize).clamp(1, MAX_CELLS);
        let mut counts = vec![0u32; self.columns * self.rows + 1];
        for pass in 0..2 {
            if pass == 1 {
                let mut total = 0;
                for count in counts.iter_mut() {
                    let here = *count;
                    *count = total;
                    total += here;
                }
                self.items = vec![0; total as usize];
                self.starts = counts.clone();
            }
            for (i, &(a, b)) in self.pieces.iter().enumerate() {
                let (x0, y0) = self.cell_of(a.min(b));
                let (x1, y1) = self.cell_of(a.max(b));
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let c = y * self.columns + x;
                        if pass == 0 {
                            counts[c] += 1;
                        } else {
                            self.items[counts[c] as usize] = i as u32;
                            counts[c] += 1;
                        }
                    }
                }
            }
        }
    }

    fn cell_of(&self, p: Vec2) -> (usize, usize) {
        let local = (p - self.origin) / self.cell;
        let clamp = |v: f32, n: usize| if v.is_finite() { (v.max(0.0) as usize).min(n - 1) } else { 0 };
        (clamp(local.x, self.columns), clamp(local.y, self.rows))
    }

    /// The flattened outlines, in the shape's own units.
    pub fn lines(&self) -> &[Polyline] {
        &self.lines
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Whether `point` (in the shape's own units) is inside its closed outlines.
    pub fn inside_local(&self, point: Vec2) -> bool {
        bpe_scene::shapes::inside(&self.lines, point)
    }

    /// Calls `f` with each piece (in the shape's own units) near the box from `a` to `b`
    /// grown by `reach`, each once.
    fn near(&mut self, a: Vec2, b: Vec2, reach: f32, mut f: impl FnMut(Vec2, Vec2)) {
        if self.columns == 0 {
            return;
        }
        self.query = self.query.wrapping_add(1);
        if self.query == 0 {
            self.seen.iter_mut().for_each(|s| *s = 0);
            self.query = 1;
        }
        let (x0, y0) = self.cell_of(a.min(b) - Vec2::splat(reach));
        let (x1, y1) = self.cell_of(a.max(b) + Vec2::splat(reach));
        // A box bigger than the shape just checks every piece.
        if (x1 - x0 + 1) * (y1 - y0 + 1) > self.pieces.len() {
            for &(p, q) in &self.pieces {
                f(p, q);
            }
            return;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let c = y * self.columns + x;
                for k in self.starts[c]..self.starts[c + 1] {
                    let i = self.items[k as usize] as usize;
                    if self.seen[i] != self.query {
                        self.seen[i] = self.query;
                        let (p, q) = self.pieces[i];
                        f(p, q);
                    }
                }
            }
        }
    }

    /// Bounces a ball off the shape placed at `place`, from whichever side it is on. `prev` is
    /// where the ball was before this substep, already seen from where the shape is now.
    /// `pivot`, `spin` and `growth` say how the shape's walls are moving. As an arena (`keep_in`)
    /// a ball that still ends up outside is put back in; as an obstacle a ball inside a closed
    /// outline is pushed out.
    #[allow(clippy::too_many_arguments)]
    pub fn collide(
        &mut self,
        ball: &mut Ball,
        prev: Vec2,
        place: Placement,
        thickness: f32,
        motion: (Vec2, f32, f32),
        restitution: f32,
        keep_in: bool,
    ) -> Option<Hit> {
        let size = place.size.max(1e-6);
        let reach = (ball.radius + thickness * 0.5) / size;
        let (from, to) = (place.invert(prev), place.invert(ball.position));
        let (pivot, spin, growth) = motion;
        let mut hit: Option<Hit> = None;
        let mut candidates = std::mem::take(&mut self.scratch);
        candidates.clear();
        self.near(from, to, reach, |p, q| candidates.push((p, q)));
        for &(p, q) in &candidates {
            let piece = Piece::Segment { from: place.apply(p), to: place.apply(q) };
            if let Some(h) = arena::collide_piece(ball, prev, &piece, thickness, pivot, spin, growth, restitution)
                && hit.is_none_or(|(_, s)| h.1 > s)
            {
                hit = Some(h);
            }
        }
        self.scratch = candidates;
        if self.closed && keep_in != self.inside_local(place.invert(ball.position)) {
            let h = self.push_back(ball, place, thickness, motion, restitution, keep_in);
            if let Some(h) = h
                && hit.is_none_or(|(_, s)| h.1 > s)
            {
                hit = Some(h);
            }
        }
        hit
    }

    /// Moves a ball that ended up on the wrong side of a closed outline to the nearest point of
    /// the outline, on the right side, and bounces it. This is the safety net for corners and
    /// shrinking arenas; normally the walls stop balls before they cross.
    fn push_back(
        &self,
        ball: &mut Ball,
        place: Placement,
        thickness: f32,
        motion: (Vec2, f32, f32),
        restitution: f32,
        keep_in: bool,
    ) -> Option<Hit> {
        let local = place.invert(ball.position);
        let mut best = (f32::MAX, Vec2::ZERO);
        for &(p, q) in &self.pieces {
            let d = distance_to_segment(local, p, q);
            if d < best.0 {
                let e = q - p;
                let t = if e == Vec2::ZERO { 0.0 } else { ((local - p).dot(e) / e.length_squared()).clamp(0.0, 1.0) };
                best = (d, p + e * t);
            }
        }
        if best.0 == f32::MAX {
            return None;
        }
        let wall = place.apply(best.1);
        // The side to end up on: a little step from the wall towards the inside (arena) or the
        // outside (obstacle), found by testing which way is which.
        let away = (ball.position - wall).try_normalize().unwrap_or(Vec2::NEG_Y);
        let probe = place.invert(wall + away * 0.01 * place.size.max(1.0) / 100.0);
        let normal = if self.inside_local(probe) == keep_in { away } else { -away };
        let reach = ball.radius + thickness * 0.5;
        ball.position = wall + normal * reach;
        let (pivot, spin, growth) = motion;
        let moving = arena::wall_velocity(wall, pivot, spin, growth);
        arena::bounce(ball, normal, moving, restitution).map(|speed| (wall, speed))
    }

    /// Every straight piece placed in the world, for drawing or for the graphics card's walls.
    pub fn world_pieces(&self, place: Placement) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        self.pieces.iter().map(move |&(a, b)| (place.apply(a), place.apply(b)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ball(position: Vec2, velocity: Vec2) -> Ball {
        Ball { position, velocity, radius: 5.0, color: [1.0; 4], wall_hits: 0, group: 0, alive: true }
    }

    const STILL: (Vec2, f32, f32) = (Vec2::ZERO, 0.0, 0.0);

    #[test]
    fn heart_arena_keeps_a_fast_ball_in() {
        let heart = Shape::heart();
        let place = Placement { center: Vec2::ZERO, size: 400.0, rotation: 0.3 };
        let mut walls = ShapeWalls::new(&heart, 400.0);
        let mut b = ball(Vec2::ZERO, Vec2::new(5000.0, 3700.0));
        let dt = 1.0 / 240.0 / 4.0;
        for _ in 0..20_000 {
            let prev = b.position;
            b.position += b.velocity * dt;
            walls.collide(&mut b, prev, place, 2.0, STILL, 1.0, true);
            assert!(walls.inside_local(place.invert(b.position)), "escaped at {}", b.position);
        }
        // Elastic walls keep its speed.
        assert!((b.velocity.length() - Vec2::new(5000.0, 3700.0).length()).abs() < 1.0);
    }

    #[test]
    fn obstacle_star_pushes_balls_out() {
        let star = Shape::star(5, 0.5);
        let place = Placement { center: Vec2::new(100.0, 0.0), size: 80.0, rotation: 0.0 };
        let mut walls = ShapeWalls::new(&star, 80.0);
        // Dropped right into the middle, it is pushed out.
        let mut b = ball(Vec2::new(100.0, 0.0), Vec2::ZERO);
        walls.collide(&mut b, Vec2::new(100.0, 0.0), place, 2.0, STILL, 1.0, false);
        assert!(!walls.inside_local(place.invert(b.position)));
        // Flying straight at it, it bounces back.
        let mut b = ball(Vec2::new(0.0, 0.0), Vec2::new(2000.0, 0.0));
        let mut hits = 0;
        for _ in 0..240 {
            let prev = b.position;
            b.position += b.velocity / 240.0 / 4.0;
            hits += walls.collide(&mut b, prev, place, 2.0, STILL, 1.0, false).is_some() as u32;
        }
        assert_eq!(hits, 1);
        // It glanced off a slanted side of the star, so it is no longer heading straight on.
        assert!(b.velocity.x < 1990.0 && b.velocity.y.abs() > 10.0, "{}", b.velocity);
        assert!(!walls.inside_local(place.invert(b.position)));
    }

    #[test]
    fn open_wave_bounces_from_both_sides() {
        let wave = Shape::wave();
        let place = Placement { center: Vec2::ZERO, size: 200.0, rotation: 0.0 };
        let mut walls = ShapeWalls::new(&wave, 200.0);
        assert!(!walls.is_closed());
        for (start, velocity) in
            [(Vec2::new(0.0, -100.0), Vec2::new(0.0, 900.0)), (Vec2::new(0.0, 100.0), Vec2::new(0.0, -900.0))]
        {
            let mut b = ball(start, velocity);
            for _ in 0..200 {
                let prev = b.position;
                b.position += b.velocity / 960.0;
                walls.collide(&mut b, prev, place, 2.0, STILL, 1.0, false);
            }
            assert_eq!(b.velocity.y.signum(), -velocity.y.signum(), "from {start}");
        }
    }

    #[test]
    fn empty_and_tiny_shapes_do_nothing() {
        let mut walls = ShapeWalls::new(&Shape::default(), 100.0);
        let mut b = ball(Vec2::ZERO, Vec2::X);
        let place = Placement { center: Vec2::ZERO, size: 0.0, rotation: 0.0 };
        assert!(walls.collide(&mut b, Vec2::ZERO, place, 2.0, STILL, 1.0, true).is_none());
        let dot = Shape { name: "dot".into(), paths: vec![bpe_scene::shapes::Path::polygon(&[Vec2::ONE; 3], true)] };
        let mut walls = ShapeWalls::new(&dot, 100.0);
        assert!(walls.collide(&mut b, Vec2::ZERO, place, 2.0, STILL, 1.0, true).is_none());
        assert!(b.position.is_finite());
    }
}
