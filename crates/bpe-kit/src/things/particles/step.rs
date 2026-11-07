//! One particle step (1/120 s): births, then every particle moved by the forces and the things it
//! meets, in chunks of [`CHUNK`] on threads, then the dead removed in order. Each chunk reads only
//! what is shared and read-only (the balls, the walls, the edges) and writes only its own
//! particles, so any number of threads gives the same particles.

use std::collections::{BTreeMap, BTreeSet};

use bpe_core::ThingId;
use bpe_runtime::{Bodies, EventKind, Log, World};

use crate::air::Air;
use glam::Vec2;

use super::births::{self, BirthCx, Place, unit};
use super::feeds::{FeedKind, Feeds};
use super::forces::{ForceCx, Forces};
use super::meets::{
    AtBalls, AtEdges, AtShapes, BallAt, BallGrid, ChunkOut, Edges, Landing, MeetsSettings, TouchCx, WallAt, kicked,
    meet_balls, meet_edges, meet_walls,
};
use super::piles::{self, Pile, Piles, Surfaces};
use super::ready::Ready;
use super::state::{DEAD, LOOSE, ParticlesState, RESTING, STUCK, Speck};
use crate::modules::look::layers::particles::emit::hash;

/// Seconds a particle step lasts.
pub const DT: f32 = 1.0 / 120.0;
/// Particle steps a second.
pub const STEPS_PER_SECOND: u64 = 120;
/// Particles a thread works on at a time.
pub const CHUNK: usize = 4096;
/// The most threads a step uses.
const MOST_THREADS: usize = 16;
/// How long a resting particle takes to fade away, in seconds.
pub const SETTLE_FADE: f32 = 0.5;
/// How hard a ball must hit something to shake its stuck particles off, in pixels a second.
pub const SHAKE_SPEED: f32 = 60.0;
/// How fast a particle shaken off a ball leaves its surface, in pixels a second.
const SHAKE_KICK: f32 = 40.0;

/// What the move reads, shared by every chunk.
pub struct MoveCx<'a> {
    pub forces: &'a Forces,
    pub meets: &'a MeetsSettings,
    pub grid: Option<&'a BallGrid>,
    pub walls: &'a [WallAt],
    pub edges: Option<Edges>,
    pub force: ForceCx<'a>,
    /// What part 3's forces read.
    pub ready: &'a Ready,
    /// The particle step being taken.
    pub now: u32,
    /// The fastest a particle may go.
    pub ceiling: f32,
    /// The thing's piles, when its particles pile up.
    pub piles: Option<&'a Piles>,
    /// Every ball, for the particles stuck to them.
    pub bodies: &'a BTreeMap<ThingId, Bodies>,
    /// The balls (thing, row) that landed since the last particle step, shaking stuck particles off.
    pub shaken: &'a BTreeSet<(u32, u32)>,
    /// Two-way pushes and slowing.
    pub touch: TouchCx,
    /// Two-way: stuck particles weigh their balls down.
    pub weighs: bool,
}

impl MoveCx<'_> {
    /// What one chunk gives back, empty, for `wants` moments.
    pub fn out(&self, wants: u32) -> ChunkOut {
        let balls = self.grid.map_or(0, |g| g.balls.len());
        let sized = |on: bool| if on { vec![Vec2::ZERO; balls] } else { Vec::new() };
        ChunkOut {
            feeds: Feeds::new(wants, self.now),
            pushes: sized(self.touch.pushes),
            slows: sized(self.touch.slows),
            ..ChunkOut::default()
        }
    }
}

/// Lets a resting particle go at particle step `now`: its life runs on from where it stopped.
pub fn free(s: &mut Speck, now: u32) {
    if s.resting() && s.rest > 0 {
        s.born = s.born.saturating_add(now.saturating_sub(s.rest));
    }
    s.flags &= !(RESTING | STUCK);
    s.rest = 0;
    s.fade_from = 0;
}

