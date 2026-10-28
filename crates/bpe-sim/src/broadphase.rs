//! Ball-to-ball collisions (the stage after balls move in each substep), and a uniform grid
//! for finding things that are close to each other: balls touching balls, soft-body points
//! touching balls, and liquid particles near each other.
//!
//! The grid keeps its memory between steps, so rebuilding it every substep allocates nothing
//! once it has grown to the scene's size. Cells are kept sorted by (column, row), and items in
//! a cell by their index, so every pass over the pairs visits them in the same order on every
//! run and every machine.

use crate::contacts::{Contacts, Touch, WALL_MARGIN};
use crate::grid::BallGrid;
use crate::{Ball, Event, EventKind, Outline, Simulation};
use bpe_scene::Vec2;

/// State kept between steps for ball-to-ball contacts.
#[derive(Clone, Default)]
pub(crate) struct Broadphase {
    grid: BallGrid,
    /// The contact solver used when "stacking quality" is above 0, with spin and sleeping.
    pub(crate) contacts: Contacts,
    touches: Vec<Touch>,
    /// Balls touching a wall this substep, as (ball, direction away from the wall).
    walls: Vec<(u32, Vec2)>,
    /// When each ball last made a ball event, so a jostling crowd doesn't make thousands.
    last_touch: Vec<f64>,
}

/// Shortest time between two ball events of the same ball, in seconds.
const TOUCH_GAP: f64 = 0.04;

impl Broadphase {
    pub fn is_asleep(&self, ball: usize) -> bool {
        self.contacts.is_asleep(ball)
    }

    /// Angle a ball has turned from spinning, in radians.
    pub fn angle(&self, ball: usize) -> f32 {
        self.contacts.angle.get(ball).copied().unwrap_or(0.0)
    }
}

/// Whether the scene uses the contact solver rather than the classic push-apart.
fn solver(sim: &Simulation) -> bool {
    let physics = &sim.scene.physics;
    physics.ball_collisions && physics.contact.quality > 0
}

/// Furthest a ball may move in one substep with the contact solver: its smallest ball's
/// radius, so balls can't jump through each other. None means the usual limit.
pub(crate) fn safe_move(sim: &Simulation) -> Option<f32> {
    if !solver(sim) {
        return None;
    }
    let smallest = sim.balls.iter().filter(|b| b.alive).map(|b| b.radius).fold(f32::MAX, f32::min);
    Some(smallest.clamp(1.0, crate::SAFE_MOVE))
}

/// Pushes overlapping balls apart and bounces them off each other, when the scene asks for it.
/// With the contact solver, the first moment two balls meet hard enough is a ball event.
pub(crate) fn ball_ball(sim: &mut Simulation, dt: f32, time: f64, closed: Option<Outline>, events: &mut Vec<Event>) {
    if !sim.scene.physics.contact.pockets.is_empty() {
        pockets(sim, time, events);
    }
    let physics = &sim.scene.physics;
    if !physics.ball_collisions {
        return;
    }
    let restitution = physics.restitution;
    if !solver(sim) {
        collide_balls(&mut sim.balls, restitution, &mut sim.broadphase.grid);
        return;
    }
    // Same threshold as wall hits: slower touches are balls resting on each other.
    let resting = physics.gravity.length() * sim.dt * 4.0 + 1.0;
    // Sleeping only makes sense while nothing around the balls moves.
    let still = sim.arena.spin == 0.0 && sim.arena.scale_rate == 0.0 && !sim.animated;
    find_walls(sim, closed);
    // Balls squeezed together (say by an arena shrinking onto a crowd) can be pushed apart
    // faster and faster. Keep them to the top speed, as bounces off walls do, and below the
    // speed at which they would jump through each other.
    let rules = &sim.scene.rules;
    let rate = sim.scene.physics.steps_per_second.max(1) as f32;
    let mut limit = safe_move(sim).unwrap_or(crate::SAFE_MOVE) * crate::MAX_SUBSTEPS as f32 * rate;
    if rules.limit_speed {
        limit = limit.min(rules.max_speed.max(0.0));
    }
    let physics = &sim.scene.physics;
    let state = &mut sim.broadphase;
    state.touches.clear();
    let contact = &physics.contact;
    state.contacts.solve(&mut sim.balls, contact, restitution, dt, resting, still, &state.walls, &mut state.touches);
    for ball in &mut sim.balls {
        ball.velocity = ball.velocity.clamp_length_max(limit);
    }
    state.last_touch.resize(sim.balls.len(), f64::NEG_INFINITY);
    for touch in &state.touches {
        let (a, b) = (touch.a as usize, touch.b as usize);
        if time - state.last_touch[a] < TOUCH_GAP || time - state.last_touch[b] < TOUCH_GAP {
            continue;
        }
        state.last_touch[a] = time;
        state.last_touch[b] = time;
        events.push(Event {
            time,
            ball: touch.a,
            position: touch.position,
            depth: 0.0,
            speed: touch.speed,
            kind: EventKind::Ball(touch.b),
        });
    }
}

