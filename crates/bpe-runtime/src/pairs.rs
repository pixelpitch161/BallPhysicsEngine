//! Balls hitting each other: a grid to find the pairs that are close, and a few passes over
//! them that swap momentum and ease the overlaps apart, with the walls the balls are already
//! touching holding them up so piles can rest.

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

/// How many times the contacts are solved per substep.
const PAIR_PASSES: usize = 6;
/// Overlap that is left alone, in pixels.
const PAIR_SLOP: f32 = 0.5;
/// How much of the rest of an overlap one substep eases apart.
const PAIR_BIAS: f32 = 0.2;

/// Which two balls a contact is between, `a` first in `(thing, item)` order.
type PairKey = (ThingId, u32, ThingId, u32);

/// What the ball-to-ball stage keeps between substeps: the grid, and last substep's contact
/// impulses. Starting a contact from the impulse it needed a moment ago is what lets a pile of
/// balls find its rest in a few passes instead of sinking into itself.
#[derive(Clone, Debug, Default)]
pub struct Pairs {
    pub grid: Grid,
    /// Each contact's accumulated (normal, tangent) impulse at the end of the last substep.
    warm: BTreeMap<PairKey, (f32, f32)>,
}

/// A wall or solid a ball is touching this substep, found by the wall stage. In the pair solve
/// it holds the ball up, so a pile can lean on the floor instead of sinking through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Support {
    pub thing: ThingId,
    pub item: u32,
    /// From the wall towards the ball.
    pub normal: Vec2,
    /// How fast the wall is moving there.
    pub wall_velocity: Vec2,
}

/// A support, resolved to a ball in the solve.
struct Hold {
    ball: usize,
    normal: Vec2,
    wall_velocity: Vec2,
    friction: f32,
    impulse: f32,
    tangent_impulse: f32,
    push_impulse: f32,
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
    /// Where the overlap solve moves the ball this substep, as a speed that is never kept.
    shift: Vec2,
    radius: f32,
}

/// One pair of overlapping balls this substep.
struct ContactPair {
    a: usize,
    b: usize,
    /// From `a` towards `b`.
    normal: Vec2,
    mass_a: f32,
    mass_b: f32,
    /// The speed the pair should separate at after the bounce.
    bounce: f32,
    /// The speed the pair's positions ease apart at to undo the overlap.
    push: f32,
    friction: f32,
    /// Accumulated impulses, so later passes can take back what earlier ones gave without ever
    /// pulling the balls together.
    normal_impulse: f32,
    tangent_impulse: f32,
    push_impulse: f32,
    /// How fast they were closing before anything was done, when that counts as a hit.
    hit_speed: Option<f32>,
}

/// One real hit between two balls, to log and to hand to the modules.
struct PairHit {
    a: usize,
    b: usize,
    point: Vec2,
    /// Pointing at `a`, like a wall contact's normal points at the ball.
    normal: Vec2,
    speed: f32,
}