/// Moves a resting particle with what it rests on. True when it has come free and moves on
/// this step as any particle does.
fn rest_move(s: &mut Speck, cx: &MoveCx, out: &mut ChunkOut) -> bool {
    let now = cx.now;
    if s.fade_from > 0 && s.rest_fade(now) <= 0.0 {
        s.flags |= DEAD;
        out.feeds.note(FeedKind::Die, s, s.velocity, Vec2::ZERO);
        return false;
    }
    if s.flags & STUCK != 0 {
        let ball = cx.bodies.get(&ThingId(s.on)).and_then(|b| {
            let r = s.on_row as usize;
            (r < b.len() && b.alive[r] && b.position[r].is_finite())
                .then(|| (b.position[r], b.velocity[r], b.radius[r]))
        });
        let Some((place, velocity, radius)) = ball else {
            free(s, now);
            return true;
        };
        if cx.shaken.contains(&(s.on, s.on_row)) && unit(hash(&[s.seed, now]), 71) < cx.meets.fall_off {
            free(s, now);
            s.flags |= LOOSE;
            // It flies on as it was going (the ball's speed before the landing), off the surface.
            s.velocity += s.offset * SHAKE_KICK;
            return true;
        }
        s.previous = s.place;
        s.place = place + s.offset * radius.max(0.0);
        s.velocity = velocity;
        if cx.weighs {
            *out.stuck.entry((s.on, s.on_row)).or_default() += cx.touch.mass(s);
        }
        return false;
    }
    // Settled on a shape or the floor.
    let m = cx.meets;
    if m.fade_after > 0.0 && s.fade_from == 0 && now.saturating_sub(s.rest) as f32 * DT >= m.fade_after {
        s.fade_from = now;
    }
    if let Some(grid) = cx.grid
        && kicked(s, grid, m)
    {
        free(s, now);
        return true;
    }
    s.previous = s.place;
    if s.on != 0 {
        let Some(w) = cx.walls.iter().find(|w| w.id == s.on) else {
            free(s, now);
            return true;
        };
        // Carried by the shape's move since the last particle step: moved, turned and grown.
        let (c0, c1) = (w.before.center(), w.wall.outline.center());
        let scale = if w.before.reach() > 0.0 { w.wall.outline.reach() / w.before.reach() } else { 1.0 };
        let turn = Vec2::from_angle(w.wall.outline.angle() - w.before.angle());
        let to = c1 + turn.rotate((s.place - c0) * scale);
        if to.is_finite() {
            s.velocity = (to - s.place) / DT;
            s.place = to;
        }
    } else {
        s.velocity = Vec2::ZERO;
    }
    false
}

/// Moves one particle one step, marking it dead when its life is over or it vanishes; its
/// moments, landings and pushes go into `out`.
pub fn move_one(s: &mut Speck, index: usize, cx: &MoveCx, out: &mut ChunkOut) {
    if s.resting() && !rest_move(s, cx, out) {
        return;
    }
    if s.flags & DEAD != 0 {
        return;
    }
    let age = s.age(cx.now);
    if age >= s.life {
        s.flags |= DEAD;
        out.feeds.note(FeedKind::Die, s, s.velocity, Vec2::ZERO);
        return;
    }
    s.previous = s.place;
    let rising = s.velocity.y < 0.0;
    cx.forces.apply(s, index, age, &cx.force, cx.ready);
    if s.flags & DEAD != 0 {
        out.feeds.note(FeedKind::Die, s, s.velocity, Vec2::ZERO);
        return;
    }
    if rising && s.velocity.y >= 0.0 {
        out.feeds.note(FeedKind::StopRising, s, s.velocity, Vec2::ZERO);
    }
    let speed = s.velocity.length();
    if speed > cx.ceiling {
        s.velocity *= cx.ceiling / speed;
    }
    s.place += s.velocity * DT;
    s.turn += s.spin * DT;
    if let Some(piles) = cx.piles
        && let Some((n, i, top)) = piles::meet_piles(piles, s.previous, s.place)
    {
        s.place.y = top;
        s.flags |= DEAD;
        if s.velocity.y >= super::feeds::LAND_SPEED {
            out.feeds.note(FeedKind::Land, s, s.velocity, Vec2::NEG_Y);
        }
        out.landings.push(Landing { key: piles.list[n].key, at: s.place, pile: Some((n, i)) });
        return;
    }
    if let Some(grid) = cx.grid {
        meet_balls(s, grid, cx.meets, DT, &cx.touch, out);
    }
    if s.flags & (DEAD | RESTING) == 0 {
        meet_walls(s, cx.walls, cx.meets, out);
    }
    if s.flags & (DEAD | RESTING) == 0
        && let Some(e) = &cx.edges
    {
        meet_edges(s, e, cx.meets, out);
    }
    if !(s.place.is_finite() && s.velocity.is_finite() && s.turn.is_finite() && s.flip.is_finite()) {
        s.flags |= DEAD;
    }
}

