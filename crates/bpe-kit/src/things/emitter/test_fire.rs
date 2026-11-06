//! Through bounces: aiming by test-firing a lone copy of each ball in a stripped copy of the
//! world (the walls and the ball's own Motion and Bounce cards, nothing else:
//! [`bpe_runtime::World::lone`]), so the balls of a round reach the target together after
//! bouncing off whatever is in the way.
//!
//! A test flight *is* the world's own stepping, from the very moment the real ball sets off, so a
//! way found is the way the real ball goes as long as nothing stripped away acts on it. What can
//! still differ: other balls it would bump into, a Motion card varied across the balls (the lone
//! ball has the card's own values), substeps the scene's fastest ball would cause (above 3,840
//! px/s), a physics setting following sound (it hears no hits here).
//!
//! Leaving together (the usual way), a round is aimed in two passes over its balls:
//! 1. each ball's first ways, one per arc (how many bounces before it passes the target), from
//!    the fewest allowed up: Keep mine searches the speed along its own direction; Adjust each
//!    ball searches the turn off its direction at Speed;
//! 2. the arrival moment `T` is the latest of those first ways (or Arrive after), and each ball
//!    then finds how to be *at* the target at `T`: Keep mine by its speed alone (possible only
//!    where nothing bends the paths), Adjust each ball by its speed and turn together.
//!
//! Leaving when needed (part 4's way), each ball keeps its direction and a speed near Speed and
//! leaves when that way takes it there with the others.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bpe_core::ThingId;
use bpe_runtime::{Bodies, EventKind, Log, World};
use glam::{DVec2, Vec2};

use super::aim::{Aimer, MOST_STEPS, now};
use super::aim_cache::{What, cached};
use super::fire::{direction, start_offset};
use super::sim::{EmitterState, emitter_state, entry};
use super::{AimArrive, AimLeave, AimWay, EmitterSettings};
use super::{CatchFrom, Catches};

/// The most test flights the search for one ball's way may take.
pub const MOST_FLIGHTS: u32 = 40;
/// The most more flights checking that way again from when it sets off may take, when the scene
/// moves on meanwhile (a still scene takes one).
pub const SETTLE_FLIGHTS: u32 = 24;
/// How many speeds the first scan tries.
pub const SCAN: usize = 12;
/// How near the target (pixels) closing in tries to bring a pass.
pub const TOL: f64 = 0.25;
/// How near the target (pixels) a pass must go to count as through it, when the miss jumps
/// over [`TOL`] (see [`Search::close_in`]).
pub const GOOD: f64 = 0.45;
/// The most a ball's direction is turned to take it through (radians: 0.3°), unseen.
pub const MOST_TWEAK: f64 = 0.3 * std::f64::consts::PI / 180.0;
/// The most test flights finding one ball's way to be at the target at the round's moment may take.
pub const SOLVE_FLIGHTS: u32 = 60;
/// How near the target (pixels) a ball must be at the round's moment to count as there.
pub const REACH: f64 = 1.0;
/// How near the target (pixels) a first way found by turning must pass: the second pass makes it
/// exact.
pub const ROUGH: f64 = 3.0;
/// How many turns the first scan of Adjust each ball tries.
const TURN_SCAN: usize = 16;
/// How many first ways (one per arc) each ball looks for.
const WAYS: usize = 3;
/// How many places along each found way are kept for the preview.
pub const PATH_POINTS: usize = 96;
/// How many times the way is checked again from when it would set off, at most.
const SETTLE_TRIES: usize = 6;
/// How many steps without touching a wall end a test flight (ten seconds).
const QUIET_STEPS: u64 = 2400;
/// How many steps all of one ball's test flights in one pass may take together.
const BALL_STEPS: u64 = 400_000;
/// How many threads a round's balls are aimed on, at most.
const MOST_THREADS: usize = 8;

/// One ball of a round as test-firing aims it: the step it leaves, how, whether it gets there
/// with the others (not when it goes late or unaimed) and places along its way (empty unaimed).
#[derive(Clone, Debug, PartialEq)]
pub struct BallAim {
    pub leave: u64,
    pub velocity: Vec2,
    pub reaches: bool,
    pub path: Vec<Vec2>,
}

/// A round as test-firing aims it: fired on step `fire`, arriving at the moment `arrive` (in
/// steps) at `meet`, each ball's aim in ball order (`None`: skipped).
#[derive(Clone, Debug, PartialEq)]
pub struct RoundAim {
    pub round: u32,
    pub fire: u64,
    pub arrive: u64,
    pub meet: Vec2,
    pub balls: Vec<Option<BallAim>>,
}

impl RoundAim {
    /// How many of its balls don't get there with the others.
    pub fn cant_reach(&self) -> u32 {
        self.balls.iter().filter(|b| !b.as_ref().is_some_and(|b| b.reaches)).count() as u32
    }
}

/// Where a ball goes from and which way, at the moment it sets off.
#[derive(Clone, Copy, Debug)]
pub enum Shot {
    /// From the emitter's place then: `offset` from it and the way `way` (radians), both turned
    /// with it.
    Emitter { offset: Vec2, way: f32 },
    /// From `start` the way `u`, wherever the emitter is (a caught ball held where it was put).
    Fixed { start: Vec2, u: Vec2 },
}

impl Shot {
    fn at(&self, lone: &Lone) -> (Vec2, Vec2) {
        match *self {
            Shot::Emitter { offset, way } => {
                let pose = lone.world.poses.get(&lone.me).copied().unwrap_or_default();
                (pose.position + Vec2::from_angle(pose.angle).rotate(offset), Vec2::from_angle(way + pose.angle))
            }
            Shot::Fixed { start, u } => (start, u.normalize_or_zero()),
        }
    }
}

/// The stripped copy of the world, standing at some moment, that test flights start from: the
/// things without balls (walls, folders, the camera, the scene) and the emitter, quiet (it fires
/// and catches nothing) and with no balls.
#[derive(Clone)]
pub struct Lone {
    world: World,
    me: ThingId,
    radius: f32,
    /// The catch circles that would take the ball out of the world: (on the target, radius).
    /// On the target, the arc it is caught on is the last it has; on the emitter itself, the
    /// flight ends there.
    catchers: Vec<(bool, f32)>,
}

impl Lone {
    /// A lone copy of `world` for firing a ball of radius `radius` from emitter `me` with
    /// `owner`'s Motion and Bounce cards. `world` must stand at the moment `moment`: in the Spawn
    /// stage of step `moment + 1`, or after step `moment`.
    pub fn new(world: &World, me: ThingId, owner: ThingId, radius: f32, moment: u64) -> Lone {
        let mut lone = world.lone(&|id| id == me || !world.bodies.contains_key(&id));
        lone.step = moment;
        if owner != me {
            lone.lend_cards(world, owner, me);
        }
        lone.bodies.insert(me, Bodies::default());
        let mut catchers = Vec::new();
        if let Some(st) = emitter_state(world, me) {
            lone.states.insert(me, Box::new(EmitterState::quiet(st.settings.clone())));
            let target = ThingId(st.settings.aim_target.max(0) as u32);
            if st.settings.aim_target > 0
                && target != me
                && let Some(r) = catch_radius(world, target, owner)
            {
                catchers.push((true, r));
            }
            if let Some(r) = catch_radius(world, me, owner) {
                catchers.push((false, r));
            }
        }
        Lone { world: lone, me, radius, catchers }
    }