/// Runs the ball-to-ball stage: every alive ball of every thing whose Bounce module wants pairs
/// meets the balls it overlaps (when both sides' targets agree). The pairs are solved with
/// sequential impulses: momentum is swapped with the average bounciness and friction, slow
/// touches stick instead of bouncing so piles settle, and overlaps are eased apart over a few
/// substeps rather than snapped. Hits faster than resting are logged with `a` the ball earlier
/// in `(thing, item)` order; each side's `respond_pair` and `effects` then run, and a side that
/// asks to pop is removed. `supports` are the walls the balls touched this substep: the solve
/// never pushes a ball back into one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collide_pairs(
    bodies: &mut BTreeMap<ThingId, Bodies>,
    modules: &BTreeMap<ThingId, Prepared>,
    matches: &dyn Fn(&Target, ThingId, ThingId) -> bool,
    sims: &Sims,
    pairs: &mut Pairs,
    supports: &[Support],
    step: u64,
    dt: f32,
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
                    shift: Vec2::ZERO,
                    radius: b.radius[i],
                });
            }
        }
    }
    if balls.len() < 2 {
        pairs.warm.clear();
        return;
    }
    let Pairs { grid, warm } = pairs;
    grid.rebuild(largest * 2.0, balls.iter().map(|b| b.position));

    // The walls the balls lean on, in support order; balls are sorted by (thing, item).
    let mut holds: Vec<Hold> = Vec::new();
    for s in supports {
        if let Ok(ball) = balls.binary_search_by_key(&(s.thing, s.item), |b| (b.thing, b.item)) {
            holds.push(Hold {
                ball,
                normal: s.normal,
                wall_velocity: s.wall_velocity,
                friction: rules[balls[ball].rule].3.friction,
                impulse: 0.0,
                tangent_impulse: 0.0,
                push_impulse: 0.0,
            });
        }
    }

    // Gather this substep's contacts in grid order, with what each one aims for.
    let mut contacts: Vec<ContactPair> = Vec::new();
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
        let closing = (balls[i].velocity - balls[j].velocity).dot(normal);
        // Slow touches don't bounce back: they stick, so piles settle.
        let real_hit = closing >= limits.resting;
        let bounce = if real_hit { closing * (rules[ri].4 + rules[rj].4) * 0.5 } else { 0.0 };
        let push = (overlap - PAIR_SLOP).max(0.0) * PAIR_BIAS / dt;
        contacts.push(ContactPair {
            a: i,
            b: j,
            normal,
            mass_a: balls[i].radius * balls[i].radius,
            mass_b: balls[j].radius * balls[j].radius,
            bounce,
            push,
            friction: (rules[ri].3.friction + rules[rj].3.friction) * 0.5,
            normal_impulse: 0.0,
            tangent_impulse: 0.0,
            push_impulse: 0.0,
            hit_speed: real_hit.then_some(closing),
        });
    });

    // Warm start: each contact begins with the impulse it ended on last substep.
    for c in &mut contacts {
        let key = (balls[c.a].thing, balls[c.a].item, balls[c.b].thing, balls[c.b].item);
        let Some(&(normal, tangent)) = warm.get(&key) else { continue };
        c.normal_impulse = normal;
        c.tangent_impulse = tangent;
        let impulse = c.normal * normal + c.normal.perp() * tangent;
        balls[c.a].velocity -= impulse * c.mass_b;
        balls[c.b].velocity += impulse * c.mass_a;
    }

    for _ in 0..PAIR_PASSES {
        for h in &mut holds {
            let ball = &mut balls[h.ball];
            // Into the wall is negative along its normal; the wall gives back just enough.
            let relative = ball.velocity - h.wall_velocity;
            let into = relative.dot(h.normal);
            let before = h.impulse;
            h.impulse = (before - into).max(0.0);
            ball.velocity += h.normal * (h.impulse - before);
            // The weight the wall carries for the pile rubs off sideways speed too; the ball's
            // own touch already paid its friction in the wall stage.
            let tangent = h.normal.perp();
            let most = h.friction * h.impulse;
            let before = h.tangent_impulse;
            h.tangent_impulse = (before - relative.dot(tangent)).clamp(-most, most);
            ball.velocity += tangent * (h.tangent_impulse - before);
            let before = h.push_impulse;
            h.push_impulse = (before - ball.shift.dot(h.normal)).max(0.0);
            ball.shift += h.normal * (h.push_impulse - before);
        }
        for c in &mut contacts {
            let total = c.mass_a + c.mass_b;
            let relative = balls[c.a].velocity - balls[c.b].velocity;
            let closing = relative.dot(c.normal);
            // The impulse that leaves them separating at `bounce`, never pulling them together.
            let wanted = (closing + c.bounce) / total;
            let before = c.normal_impulse;
            c.normal_impulse = (before + wanted).max(0.0);
            let along_normal = c.normal_impulse - before;
            // Friction rubs off sideways speed, up to what the normal impulse allows.
            let tangent = c.normal.perp();
            let slip = relative.dot(tangent);
            let most = c.friction * c.normal_impulse;
            let before_t = c.tangent_impulse;
            c.tangent_impulse = (before_t + slip / total).clamp(-most, most);
            let along_tangent = c.tangent_impulse - before_t;
            let impulse = c.normal * along_normal + tangent * along_tangent;
            balls[c.a].velocity -= impulse * c.mass_b;
            balls[c.b].velocity += impulse * c.mass_a;
            // The overlap is eased apart on the side, so the push never becomes speed the balls
            // keep: that is what lets a pile lie still instead of boiling.
            let shifting = (balls[c.a].shift - balls[c.b].shift).dot(c.normal);
            let wanted = (shifting + c.push) / total;
            let before = c.push_impulse;
            c.push_impulse = (before + wanted).max(0.0);
            let along = c.normal * (c.push_impulse - before);
            balls[c.a].shift -= along * c.mass_b;
            balls[c.b].shift += along * c.mass_a;
        }
    }
    for ball in &mut balls {
        ball.position += ball.shift * dt;
    }
    warm.clear();
    for c in &contacts {
        if c.normal_impulse > 0.0 {
            let key = (balls[c.a].thing, balls[c.a].item, balls[c.b].thing, balls[c.b].item);
            warm.insert(key, (c.normal_impulse, c.tangent_impulse));
        }
    }

    let hits: Vec<PairHit> = contacts
        .iter()
        .filter_map(|c| {
            let speed = c.hit_speed?;
            let point = balls[c.a].position + c.normal * balls[c.a].radius;
            Some(PairHit { a: c.a, b: c.b, point, normal: -c.normal, speed })
        })
        .collect();

    for ball in &balls {
        let b = bodies.get_mut(&ball.thing).expect("gathered from this map");
        b.position[ball.item as usize] = ball.position;
        b.velocity[ball.item as usize] = ball.velocity;
    }

    // Log in (thing, item) order, like the wall hits.
    let mut hits = hits;
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
            let item = side.item as usize;
            if !body.alive[item] {
                continue;
            }
            sim.respond_pair(*state, body, item, hit.speed, limits);
            if sim.effects(*state, body, item, hit.speed, limits) {
                pop(body, item, side.thing, step, log);
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
