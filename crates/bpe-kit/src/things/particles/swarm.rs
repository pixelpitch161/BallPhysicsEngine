//! A Huge swarm (particles part 8): a Particles thing whose Look card's Engine is Huge swarm moves
//! its particles on the graphics card (`bpe_gfx::swarm`). Its state keeps the card's buffers;
//! each particle step works out what the step reads on the processor (the births, with the exact
//! engine's own dice, the forces, the balls and their grid, the shapes as segments and their
//! grid, the air, the edges) and hands it to the card. Nothing comes back but a late count for
//! the status line. Its drawing is [`super::draw`]'s `swarm_runs`.

use std::sync::Arc;

use bpe_core::ThingId;
use bpe_gfx::swarm::{
    self, FORCE_AIR, FORCE_DRAG, FORCE_FLOW, FORCE_GRAVITY, FORCE_MAGNETIC, FORCE_PULL, FORCE_PULSE, FORCE_RIPPLE,
    FORCE_RISE, FORCE_SPIROGRAPH, FORCE_SPRING, FORCE_SWAY, FORCE_SWIRL, FORCE_WANDER, FORCE_WIND, ForceGpu,
    MOST_FORCES, STEP_AIR, STEP_BALLS, STEP_CROWD, STEP_EDGES, STEP_STILL, STEP_WALLS, SpeckMain, SpeckMore, StepGpu,
    StepIn, SwarmBuffers,
};
use bpe_runtime::{Log, Outline, World};
use glam::Vec2;

use super::births::{self, BirthCx, MOST_SWARM_AT_ONCE};
use super::forces::{Force, Forces, OnlyFor};
use super::meets::{AtBalls, AtEdges, AtShapes, BallGrid, MeetsSettings};
use super::more_forces::{Falloff, FieldFrom, FieldPoints, Pattern, Pulling, PulseWay, Toward, pulse_share};
use super::ready::{ForceReady, Ready, Ring};
use super::state::{ParticlesState, Speck};
use super::step::{DT, balls_in, note_walls};
use super::{Emit, MOST_SWARM};
use crate::air::Air;

/// A swarm's particles on the graphics card: `None` where the thread its world was built on
/// named no card (a world without a picture to draw). Clones share the buffers until one of them
/// steps, which copies them on the card first, so a checkpoint, a snapshot or the preview's
/// borrowed copy costs nothing until the swarm moves on.
#[derive(Clone, Debug, Default)]
pub struct Swarm {
    pub buffers: Option<Arc<SwarmBuffers>>,
}

impl Swarm {
    /// A swarm with no particles yet, on the card this thread names.
    pub fn new() -> Swarm {
        Swarm { buffers: swarm::on_this_thread().map(|gpu| Arc::new(SwarmBuffers::new(gpu))) }
    }

    /// Bytes its particles take in video memory.
    pub fn gpu_memory(&self) -> usize {
        self.buffers.as_ref().map_or(0, |b| b.bytes())
    }

    /// How many particles the card last said there were (a step or two late; 0 without a card).
    pub fn count_hint(&self) -> u32 {
        self.buffers.as_ref().map_or(0, |b| b.count_hint())
    }
}