/// How many threads a step uses on this machine.
pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, MOST_THREADS)
}

/// Runs `f` on every particle of `specks`, in chunks of [`CHUNK`] over `threads` threads.
pub fn run_chunks(specks: &mut [Speck], threads: usize, f: &(dyn Fn(&mut Speck) + Sync)) {
    run_chunks_indexed(specks, threads, &|_, s| f(s));
}

/// [`run_chunks`] telling `f` each particle's index.
pub fn run_chunks_indexed(specks: &mut [Speck], threads: usize, f: &(dyn Fn(usize, &mut Speck) + Sync)) {
    if specks.len() <= CHUNK || threads <= 1 {
        specks.iter_mut().enumerate().for_each(|(i, s)| f(i, s));
        return;
    }
    let chunks: Vec<(usize, &mut [Speck])> = specks.chunks_mut(CHUNK).enumerate().collect();
    let per = chunks.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let mut left = chunks.into_iter();
        loop {
            let group: Vec<(usize, &mut [Speck])> = left.by_ref().take(per).collect();
            if group.is_empty() {
                break;
            }
            scope.spawn(move || {
                for (k, chunk) in group {
                    chunk.iter_mut().enumerate().for_each(|(i, s)| f(k * CHUNK + i, s));
                }
            });
        }
    });
}

/// Runs `f` on every chunk of [`CHUNK`] particles of `specks` (with its first particle's index)
/// over `threads` threads, giving back what each gave, in chunk order: the same for any number of threads.
pub fn run_chunks_with<T: Send>(
    specks: &mut [Speck],
    threads: usize,
    f: &(dyn Fn(usize, &mut [Speck]) -> T + Sync),
) -> Vec<T> {
    // Each chunk with the index of its first particle.
    let chunks: Vec<(usize, &mut [Speck])> =
        specks.chunks_mut(CHUNK).enumerate().map(|(k, c)| (k * CHUNK, c)).collect();
    let run = |(base, chunk): (usize, &mut [Speck])| f(base, chunk);
    if chunks.len() <= 1 || threads <= 1 {
        return chunks.into_iter().map(run).collect();
    }
    let per = chunks.len().div_ceil(threads);
    let mut groups: Vec<Vec<(usize, &mut [Speck])>> = Vec::new();
    let mut left = chunks.into_iter();
    loop {
        let group: Vec<(usize, &mut [Speck])> = left.by_ref().take(per).collect();
        if group.is_empty() {
            break;
        }
        groups.push(group);
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = groups
            .into_iter()
            .map(|group| scope.spawn(move || group.into_iter().map(run).collect::<Vec<T>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("a particle chunk")).collect()
    })
}

/// Every alive ball in the world, by thing then row, for the particles to meet.
pub fn balls_in(world: &World) -> Vec<BallAt> {
    let mut out = Vec::new();
    for (id, b) in &world.bodies {
        for r in 0..b.len() {
            if b.alive[r] && b.position[r].is_finite() {
                out.push(BallAt {
                    place: b.position[r],
                    velocity: b.velocity[r],
                    radius: b.radius[r].max(0.0),
                    thing: *id,
                    row: r as u32,
                });
            }
        }
    }
    out
}

/// The particle steps due by the end of world step `world_step`: the world's time × 120, so a
/// particle lives as long at any Physics rate (at 240 one every second world step, at 60 two a
/// world step, at 480 one every fourth).
pub fn due(world_step: u64, steps_per_second: u32) -> u32 {
    (world_step * STEPS_PER_SECOND / u64::from(steps_per_second.max(1))) as u32
}

/// Puts the heat of `st`'s particles that heat the air into it, in particle order.
pub fn put_heat(st: &ParticlesState, air: &mut Air) {
    if !st.forces.heats_air() {
        return;
    }
    for s in &st.specks {
        let heat = st.forces.heat_of(s, s.age(st.step), DT);
        if heat > 0.0 {
            air.add_heat(s.place, heat);
        }
    }
}

