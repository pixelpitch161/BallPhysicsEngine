//! Balls hitting each other: a grid to find the pairs that are close, and one pass over them
//! that pushes each pair apart and swaps momentum.

use std::collections::BTreeMap;
use std::ops::Range;

use bpe_core::{SlotId, Target, ThingId};
use glam::Vec2;

use crate::plugins::{Limits, ModuleSim, PairRule, Sims, ThingState};
use crate::{Bodies, Event, EventKind, Log, Party};

/// Points sorted into square cells, so each one only meets its neighbours.
#[derive(Clone, Debug, Default)]
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

fn unkey(k: u64) -> (i32, i32) {
    (((k >> 32) as u32 ^ 0x8000_0000) as i32, (k as u32 ^ 0x8000_0000) as i32)
}

impl Grid {
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

    /// Each occupied cell's (column, row) and its run of entries, in sorted order.
    pub fn cells(&self) -> impl Iterator<Item = ((i32, i32), Range<usize>)> + '_ {
        self.cells.iter().map(|&(k, start, end)| (unkey(k), start as usize..end as usize))
    }

    /// The items of a run of entries from [`Grid::cells`] or [`Grid::runs_around`].
    pub fn run(&self, run: Range<usize>) -> impl Iterator<Item = u32> + '_ {
        self.entries[run].iter().map(|e| e.1)
    }

    /// The three runs of entries (one per column) that make up the 3 x 3 cells around (cx, cy).
    pub fn runs_around(&self, cx: i32, cy: i32) -> [Range<usize>; 3] {
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
    /// neighbouring cells column by column, then `j` by index.
    pub fn for_each_pair(&self, mut f: impl FnMut(u32, u32)) {
        for ((cx, cy), run) in self.cells() {
            let around = self.runs_around(cx, cy);
            for i in self.run(run) {
                for column in &around {
                    for j in self.run(column.clone()) {
                        if j > i {
                            f(i, j);
                        }
                    }
                }
            }
        }
    }
}

/// A thing's prepared modules: per slot, the module kind and its state.
pub(crate) type Prepared = BTreeMap<SlotId, (String, Box<dyn ThingState>)>;

/// One ball that can bump into others.
struct Ball {
    thing: ThingId,
    item: u32,
    /// Index into `rules`.
    rule: usize,
    position: Vec2,
    velocity: Vec2,
    radius: f32,
}

/// One real hit between two balls.
struct PairHit {
    a: usize,
    b: usize,
    point: Vec2,
    normal: Vec2,
    speed: f32,
}