    /// The moment it stands at.
    pub fn moment(&self) -> u64 {
        self.world.step
    }

    /// Steps it on, with no ball, until it stands at `moment`.
    pub fn advance(&mut self, moment: u64) {
        let sims = crate::lone_sims();
        let mut log = Log::default();
        while self.world.step < moment {
            self.world.step(sims, &mut log);
        }
    }
}

/// How big `catcher`'s catch circle is for balls of `owner`, when it takes them out of the world
/// (Remove or Send back out; Pass through lets them fly on).
fn catch_radius(world: &World, catcher: ThingId, owner: ThingId) -> Option<f32> {
    let s = &emitter_state(world, catcher)?.settings;
    let takes = match s.catch_from {
        CatchFrom::AnyBalls => true,
        CatchFrom::ItsOwn => owner == catcher,
        CatchFrom::Chosen => world.names(&s.catch_things, catcher, owner),
    };
    (takes && matches!(s.catches, Catches::Remove | Catches::SendBackOut)).then_some(s.catch_size.max(0.0))
}

/// A ball going by the target: how far off its way the target was (left positive) and when, in
/// steps after it set off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pass {
    pub miss: f64,
    pub e: f64,
}

/// The passes along a polyline of places `rel` relative to the target, one a step from the
/// moment the ball set off, with `hits[j]` the wall hits in the step that ended at place `j`:
/// for each arc (hits before), the pass whose miss is smallest. A step with a hit in it isn't a
/// pass.
pub fn passes(rel: &[Vec2], hits: &[u32]) -> Vec<Option<Pass>> {
    let mut out: Vec<Option<Pass>> = vec![None];
    let mut arc = 0usize;
    for j in 1..rel.len() {
        let h = hits.get(j).copied().unwrap_or(0);
        if h > 0 {
            arc += h as usize;
            if out.len() <= arc {
                out.resize(arc + 1, None);
            }
            continue;
        }
        let (r0, r1) = (rel[j - 1].as_dvec2(), rel[j].as_dvec2());
        let d = r1 - r0;
        let dd = d.length_squared();
        if dd <= 1e-18 {
            continue;
        }
        let t = -r0.dot(d) / dd;
        if !(0.0..1.0).contains(&t) {
            continue;
        }
        // The target is at the origin: how far it is to the left of the way.
        let miss = (d.x * -r0.y - d.y * -r0.x) / dd.sqrt();
        let pass = Pass { miss, e: (j - 1) as f64 + t };
        if out[arc].is_none_or(|p| p.miss.abs() > miss.abs()) {
            out[arc] = Some(pass);
        }
    }
    out
}

/// One test flight: its passes per arc, the places it went through (a step apart from where it
/// set off), each relative to the target then, the arc it was on at each, and the arc the target
/// caught it on (if it catches).
struct Flight {
    passes: Vec<Option<Pass>>,
    places: Vec<Vec2>,
    rel: Vec<Vec2>,
    arcs: Vec<usize>,
    caught_on: Option<usize>,
}

impl Flight {
    fn on(&self, arc: usize) -> Option<Pass> {
        self.passes.get(arc).copied().flatten()
    }

    /// Where it is against the target `t` steps after setting off (its place less the target's,
    /// between the steps either side), the arc it is on then and which way it goes; `None` when
    /// the flight ended before, or the target had caught it on an earlier arc.
    fn at(&self, t: f64) -> Option<(DVec2, usize, DVec2)> {
        let f = t.floor().max(0.0) as usize;
        let c = f + 1;
        if c >= self.rel.len() {
            return None;
        }
        let x = (t - f as f64).clamp(0.0, 1.0);
        let off = self.rel[f].as_dvec2().lerp(self.rel[c].as_dvec2(), x);
        let arc = self.arcs[c];
        if self.caught_on.is_some_and(|a| a < arc) {
            return None;
        }
        let way = (self.places[c] - self.places[f]).as_dvec2().normalize_or_zero();
        Some((off, arc, way))
    }
}

/// Fires the lone ball from `p0` at `v` in a copy of `lone`, for at most `most` steps or until
/// it has bounced more than `most_arc` times.
fn fly(lone: &Lone, aim: &Aimer, p0: Vec2, v: Vec2, most: u64, most_arc: usize) -> Flight {
    let sims = crate::lone_sims();
    let mut w = lone.world.clone();
    let setoff = w.step;
    if let Some(b) = w.bodies.get_mut(&lone.me) {
        b.push(p0, v, lone.radius);
    }
    let mut log = Log::default();
    let mut places = vec![p0];
    let mut rel = vec![p0 - aim.target_at(setoff as f64)];
    let mut hits = vec![0u32];
    let mut arcs = vec![0usize];
    let mut still = 0u32;
    let mut quiet = 0u64;
    let mut last_arc = None;
    let mut arc = 0usize;
    for _ in 0..most {
        let before = log.len();
        w.step(sims, &mut log);
        let Some(b) = w.bodies.get(&lone.me) else { break };
        if !b.alive.first().copied().unwrap_or(false) {
            break;
        }
        let p = b.position[0];
        let h = log.events()[before..]
            .iter()
            .filter(|e| e.kind == EventKind::Hit && e.a.thing == lone.me && e.a.item == 0)
            .count();
        arc += h;
        if arc > most_arc {
            break;
        }
        // At rest a second, or ten seconds without touching a wall (it has left them all, or
        // slides to and fro in a bowl): it goes nowhere new.
        still = if b.velocity[0].length_squared() < 1.0 { still + 1 } else { 0 };
        quiet = if h > 0 { 0 } else { quiet + 1 };
        if still > 240 || quiet > QUIET_STEPS {
            break;
        }
        let previous = *places.last().unwrap_or(&p0);
        let target = aim.target_at(w.step as f64);
        places.push(p);
        rel.push(p - target);
        hits.push(h as u32);
        arcs.push(arc);
        // Caught on the way: by the target, the arc it comes in on is its last; by the emitter
        // itself, it goes no further.
        let own = w.poses.get(&lone.me).map_or(p0, |pose| pose.position);
        let caught = |on_target: bool, r: f32| entry(previous, p, if on_target { target } else { own }, r).is_some();
        if lone.catchers.iter().any(|&(on_target, r)| !on_target && caught(false, r)) {
            break;
        }
        if last_arc.is_none() && lone.catchers.iter().any(|&(on_target, r)| on_target && caught(true, r)) {
            last_arc = Some(arc);
        }
    }
    let mut passes = passes(&rel, &hits);
    passes.truncate(most_arc + 1);
    if let Some(last) = last_arc {
        passes.truncate(last + 1);
    }
    Flight { passes, places, rel, arcs, caught_on: last_arc }
}

/// A way through the target: the speed, the small turn off its own direction (radians), the arc
/// it passes on, when (steps after setting off) and the places it goes through up to there.
#[derive(Clone, Debug)]
struct Found {
    speed: f64,
    tweak: f64,
    arc: usize,
    e: f64,
    places: Vec<Vec2>,
    /// How fast the miss changes with speed there (pixels per px/s), when known.
    slope: f64,
}

impl Found {
    fn of(speed: f64, tweak: f64, arc: usize, pass: Pass, flight: &Flight) -> Found {
        // A few places past the target too, to see where it is a moment later.
        let upto = (pass.e.ceil() as usize + 3).min(flight.places.len());
        let places = flight.places[..upto].to_vec();
        Found { speed, tweak, arc, e: pass.e, places, slope: f64::NAN }
    }