/// Takes out every ball whose middle is over a pocket, with an escape event (numbered by
/// pocket) and then a remove event.
fn pockets(sim: &mut Simulation, time: f64, events: &mut Vec<Event>) {
    for index in 0..sim.balls.len() {
        let ball = &sim.balls[index];
        if !ball.alive {
            continue;
        }
        let pocket =
            sim.scene.physics.contact.pockets.iter().position(|p| ball.position.distance(p.position) < p.radius);
        if let Some(k) = pocket {
            let speed = ball.velocity.length();
            let position = sim.scene.physics.contact.pockets[k].position;
            events.push(Event {
                time,
                ball: index as u32,
                position,
                depth: 0.0,
                speed,
                kind: EventKind::Escape(k as u32),
            });
            sim.remove(index, time, events);
        }
    }
}

/// Finds the balls touching a wall, peg or shape (not escape rings, which balls are meant to
/// get through), by trying to push a slightly bigger copy of each ball out of the walls.
fn find_walls(sim: &mut Simulation, closed: Option<Outline>) {
    let mut walls = std::mem::take(&mut sim.broadphase.walls);
    walls.clear();
    let frame = sim.wall_frame(closed, 0.0);
    for index in 0..sim.balls.len() {
        let ball = &sim.balls[index];
        // Sleeping balls are probed too: a ball resting alone on the floor stays asleep only
        // while it still has the floor to rest on.
        if !ball.alive {
            continue;
        }
        let mut probe = Ball { velocity: Vec2::ZERO, radius: ball.radius + WALL_MARGIN, ..*ball };
        let start = probe.position;
        sim.bounce_point(&mut probe, start, &frame, &[]);
        if let Some(normal) = (probe.position - start).try_normalize() {
            walls.push((index as u32, normal));
        }
    }
    sim.broadphase.walls = walls;
}

/// Elastic collisions between balls, found with a uniform grid. Heavier (bigger) balls move less.
pub(crate) fn collide_balls(balls: &mut [Ball], restitution: f32, grid: &mut BallGrid) {
    let max_radius = balls.iter().map(|b| b.radius).fold(0.0f32, f32::max);
    if balls.len() < 2 || max_radius <= 0.0 {
        return;
    }
    // Removed balls stay in the list where they left. Keeping them out of the grid stops
    // thousands of them piling up in one cell and every step checking each pair of them.
    let alive = balls.iter().enumerate().filter(|(_, b)| b.alive).map(|(i, b)| (i as u32, b.position));
    grid.build(alive, max_radius * 2.0);
    grid.for_each_pair(|i, j| resolve_pair(balls, i, j, restitution));
}

pub(crate) fn resolve_pair(balls: &mut [Ball], i: usize, j: usize, restitution: f32) {
    let (left, right) = balls.split_at_mut(j);
    let (a, b) = (&mut left[i], &mut right[0]);
    if !a.alive || !b.alive {
        return;
    }
    let offset = b.position - a.position;
    let distance = offset.length();
    let overlap = a.radius + b.radius - distance;
    if overlap <= 0.0 || distance == 0.0 {
        return;
    }
    let normal = offset / distance;
    let (mass_a, mass_b) = (a.radius * a.radius, b.radius * b.radius);
    let total = mass_a + mass_b;
    a.position -= normal * overlap * (mass_b / total);
    b.position += normal * overlap * (mass_a / total);
    let closing = (a.velocity - b.velocity).dot(normal);
    if closing > 0.0 {
        let impulse = closing * (1.0 + restitution) / total;
        a.velocity -= normal * impulse * mass_b;
        b.velocity += normal * impulse * mass_a;
    }
}

#[derive(Clone, Debug, Default)]
pub struct Grid {
    cell: f32,
    /// (cell key, item index), sorted.
    entries: Vec<(u64, u32)>,
    /// Each occupied cell: its key and its run in `entries`.
    cells: Vec<(u64, u32, u32)>,
    /// Each entry's position, in the same order, so neighbours are read one after another.
    positions: Vec<Vec2>,
    unsorted: Vec<Vec2>,
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
        self.unsorted.clear();
        for (i, p) in points.enumerate() {
            let (cx, cy) = self.cell_of(p);
            self.entries.push((key(cx, cy), i as u32));
            self.unsorted.push(p);
        }
        self.entries.sort_unstable();
        self.cells.clear();
        self.positions.clear();
        self.positions.extend(self.entries.iter().map(|e| self.unsorted[e.1 as usize]));
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