/// The moments thing `id` keeps for the things it feeds among `states` (fed children ask for
/// kinds of moment).
pub fn wants_of<'a>(states: impl IntoIterator<Item = &'a ParticlesState>, id: ThingId) -> u32 {
    states
        .into_iter()
        .filter(|c| c.parent == Some(id) && c.settings.from == super::BornFrom::ItsParentsParticles)
        .fold(0, |w, c| w | c.settings.fed_when.wants())
}

/// The balls (thing, row) that hit something at least [`SHAKE_SPEED`] after world step `since`.
fn shaken_since(world: &World, log: &Log, since: u64) -> BTreeSet<(u32, u32)> {
    let events = log.events();
    let from = events.partition_point(|e| e.step <= since);
    let mut out = BTreeSet::new();
    for e in &events[from..] {
        if e.kind != EventKind::Hit || e.speed < SHAKE_SPEED {
            continue;
        }
        for side in std::iter::once(e.a).chain(e.b) {
            if world.bodies.contains_key(&side.thing) {
                out.insert((side.thing.0, side.item));
            }
        }
    }
    out
}

/// Whether the thing's particles push back on the balls: two-way on, and not fed by another
/// Particles thing (fed things are decoration).
pub fn two_way(st: &ParticlesState) -> bool {
    st.meets.two_way && st.settings.from != super::BornFrom::ItsParentsParticles && st.swarm.is_none()
}

/// A grain of sand thrown up (from a pile a ball cut, off a ledge or out of a crumbling pile) as
/// a new particle of the thing at `at`, flying at `velocity` (its own dice spread it a little).
pub fn throw(st: &mut ParticlesState, world_seed: u64, at: Vec2, velocity: Vec2) {
    let index = st.births;
    st.births += 1;
    if st.specks.len() >= st.settings.most.clamp(1, super::MOST_EVER) as usize {
        return;
    }
    let seed = births::seed_of(world_seed, st.id, index, st.settings.seed);
    let place = Place { at, impact: 1.0, ..Place::default() };
    let mut speck = births::spawn(&st.settings, &place, seed, st.step);
    let jitter = Vec2::new(births::signed(seed, 81), births::signed(seed, 82)) * (20.0 + velocity.length() * 0.25);
    speck.velocity = velocity + jitter;
    if speck.place.is_finite() && speck.velocity.is_finite() && speck.flags & DEAD == 0 {
        st.specks.push(speck);
    }
}

/// Throws the sand `cut` (places and heights) back up as grains, `share` of it, at `velocity`.
pub fn throw_sand(
    st: &mut ParticlesState,
    world_seed: u64,
    cut: &[(Vec2, f32)],
    share: f32,
    velocity: Vec2,
    carry: &mut f32,
) {
    let grain = st.meets.grain.max(1e-3);
    let mut thrown = 0;
    for &(at, height) in cut {
        *carry += height * share.clamp(0.0, 1.0) / grain;
        while *carry >= 1.0 && thrown < piles::MOST_THROWN {
            *carry -= 1.0;
            thrown += 1;
            throw(st, world_seed, at - Vec2::Y * 1.0, velocity);
        }
    }
    *carry = carry.min(1.0);
}

/// One particle step of `st` in `world`, after world step `world.step` (whose log is `log`),
/// over `threads` threads, with no air; `parent` the state of the Particles thing feeding it
/// (stepped just before). Hits since the last particle step let particles off.
pub fn step(st: &mut ParticlesState, world: &World, log: &Log, threads: usize, parent: Option<&ParticlesState>) {
    step_in(st, world, log, threads, None, parent);
}