    /// Its velocity leaving the way `u`.
    fn velocity(&self, u: Vec2) -> Vec2 {
        Vec2::from_angle(self.tweak as f32).rotate(u) * self.speed as f32
    }
}

/// What a bracket closes in on: the speed (turned by the search's own tweak), or the small turn
/// off its direction at a speed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Knob {
    Speed,
    Tweak(f64),
}

/// How the search goes: the fewest bounces allowed, the speed it would rather have, how near
/// (pixels) both ends of a bracket must pass for it to be tried, the turn its speeds fly at, the
/// flights it has left.
#[derive(Clone, Copy)]
struct Search<'a> {
    lone: &'a Lone,
    aim: &'a Aimer,
    p0: Vec2,
    u: Vec2,
    least_arc: usize,
    most_arc: usize,
    prefer: f64,
    near: f64,
    tweak: f64,
    /// How near a pass must go to count as a way.
    accept: f64,
    left: u32,
    /// The steps its flights may still take together.
    steps: u64,
}

/// A way to try: a flight already through, or a bracket to close in on (flight index, arc).
enum Try {
    Through(usize, usize),
    Bracket(usize, usize),
}

impl Search<'_> {
    fn fly_at(&mut self, speed: f64, tweak: f64, most: u64) -> Option<Flight> {
        // A background run told to stop gives up: its answer is never kept.
        if self.left == 0 || self.steps == 0 || super::aim_cache::stopped() {
            self.left = 0;
            return None;
        }
        self.left -= 1;
        let v = Vec2::from_angle(tweak as f32).rotate(self.u) * speed as f32;
        let flight = fly(self.lone, self.aim, self.p0, v, most.min(self.steps), self.most_arc);
        self.steps = self.steps.saturating_sub(flight.places.len() as u64);
        Some(flight)
    }

    fn fly(&mut self, speed: f64, most: u64) -> Option<Flight> {
        self.fly_at(speed, self.tweak, most)
    }

    /// Closes in on a way between `(a, fa)` and `(b, fb)` (knob settings whose misses on `arc`
    /// have opposite signs) by the Illinois method. The world moves in whole steps, so the miss
    /// jumps a little wherever a bounce moves to the next step; if the way lies in such a jump,
    /// the nearest flight does when it is within [`GOOD`], else (closing in on speed) a small
    /// turn off its direction is closed in on at that speed. `None` when the arc breaks in
    /// between or the flights run out.
    fn close_in(
        &mut self,
        arc: usize,
        knob: Knob,
        (mut a, mut fa): (f64, f64),
        (mut b, mut fb): (f64, f64),
        most: u64,
    ) -> Option<Found> {
        let settings = |x: f64, tweak: f64| match knob {
            Knob::Speed => (x, tweak),
            Knob::Tweak(speed) => (speed, x),
        };
        let own_tweak = self.tweak;
        let mut side = 0i8;
        // The last miss really flown (the ends' are halved as Illinois goes), for the slope; and
        // the nearest flown.
        let mut last = (b, fb);
        let mut nearest: Option<(f64, Pass, Flight)> = None;
        let stalled = |lo: f64, hi: f64| match knob {
            Knob::Speed => hi - lo <= hi.abs() * 1e-6,
            Knob::Tweak(_) => hi - lo <= 1e-7,
        };
        while self.left > 0 {
            let (lo, hi) = (a.min(b), a.max(b));
            if stalled(lo, hi) {
                break;
            }
            let mut c = (a * fb - b * fa) / (fb - fa);
            if !c.is_finite() || c <= lo || c >= hi {
                c = 0.5 * (a + b);
            }
            let (speed, tweak) = settings(c, own_tweak);
            let flight = self.fly_at(speed, tweak, most)?;
            let pass = flight.on(arc)?;
            if pass.miss.abs() <= TOL {
                let slope = (pass.miss - last.1) / (c - last.0);
                let slope = if knob == Knob::Speed { slope } else { f64::NAN };
                return Some(Found { slope, ..Found::of(speed, tweak, arc, pass, &flight) });
            }
            last = (c, pass.miss);
            if nearest.as_ref().is_none_or(|n| n.1.miss.abs() > pass.miss.abs()) {
                nearest = Some((c, pass, flight));
            }
            if pass.miss.signum() == fa.signum() {
                a = c;
                fa = pass.miss;
                if side == 1 {
                    fb *= 0.5;
                }
                side = 1;
            } else {
                b = c;
                fb = pass.miss;
                if side == -1 {
                    fa *= 0.5;
                }
                side = -1;
            }
        }
        let (c, pass, flight) = nearest?;
        let (speed, tweak) = settings(c, own_tweak);
        if pass.miss.abs() <= self.accept {
            return Some(Found::of(speed, tweak, arc, pass, &flight));
        }
        match knob {
            Knob::Speed => self.turn(arc, speed, pass.miss, most),
            Knob::Tweak(_) => None,
        }
    }

    /// At `speed`, where the miss on `arc` is `miss`, a small turn off the direction (at most
    /// [`MOST_TWEAK`]) that takes it through.
    fn turn(&mut self, arc: usize, speed: f64, miss: f64, most: u64) -> Option<Found> {
        let own = self.tweak;
        for t in [MOST_TWEAK / 16.0, MOST_TWEAK / 4.0, MOST_TWEAK] {
            for tweak in [own + t, own - t] {
                let flight = self.fly_at(speed, tweak, most)?;
                let Some(pass) = flight.on(arc) else { continue };
                if pass.miss.abs() <= TOL {
                    return Some(Found::of(speed, tweak, arc, pass, &flight));
                }
                if pass.miss.signum() != miss.signum() {
                    return self.close_in(arc, Knob::Tweak(speed), (own, miss), (tweak, pass.miss), most);
                }
            }
        }
        None
    }

    /// The best way among the flights at `speeds` (flown here): see [`Search::ways`].
    fn best(&mut self, speeds: &[f64], most: u64) -> Option<Found> {
        self.ways(speeds, Knob::Speed, most, 1).into_iter().next()
    }

    /// The first ways among the flights at `values` of `knob` (flown here), at most `want` of
    /// them, one per arc from the fewest bounces allowed up, the best on each (nearest the speed
    /// it would rather have, or the smallest turn): a flight already through, else a bracket
    /// closed in on. With none yet, the values between the two flights that came nearest are
    /// tried, until the flights run out.
    fn ways(&mut self, values: &[f64], knob: Knob, most: u64, want: usize) -> Vec<Found> {
        let own_tweak = self.tweak;
        let at = |x: f64| match knob {
            Knob::Speed => (x, own_tweak),
            Knob::Tweak(speed) => (speed, x),
        };
        let prefer = self.prefer;
        let off = |x: f64| match knob {
            Knob::Speed => (x.max(1e-9) / prefer.max(1e-9)).ln().abs(),
            Knob::Tweak(_) => x.abs(),
        };
        let mut flown: Vec<(f64, Flight)> = Vec::new();
        for &x in values {
            let (speed, _) = at(x);
            if speed > 0.0 && x.is_finite() && !flown.iter().any(|(f, _)| *f == x) {
                let (speed, tweak) = at(x);
                let Some(flight) = self.fly_at(speed, tweak, most) else { break };
                flown.push((x, flight));
            }
        }
        flown.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out: Vec<Found> = Vec::new();
        // Brackets that broke (a wall changed between their ends), by their ends and arc.
        let mut broken: Vec<(f64, f64, usize)> = Vec::new();
        loop {
            // Ways already through, then brackets, best first: fewest bounces, then nearest.
            let mut tries: Vec<(usize, f64, Try)> = Vec::new();
            for (i, (x, f)) in flown.iter().enumerate() {
                for arc in self.least_arc..f.passes.len() {
                    if f.on(arc).is_some_and(|p| p.miss.abs() <= self.accept) {
                        tries.push((arc, off(*x), Try::Through(i, arc)));
                    }
                }
            }
            for i in 1..flown.len() {
                let (lo, hi) = (&flown[i - 1], &flown[i]);
                let arcs = lo.1.passes.len().min(hi.1.passes.len());
                for arc in self.least_arc..arcs {
                    if let (Some(p), Some(q)) = (lo.1.on(arc), hi.1.on(arc))
                        && p.miss.signum() != q.miss.signum()
                        && p.miss.abs() > self.accept
                        && q.miss.abs() > self.accept
                        && p.miss.abs().max(q.miss.abs()) <= self.near
                        && !broken.contains(&(lo.0, hi.0, arc))
                    {
                        tries.push((arc, off(0.5 * (lo.0 + hi.0)), Try::Bracket(i - 1, arc)));
                    }
                }
            }
            tries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
            for (arc, _, t) in tries {
                if out.len() >= want || out.iter().any(|f| f.arc == arc) {
                    continue;
                }
                match t {
                    Try::Through(i, arc) => {
                        let (x, f) = &flown[i];
                        let Some(pass) = f.on(arc) else { continue };
                        let (speed, tweak) = at(*x);
                        out.push(Found::of(speed, tweak, arc, pass, f));
                    }
                    Try::Bracket(i, arc) => {
                        let (Some(p), Some(q)) = (flown[i].1.on(arc), flown[i + 1].1.on(arc)) else { continue };
                        let (a, b) = ((flown[i].0, p.miss), (flown[i + 1].0, q.miss));
                        match self.close_in(arc, knob, a, b, most) {
                            Some(found) => out.push(found),
                            None => broken.push((a.0, b.0, arc)),
                        }
                    }
                }
            }
            if !out.is_empty() || self.left == 0 {
                out.sort_by_key(|f| f.arc);
                return out;
            }
            // Nothing yet: try between the two neighbouring flights that came nearest the target
            // (on an arc that counts), else across the widest gap.
            let nearest = |f: &Flight| -> f64 {
                (self.least_arc..f.passes.len())
                    .filter_map(|arc| f.on(arc))
                    .map(|p| p.miss.abs())
                    .fold(f64::INFINITY, f64::min)
            };
            let mut pick: Option<(f64, usize)> = None;
            for i in 1..flown.len() {
                let (a, b) = (flown[i - 1].0, flown[i].0);
                let wide = match knob {
                    Knob::Speed if a > 0.0 => (b / a).ln(),
                    _ => b - a,
                };
                if wide <= 1e-6 {
                    continue;
                }
                let score = nearest(&flown[i - 1].1).min(nearest(&flown[i].1));
                let score = if score.is_finite() { score } else { 1e12 / wide };
                if pick.is_none_or(|(best, _)| score < best) {
                    pick = Some((score, i));
                }
            }
            let Some((_, i)) = pick else { return out };
            let (a, b) = (flown[i - 1].0, flown[i].0);
            let mid = match knob {
                Knob::Speed => (a * b).sqrt(),
                Knob::Tweak(_) => 0.5 * (a + b),
            };
            let (speed, tweak) = at(mid);
            let Some(flight) = self.fly_at(speed, tweak, most) else { return out };
            flown.insert(i, (mid, flight));
        }
    }

    /// Flies at `speed` turned `tweak` and gives where it is against the target `t` steps after
    /// setting off, when it is on an arc that counts then (outer `None`: no flights left).
    #[allow(clippy::type_complexity)]
    fn at_moment(&mut self, speed: f64, tweak: f64, t: f64) -> Option<Option<(DVec2, DVec2, Flight)>> {
        let flight = self.fly_at(speed, tweak, t.ceil() as u64 + 2)?;
        Some(
            flight
                .at(t)
                .filter(|(_, arc, _)| (self.least_arc..=self.most_arc).contains(arc))
                .map(|(off, _, way)| (off, way, flight)),
        )
    }

    /// The way found at `speed` turned `tweak`, at the target `t` steps after setting off.
    fn found_at(speed: f64, tweak: f64, t: f64, flight: &Flight) -> Found {
        let upto = (t.ceil() as usize + 1).min(flight.places.len());
        let arc = flight.arcs.get(upto.saturating_sub(1)).copied().unwrap_or(0);
        Found { speed, tweak, arc, e: t, places: flight.places[..upto].to_vec(), slope: f64::NAN }
    }

    /// Keeping its direction (and `way`'s small turn), the speed that has it at the target `t`
    /// steps after setting off, starting from `way` slowed or sped to get there then: the part
    /// of the offset along its way is brought to nothing; it counts when the whole offset is
    /// within [`REACH`], or (the nearest it got, with how near) within [`ROUGH`].
    fn solve_keep(&mut self, way: &Found, t: f64, ceiling: f64) -> Option<(f64, Found)> {
        let tweak = way.tweak;
        let dt = self.aim.dt as f64;
        let mut speed = (way.speed * way.e / t).min(ceiling);
        let mut seen: Vec<(f64, f64)> = Vec::new();
        let mut best: Option<(f64, Found)> = None;
        for _ in 0..12 {
            let Some(got) = self.at_moment(speed, tweak, t) else { break };
            let Some((off, dir, flight)) = got else { break };
            let miss = off.length();
            if best.as_ref().is_none_or(|(m, _)| miss < *m) {
                best = Some((miss, Self::found_at(speed, tweak, t, &flight)));
            }
            if miss <= GOOD {
                break;
            }
            let along = off.dot(dir);
            seen.push((speed, along));
            // Ahead of the target (along > 0): too fast. A secant once two speeds are known.
            let next = match seen.as_slice() {
                [.., (s1, g1), (s2, g2)] if (g2 - g1).abs() > 1e-9 => s2 - g2 * (s2 - s1) / (g2 - g1),
                _ => speed - along / (t * dt),
            };
            let next = next.clamp(speed * 0.5, (speed * 2.0).min(ceiling));
            if !next.is_finite() || next <= 0.0 || (next - speed).abs() <= speed * 1e-7 {
                break;
            }
            speed = next;
        }
        best.filter(|(m, _)| *m <= ROUGH)
    }

    /// Turning at most `most_turn` off its direction, a way through the target on `way`'s arc
    /// that passes it `t` steps after setting off (within half a step): along the ways through
    /// it on that arc (at each speed, the turn that takes it through, [`Search::through`]), a
    /// secant on the speed until it passes then. The way, with how far off the target it passes,
    /// when within [`ROUGH`].
    fn solve_turn(&mut self, (way, own): (&Found, bool), t: f64, most_turn: f64, ceiling: f64) -> Option<(f64, Found)> {
        let most = ((t * 1.6) as u64 + 8).min(MOST_STEPS as u64);
        let mut rough: Option<(f64, Found)> = None;
        // Slowed or sped as if its way stayed the same first, then the other way: on a curved
        // path, slower can be sooner.
        let plain = (way.e / t).clamp(1.0 / 1.15, 1.15);
        let plain = if (plain - 1.0).abs() < 0.02 { 1.02f64.copysign(plain - 1.0) } else { plain };
        for first in [plain, 1.0 / plain] {
            // Another ball's way (from elsewhere) is only a first guess for this one.
            let mut tried: Vec<(f64, f64)> = if own { vec![(way.speed, way.e - t)] } else { Vec::new() };
            let mut tweak = way.tweak;
            let mut turns: Vec<(f64, f64)> = if own { vec![(way.speed, way.tweak)] } else { Vec::new() };
            let mut good = way.speed;
            let mut stalls = 0;
            let mut speed = (way.speed * first).clamp(1.0, ceiling);
            for _ in 0..7 {
                match self.through(speed, tweak, way.arc, most_turn, most) {
                    Some(found) => {
                        let late = found.e - t;
                        // Where it is then counts, not when it passes: going by at a slant, a
                        // hair's change of speed moves when it passes a lot.
                        if let Some(off) = self.off_at(&found, t) {
                            if off <= REACH {
                                return Some((off, found));
                            }
                            if off <= ROUGH && rough.as_ref().is_none_or(|(r, _)| off < *r) {
                                rough = Some((off, found.clone()));
                            }
                        }
                        // Once some speed gets there early and another late, it closes in between
                        // the nearest two; until then, ways through on one arc may all take about
                        // as long (a fold): it stops once two tries in a row hardly bring it nearer.
                        let was = tried.last().map_or(f64::INFINITY, |p| p.1.abs());
                        good = speed;
                        tried.push((speed, late));
                        turns.push((speed, found.tweak));
                        let bracket = tried
                            .iter()
                            .filter(|p| p.1.signum() != late.signum())
                            .min_by(|a, b| (a.0 - speed).abs().total_cmp(&(b.0 - speed).abs()))
                            .copied();
                        let next = if let Some((s1, g1)) = bracket {
                            let c = speed - late * (speed - s1) / (late - g1);
                            // Halfway when the secant would hug one end.
                            let (lo, hi) = (s1.min(speed), s1.max(speed));
                            if c > lo + 0.1 * (hi - lo) && c < hi - 0.1 * (hi - lo) { c } else { 0.5 * (lo + hi) }
                        } else {
                            stalls = if late.abs() > 0.8 * was { stalls + 1 } else { 0 };
                            if stalls >= 2 {
                                break;
                            }
                            let [.., (s1, g1), (s2, g2)] = tried.as_slice() else {
                                speed *= first;
                                tweak = found.tweak;
                                continue;
                            };
                            let next =
                                if (g2 - g1).abs() > 1e-9 { s2 - g2 * (s2 - s1) / (g2 - g1) } else { s2 * first };
                            // Small steps keep it on the same run of ways.
                            next.clamp(speed / 1.15, (speed * 1.15).min(ceiling))
                        };
                        if !next.is_finite() || (next - speed).abs() <= speed * 1e-6 {
                            break;
                        }
                        speed = next.max(1.0);
                        // The turn there, from the two found nearest that speed.
                        let mut near = turns.clone();
                        near.sort_by(|a, b| (a.0 - speed).abs().total_cmp(&(b.0 - speed).abs()));
                        tweak = match near.as_slice() {
                            [(s1, t1), (s2, t2), ..] if (s2 - s1).abs() > 1e-9 => {
                                t1 + (t2 - t1) * (speed - s1) / (s2 - s1)
                            }
                            [(_, t1), ..] => *t1,
                            [] => tweak,
                        }
                        .clamp(-most_turn, most_turn);
                    }
                    // No way through there: halfway back to the last speed that had one.
                    None if (speed - good).abs() > good * 1e-3 => speed = 0.5 * (speed + good),
                    None => break,
                }
                if self.left == 0 {
                    return rough;
                }
            }
        }
        rough
    }

    fn off_at(&self, found: &Found, t: f64) -> Option<f64> {
        off_at(self.aim, self.lone.moment(), found, t)
    }

    /// At `speed`, the turn near `tweak` (within `most_turn`) that takes it through the target
    /// on `arc`: secant steps on the turn until the miss changes side, then closed in on.
    fn through(&mut self, speed: f64, tweak: f64, arc: usize, most_turn: f64, most: u64) -> Option<Found> {
        let flight = self.fly_at(speed, tweak, most)?;
        let pass = flight.on(arc)?;
        if pass.miss.abs() <= TOL {
            return Some(Found::of(speed, tweak, arc, pass, &flight));
        }
        let mut seen = vec![(tweak, pass.miss)];
        let mut next = (tweak + 0.003 * -pass.miss.signum()).clamp(-most_turn, most_turn);
        for _ in 0..7 {
            if seen.iter().any(|(x, _)| (x - next).abs() < 1e-9) {
                return None;
            }
            let flight = self.fly_at(speed, next, most)?;
            let pass = flight.on(arc)?;
            if pass.miss.abs() <= TOL {
                return Some(Found::of(speed, next, arc, pass, &flight));
            }
            if let Some(&other) = seen
                .iter()
                .filter(|(_, m)| m.signum() != pass.miss.signum())
                .min_by(|a, b| (a.0 - next).abs().total_cmp(&(b.0 - next).abs()))
            {
                return self.close_in(arc, Knob::Tweak(speed), other, (next, pass.miss), most);
            }
            let &(x0, m0) = seen.last()?;
            seen.push((next, pass.miss));
            let step = if (pass.miss - m0).abs() > 1e-12 { -pass.miss * (next - x0) / (pass.miss - m0) } else { 0.0 };
            // Small steps: a turn too far may change which walls it meets.
            let step = if step.is_finite() && step != 0.0 { step.clamp(-0.1, 0.1) } else { 0.01 * -pass.miss.signum() };
            next = (next + step).clamp(-most_turn, most_turn);
        }
        None
    }

    /// A way near `speed` (the scene has moved on since it was found on `arc`, where the miss
    /// changed with speed at `slope`): that speed again (through on any arc that counts), else
    /// secant steps along `arc` until the miss changes side and a bracket closes in, else a fresh
    /// search round it.
    fn near(&mut self, arc: usize, speed: f64, slope: f64, most: u64) -> Option<Found> {
        let flight = self.fly(speed, most)?;
        let through = (self.least_arc..flight.passes.len())
            .find_map(|a| flight.on(a).filter(|p| p.miss.abs() <= GOOD).map(|p| (a, p)));
        if let Some((a, pass)) = through {
            return Some(Found::of(speed, self.tweak, a, pass, &flight));
        }
        if let Some(pass) = flight.on(arc) {
            let mut seen = vec![(speed, pass.miss)];
            let mut slope = slope;
            if !slope.is_finite() || slope == 0.0 {
                let other = speed * 1.01;
                if let Some(q) = self.fly(other, most)?.on(arc) {
                    slope = (q.miss - pass.miss) / (other - speed);
                    seen.push((other, q.miss));
                }
            }
            for _ in 0..8 {
                let &(s0, m0) = seen.last()?;
                if !slope.is_finite() || slope == 0.0 {
                    break;
                }
                // Small steps: the way may change side of a wall not far off.
                let next = (s0 - m0 / slope).clamp(s0 * 0.94, s0 * 1.06);
                let Some(q) = self.fly(next, most)?.on(arc) else { break };
                if q.miss.abs() <= GOOD {
                    // Fly it again for its places: cheaper than keeping every flight.
                    let flight = self.fly(next, most)?;
                    let pass = flight.on(arc)?;
                    return Some(Found::of(next, self.tweak, arc, pass, &flight));
                }
                if let Some(&other) = seen
                    .iter()
                    .filter(|(_, m)| m.signum() != q.miss.signum())
                    .min_by(|a, b| (a.0 - next).abs().total_cmp(&(b.0 - next).abs()))
                {
                    return self.close_in(arc, Knob::Speed, other, (next, q.miss), most);
                }
                slope = (q.miss - m0) / (next - s0);
                seen.push((next, q.miss));
            }
        }
        // The way has changed too much: search afresh, the speed it had preferred.
        let prefer = std::mem::replace(&mut self.prefer, speed);
        let speeds: Vec<f64> = [0.6, 0.75, 0.9, 1.1, 1.3, 1.6].iter().map(|r| speed * r).collect();
        let found = self.best(&speeds, most);
        self.prefer = prefer;
        found
    }
}