/// Runs the ball-to-ball stage: every alive ball of every thing whose Bounce module wants pairs
/// is pushed off the balls it overlaps (when both sides' targets agree), momentum is swapped
/// with the average bounciness and friction, and hits faster than resting are logged with
/// `a` the ball earlier in `(thing, item)` order. Each side's `effects` then runs, and a side
/// that asks to pop is removed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collide_pairs(
    bodies: &mut BTreeMap<ThingId, Bodies>,
    modules: &BTreeMap<ThingId, Prepared>,
    matches: &dyn Fn(&Target, ThingId, ThingId) -> bool,
    sims: &Sims,
    grid: &mut Grid,
    step: u64,
    limits: &Limits,
    log: &mut Log,
) {
    // Gather the balls that take part, in (thing, item) order.
    let mut rules: Vec<(ThingId, &dyn ModuleSim, &dyn ThingState, PairRule, f32)> = Vec::new();
    let mut balls: Vec<Ball> = Vec::new();
    let mut largest: f32 = 0.0;
    for (thing, b) in bodies.iter() {
        let Some((kind, state)) = modules.get(thing).and_then(|m| m.get(&SlotId::Bounce)) else { continue };
        let Some(sim) = sims.module(kind) else { continue };
        let Some(rule) = sim.pairs(state.as_ref()) else { continue };
        let bounciness = sim.bounciness(state.as_ref());
        rules.push((*thing, sim, state.as_ref(), rule, bounciness));
        let rule = rules.len() - 1;
        for i in 0..b.len() {
            if b.alive[i] {
                largest = largest.max(b.radius[i]);
                balls.push(Ball {
                    thing: *thing,
                    item: i as u32,
                    rule,
                    position: b.position[i],
                    velocity: b.velocity[i],
                    radius: b.radius[i],
                });
            }
        }
    }
    if balls.len() < 2 {
        return;
    }
    grid.rebuild(largest * 2.0, balls.iter().map(|b| b.position));

    let mut hits: Vec<PairHit> = Vec::new();
    grid.for_each_pair(|i, j| {
        let (i, j) = (i as usize, j as usize);
        let (ri, rj) = (balls[i].rule, balls[j].rule);
        let (ti, tj) = (balls[i].thing, balls[j].thing);
        if !matches(&rules[ri].3.target, ti, tj) || !matches(&rules[rj].3.target, tj, ti) {
            return;
        }
        let offset = balls[j].position - balls[i].position;
        let distance = offset.length();
        let overlap = balls[i].radius + balls[j].radius - distance;
        if overlap <= 0.0 {
            return;
        }
        let normal = if distance > 1e-6 { offset / distance } else { Vec2::NEG_Y };
        let (mass_i, mass_j) = (balls[i].radius * balls[i].radius, balls[j].radius * balls[j].radius);
        let total = mass_i + mass_j;
        balls[i].position -= normal * overlap * (mass_j / total);
        balls[j].position += normal * overlap * (mass_i / total);
        let relative = balls[i].velocity - balls[j].velocity;
        let closing = relative.dot(normal);
        if closing <= 0.0 {
            return;
        }
        let restitution = (rules[ri].4 + rules[rj].4) * 0.5;
        let friction = (rules[ri].3.friction + rules[rj].3.friction) * 0.5;
        let impulse = closing * (1.0 + restitution) / total;
        let tangent = relative - normal * closing;
        let slip = tangent.length();
        let rub = if slip > 1e-6 { tangent / slip * (slip / total).min(impulse * friction) } else { Vec2::ZERO };
        balls[i].velocity -= (normal * impulse + rub) * mass_j;
        balls[j].velocity += (normal * impulse + rub) * mass_i;
        if closing >= limits.resting {
            let point = balls[i].position + normal * balls[i].radius;
            hits.push(PairHit { a: i, b: j, point, normal: -normal, speed: closing });
        }
    });

    for ball in &balls {
        let b = bodies.get_mut(&ball.thing).expect("gathered from this map");
        b.position[ball.item as usize] = ball.position;
        b.velocity[ball.item as usize] = ball.velocity;
    }

    // Log in (thing, item) order, like the wall hits.
    hits.sort_by_key(|h| (balls[h.a].thing, balls[h.a].item, balls[h.b].thing, balls[h.b].item));
    for hit in &hits {
        let (a, b) = (&balls[hit.a], &balls[hit.b]);
        for side in [a, b] {
            bodies.get_mut(&side.thing).expect("gathered").hits[side.item as usize] += 1;
        }
        log.push(Event {
            step,
            kind: EventKind::Hit,
            a: Party { thing: a.thing, item: a.item },
            b: Some(Party { thing: b.thing, item: b.item }),
            point: hit.point,
            normal: hit.normal,
            speed: hit.speed,
        });
        for side in [a, b] {
            let (_, sim, state, _, _) = &rules[side.rule];
            let body = bodies.get_mut(&side.thing).expect("gathered");
            if body.alive[side.item as usize] && sim.effects(*state, body, side.item as usize, hit.speed, limits) {
                pop(body, side.item as usize, side.thing, step, log);
            }
        }
    }
}

/// Removes ball `item` of `thing` and logs it.
pub(crate) fn pop(bodies: &mut Bodies, item: usize, thing: ThingId, step: u64, log: &mut Log) {
    bodies.alive[item] = false;
    log.push(Event {
        step,
        kind: EventKind::Remove,
        a: Party { thing, item: item as u32 },
        b: None,
        point: bodies.position[item],
        normal: Vec2::ZERO,
        speed: 0.0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_are_visited_once_in_a_fixed_order() {
        // 50 points on a 7-wide lattice, 10 units apart, in cells of 25.
        let points: Vec<Vec2> = (0..50).map(|i| Vec2::new((i % 7) as f32 * 10.0, (i / 7) as f32 * 10.0)).collect();
        let mut grid = Grid::default();
        grid.rebuild(25.0, points.iter().copied());
        let mut seen = Vec::new();
        grid.for_each_pair(|i, j| seen.push((i, j)));
        assert!(seen.iter().all(|&(i, j)| i < j));
        let mut unique = seen.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), seen.len(), "every pair once");
        // Every pair closer than a cell is among them.
        for i in 0..50u32 {
            for j in i + 1..50 {
                if (points[i as usize] - points[j as usize]).length() < 25.0 {
                    assert!(seen.contains(&(i, j)), "{i} {j}");
                }
            }
        }
        let mut again = Grid::default();
        again.rebuild(25.0, points.iter().copied());
        let mut seen2 = Vec::new();
        again.for_each_pair(|i, j| seen2.push((i, j)));
        assert_eq!(seen, seen2);
        // Broken positions land in outer cells instead of panicking.
        grid.rebuild(25.0, [Vec2::new(f32::NAN, 1e30), Vec2::ZERO].into_iter());
        assert_eq!(grid.cells().count(), 2);
        grid.rebuild(f32::NAN, [Vec2::ZERO].into_iter());
        assert_eq!(grid.cell, 1.0);
    }
}