/// [`step`] in the world's `air` (awake, at this step), which Follow the air reads.
pub fn step_in(
    st: &mut ParticlesState,
    world: &World,
    log: &Log,
    threads: usize,
    air: Option<&Air>,
    parent: Option<&ParticlesState>,
) {
    let since = st.world_step;
    st.world_step = world.step;
    st.start_moments(world.step);
    st.step += 1;
    let turn = st.settings.homes_turn.to_radians() * DT;
    if turn.is_finite() {
        st.home_turn = (st.home_turn + turn).rem_euclid(std::f32::consts::TAU);
    }
    st.feeds.clear();
    let anchor = world.anchor(st.id);
    let homes = st.home_frame(anchor.position, anchor.angle, world.camera.center);
    let cx = BirthCx { world, log, anchor: anchor.position, angle: anchor.angle, homes: homes.clone() };
    births::births(st, &cx, since, false, parent);
    let meets = st.meets.clone();
    let grid = meets.reads_balls().then(|| BallGrid::new(balls_in(world), meets.reach.max(1.0)));
    let walls: Vec<WallAt> = if meets.at_shapes == AtShapes::Pass {
        Vec::new()
    } else {
        world
            .walls
            .iter()
            .map(|(id, w)| {
                let before = st.walls_before.get(id).cloned().unwrap_or_else(|| w.outline.clone());
                WallAt::new(*id, w.clone(), before)
            })
            .collect()
    };
    let edges = (meets.at_edges != AtEdges::Pass).then(|| {
        let (centre, half) = births::screen(world, meets.edge_margin);
        Edges { centre, half }
    });
    // Piles: their surfaces this step; those whose shape moved or went crumble.
    let piling = meets.at_shapes == AtShapes::PileUp;
    let floor =
        edges.filter(|_| meets.floor()).map(|e| (e.centre.y + e.half.y, e.centre.x - e.half.x, e.centre.x + e.half.x));
    let surfaces = if piling { Surfaces::of(world, floor) } else { Surfaces::default() };
    let mut carry = 0.0;
    if !st.piles.list.is_empty() {
        let crumbled = piles::crumble_moved(&mut st.piles, &surfaces);
        throw_sand(st, world.seed, &crumbled, 1.0, Vec2::ZERO, &mut carry);
    }
    let shaken = if meets.at_balls == AtBalls::Stick { shaken_since(world, log, since) } else { BTreeSet::new() };
    let active = two_way(st);
    let touch = TouchCx { pushes: active && meets.pushes, slows: active && meets.slows, weight: meets.weight };
    let forces = st.forces.clone();
    let ready = Ready::new(st, &forces, world, log, homes);
    let piles_now = std::mem::take(&mut st.piles);
    let move_cx = MoveCx {
        forces: &forces,
        meets: &meets,
        grid: grid.as_ref(),
        walls: &walls,
        edges,
        force: ForceCx { time: world.time() as f32, dt: DT, gravity: world.gravity, air },
        ready: &ready,
        now: st.step,
        ceiling: world.limits().ceiling,
        piles: piling.then_some(&piles_now),
        bodies: &world.bodies,
        shaken: &shaken,
        touch,
        weighs: active && meets.weighs,
    };
    let wants = st.wants;
    let outs = run_chunks_with(&mut st.specks, threads, &|base, chunk| {
        let mut out = move_cx.out(wants);
        for (i, s) in chunk.iter_mut().enumerate() {
            move_one(s, base + i, &move_cx, &mut out);
        }
        out
    });
    st.piles = piles_now;
    before_retain(st);
    // The dead go, in order, and what the particles met becomes this step's moments.
    let mut specks = std::mem::take(&mut st.specks);
    specks.retain_mut(|s| {
        super::moments::collect(st, s);
        s.flags & DEAD == 0
    });
    st.specks = specks;
    // Joined in chunk order.
    let mut landings: Vec<Landing> = Vec::new();
    let balls = grid.as_ref().map_or(0, |g| g.balls.len());
    let (mut pushes, mut slows) = (vec![Vec2::ZERO; balls], vec![Vec2::ZERO; balls]);
    let mut stuck: BTreeMap<(u32, u32), f32> = BTreeMap::new();
    for out in outs {
        st.feeds.extend(out.feeds.list);
        landings.extend(out.landings);
        for (a, b) in pushes.iter_mut().zip(&out.pushes) {
            *a += *b;
        }
        for (a, b) in slows.iter_mut().zip(&out.slows) {
            *a += *b;
        }
        for (k, m) in out.stuck {
            *stuck.entry(k).or_default() += m;
        }
    }
    // Grains join their piles, in order; what pours off a ledge falls again.
    let repose = meets.repose.to_radians();
    let mut spilled = Vec::new();
    for l in &landings {
        let (n, i) = match l.pile {
            Some(at) => at,
            None => {
                let i = Pile::column_at(l.at.x);
                let lift = piles::lift_of(&surfaces, l.key, l.at);
                (piles::pile_for(&mut st.piles, l.key, i, l.at.y, lift), i)
            }
        };
        spilled.extend(piles::add_sand(&mut st.piles, n, i, meets.grain, repose, meets.grain, &surfaces));
    }
    for (at, side) in spilled.into_iter().take(piles::MOST_THROWN) {
        throw(st, world.seed, at, Vec2::new(side * piles::SPILL_SPEED, 0.0));
    }
    piles::merge_touching(&mut st.piles);
    // Balls cut through the piles (unless, two-way, they rest on them) and throw sand up.
    if piling && !st.piles.list.is_empty() && !(active && meets.holds_up) {
        for b in grid.as_ref().map_or(&[][..], |g| &g.balls[..]) {
            let cut = piles::cut_by_ball(&mut st.piles, b.place, b.radius);
            if !cut.is_empty() {
                throw_sand(st, world.seed, &cut, meets.thrown_up, thrown_velocity(b.velocity), &mut carry);
            }
        }
    }
    st.piles.list.retain(|p| !p.is_empty());
    // Past Most kept, the oldest settled particles start to fade.
    if matches!(meets.at_shapes, AtShapes::Settle | AtShapes::PileUp) {
        let settled = |s: &Speck| s.resting() && s.flags & STUCK == 0 && s.fade_from == 0;
        let mut over = st.specks.iter().filter(|s| settled(s)).count().saturating_sub(meets.most_kept.max(1) as usize);
        for s in st.specks.iter_mut() {
            if over == 0 {
                break;
            }
            if settled(s) {
                s.fade_from = st.step;
                over -= 1;
            }
        }
    }
    st.two_way = if active {
        // Pushes not yet given (two particle steps in one world step, at a Physics rate under
        // 120) add up, so the balls get them all at any rate.
        let mut out = super::two_way::TwoWayOut::of(grid.as_ref(), &pushes, &slows, &stuck, world);
        out.carry(std::mem::take(&mut st.two_way.pushes), world.limits().ceiling);
        out
    } else {
        super::two_way::TwoWayOut::default()
    };
    note_trails(st);
    note_walls(st, world);
}