/// How far from the target the way `found` (setting off at the moment `setoff`) is `t` steps
/// after setting off (between the steps either side), when it has got that far.
fn off_at(aim: &Aimer, setoff: u64, found: &Found, t: f64) -> Option<f64> {
    let f = t.floor().max(0.0) as usize;
    let (a, b) = (found.places.get(f)?, found.places.get(f + 1)?);
    let place = a.as_dvec2().lerp(b.as_dvec2(), t - f as f64);
    Some((place - aim.target_at(setoff as f64 + t).as_dvec2()).length())
}

/// A search for shot `shot` from the lone copy `base`, `reach` pixels from the target.
fn search_for<'a>(base: &'a Lone, aim: &'a Aimer, s: &EmitterSettings, shot: Shot, reach: f64) -> Search<'a> {
    let (p0, u) = shot.at(base);
    let least_arc = s.aim_bounces as usize;
    Search {
        lone: base,
        aim,
        p0,
        u,
        least_arc,
        most_arc: (s.aim_bounces_most as usize).max(least_arc),
        prefer: s.speed.max(1.0) as f64,
        near: (0.3 * reach).max(40.0),
        tweak: 0.0,
        accept: GOOD,
        left: MOST_FLIGHTS,
        steps: BALL_STEPS,
    }
}

