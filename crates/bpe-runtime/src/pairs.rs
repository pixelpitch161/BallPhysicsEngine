//! Balls hitting each other: a grid to find the pairs that are close, and a few passes over
//! them that swap momentum and ease the overlaps apart, with the walls the balls are already
//! touching holding them up so piles can rest.

use std::collections::BTreeMap;
use std::ops::Range;

use bpe_core::{SlotId, Target, ThingId};
use glam::Vec2;

use crate::events::{crossing, step_fraction};
use crate::plugins::{Limits, ModuleSim, PairRule, Sims};
use crate::world::{Prepared, Slotted};
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
        [-1, 0, 1].map(|dx| self.column_run(cx + dx, cy - 1, cy + 1))
    }

    /// The run of entries of column `cx`'s cells from row `cy0` to `cy1`.
    fn column_run(&self, cx: i32, cy0: i32, cy1: i32) -> Range<usize> {
        let (from, to) = (key(cx, cy0), key(cx, cy1));
        let first = self.cells.partition_point(|c| c.0 < from);
        let last = first + self.cells[first..].partition_point(|c| c.0 <= to);
        match (self.cells.get(first), last > first) {
            (Some(c), true) => c.1 as usize..self.cells[last - 1].2 as usize,
            _ => 0..0,
        }
    }

    /// Runs of entries that together hold every item in the cells from `(cx0, cy0)` to `(cx1, cy1)`
    /// (corners included), each item once: one run per column, or one per occupied cell when the
    /// box is wider than there are cells.
    pub fn runs_in(&self, (cx0, cy0): (i32, i32), (cx1, cy1): (i32, i32)) -> Vec<Range<usize>> {
        let columns = cx1 as i64 - cx0 as i64 + 1;
        if columns <= 0 || cy1 < cy0 {
            return Vec::new();
        }
        if columns as usize <= self.cells.len() {
            (cx0..=cx1).map(|cx| self.column_run(cx, cy0, cy1)).filter(|r| !r.is_empty()).collect()
        } else {
            self.cells()
                .filter(|((cx, cy), _)| (cx0..=cx1).contains(cx) && (cy0..=cy1).contains(cy))
                .map(|(_, run)| run)
                .collect()
        }
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

/// A ball this many times the median radius is big: it meets its neighbours apart from the grid
/// (W7, S16), so the grid's cells stay sized for the others.
const BIG_TIMES: f32 = 4.0;
/// Big balls are handled apart only while there is at most one in this many balls.
const BIG_SHARE: usize = 8;

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

/// One ball that can bump into others.
struct Ball {
    thing: ThingId,
    item: u32,
    /// Index into `rules`.
    rule: usize,
    position: Vec2,
    /// Where it was before this substep moved it.
    start: Vec2,
    velocity: Vec2,
    /// Where the overlap solve moves the ball this substep, as a speed that is never kept.
    shift: Vec2,
    radius: f32,
    /// Its own bounciness, friction and press: its thing's, or its own when the thing's cards
    /// vary across its balls.
    bounciness: f32,
    friction: f32,
    press: Vec2,
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
    /// How far through the step they touched (see [`Event::at`]).
    at: f32,
}

/// One real hit between two balls, to log and to hand to the modules.
struct PairHit {
    a: usize,
    b: usize,
    point: Vec2,
    /// Pointing at `a`, like a wall contact's normal points at the ball.
    normal: Vec2,
    speed: f32,
    at: f32,
}

/// What a body thing brings to the ball-to-ball stage: the thing, its Bounce module and card, its
/// pair rule, its bounciness and its balls' own press (see [`Limits::press`]).
type RuleOf<'a> = (ThingId, &'a dyn ModuleSim, &'a Slotted, PairRule, f32, Vec2);

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
    press: &dyn Fn(ThingId, usize) -> Vec2,
    sims: &Sims,
    pairs: &mut Pairs,
    supports: &[Support],
    step: u64,
    substep: (u32, u32),
    dt: f32,
    limits: &Limits,
    log: &mut Log,
) {
    // Gather the balls that take part, in (thing, item) order.
    let mut rules: Vec<RuleOf> = Vec::new();
    let mut balls: Vec<Ball> = Vec::new();
    for (thing, b) in bodies.iter() {
        let Some(s) = modules.get(thing).and_then(|m| m.get(&SlotId::Bounce)) else { continue };
        let Some(sim) = sims.module(&s.kind) else { continue };
        let Some(rule) = sim.pairs(s.state.as_ref()) else { continue };
        let bounciness = sim.bounciness(s.state.as_ref());
        let pressed = modules.get(thing).and_then(|m| m.get(&SlotId::Motion)).is_some_and(Slotted::varies);
        let thing_press = press(*thing, 0);
        let (friction, varies) = (rule.friction, s.varies());
        rules.push((*thing, sim, s, rule, bounciness, thing_press));
        let rule = rules.len() - 1;
        for i in b.live_rows() {
            if b.alive[i] && !b.held[i] {
                let (bounciness, friction) = if varies {
                    let own = s.state_for(i);
                    (sim.bounciness(own), sim.pairs(own).map_or(friction, |r| r.friction))
                } else {
                    (bounciness, friction)
                };
                balls.push(Ball {
                    bounciness,
                    friction,
                    press: if pressed { press(*thing, i) } else { thing_press },
                    thing: *thing,
                    item: i as u32,
                    rule,
                    position: b.position[i],
                    start: b.start[i],
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

    // The walls the balls lean on, in support order; balls are sorted by (thing, item).
    let mut holds: Vec<Hold> = Vec::new();
    for s in supports {
        if let Ok(ball) = balls.binary_search_by_key(&(s.thing, s.item), |b| (b.thing, b.item)) {
            holds.push(Hold {
                ball,
                normal: s.normal,
                wall_velocity: s.wall_velocity,
                friction: balls[ball].friction,
                impulse: 0.0,
                tangent_impulse: 0.0,
                push_impulse: 0.0,
            });
        }
    }

    // Gather this substep's contacts in grid order, with what each one aims for.
    let mut contacts: Vec<ContactPair> = Vec::new();
    let spot = |i: usize| (balls[i].position, balls[i].radius);
    close_pairs(grid, balls.len(), spot, |i, j| {
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
        // Slow touches don't bounce back: they stick, so piles settle. The side pressed in
        // harder decides what is slow.
        let along = |press: Vec2| Limits { press, ..*limits }.resting_along(normal);
        let real_hit = closing >= along(balls[i].press).max(along(balls[j].press));
        let reach = balls[i].radius + balls[j].radius;
        let within = crossing((balls[j].start - balls[i].start).length() - reach, -overlap);
        let at = step_fraction(substep.0, substep.1, within);
        let bounce = if real_hit { closing * (balls[i].bounciness + balls[j].bounciness) * 0.5 } else { 0.0 };
        let push = (overlap - PAIR_SLOP).max(0.0) * PAIR_BIAS / dt;
        contacts.push(ContactPair {
            a: i,
            b: j,
            normal,
            mass_a: balls[i].radius * balls[i].radius,
            mass_b: balls[j].radius * balls[j].radius,
            bounce,
            push,
            friction: (balls[i].friction + balls[j].friction) * 0.5,
            normal_impulse: 0.0,
            tangent_impulse: 0.0,
            push_impulse: 0.0,
            hit_speed: real_hit.then_some(closing),
            at,
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
            Some(PairHit { a: c.a, b: c.b, point, normal: -c.normal, speed, at: c.at })
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
        // A ball that popped on an earlier hit this substep is gone: nothing more hits it.
        if [a, b].iter().any(|side| !bodies[&side.thing].alive[side.item as usize]) {
            continue;
        }
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
            at: hit.at,
        });
        for side in [a, b] {
            let (_, sim, card, _, _, _) = &rules[side.rule];
            let body = bodies.get_mut(&side.thing).expect("gathered");
            let item = side.item as usize;
            if !body.alive[item] {
                continue;
            }
            let state = card.state_for(item);
            sim.respond_pair(state, body, item, hit.speed, limits);
            if sim.effects(state, body, item, hit.speed, limits) {
                pop(body, item, side.thing, step, log);
            }
        }
    }
}

/// Calls `f(i, j)` with `i < j` once for every pair of the `count` balls (`spot(i)` is ball `i`'s
/// position and radius) that may touch, in a fixed order. Usually every pair from the grid, in grid
/// order (see [`Grid::for_each_pair`]). When a few balls are much bigger than the rest (W7, S16)
/// the grid holds only the others, sized for them, and the big balls' pairs (the grid cells each
/// one's reach covers, and the other big balls) come after, in index order.
fn close_pairs(grid: &mut Grid, count: usize, spot: impl Fn(usize) -> (Vec2, f32), mut f: impl FnMut(usize, usize)) {
    let bigs = big_balls(count, |i| spot(i).1);
    let largest = |of: &mut dyn Iterator<Item = usize>| of.map(|i| spot(i).1).fold(0.0f32, f32::max);
    if bigs.is_empty() {
        grid.rebuild(largest(&mut (0..count)) * 2.0, (0..count).map(|i| spot(i).0));
        grid.for_each_pair(|i, j| f(i as usize, j as usize));
        return;
    }
    // The grid's items are the small balls, in order: `small` maps them back.
    let small: Vec<usize> = (0..count).filter(|i| bigs.binary_search(i).is_err()).collect();
    let reach_small = largest(&mut small.iter().copied());
    grid.rebuild(reach_small * 2.0, small.iter().map(|&i| spot(i).0));
    grid.for_each_pair(|i, j| f(small[i as usize], small[j as usize]));
    let mut big_pairs: Vec<(usize, usize)> = Vec::new();
    for (k, &g) in bigs.iter().enumerate() {
        let (at, radius) = spot(g);
        let reach = Vec2::splat(radius + reach_small);
        for run in grid.runs_in(grid.cell_of(at - reach), grid.cell_of(at + reach)) {
            big_pairs.extend(grid.run(run).map(|j| small[j as usize]).map(|j| (g.min(j), g.max(j))));
        }
        big_pairs.extend(bigs[k + 1..].iter().map(|&h| (g, h)));
    }
    big_pairs.sort_unstable();
    big_pairs.dedup();
    for (i, j) in big_pairs {
        f(i, j);
    }
}

/// The balls (indices, in order) much bigger than most, when there are a few of them: radius over
/// [`BIG_TIMES`] the median, and no more than one ball in [`BIG_SHARE`]. Empty otherwise.
fn big_balls(count: usize, radius: impl Fn(usize) -> f32) -> Vec<usize> {
    if count < BIG_SHARE {
        return Vec::new();
    }
    let mut radii: Vec<f32> = (0..count).map(&radius).collect();
    let median = *radii.select_nth_unstable_by(count / 2, f32::total_cmp).1;
    let bigs: Vec<usize> = (0..count).filter(|&i| radius(i) > median * BIG_TIMES).collect();
    if bigs.len() * BIG_SHARE <= count { bigs } else { Vec::new() }
}

/// Removes ball `item` of `thing` and logs it.
pub fn pop(bodies: &mut Bodies, item: usize, thing: ThingId, step: u64, log: &mut Log) {
    bodies.alive[item] = false;
    log.push(Event {
        step,
        kind: EventKind::Remove,
        a: Party { thing, item: item as u32 },
        b: None,
        point: bodies.position[item],
        normal: Vec2::ZERO,
        speed: 0.0,
        at: 0.0,
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

    /// Every pair of balls that overlaps, by checking them all.
    fn overlapping(spots: &[(Vec2, f32)]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for i in 0..spots.len() {
            for j in i + 1..spots.len() {
                if (spots[j].0 - spots[i].0).length() < spots[i].1 + spots[j].1 {
                    out.push((i, j));
                }
            }
        }
        out
    }

    /// With a few big balls among small ones, the pairs offered still hold every overlapping pair
    /// (and each pair once); without any, they are the grid's, in its order.
    #[test]
    fn big_balls_apart_still_meet_everything_they_touch() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32
        };
        for layout in 0..12 {
            let n = 200 + layout * 37;
            let mut spots: Vec<(Vec2, f32)> =
                (0..n).map(|_| (Vec2::new(next() * 600.0 - 300.0, next() * 900.0), 3.0 + next() * 4.0)).collect();
            let giants = 1 + layout % 4;
            for g in 0..giants {
                let i = (g * 53 + layout * 7) % n;
                spots[i].1 = 60.0 + next() * 200.0;
            }
            assert_eq!(big_balls(n, |i| spots[i].1).len(), giants, "layout {layout}");
            let mut grid = Grid::default();
            let mut offered = Vec::new();
            close_pairs(&mut grid, n, |i| spots[i], |i, j| offered.push((i, j)));
            assert!(offered.iter().all(|&(i, j)| i < j));
            let mut unique = offered.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), offered.len(), "every pair once");
            for pair in overlapping(&spots) {
                assert!(unique.binary_search(&pair).is_ok(), "layout {layout}: {pair:?} missed");
            }
            assert!(offered.len() < n * n / 8, "not every pair: {}", offered.len());
        }
        // No big ball: exactly the grid's pairs over every ball, as before.
        let spots: Vec<(Vec2, f32)> = (0..300).map(|_| (Vec2::new(next() * 300.0, next() * 300.0), 4.0)).collect();
        let (mut grid, mut plain) = (Grid::default(), Grid::default());
        let mut offered = Vec::new();
        close_pairs(&mut grid, spots.len(), |i| spots[i], |i, j| offered.push((i, j)));
        plain.rebuild(8.0, spots.iter().map(|s| s.0));
        let mut before = Vec::new();
        plain.for_each_pair(|i, j| before.push((i as usize, j as usize)));
        assert_eq!(offered, before);
        // Too many big ones (more than one in eight) is not "a few": the grid takes them all.
        let mut many = spots.clone();
        for s in many.iter_mut().step_by(5) {
            s.1 = 40.0;
        }
        assert!(big_balls(many.len(), |i| many[i].1).is_empty());
    }

    #[test]
    fn runs_in_a_box_hold_each_item_once() {
        let points: Vec<Vec2> = (0..400).map(|i| Vec2::new((i % 20) as f32 * 7.0, (i / 20) as f32 * 7.0)).collect();
        let mut grid = Grid::default();
        grid.rebuild(10.0, points.iter().copied());
        for (from, to) in [((2, 3), (6, 9)), ((-5, -5), (100, 100)), ((-3, 0), (-1, 4)), ((4, 4), (4, 4))] {
            let mut got: Vec<u32> = grid.runs_in(from, to).into_iter().flat_map(|r| grid.run(r)).collect();
            got.sort_unstable();
            let want: Vec<u32> = (0..400u32)
                .filter(|&i| {
                    let (cx, cy) = grid.cell_of(points[i as usize]);
                    (from.0..=to.0).contains(&cx) && (from.1..=to.1).contains(&cy)
                })
                .collect();
            assert_eq!(got, want, "{from:?} {to:?}");
        }
    }
}