    /// Each occupied cell's (column, row) and its run of items, in sorted order.
    pub fn cells(&self) -> impl Iterator<Item = ((i32, i32), std::ops::Range<usize>)> + '_ {
        self.cells.iter().map(|&(k, start, end)| {
            let (cx, cy) = (((k >> 32) as u32 ^ 0x8000_0000) as i32, (k as u32 ^ 0x8000_0000) as i32);
            ((cx, cy), start as usize..end as usize)
        })
    }

    /// The items of a run of entries from [`Grid::cells`] or [`Grid::runs_around`].
    pub fn run(&self, run: std::ops::Range<usize>) -> impl Iterator<Item = u32> + '_ {
        self.entries[run].iter().map(|e| e.1)
    }

    /// The item at position `e` of the grid's sorted entries, and where it was.
    pub fn entry(&self, e: usize) -> (u32, Vec2) {
        (self.entries[e].1, self.positions[e])
    }

    /// The three runs of entries (one per column) that make up the 3 x 3 cells around (cx, cy).
    pub fn runs_around(&self, cx: i32, cy: i32) -> [std::ops::Range<usize>; 3] {
        [-1, 0, 1].map(|dx| {
            let (from, to) = (key(cx + dx, cy - 1), key(cx + dx, cy + 1));
            let first = self.cells.partition_point(|c| c.0 < from);
            let last = first + self.cells[first..].partition_point(|c| c.0 <= to);
            match (self.cells.get(first), last > first) {
                (Some(c), true) => c.1 as usize..self.cells[last - 1].2 as usize,
                _ => 0..0,
            }
        })
    }

    /// Calls `f(i, j)` once for every pair of items in the same or neighbouring cells, with
    /// `i < j`, in a fixed order: cells by (column, row), then `i` by index, then the 3 x 3
    /// neighbouring cells column by column, then `j` by index. The solver walks the pairs
    /// itself; this is the plain version the tests check it against.
    #[cfg(test)]
    pub fn for_each_pair(&self, mut f: impl FnMut(usize, usize)) {
        for &(k, start, end) in &self.cells {
            let (cx, cy) = (((k >> 32) as u32 ^ 0x8000_0000) as i32, (k as u32 ^ 0x8000_0000) as i32);
            for e in start..end {
                let i = self.entries[e as usize].1;
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for j in self.cell(cx + dx, cy + dy) {
                            if j > i {
                                f(i as usize, j as usize);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_are_found_once_each() {
        let points: Vec<Vec2> = (0..50).map(|i| Vec2::new((i % 7) as f32 * 3.0, (i / 7) as f32 * 3.0)).collect();
        let mut grid = Grid::default();
        grid.rebuild(4.0, points.iter().copied());
        let mut found = std::collections::BTreeSet::new();
        grid.for_each_pair(|i, j| {
            assert!(i < j);
            assert!(found.insert((i, j)), "pair {i} {j} twice");
        });
        // Every pair closer than one cell is found.
        for i in 0..points.len() {
            for j in i + 1..points.len() {
                if points[i].distance(points[j]) < 4.0 {
                    assert!(found.contains(&(i, j)), "missed {i} {j}");
                }
            }
        }
    }

    #[test]
    fn neighbours_cover_the_surrounding_cells() {
        let points = [Vec2::new(-0.5, -0.5), Vec2::new(0.5, 0.5), Vec2::new(1.5, -0.5), Vec2::new(5.0, 5.0)];
        let mut grid = Grid::default();
        grid.rebuild(1.0, points.iter().copied());
        let near: Vec<u32> = grid.around(0, 0).collect();
        assert_eq!(near, vec![0, 1, 2]);
        assert_eq!(grid.cell(5, 5).collect::<Vec<_>>(), vec![3]);
    }

    #[test]
    fn empty_broken_and_far_points_are_safe() {
        let mut grid = Grid::default();
        grid.rebuild(1.0, std::iter::empty());
        assert!(grid.is_empty());
        grid.for_each_pair(|_, _| panic!());
        grid.rebuild(0.0, [Vec2::NAN, Vec2::splat(1e30), Vec2::splat(-1e30)].into_iter());
        let mut count = 0;
        grid.for_each_pair(|_, _| count += 1);
        assert!(count <= 3);
    }

    #[test]
    fn rebuilding_reuses_memory() {
        let points: Vec<Vec2> = (0..1000).map(|i| Vec2::new(i as f32, 0.0)).collect();
        let mut grid = Grid::default();
        grid.rebuild(2.0, points.iter().copied());
        let capacity = (grid.entries.capacity(), grid.cells.capacity());
        grid.rebuild(2.0, points.iter().rev().copied());
        assert_eq!((grid.entries.capacity(), grid.cells.capacity()), capacity);
    }
}