/// The fastest a ball may go: a hair under the world's own limit, and arriving as soon as all
/// can, no faster than Speed (the round's moment is when the last can be there at Speed).
fn ceiling_of(aim: &Aimer, s: &EmitterSettings) -> f64 {
    let world = aim.ceiling as f64 * 0.999;
    if s.aim_arrive == AimArrive::AsSoonAsAllCan { world.min(s.speed.max(1.0) as f64) } else { world }
}

/// Shot `shot`'s first ways from `base` (standing at the moment the round's balls set off), one
/// per arc from the fewest bounces allowed up, flying at most `horizon` steps (`limit`: the
/// steps the round has to get there in, when it has a time to arrive by). `adjust`: at Speed,
/// turning; else along its own direction, by speed.
fn first_ways(
    base: &Lone,
    aim: &Aimer,
    s: &EmitterSettings,
    shot: Shot,
    horizon: u64,
    limit: Option<f64>,
    adjust: bool,
) -> Vec<Found> {
    let (p0, u) = shot.at(base);
    if u == Vec2::ZERO || horizon == 0 {
        return Vec::new();
    }
    let setoff = base.moment() as f64;
    let reach = (aim.target_at(setoff + limit.unwrap_or(0.0)) - p0).length().max(1.0) as f64;
    let mut search = search_for(base, aim, s, shot, reach);
    // No more arcs are looked for than `WAYS`: flights stop past them.
    search.most_arc = search.most_arc.min(search.least_arc + WAYS - 1);
    let ceiling = ceiling_of(aim, s);
    if adjust {
        let speed = search.prefer.min(ceiling);
        let most_turn = (s.aim_turn.max(0.0) as f64).to_radians();
        search.accept = ROUGH;
        let mut turns = vec![0.0];
        if most_turn > 0.0 {
            turns.extend((0..TURN_SCAN).map(|i| -most_turn + 2.0 * most_turn * i as f64 / (TURN_SCAN - 1) as f64));
        }
        return search.ways(&turns, Knob::Tweak(speed), horizon, WAYS);
    }
    let limited = limit.is_some();
    let quickest = reach / (limit.unwrap_or(horizon as f64).max(1.0) * aim.dt as f64);
    let prefer = if limited && s.speed <= 0.0 { quickest } else { search.prefer };
    search.prefer = prefer;
    // Speed first, then the scan: with a time limit, round what gets there in time; without,
    // round Speed.
    let (lo, hi) = if limited {
        (0.5 * quickest.min(prefer), (4.0 * quickest.max(prefer)).min(ceiling))
    } else {
        (prefer / 8.0, (prefer * 8.0).min(ceiling))
    };
    let mut speeds = vec![prefer.min(ceiling)];
    if hi > lo {
        let ratio = (hi / lo).powf(1.0 / (SCAN - 1) as f64);
        speeds.extend((0..SCAN).map(|i| lo * ratio.powi(i as i32)));
    }
    search.ways(&speeds, Knob::Speed, horizon, WAYS)
}