/// After a step's move and before the dead go: the marks it left, and the dead's trail rows
/// dropped (every particle born this step given its rows first).
pub fn before_retain(st: &mut ParticlesState) {
    super::marks::collect(st);
    st.trail.grow(&st.specks);
    if st.trail.points > 0 && st.specks.iter().any(|s| s.flags & DEAD != 0) {
        let keep: Vec<bool> = st.specks.iter().map(|s| s.flags & DEAD == 0).collect();
        st.trail.keep(&keep);
    }
}

/// At a step's end: every particle's place noted in its trail (grains thrown up since the dead
/// went given their rows first).
pub fn note_trails(st: &mut ParticlesState) {
    st.trail.grow(&st.specks);
    st.trail.write(&st.specks, st.step);
}

/// How fast sand a ball going at `v` cuts flies up.
pub fn thrown_velocity(v: Vec2) -> Vec2 {
    Vec2::new(v.x * 0.4, -(80.0 + v.length() * 0.5))
}

/// Notes every wall's outline as it is now, for the next step's sides.
pub fn note_walls(st: &mut ParticlesState, world: &World) {
    if st.meets.at_shapes == super::meets::AtShapes::Pass {
        st.walls_before.clear();
        return;
    }
    st.walls_before = world.walls.iter().map(|(id, w)| (*id, w.outline.clone())).collect();
}

/// The first births, when the world is built (step 0).
pub fn first(st: &mut ParticlesState, world: &World) {
    let anchor = world.anchor(st.id);
    let log = Log::default();
    let homes = st.home_frame(anchor.position, anchor.angle, world.camera.center);
    let cx = BirthCx { world, log: &log, anchor: anchor.position, angle: anchor.angle, homes };
    births::births(st, &cx, 0, true, None);
    note_trails(st);
    note_walls(st, world);
}

/// The hash of every particle's place, velocity and life, for the goldens.
pub fn hash_of(st: &ParticlesState) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut put = |x: f32| {
        for b in x.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    for s in &st.specks {
        put(s.place.x);
        put(s.place.y);
        put(s.velocity.x);
        put(s.velocity.y);
        put(s.life - s.age(st.step));
    }
    h
}

/// The middle of the particles, for tests and the editor.
pub fn middle(st: &ParticlesState) -> Option<Vec2> {
    let n = st.specks.len();
    (n > 0).then(|| st.specks.iter().map(|s| s.place).sum::<Vec2>() / n as f32)
}