/// A number that is finite, else 0.
fn fin(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

fn only_code(o: OnlyFor) -> u32 {
    match o {
        OnlyFor::All => 0,
        OnlyFor::YoungerThan => 1,
        OnlyFor::OlderThan => 2,
        OnlyFor::AShare => 3,
        OnlyFor::TheRest => 4,
    }
}

fn falloff_code(f: Falloff) -> f32 {
    match f {
        Falloff::Even => 0.0,
        Falloff::WeakerFurther => 1.0,
        Falloff::MuchWeakerFurther => 2.0,
    }
}

/// A fixed-centre force's mode on the card (0 the step's centre, put at the word it gives, 1
/// none, 2 the particle's ball, 3 balls nearby) for `toward` and what the step gathered.
fn centre_mode(w: &mut Words, toward: Toward, gathered: &ForceReady) -> (u32, u32) {
    match (toward, gathered) {
        (Toward::BallsNearby, _) => (3, 0),
        (Toward::ItsBall, _) => (2, 0),
        (_, ForceReady::Centre(Some((place, velocity)))) => {
            let at = w.at();
            w.v(*place);
            w.v(*velocity);
            (0, at)
        }
        _ => (1, 0),
    }
}

/// Whether a swarm moves particles by `f` (Flock needs every particle's neighbours, which the
/// card doesn't gather).
pub fn swarm_moves_by(f: &Force) -> bool {
    !matches!(f, Force::Flock(_))
}

/// The first [`MOST_FORCES`] forces of `forces` a swarm moves by, as the card applies them, with
/// the scene's `gravity` and what the step gathered (`ready`); their lists go into `w`.
fn forces_gpu(forces: &Forces, gravity: Vec2, ready: &Ready, w: &mut Words) -> (u32, [ForceGpu; MOST_FORCES]) {
    let mut out = [ForceGpu::default(); MOST_FORCES];
    let mut n = 0;
    for (k, f) in forces.list.iter().enumerate() {
        if n == MOST_FORCES {
            break;
        }
        if !swarm_moves_by(f) {
            continue;
        }
        let (amount, only, age, share) = f.common();
        // A share that isn't a number picks none (and none of the rest), as on the processor.
        if share.is_nan() && matches!(only, OnlyFor::AShare | OnlyFor::TheRest) {
            continue;
        }
        let gathered = ready.of(k);
        let mut kind = [0, only_code(only), 0, 0];
        let mut a = [fin(amount), age, share, 0.0];
        let (code, b, c) = match f {
            Force::Gravity(g) => {
                let pull =
                    if g.scene { gravity } else { Vec2::from_angle(fin(g.direction).to_radians()) * fin(g.strength) };
                (FORCE_GRAVITY, [fin(pull.x), fin(pull.y), 0.0, 0.0], [0.0; 4])
            }
            Force::Wind(w) => {
                let way = Vec2::from_angle(fin(w.direction).to_radians());
                let every = if w.gust_every.is_nan() { 0.01 } else { w.gust_every };
                (FORCE_WIND, [way.x, way.y, fin(w.speed), fin(w.grip)], [fin(w.gusts), every, fin(w.waves), 0.0])
            }
            Force::Drag(d) => (FORCE_DRAG, [fin(d.drag), 0.0, 0.0, 0.0], [0.0; 4]),
            Force::Swirl(w) => {
                let size = if w.size.is_nan() { 1.0 } else { w.size.max(1.0) };
                (FORCE_SWIRL, [fin(w.strength), size, fin(w.drift), 0.0], [0.0; 4])
            }
            Force::Wander(w) => (FORCE_WANDER, [fin(w.strength), fin(w.turns), 0.0, 0.0], [0.0; 4]),
            Force::Rise(r) => (FORCE_RISE, [fin(r.strength), fin(r.cools), 0.0, 0.0], [0.0; 4]),
            Force::Sway(w) => {
                (FORCE_SWAY, [fin(w.sway), fin(w.sway_speed), fin(w.flutter), fin(w.flutter_drift)], [0.0; 4])
            }
            Force::Air(a) => (FORCE_AIR, [fin(a.grip), 0.0, 0.0, 0.0], [0.0; 4]),
            Force::Pull(_) | Force::Push(_) => {
                let p: Pulling = match f {
                    Force::Pull(p) => p.into(),
                    Force::Push(p) => p.into(),
                    _ => unreachable!(),
                };
                let (mode, at) = centre_mode(w, p.toward, gathered);
                kind[2] = mode;
                kind[3] = at;
                a[3] = p.sign;
                let b = [fin(p.strength), fin(p.at), fin(p.reach), fin(p.orbit)];
                (FORCE_PULL, b, [fin(p.swallow), fin(p.ball_reach), falloff_code(p.falloff), 0.0])
            }
            Force::Spring(p) => (FORCE_SPRING, [fin(p.stiffness), fin(p.damping), 0.0, 0.0], [0.0; 4]),
            Force::Magnetic(m) => {
                kind[2] = u32::from(m.from == FieldFrom::ItsPlace);
                kind[3] = u32::from(m.points == FieldPoints::AWay);
                let fixed = Vec2::from_angle(fin(m.angle).to_radians());
                (
                    FORCE_MAGNETIC,
                    [fixed.x, fixed.y, fin(m.size), fin(m.size_px)],
                    [fin(m.line_up), fin(m.pull), 0.0, 0.0],
                )
            }
            Force::Flow(p) => {
                kind[2] = match p.pattern {
                    Pattern::Noise => 0,
                    Pattern::Spiral => 1,
                    Pattern::Waves => 2,
                    Pattern::Vortex => 3,
                };
                let b = [fin(p.speed), fin(p.size), fin(p.grip), fin(p.changes)];
                let c = [fin(p.angle).to_radians(), fin(p.pull_in).to_radians(), fin(p.sway).to_radians(), 0.0];
                (FORCE_FLOW, b, c)
            }
            Force::Spirograph(p) => {
                let toward = p.round.toward();
                let (mode, at) = centre_mode(w, toward, gathered);
                kind[2] = mode;
                kind[3] = at;
                (
                    FORCE_SPIROGRAPH,
                    [fin(p.ring), fin(p.wheel), fin(p.pen), fin(p.speed)],
                    [fin(p.differ), 0.0, 0.0, 0.0],
                )
            }
            Force::Pulse(p) => {
                let share = match gathered {
                    ForceReady::Beats(Some(beats)) => pulse_share(beats, DT as f64, fin(p.decay).max(0.0) as f64),
                    _ => 0.0,
                };
                kind[2] = match p.way {
                    PulseWay::Outwards => 0,
                    PulseWay::Up => 1,
                    PulseWay::AWay => 2,
                    PulseWay::AllRound => 3,
                };
                let way = Vec2::from_angle(fin(p.angle).to_radians());
                (FORCE_PULSE, [fin(p.strength), fin(share as f32), way.x, way.y], [0.0; 4])
            }
            Force::Ripple(r) => {
                let rings: &[Ring] = match gathered {
                    ForceReady::Rings(rings) => rings,
                    _ => &[],
                };
                kind[2] = rings.len() as u32;
                kind[3] = w.at();
                for ring in rings {
                    w.v(ring.centre);
                    w.f(ring.age);
                    w.f(ring.impact);
                }
                let b = [fin(r.speed), fin(r.width), fin(r.strength), fin(r.lasts)];
                (FORCE_RIPPLE, b, [0.0; 4])
            }
            Force::Flock(_) => unreachable!("left out above"),
        };
        kind[0] = code;
        out[n] = ForceGpu { kind, a, b, c };
        n += 1;
    }
    (n as u32, out)
}

/// What part 3's forces read besides each force's own: the thing's middle and the clock, the
/// parent's balls (Its ball), the homes, and the grid of balls near each particle.
fn put_ready(w: &mut Words, ready: &Ready, step: &mut StepGpu) {
    step.middle = [ready.middle.x, ready.middle.y, ready.time as f32, 0.0];
    let (place, velocity) = ready.parent_middle.unwrap_or((ready.middle, Vec2::ZERO));
    step.parent = [place.x, place.y, velocity.x, velocity.y];
    step.homes = [ready.homes.centre.x, ready.homes.centre.y, ready.homes.way.x, ready.homes.way.y];
    let balls_at = w.at();
    for b in &ready.parent_balls {
        let (place, velocity) = b.map_or((Vec2::ZERO, Vec2::ZERO), |b| (b.place, b.velocity));
        w.v(place);
        w.v(velocity);
        w.words.push(u32::from(b.is_some()));
    }
    let spots_at = w.at();
    for p in ready.homes.spots.iter() {
        w.v(*p);
    }
    step.lists = [balls_at, ready.parent_balls.len() as u32, spots_at, ready.homes.spots.len() as u32];
    if let Some(grid) = &ready.balls {
        let parts = grid.parts();
        if !parts.balls.is_empty() {
            let first = w.at();
            for b in parts.balls {
                w.v(b.place);
                w.v(b.velocity);
                w.f(b.radius);
            }
            let cells_at = w.at();
            w.words.extend_from_slice(parts.starts);
            let items_at = w.at();
            w.words.extend_from_slice(parts.items);
            step.fballs = [parts.origin.x, parts.origin.y, parts.cell, parts.reach];
            step.fball_at = [parts.cols as u32, parts.rows as u32, first, cells_at];
            step.fball_more = [items_at, parts.balls.len() as u32, 0, 0];
        }
    }
}

/// The words of a step's inputs, and where each list starts in them.
#[derive(Default)]
struct Words {
    words: Vec<u32>,
}

impl Words {
    fn at(&self) -> u32 {
        self.words.len() as u32
    }
    fn f(&mut self, x: f32) {
        self.words.push(x.to_bits());
    }
    fn v(&mut self, v: Vec2) {
        self.f(v.x);
        self.f(v.y);
    }
}

/// The cells of a segment grid a side has at most, and the most entries it holds.
const MOST_WALL_CELLS: usize = 128;
const MOST_SEGMENTS: usize = 65_536;
/// The smallest cell of a segment grid, in pixels.
const LEAST_WALL_CELL: f32 = 32.0;

/// One side of a wall: where it ran at the last particle step and where it runs now.
struct Segment {
    a0: Vec2,
    b0: Vec2,
    a1: Vec2,
    b1: Vec2,
    half: f32,
}

/// The segments of every wall particles meet: each wall's outline now and as it was at the last
/// particle step (`before`), point by point.
fn segments(world: &World, before: &std::collections::BTreeMap<ThingId, Outline>) -> Vec<Segment> {
    let mut out = Vec::new();
    for (id, w) in &world.walls {
        if w.passes {
            continue;
        }
        let half = if w.solid { 0.0 } else { fin(w.outline.thickness()).max(0.0) * 0.5 };
        let nows = crate::outline::contours(&w.outline);
        let thens = before.get(id).map(crate::outline::contours);
        let same = |t: &Vec<Vec<Vec2>>| t.len() == nows.len() && t.iter().zip(&nows).all(|(a, b)| a.len() == b.len());
        let thens = thens.filter(same).unwrap_or_else(|| nows.clone());
        for (now, then) in nows.iter().zip(&thens) {
            let n = now.len();
            if n < 2 {
                continue;
            }
            for k in 0..n {
                let j = (k + 1) % n;
                let s = Segment { a0: then[k], b0: then[j], a1: now[k], b1: now[j], half };
                if [s.a0, s.b0, s.a1, s.b1].iter().all(|p| p.is_finite()) && s.half.is_finite() {
                    out.push(s);
                }
            }
        }
    }
    out
}

/// Puts `segments` and a uniform grid over them into `w`; `reach` is how far a particle can
/// go in a step (each segment is listed in every cell within it). Gives the step's wall fields.
fn put_walls(w: &mut Words, segments: &[Segment], reach: f32, step: &mut StepGpu) -> bool {
    if segments.is_empty() {
        return false;
    }
    let grow = |s: &Segment| s.half + 1.5 + reach;
    let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    for s in segments {
        let g = Vec2::splat(grow(s));
        for p in [s.a0, s.b0, s.a1, s.b1] {
            lo = lo.min(p - g);
            hi = hi.max(p + g);
        }
    }
    let span = (hi - lo).max(Vec2::ONE);
    let mut cell = LEAST_WALL_CELL.max(span.x / MOST_WALL_CELLS as f32).max(span.y / MOST_WALL_CELLS as f32);
    // Larger cells until every segment's entries fit.
    let (cols, rows, starts, items) = loop {
        let cols = ((span.x / cell).ceil() as usize).clamp(1, MOST_WALL_CELLS);
        let rows = ((span.y / cell).ceil() as usize).clamp(1, MOST_WALL_CELLS);
        let index = |p: Vec2| {
            let c = (((p.x - lo.x) / cell) as isize).clamp(0, cols as isize - 1) as usize;
            let r = (((p.y - lo.y) / cell) as isize).clamp(0, rows as isize - 1) as usize;
            (c, r)
        };
        let boxes: Vec<((usize, usize), (usize, usize))> = segments
            .iter()
            .map(|s| {
                let g = Vec2::splat(grow(s));
                let a = s.a0.min(s.b0).min(s.a1).min(s.b1) - g;
                let b = s.a0.max(s.b0).max(s.a1).max(s.b1) + g;
                (index(a), index(b))
            })
            .collect();
        let entries: usize = boxes.iter().map(|((c0, r0), (c1, r1))| (c1 - c0 + 1) * (r1 - r0 + 1)).sum();
        if entries > MOST_SEGMENTS && cell < span.max_element() {
            cell *= 2.0;
            continue;
        }
        let mut counts = vec![0u32; cols * rows];
        for ((c0, r0), (c1, r1)) in &boxes {
            for r in *r0..=*r1 {
                for c in *c0..=*c1 {
                    counts[r * cols + c] += 1;
                }
            }
        }
        let mut starts = vec![0u32; cols * rows + 1];
        for k in 0..cols * rows {
            starts[k + 1] = starts[k] + counts[k];
        }
        let mut fill = starts.clone();
        let mut items = vec![0u32; starts[cols * rows] as usize];
        for (i, ((c0, r0), (c1, r1))) in boxes.iter().enumerate() {
            for r in *r0..=*r1 {
                for c in *c0..=*c1 {
                    let k = r * cols + c;
                    items[fill[k] as usize] = i as u32;
                    fill[k] += 1;
                }
            }
        }
        break (cols, rows, starts, items);
    };
    let first = w.at();
    for s in segments {
        w.v(s.a0);
        w.v(s.b0);
        w.v(s.a1);
        w.v(s.b1);
        w.f(s.half);
    }
    let cells_at = w.at();
    w.words.extend_from_slice(&starts);
    let items_at = w.at();
    w.words.extend_from_slice(&items);
    step.walls = [lo.x, lo.y, cell, 0.0];
    step.wall_at = [cols as u32, rows as u32, first, cells_at];
    step.wall_more = [items_at, segments.len() as u32, 0, 0];
    true
}

/// Puts the balls of `grid` and its cells into `w`. Gives whether there are any.
fn put_balls(w: &mut Words, grid: &BallGrid, step: &mut StepGpu) -> bool {
    let parts = grid.parts();
    if parts.balls.is_empty() {
        return false;
    }
    let first = w.at();
    for b in parts.balls {
        w.v(b.place);
        w.v(b.velocity);
        w.f(b.radius);
    }
    let cells_at = w.at();
    w.words.extend_from_slice(parts.starts);
    let items_at = w.at();
    w.words.extend_from_slice(parts.items);
    step.balls = [parts.origin.x, parts.origin.y, parts.cell, 0.0];
    step.ball_at = [parts.cols as u32, parts.rows as u32, first, cells_at];
    step.ball_more = [items_at, parts.balls.len() as u32, 0, 0];
    step.time[3] = parts.reach;
    true
}

/// Puts the air's grid into `w`, and its ambient wind into the step.
fn put_air(w: &mut Words, air: &Air, step: &mut StepGpu) {
    let (cols, rows) = air.size();
    let first = w.at();
    for v in air.velocities() {
        w.v(*v);
    }
    let origin = air.origin();
    step.air = [air.cell_size(), origin.x as f32, origin.y as f32, 0.0];
    step.air_at = [cols as u32, rows as u32, first, u32::from(air.awake() && cols > 0)];
    let spec = &air.spec;
    let way = spec.way();
    let waves = if spec.waves.is_finite() { spec.waves } else { 0.0 };
    let every = if spec.gust_every.is_nan() { 0.01 } else { spec.gust_every };
    step.wind = [way.x, way.y, fin(spec.wind_speed), fin(spec.gusts)];
    step.gust = [every, waves, air.time(), 0.0];
}

fn at_balls_code(a: AtBalls) -> u32 {
    match a {
        AtBalls::Pass => 0,
        AtBalls::BounceOff | AtBalls::Stick => 1,
        // Popping and sticking need the processor: a swarm's vanish and bounce off.
        AtBalls::Vanish | AtBalls::Pop => 2,
    }
}

fn at_shapes_code(a: AtShapes) -> u32 {
    match a {
        AtShapes::Pass => 0,
        AtShapes::Bounce => 1,
        // Settling and piling up need the processor: a swarm's particles stop.
        AtShapes::Stop | AtShapes::Settle | AtShapes::PileUp => 2,
        AtShapes::Vanish => 3,
    }
}

fn at_edges_code(a: AtEdges) -> u32 {
    match a {
        AtEdges::Pass => 0,
        AtEdges::WrapRound => 1,
        AtEdges::Bounce => 2,
        AtEdges::Vanish => 3,
    }
}

/// `births` as the card takes them.
fn packed(births: &[Speck]) -> Vec<(SpeckMain, SpeckMore)> {
    births
        .iter()
        .map(|s| {
            (
                SpeckMain {
                    place: s.place.into(),
                    velocity: s.velocity.into(),
                    born: s.born,
                    life: s.life,
                    seed: s.seed,
                    turn: s.turn,
                },
                SpeckMore {
                    home: s.home.into(),
                    size: s.size,
                    spin: s.spin,
                    flip: s.flip,
                    ball: s.ball,
                    spot: s.spot,
                    spare: 0,
                },
            )
        })
        .collect()
}

/// The step's own settings: the counts, the Source's caps and crowd, and the time.
fn step_head(st: &ParticlesState, world: &World) -> StepGpu {
    let s = &st.settings;
    let mut step = StepGpu {
        counts: [st.step, 0, s.most.clamp(1, MOST_SWARM), 0],
        time: [world.time() as f32, DT, world.limits().ceiling, 1.0],
        ..StepGpu::default()
    };
    // A word's, picture's or grid's crowd is every place, born at the start (not refilled).
    if s.emit == Emit::KeepACrowd && !births::has_spots(s) {
        step.more[2] |= STEP_CROWD;
        step.counts[3] = s.count.min(MOST_SWARM_AT_ONCE);
    }
    step
}

/// The first births, when the world is built (step 0): put in, nothing moved.
pub fn first(st: &mut ParticlesState, world: &World) {
    note_walls(st, world);
    if st.swarm.as_ref().is_none_or(|s| s.buffers.is_none()) {
        return;
    }
    let anchor = world.anchor(st.id);
    let log = Log::default();
    let homes = st.home_frame(anchor.position, anchor.angle, world.camera.center);
    let cx =
        BirthCx { world, log: &log, anchor: anchor.position, angle: anchor.angle, homes, text: Default::default() };
    let born = births::plan(st, &cx, 0, true);
    let mut step = step_head(st, world);
    step.more[2] |= STEP_STILL;
    let births = packed(&born);
    if let Some(b) = st.swarm.as_mut().and_then(|s| s.buffers.as_mut()) {
        Arc::make_mut(b).step(&StepIn { step, words: &[], births: &births });
    }
}

/// One particle step of a swarm `st` in `world`, after world step `world.step` (whose log is
/// `log`), in the world's `air` when it is awake. As the exact engine's `step_in`, on the card.
pub fn step_in(st: &mut ParticlesState, world: &World, log: &Log, air: Option<&Air>) {
    let since = st.world_step;
    st.world_step = world.step;
    st.step += 1;
    let turn = st.settings.homes_turn.to_radians() * DT;
    if turn.is_finite() {
        st.home_turn = (st.home_turn + turn).rem_euclid(std::f32::consts::TAU);
    }
    let anchor = world.anchor(st.id);
    let homes = st.home_frame(anchor.position, anchor.angle, world.camera.center);
    let cx = BirthCx {
        world,
        log,
        anchor: anchor.position,
        angle: anchor.angle,
        homes: homes.clone(),
        text: Default::default(),
    };
    // With no card there are no particles: nothing to plan.
    if st.swarm.as_ref().is_none_or(|s| s.buffers.is_none()) {
        note_walls(st, world);
        return;
    }
    let born = births::plan(st, &cx, since, false);
    let meets: MeetsSettings = st.meets.clone();
    let mut step = step_head(st, world);
    let mut w = Words::default();
    let ready = Ready::new(st, &st.forces, world, log, homes);
    put_ready(&mut w, &ready, &mut step);
    let (n, forces) = forces_gpu(&st.forces, world.gravity, &ready, &mut w);
    step.more[0] = n;
    step.forces = forces;
    step.meets = [fin(meets.push), fin(meets.ball_bounce), fin(meets.bounciness), fin(meets.friction)];
    step.more[3] =
        at_balls_code(meets.at_balls) | at_shapes_code(meets.at_shapes) << 4 | at_edges_code(meets.at_edges) << 8;
    if meets.reads_balls() {
        let grid = BallGrid::new(balls_in(world), meets.reach.max(1.0));
        if put_balls(&mut w, &grid, &mut step) {
            step.more[2] |= STEP_BALLS;
        }
    }
    if meets.at_shapes != AtShapes::Pass {
        let reach = (step.time[2] * DT).clamp(0.0, 256.0);
        if put_walls(&mut w, &segments(world, &st.walls_before), reach, &mut step) {
            step.more[2] |= STEP_WALLS;
        }
    }
    if meets.at_edges != AtEdges::Pass {
        let (centre, half) = births::screen(world, meets.edge_margin);
        let (lo, hi) = (centre - half, centre + half);
        step.edges = [lo.x, lo.y, hi.x, hi.y];
        step.more[2] |= STEP_EDGES;
    }
    if let Some(air) = air.filter(|a| a.awake() && st.forces.follows_air()) {
        put_air(&mut w, air, &mut step);
        step.more[2] |= STEP_AIR;
    }
    let births = packed(&born);
    if let Some(b) = st.swarm.as_mut().and_then(|s| s.buffers.as_mut()) {
        Arc::make_mut(b).step(&StepIn { step, words: &w.words, births: &births });
    }
    note_walls(st, world);
}