/// Ball `k` of a round (`shots`, each with its first ways `firsts`) from `base` at the target
/// `t` steps after setting off, all of the round leaving together. Keeping its
/// direction: one of its own first ways slowed or sped to be there then. Adjusting it: a way
/// another ball of the round found that is there then and needs a turn within Turn at most
/// (the smallest), else the speed and turn worked out from the ways nearest `t` (its own and,
/// turned to its direction, the others'). `None`: it can't.
fn meet(
    base: &Lone,
    aim: &Aimer,
    s: &EmitterSettings,
    (shots, firsts): (&[Shot], &[Vec<Found>]),
    k: usize,
    t: f64,
) -> Option<(f64, Found)> {
    let (p0, u) = shots[k].at(base);
    let reach = (aim.target_at(base.moment() as f64 + t) - p0).length().max(1.0) as f64;
    let mut search = search_for(base, aim, s, shots[k], reach);
    search.left = SOLVE_FLIGHTS;
    let ceiling = ceiling_of(aim, s);
    // How far from the target a way of its own is then.
    let off = |f: &Found| off_at(aim, base.moment(), f, t).unwrap_or(f64::INFINITY);
    // The nearest way found so far that is within [`ROUGH`] but not [`REACH`]: it goes if none
    // comes nearer (where the paths bunch up after a curved wall, some places can't be reached
    // closer).
    let mut rough: Option<(f64, Found)> = None;
    let mut keep_rough = |m: f64, f: Found| {
        if m <= ROUGH && rough.as_ref().is_none_or(|(r, _)| m < *r) {
            rough = Some((m, f));
        }
    };
    if s.aim_way == AimWay::KeepMine {
        let mut own: Vec<&Found> = firsts[k].iter().collect();
        // Those already there before `t` slow down, nearest first; then the rest.
        own.sort_by(|a, b| {
            let key = |f: &Found| if f.e <= t + 0.5 { (0, t - f.e) } else { (1, f.e) };
            let (ka, kb) = (key(a), key(b));
            ka.0.cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
        });
        for way in own {
            let d = off(way);
            if d <= REACH {
                return Some((d, way.clone()));
            }
            keep_rough(d, way.clone());
            match search.solve_keep(way, t, ceiling) {
                Some((m, f)) if m <= REACH => return Some((m, f)),
                Some((m, f)) => keep_rough(m, f),
                None => {}
            }
            if search.left == 0 {
                break;
            }
        }
        return rough;
    }
    let most_turn = (s.aim_turn.max(0.0) as f64).to_radians();
    search.accept = ROUGH;
    // Every way of the round, as a turn off this ball's own direction: (way, from where it sets
    // off too).
    let mut ways: Vec<(Found, bool)> = Vec::new();
    for (j, found) in firsts.iter().enumerate() {
        let (q0, v) = shots[j].at(base);
        let same = (q0 - p0).length() < 1e-3;
        for f in found {
            let dir = Vec2::from_angle(f.tweak as f32).rotate(v);
            let turn = u.angle_to(dir) as f64;
            if turn.abs() <= most_turn + 1e-9 {
                ways.push((Found { tweak: turn, ..f.clone() }, same || j == k));
            }
        }
    }
    // One that is there already, turned least.
    if let Some((d, f)) = ways
        .iter()
        .filter(|(_, same)| *same)
        .map(|(f, _)| (off(f), f))
        .filter(|(d, _)| *d <= REACH)
        .min_by(|a, b| a.1.tweak.abs().total_cmp(&b.1.tweak.abs()))
    {
        return Some((d, f.clone()));
    }
    for (f, same) in &ways {
        if *same {
            keep_rough(off(f), f.clone());
        }
    }
    // Else from the nearest in time (then turned least), each guess slowed or sped to get there
    // then.
    ways.sort_by(|a, b| {
        (a.0.e - t).abs().total_cmp(&(b.0.e - t).abs()).then(a.0.tweak.abs().total_cmp(&b.0.tweak.abs()))
    });
    let mut tried: Vec<(f64, f64)> = Vec::new();
    for (way, same) in &ways {
        let guess = (way.speed, way.tweak);
        if tried.iter().any(|g| (g.0 / guess.0 - 1.0).abs() < 0.01 && (g.1 - guess.1).abs() < 0.002) {
            continue;
        }
        tried.push(guess);
        match search.solve_turn((way, *same), t, most_turn, ceiling) {
            Some((m, f)) if m <= REACH => return Some((m, f)),
            Some((m, f)) => keep_rough(m, f),
            None => {}
        }
        if search.left == 0 {
            break;
        }
    }
    rough
}

/// A ball that found no way, when Can't reach says it goes anyway: at once, its own way, at Speed.
fn unaimed(aim: &Aimer, s: &EmitterSettings, u: Vec2, fire: u64) -> Option<BallAim> {
    aim.fire_anyway.then(|| BallAim { leave: fire, velocity: u * s.speed.max(0.0), reaches: false, path: Vec::new() })
}

/// Leaving when needed: shot `shot` from `base` (standing at the earliest moment it can set off
/// from, `now(fire)`) along its first way `found`, leaving so it reaches the target at the moment
/// `arrive` (in steps). The walls, the target or the emitter may have moved by then, changing
/// the way and so when it must set off: a secant on how late it would arrive against when it
/// sets off.
fn settle(
    base: &Lone,
    aim: &Aimer,
    s: &EmitterSettings,
    shot: Shot,
    found: Found,
    fire: u64,
    arrive: u64,
) -> Option<BallAim> {
    let earliest = now(fire);
    let (p0, u) = shot.at(base);
    let most = arrive.saturating_sub(earliest) + 2;
    let reach = (aim.target_at(arrive as f64) - p0).length().max(1.0) as f64;
    let mut search = search_for(base, aim, s, shot, reach);
    search.left = SETTLE_FLIGHTS;
    let mut found = found;
    let leave_at = |start: u64| if start == earliest { fire } else { start + 1 };
    // `u`: the way it goes from where `found` was worked out.
    let late = |found: &Found, u: Vec2| -> Option<BallAim> {
        aim.fire_anyway.then(|| BallAim {
            leave: fire,
            velocity: found.velocity(u),
            reaches: false,
            path: thin(&found.places),
        })
    };
    let mut lone = base.clone();
    let mut found_u = u;
    let mut tried: Vec<(f64, f64)> = vec![(earliest as f64, earliest as f64 + found.e - arrive as f64)];
    for _ in 0..SETTLE_TRIES {
        let plain = arrive as f64 - found.e;
        let guess = match tried.as_slice() {
            [.., (s1, g1), (s2, g2)] if (g2 - g1).abs() > 1e-9 => s2 - g2 * (s2 - s1) / (g2 - g1),
            _ => plain,
        };
        let guess = if guess.is_finite() { guess } else { plain };
        if plain.round() < earliest as f64 && guess.round() < earliest as f64 {
            // It can only get there late.
            return late(&found, found_u);
        }
        let start = guess.round().max(earliest as f64) as u64;
        if lone.moment() == start {
            // Back where `found` was worked out: it gets there in time only if it arrives within
            // half a step of the others from here.
            if (start as f64 + found.e - arrive as f64).abs() > 0.5 {
                break;
            }
            let path = thin(&found.places);
            return Some(BallAim { leave: leave_at(start), velocity: found.velocity(found_u), reaches: true, path });
        }
        if lone.moment() > start {
            lone = base.clone();
        }
        lone.advance(start);
        let (p0, u) = shot.at(&lone);
        let mut again = Search { lone: &lone, p0, u, tweak: found.tweak, ..search };
        let next = again.near(found.arc, found.speed, found.slope, most);
        (search.left, search.steps) = (again.left, again.steps);
        let Some(next) = next else { break };
        found = next;
        found_u = u;
        let off = start as f64 + found.e - arrive as f64;
        if off.abs() <= 0.5 {
            let path = thin(&found.places);
            return Some(BallAim { leave: leave_at(start), velocity: found.velocity(u), reaches: true, path });
        }
        tried.push((start as f64, off));
    }
    unaimed(aim, s, u, fire)
}

/// A round of `shots` fired on step `fire` from `base` (standing at `now(fire)`), aimed through
/// bounces: the moment (steps) they reach the target and each ball's aim, in order.
pub fn aim_shots(
    base: &Lone,
    aim: &Aimer,
    s: &EmitterSettings,
    shots: &[Shot],
    fire: u64,
) -> (f64, Vec<Option<BallAim>>) {
    let earliest = now(fire);
    let limit = (s.aim_arrive == AimArrive::After).then(|| (fire + aim.arrive).saturating_sub(earliest) as f64);
    let together = s.aim_leave == AimLeave::Together;
    // With a time to arrive by, ways a little later are looked for too when leaving together:
    // they may speed up (leaving when needed, a later way can only be late).
    let later = if together { 2.0 } else { 1.0 };
    let horizon = limit.map_or(MOST_STEPS as u64, |t| (later * t).ceil() as u64 + 1).min(MOST_STEPS as u64);
    let adjust = together && s.aim_way == AimWay::AdjustEachBall;
    let firsts = in_parallel(shots.len(), |k| first_ways(base, aim, s, shots[k], horizon, limit, adjust));
    // As soon as all can: when the last of them can be there, each by its soonest way.
    let soonest = |w: &Vec<Found>| w.iter().map(|f| f.e).reduce(f64::min);
    let t = limit.or_else(|| firsts.iter().filter_map(soonest).reduce(f64::max));
    let Some(t) = t else {
        let balls = shots.iter().map(|shot| unaimed(aim, s, shot.at(base).1, fire)).collect();
        return (earliest as f64, balls);
    };
    let arrive = earliest as f64 + t;
    if together {
        return (arrive, together_balls(base, aim, s, (shots, &firsts), t, fire));
    }
    // Each by its soonest way: no later than the round's moment.
    let balls = in_parallel(shots.len(), |k| match firsts[k].iter().min_by(|a, b| a.e.total_cmp(&b.e)) {
        Some(f) => settle(base, aim, s, shots[k], f.clone(), fire, arrive.round() as u64),
        None => unaimed(aim, s, shots[k].at(base).1, fire),
    });
    (arrive, balls)
}

/// Leaving together, each ball's aim to be at the target `t` steps after setting off ([`meet`]).
/// Adjusting each ball, those that found no way look again from the ways the others found
/// (they are at the target then: near ones need little more), and those still without one
/// borrow another's ([`borrow_ways`]).
fn together_balls(
    base: &Lone,
    aim: &Aimer,
    s: &EmitterSettings,
    (shots, firsts): (&[Shot], &[Vec<Found>]),
    t: f64,
    fire: u64,
) -> Vec<Option<BallAim>> {
    let mut ways = in_parallel(shots.len(), |k| meet(base, aim, s, (shots, firsts), k, t));
    let adjust = s.aim_way == AimWay::AdjustEachBall;
    let missing = |w: &Option<(f64, Found)>| w.as_ref().is_none_or(|(d, _)| *d > REACH);
    if adjust && ways.iter().any(missing) && ways.iter().any(|w| !missing(w)) {
        // The ways found, as if each passed at the round's moment (where it is then).
        let found: Vec<Vec<Found>> = ways
            .iter()
            .map(|w| w.iter().filter(|(d, _)| *d <= REACH).map(|(_, f)| Found { e: t, ..f.clone() }).collect())
            .collect();
        let again = in_parallel(shots.len(), |k| {
            if !missing(&ways[k]) {
                return None;
            }
            let mut mine = found.clone();
            mine[k] = Vec::new();
            meet(base, aim, s, (shots, &mine), k, t)
        });
        for (way, again) in ways.iter_mut().zip(again) {
            if let Some((d, f)) = again
                && way.as_ref().is_none_or(|(w, _)| d < *w)
            {
                *way = Some((d, f));
            }
        }
    }
    let balls: Vec<(Option<BallAim>, f64)> = ways
        .iter()
        .zip(shots)
        .map(|(way, shot)| {
            let u = shot.at(base).1;
            match way {
                Some((d, f)) => {
                    (Some(BallAim { leave: fire, velocity: f.velocity(u), reaches: true, path: thin(&f.places) }), *d)
                }
                None => (unaimed(aim, s, u, fire), f64::INFINITY),
            }
        })
        .collect();
    if adjust { borrow_ways(base, s, shots, balls) } else { balls.into_iter().map(|(b, _)| b).collect() }
}

/// Adjusting each ball: a ball that found no way of its own (or only a rough one) takes another
/// ball's that gets there from where it sets off too, when that needs a turn within Turn at most
/// (the smallest such): from one place, the ways that get there together are few, so they are
/// often shared.
fn borrow_ways(
    base: &Lone,
    s: &EmitterSettings,
    shots: &[Shot],
    balls: Vec<(Option<BallAim>, f64)>,
) -> Vec<Option<BallAim>> {
    let most_turn = s.aim_turn.max(0.0).to_radians() + 1e-6;
    let at: Vec<(Vec2, Vec2)> = shots.iter().map(|shot| shot.at(base)).collect();
    let mut out: Vec<Option<BallAim>> = balls.iter().map(|(b, _)| b.clone()).collect();
    for (k, ball) in out.iter_mut().enumerate() {
        // Within [`REACH`] already: its own stays.
        if balls[k].1 <= REACH {
            continue;
        }
        let (p0, u) = at[k];
        let lent = balls
            .iter()
            .enumerate()
            .filter(|(j, (_, d))| (at[*j].0 - p0).length() < 1e-3 && *d <= REACH)
            .filter_map(|(_, (b, _))| b.as_ref().filter(|b| b.reaches))
            .map(|b| (u.angle_to(b.velocity).abs(), b))
            .filter(|(turn, _)| *turn <= most_turn)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, b)) = lent {
            *ball = Some(b.clone());
        }
    }
    out
}

/// One ball's aim as a round of its own: see [`aim_shots`].
pub fn aim_ball(base: &Lone, aim: &Aimer, s: &EmitterSettings, shot: Shot, fire: u64) -> Option<BallAim> {
    aim_shots(base, aim, s, &[shot], fire).1.pop().flatten()
}

/// At most [`PATH_POINTS`] of `places`, evenly, the first and last kept.
fn thin(places: &[Vec2]) -> Vec<Vec2> {
    if places.len() <= PATH_POINTS {
        return places.to_vec();
    }
    let last = places.len() - 1;
    (0..PATH_POINTS).map(|i| places[i * last / (PATH_POINTS - 1)]).collect()
}

/// `f(0..n)` on several threads, the answers in order.
fn in_parallel<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get()).min(MOST_THREADS).min(n);
    if threads <= 1 {
        return (0..n).map(f).collect();
    }
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, T)>> = Mutex::new(Vec::with_capacity(n));
    let stop = super::aim_cache::stop_flag();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                let _stop = super::aim_cache::StopWith::new(stop.clone());
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let t = f(i);
                    done.lock().unwrap_or_else(|e| e.into_inner()).push((i, t));
                }
            });
        }
    });
    let mut done = done.into_inner().unwrap_or_else(|e| e.into_inner());
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, t)| t).collect()
}

/// Round `round` of `n` balls of emitter `me`, fired on step `fire` (the world standing at
/// `now(fire)`), aimed through bounces: kept from before for this scene, or worked out now.
pub fn aim_round(
    world: &World,
    me: ThingId,
    s: &EmitterSettings,
    aim: &Aimer,
    fire: u64,
    round: u32,
    n: u32,
) -> Arc<RoundAim> {
    let work = || {
        let base = Lone::new(world, me, me, s.radius.max(0.0), now(fire));
        let seed = world.seed;
        let shots: Vec<Shot> = (0..n)
            .map(|k| {
                let way = direction(s, k, n, round, seed, me.0);
                Shot::Emitter { offset: start_offset(s, k, n, way), way }
            })
            .collect();
        let (arrive, balls) = aim_shots(&base, aim, s, &shots, fire);
        Arc::new(RoundAim { round, fire, arrive: arrive.round() as u64, meet: aim.target_at(arrive), balls })
    };
    cached(world.fingerprint, me, What::Round { round, fire }, |r: &Arc<RoundAim>| r.balls.len(), work)
}

/// A ball of `owner` (row `row`, radius `radius`) caught by emitter `me` on step `step` (the
/// world standing after it) and sent back out from `start` the way `u` as a round of one fired on
/// step `fire`, aimed through bounces: kept from before, or worked out now.
#[allow(clippy::too_many_arguments)]
pub fn aim_catch(
    world: &World,
    me: ThingId,
    s: &EmitterSettings,
    aim: &Aimer,
    (owner, row, radius): (ThingId, u32, f32),
    step: u64,
    (start, u): (Vec2, Vec2),
    fire: u64,
) -> Option<BallAim> {
    let work = || {
        let mut base = Lone::new(world, me, owner, radius, step);
        base.advance(now(fire));
        aim_ball(&base, aim, s, Shot::Fixed { start, u }, fire)
    };
    cached(world.fingerprint, me, What::Catch { step, thing: owner, row }, |_: &Option<BallAim>| 1, work)
}

#[cfg(test)]
#[path = "test_fire_tests.rs"]
mod tests;
