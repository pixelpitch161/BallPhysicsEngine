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
use std::sync::{Arc, Mutex, OnceLock};

use bpe_core::ThingId;
use bpe_runtime::{Bodies, EventKind, Log, World};
use glam::Vec2;

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
/// How near the target (pixels) closing in tries to bring a pass.
pub const TOL: f64 = 0.25;
/// How near the target (pixels) a pass must go to count as through it, when the miss jumps
/// over [`TOL`] (see [`Search::close_in`]).
pub const GOOD: f64 = 0.45;
/// The most a ball's direction is turned to take it through (radians: 0.3°), unseen.
pub const MOST_TWEAK: f64 = 0.3 * std::f64::consts::PI / 180.0;
/// The most test flights finding one ball's way to be at the target at the round's moment may take.
pub const SOLVE_FLIGHTS: u32 = 150;
/// How near the target (pixels) a ball must be at the round's moment to count as there.
pub const REACH: f64 = 1.0;
/// How near the target (pixels) a first way found by turning must pass: the second pass makes it
/// exact.
pub const ROUGH: f64 = 3.0;
/// How many directions the scan flies a group of balls (setting off from one place) at, each
/// speed, about: each ball's window gets at least [`LEAST_SCAN`] and at most [`BALL_SCAN`].
const SCAN_BUDGET: usize = 4096;
const LEAST_SCAN: usize = 5;
const BALL_SCAN: usize = 17;
/// The speeds (× Speed) the scan flies at keeping the balls' directions when gravity, wind or
/// drag bend the paths and there is no time to arrive by.
const KEEP_SPEEDS: [f64; 4] = [1.0, 0.8, 0.64, 0.5];
/// How many speeds the scan flies at, keeping directions, with a time to arrive by.
const TIMED_SPEEDS: usize = 8;
/// The most a lead found going within [`ROUGH`] of the target is turned to take it nearer
/// (radians: 0.5°).
const HAIR: f64 = 0.5 * std::f64::consts::PI / 180.0;
/// How far (a share of the round's moment) when a way passes may move from the nearest speed
/// tried to a new one and still count as the same run of ways.
const BRANCH: f64 = 0.15;
/// Ways on one arc whose directions are nearer than this (radians: 0.2°) count as one.
const SAME_WAY: f64 = 0.2 * std::f64::consts::PI / 180.0;
/// How many ways (one per arc) a ball keeping its direction looks for by speed on its own, when
/// the scan found it none.
const OWN_WAYS: usize = 3;
/// The most flights closing in on one lead may take.
const CLOSE_FLIGHTS: u32 = 16;
/// How many of its leads a ball closes in on looking for its soonest way, at most.
const SOONEST_TRIES: usize = 8;
/// How many of its ways a ball checks slowed or sped to be there at the round's moment, at most.
const CHECK_TRIES: usize = 12;
/// How many flights to the round's moment a ball's steps for meeting the others allow, at least.
const MEET_FLIGHTS: u64 = 30;
/// How many of those (the nearest) it works on to be there, at most.
const MEET_TRIES: usize = 4;
/// How many places along each found way are kept for the preview.
pub const PATH_POINTS: usize = 96;
/// How many times the way is checked again from when it would set off, at most.
const SETTLE_TRIES: usize = 6;
/// How far (pixels) a test flight goes without touching a wall before it ends: it has left them
/// all, or slides to and fro in a bowl (ten seconds at 600 px/s; by distance, so a ball slowed
/// right down still gets as far).
const QUIET_DISTANCE: f32 = 6000.0;
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
/// set off) and the arc it was on at the last of them.
struct Flight {
    passes: Vec<Option<Pass>>,
    places: Vec<Vec2>,
    #[cfg_attr(not(test), allow(dead_code))]
    bounces: usize,
}

impl Flight {
    fn on(&self, arc: usize) -> Option<Pass> {
        self.passes.get(arc).copied().flatten()
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
    let mut bounces = 0usize;
    let mut still = 0u32;
    let mut quiet = 0f32;
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
        let previous = *places.last().unwrap_or(&p0);
        // At rest a second, or [`QUIET_DISTANCE`] without touching a wall: it goes nowhere new.
        still = if b.velocity[0].length_squared() < 1.0 { still + 1 } else { 0 };
        quiet = if h > 0 { 0.0 } else { quiet + (p - previous).length() };
        if still > 240 || quiet > QUIET_DISTANCE {
            break;
        }
        let target = aim.target_at(w.step as f64);
        places.push(p);
        rel.push(p - target);
        hits.push(h as u32);
        bounces = arc;
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
    Flight { passes, places, bounces }
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

    /// Turning at most `most_turn` off its direction, a way through the target on `way`'s arc
    /// that passes it `t` steps after setting off (within half a step): along the ways through
    /// it on that arc (at each speed, the turn that takes it through, [`Search::through`]), a
    /// secant on the speed until it passes then. The way, with how far off the target it passes,
    /// when within [`ROUGH`].
    fn solve_turn(&mut self, (way, own): (&Found, bool), t: f64, most_turn: f64, ceiling: f64) -> Option<(f64, Found)> {
        let most = ((t * 1.25) as u64 + 8).min(MOST_STEPS as u64);
        let mut rough: Option<(f64, Found)> = None;
        // Slowed or sped as if its way stayed the same first, then the other way: on a curved
        // path, slower can be sooner.
        // Where the paths bend the way changes with the speed: small steps keep to it.
        let whole = if self.aim.bends() { (way.e / t).clamp(1.0 / 1.15, 1.15) } else { way.e / t };
        let plain = whole.clamp(1.0 / 1.15, 1.15);
        let plain = if (plain - 1.0).abs() < 0.02 { 1.02f64.copysign(plain - 1.0) } else { plain };
        for first in [whole, 1.0 / plain] {
            let nudge = first.clamp(1.0 / 1.15, 1.15);
            // Another ball's way (from elsewhere) is only a first guess for this one.
            let mut tried: Vec<(f64, f64)> = if own { vec![(way.speed, way.e - t)] } else { Vec::new() };
            let mut tweak = way.tweak;
            let mut turns: Vec<(f64, f64)> = if own { vec![(way.speed, way.tweak)] } else { Vec::new() };
            let mut good = way.speed;
            let mut stalls = 0;
            let mut speed = (way.speed * first).clamp(1.0, ceiling);
            // Already as fast as it may go: that way is no way.
            if own && (speed - way.speed).abs() <= way.speed * 1e-6 {
                continue;
            }
            for _ in 0..10 {
                let th = self.through(speed, tweak, way.arc, most_turn, most);
                // Passing far sooner or later than the nearest speed tried is another run of ways
                // (it went round the other side of something): as if there were none there.
                let nearest_late = tried.iter().min_by(|a, b| (a.0 - speed).abs().total_cmp(&(b.0 - speed).abs()));
                let th = th.filter(|f| nearest_late.is_none_or(|&(_, late)| (f.e - t - late).abs() <= BRANCH * t));
                match th {
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
                                speed *= nudge;
                                tweak = found.tweak;
                                continue;
                            };
                            let next =
                                if (g2 - g1).abs() > 1e-9 { s2 - g2 * (s2 - s1) / (g2 - g1) } else { s2 * nudge };
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

/// The fastest a ball may go: a hair under the world's own limit and under the speed that splits
/// the world's steps (a ball that fast would change every other ball's bounces), and arriving as
/// soon as all can, no faster than Speed (the round's moment is when the last can be there at
/// Speed).
fn ceiling_of(aim: &Aimer, s: &EmitterSettings) -> f64 {
    let unsplit = bpe_runtime::world::SAFE_MOVE as f64 / aim.dt as f64;
    let world = (aim.ceiling as f64).min(unsplit) * 0.999;
    if s.aim_arrive == AimArrive::AsSoonAsAllCan { world.min(s.speed.max(1.0) as f64) } else { world }
}

/// A way through the target the scan found, from where its group sets off: the direction it
/// goes (radians, not turned by anything), the speed, the arc it passes on, when (steps after
/// setting off) and the places it goes through up to there.
#[derive(Clone, Debug)]
struct Way {
    angle: f64,
    speed: f64,
    arc: usize,
    e: f64,
    places: Vec<Vec2>,
}

impl Way {
    /// As a way of a ball going the way `own` (radians): turned off it.
    fn turned(&self, own: f64) -> Found {
        Found {
            speed: self.speed,
            tweak: wrap(self.angle - own),
            arc: self.arc,
            e: self.e,
            places: self.places.clone(),
            slope: f64::NAN,
        }
    }
}

/// Where the scan saw a way: two neighbouring flights (direction, miss) whose misses on `arc`
/// are on either side of the target (`b`: at `speed` the next direction, or the same direction at
/// the next speed `b_speed`), or one that went through (`b` none), at `speed`;
/// `e` is when it would pass, guessed from the flights.
#[derive(Clone, Copy, Debug)]
struct Lead {
    arc: usize,
    speed: f64,
    a: (f64, f64),
    b: Option<(f64, f64)>,
    /// The other end's speed, when the two flights went the same way at neighbouring speeds.
    b_speed: f64,
    e: f64,
}

impl Lead {
    /// Whether a ball going the way `own` (radians) can turn to it, at most `window`.
    fn within(&self, own: f64, window: f64) -> bool {
        let ok = |x: f64| wrap(x - own).abs() <= window + 1e-9;
        ok(self.a.0) && self.b.is_none_or(|b| ok(b.0))
    }
}

/// The balls of a round that set off from one place (within 1e-3 px: a flight depends only on
/// where from, which way and how fast, so they share their test flights), the leads the scan
/// found from there and, once closed in on, their ways (each closed once, by whichever ball
/// wants it first).
struct Group {
    p0: Vec2,
    balls: Vec<usize>,
    near: f64,
    leads: Vec<Lead>,
    closed: Vec<OnceLock<(Option<Way>, u64)>>,
    /// How many of its balls can turn to each lead.
    sharers: Vec<u64>,
}

/// How a round is searched: how far each ball may turn (radians), the speeds the scan flies at,
/// how many steps a flight may take and the fastest a ball may go.
struct Hunt {
    window: f64,
    speeds: Vec<f64>,
    horizon: u64,
    ceiling: f64,
}

/// An angle (radians) within (-π, π].
fn wrap(a: f64) -> f64 {
    let r = a.rem_euclid(std::f64::consts::TAU);
    if r > std::f64::consts::PI { r - std::f64::consts::TAU } else { r }
}

/// The scan of a round of `shots` from `base` (standing at the moment they set off): the balls
/// in groups by where they set off; each ball's window of directions flown at every speed of
/// `hunt` (shared in its group: [`SCAN_BUDGET`] directions a group, at least [`LEAST_SCAN`] and
/// at most [`BALL_SCAN`] a ball), each flight to Bounces at most; the leads it gives. `limit`:
/// the steps the round has to get there in, when it has a time to arrive by.
fn scan(base: &Lone, aim: &Aimer, s: &EmitterSettings, shots: &[Shot], hunt: &Hunt, limit: Option<f64>) -> Vec<Group> {
    let at: Vec<(Vec2, Vec2)> = shots.iter().map(|shot| shot.at(base)).collect();
    let mut groups: Vec<Group> = Vec::new();
    for (k, &(p0, u)) in at.iter().enumerate() {
        if u == Vec2::ZERO {
            continue;
        }
        match groups.iter_mut().find(|g| (g.p0 - p0).length() < 1e-3) {
            Some(g) => g.balls.push(k),
            None => {
                let reach = (aim.target_at(base.moment() as f64 + limit.unwrap_or(0.0)) - p0).length().max(1.0) as f64;
                let near = (0.3 * reach).max(40.0);
                groups.push(Group {
                    p0,
                    balls: vec![k],
                    near,
                    leads: Vec::new(),
                    closed: Vec::new(),
                    sharers: Vec::new(),
                });
            }
        }
    }
    // (group, direction, speed), each group's by speed then direction.
    let mut jobs: Vec<(usize, f64, f64)> = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        let per = SCAN_BUDGET / (g.balls.len() * hunt.speeds.len()).max(1);
        let n = if hunt.window > 0.0 { per.clamp(LEAST_SCAN, BALL_SCAN) | 1 } else { 1 };
        let mut angles: Vec<f64> = Vec::with_capacity(n * g.balls.len());
        for &k in &g.balls {
            let own = at[k].1.to_angle() as f64;
            for i in 0..n {
                let turn = if n == 1 { 0.0 } else { hunt.window * (2.0 * i as f64 / (n - 1) as f64 - 1.0) };
                angles.push(wrap(own + turn));
            }
        }
        angles.sort_by(f64::total_cmp);
        angles.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        for &speed in &hunt.speeds {
            jobs.extend(angles.iter().map(|&a| (gi, a, speed)));
        }
    }
    let least_arc = s.aim_bounces as usize;
    let most_arc = (s.aim_bounces_most as usize).max(least_arc);
    let flown: Vec<Vec<Option<Pass>>> = in_parallel(jobs.len(), |j| {
        let (gi, angle, speed) = jobs[j];
        if super::aim_cache::stopped() {
            return Vec::new();
        }
        let v = Vec2::from_angle(angle as f32) * speed as f32;
        fly(base, aim, groups[gi].p0, v, hunt.horizon, most_arc).passes
    });
    // The leads: each run of one group's flights at one speed, in order of direction, all round.
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    let mut start = 0;
    while start < jobs.len() {
        let (gi, _, speed) = jobs[start];
        let mut end = start;
        while end < jobs.len() && jobs[end].0 == gi && jobs[end].2 == speed {
            end += 1;
        }
        let near = groups[gi].near;
        let mut leads = Vec::new();
        for i in start..end {
            for (arc, p) in flown[i].iter().enumerate().skip(least_arc) {
                if let Some(p) = p.filter(|p| p.miss.abs() <= ROUGH) {
                    leads.push(Lead { arc, speed, a: (jobs[i].1, p.miss), b: None, b_speed: speed, e: p.e });
                }
            }
            // Its neighbour the other way round, the last's the first.
            let j = if i + 1 < end { i + 1 } else { start };
            let gap = wrap(jobs[j].1 - jobs[i].1);
            if j == i || gap <= 0.0 || gap > 2.0 * hunt.window + 1e-9 {
                continue;
            }
            for (arc, p, q, e) in brackets(&flown[i], &flown[j], least_arc, near) {
                let (a, b) = ((jobs[i].1, p), Some((jobs[j].1, q)));
                leads.push(Lead { arc, speed, a, b, b_speed: speed, e });
            }
        }
        groups[gi].leads.extend(leads);
        runs.push((gi, start, end));
        start = end;
    }
    // And each direction between neighbouring speeds (one group's runs share their directions).
    for pair in runs.windows(2) {
        let ((g1, s1, e1), (g2, s2, e2)) = (pair[0], pair[1]);
        if g1 != g2 || e1 - s1 != e2 - s2 {
            continue;
        }
        let near = groups[g1].near;
        for (i, j) in (s1..e1).zip(s2..e2) {
            for (arc, p, q, e) in brackets(&flown[i], &flown[j], least_arc, near) {
                let (speed, b_speed) = (jobs[i].2, jobs[j].2);
                let (a, b) = ((jobs[i].1, p), Some((jobs[j].1, q)));
                groups[g1].leads.push(Lead { arc, speed, a, b, b_speed, e });
            }
        }
    }
    for g in &mut groups {
        g.closed = (0..g.leads.len()).map(|_| OnceLock::new()).collect();
        let owns: Vec<f64> = g.balls.iter().map(|&k| at[k].1.to_angle() as f64).collect();
        g.sharers = g
            .leads
            .iter()
            .map(|l| owns.iter().filter(|&&own| l.within(own, hunt.window)).count().max(1) as u64)
            .collect();
    }
    groups
}

/// The arcs (from `least_arc`) on which two flights' passes `p` and `q` are on either side of the
/// target, both within `near`: (arc, miss of each, when it would pass, guessed between them).
fn brackets<'p>(
    p: &'p [Option<Pass>],
    q: &'p [Option<Pass>],
    least_arc: usize,
    near: f64,
) -> impl Iterator<Item = (usize, f64, f64, f64)> + 'p {
    p.iter().zip(q).enumerate().skip(least_arc).filter_map(move |(arc, (p, q))| {
        let (p, q) = (p.as_ref()?, q.as_ref()?);
        if p.miss.signum() == q.miss.signum() || p.miss.abs().max(q.miss.abs()) > near {
            return None;
        }
        let x = p.miss.abs() / (p.miss.abs() + q.miss.abs()).max(1e-12);
        Some((arc, p.miss, q.miss, p.e + (q.e - p.e) * x))
    })
}

/// Lead `li` of group `g` closed in on (once: the way is kept for every ball that wants it): a
/// flight through it flown again for its places, or the direction between the two flights
/// closed in on, flights stopping past its arc. `None`: the arc broke between them. Its share of
/// the steps its flights took (split between the balls that can turn to it) is taken off
/// `budget`, whoever flew them: what a ball goes on to try never hangs on which thread got
/// there first.
fn close_lead<'g>(
    base: &Lone,
    aim: &Aimer,
    s: &EmitterSettings,
    (g, li): (&'g Group, usize),
    most: u64,
    budget: &mut u64,
) -> &'g Option<Way> {
    let (way, used) = g.closed[li].get_or_init(|| {
        let lead = g.leads[li];
        let mut search = Search {
            lone: base,
            aim,
            p0: g.p0,
            u: Vec2::from_angle(lead.a.0 as f32),
            least_arc: s.aim_bounces as usize,
            most_arc: lead.arc,
            prefer: lead.speed,
            near: g.near,
            tweak: 0.0,
            accept: ROUGH,
            left: CLOSE_FLIGHTS,
            steps: CLOSE_FLIGHTS as u64 * most,
        };
        let start = search.steps;
        let mut close = || -> Option<Found> {
            match lead.b {
                None => {
                    let flight = search.fly_at(lead.speed, 0.0, most)?;
                    let pass = flight.on(lead.arc).filter(|p| p.miss.abs() <= ROUGH)?;
                    let rough = Found::of(lead.speed, 0.0, lead.arc, pass, &flight);
                    // Brought nearer by a hair's turn when it can be.
                    Some(match pass.miss.abs() > TOL {
                        true => search.through(lead.speed, 0.0, lead.arc, HAIR, most).unwrap_or(rough),
                        false => rough,
                    })
                }
                Some(b) if b.0 == lead.a.0 => {
                    search.close_in(lead.arc, Knob::Speed, (lead.speed, lead.a.1), (lead.b_speed, b.1), most)
                }
                Some(b) => {
                    let ends = ((0.0, lead.a.1), (wrap(b.0 - lead.a.0), b.1));
                    search.close_in(lead.arc, Knob::Tweak(lead.speed), ends.0, ends.1, most)
                }
            }
        };
        let way = close().map(|found| Way {
            angle: wrap(lead.a.0 + found.tweak),
            speed: found.speed,
            arc: found.arc,
            e: found.e,
            places: found.places,
        });
        (way, start - search.steps)
    });
    *budget = budget.saturating_sub(*used / g.sharers[li]);
    way
}

/// Ball `k`'s leads in its group (those it can turn to), and its soonest way: its leads by when
/// they would pass, closed in turn until one holds (at most [`SOONEST_TRIES`], and
/// [`BALL_STEPS`] of flights).
fn soonest(
    base: &Lone,
    aim: &Aimer,
    s: &EmitterSettings,
    (g, own): (&Group, f64),
    hunt: &Hunt,
) -> (Vec<usize>, Option<Way>) {
    let mut mine: Vec<usize> = (0..g.leads.len()).filter(|&i| g.leads[i].within(own, hunt.window)).collect();
    mine.sort_by(|a, b| g.leads[*a].e.total_cmp(&g.leads[*b].e));
    let mut budget = BALL_STEPS;
    let mut first = None;
    for &li in mine.iter().take(SOONEST_TRIES) {
        if budget == 0 {
            break;
        }
        if let Some(way) = close_lead(base, aim, s, (g, li), hunt.horizon, &mut budget) {
            first = Some(way.clone());
            break;
        }
    }
    (mine, first)
}

/// Shot `shot`'s own ways keeping its direction, by speed: the speeds of `hunt` and between them
/// where the flights came nearest ([`Search::ways`]), up to three arcs from the fewest bounces.
fn own_ways(base: &Lone, aim: &Aimer, s: &EmitterSettings, shot: Shot, hunt: &Hunt, limit: Option<f64>) -> Vec<Way> {
    let (p0, u) = shot.at(base);
    let reach = (aim.target_at(base.moment() as f64 + limit.unwrap_or(0.0)) - p0).length().max(1.0) as f64;
    let mut search = search_for(base, aim, s, shot, reach);
    let own = u.to_angle() as f64;
    let found = search.ways(&hunt.speeds, Knob::Speed, hunt.horizon, OWN_WAYS);
    found
        .into_iter()
        .map(|f| Way { angle: wrap(own + f.tweak), speed: f.speed, arc: f.arc, e: f.e, places: f.places })
        .collect()
}

/// Ball `k` (going the way `own`, its leads `mine` in group `g`) at the target `t` steps after
/// setting off, all of the round leaving together: from its leads that need least change of
/// speed first (each closed in on, turned to its direction), the speed and turn that have it
/// there then ([`Search::solve_turn`]: with nothing bending the paths, the way slowed or sped
/// by its time over `t` is the very same way). The way, with how far off the target it is then
/// (within [`ROUGH`]); `None`: it can't.
#[allow(clippy::too_many_arguments)]
fn meet(
    base: &Lone,
    aim: &Aimer,
    s: &EmitterSettings,
    shot: Shot,
    (g, mine): (&Group, &[usize]),
    hunt: &Hunt,
    t: f64,
) -> Option<(f64, Found)> {
    let (p0, u) = shot.at(base);
    let own = u.to_angle() as f64;
    let reach = (aim.target_at(base.moment() as f64 + t) - p0).length().max(1.0) as f64;
    let mut search = search_for(base, aim, s, shot, reach);
    search.left = SOLVE_FLIGHTS;
    search.accept = ROUGH;
    // A late moment takes long flights: enough steps for a few dozen of them.
    let budget = BALL_STEPS.max(MEET_FLIGHTS * (t as u64 + 2));
    search.steps = budget;
    // Those it can get there from going no faster than it may first; then the rest, soonest
    // first. With nothing bending the paths, the fewest bounces first: slowed or sped, the way
    // stays the same, and the fewer the bounces the less the steps' rounding moves it. Else the
    // least change of speed first: the way changes with the speed.
    let bends = aim.bends();
    let mut order: Vec<(bool, f64, usize)> = mine
        .iter()
        .map(|&li| {
            let lead = &g.leads[li];
            // Where the paths bend, a slower ball may be there sooner as well as later.
            let can = bends || lead.speed * lead.e / t <= hunt.ceiling * (1.0 + 1e-9);
            let key = match (can, bends) {
                (true, true) => (lead.e / t).ln().abs(),
                (true, false) => lead.arc as f64 * 1e3 + wrap(lead.a.0 - own).abs(),
                (false, _) => lead.e,
            };
            (!can, key, li)
        })
        .collect();
    order.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut rough: Option<(f64, Found)> = None;
    let mut keep_rough = |m: f64, f: Found| {
        if m <= ROUGH && rough.as_ref().is_none_or(|(r, _)| m < *r) {
            rough = Some((m, f));
        }
    };
    // First each way slowed or sped by its time over `t`, one flight each: with nothing bending
    // the paths it is the very same way, there then. The ways checked, with how far off they
    // are then (leads next to each other often close in on one way: it is checked once).
    let mut checked: Vec<(f64, Found)> = Vec::new();
    for (closed, (_, _, li)) in order.into_iter().enumerate() {
        // Half the steps are kept for working on the nearest.
        if checked.len() >= CHECK_TRIES || closed >= 3 * CHECK_TRIES || search.left == 0 || search.steps < budget / 2 {
            break;
        }
        let Some(way) = close_lead(base, aim, s, (g, li), hunt.horizon, &mut search.steps) else { continue };
        let found = way.turned(own);
        if checked.iter().any(|(_, f)| f.arc == found.arc && (f.tweak - found.tweak).abs() < SAME_WAY) {
            continue;
        }
        let speed = found.speed * found.e / t;
        let mut off = f64::INFINITY;
        if (1.0..=hunt.ceiling * (1.0 + 1e-9)).contains(&speed)
            && let Some(flight) = search.fly_at(speed, found.tweak, t.ceil() as u64 + 2)
            && flight.on(found.arc).is_some_and(|p| (p.e - t).abs() < 2.0)
        {
            let upto = (t.ceil() as usize + 2).min(flight.places.len());
            let f = Found { speed, e: t, places: flight.places[..upto].to_vec(), ..found.clone() };
            if let Some(m) = search.off_at(&f, t) {
                if m <= REACH {
                    return Some((m, f));
                }
                keep_rough(m, f);
                off = m;
            }
        }
        checked.push((off, found));
    }
    // Then the nearest of them worked on: speed and turn together.
    checked.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, found) in checked.into_iter().take(MEET_TRIES) {
        if search.left == 0 || search.steps == 0 {
            break;
        }
        match search.solve_turn((&found, true), t, hunt.window, hunt.ceiling) {
            Some((m, f)) if m <= REACH => return Some((m, f)),
            Some((m, f)) => keep_rough(m, f),
            None => {}
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
    let ceiling = ceiling_of(aim, s);
    let prefer = (s.speed.max(1.0) as f64).min(ceiling);
    // Where the paths bend, other speeds go other ways: they are scanned too. With a time to
    // arrive by, keeping its direction, the speeds round what gets there then.
    let mut speeds: Vec<f64> = match (limit, adjust, aim.bends()) {
        (Some(t), false, _) => {
            let reach = shots
                .iter()
                .map(|shot| (aim.target_at(earliest as f64 + t) - shot.at(base).0).length() as f64)
                .fold(1.0, f64::max);
            let quickest = reach / (t.max(1.0) * aim.dt as f64);
            let (lo, hi) = (0.5 * quickest.min(prefer), (4.0 * quickest.max(prefer)).min(ceiling));
            let ratio = (hi / lo).max(1.0).powf(1.0 / (TIMED_SPEEDS - 1) as f64);
            std::iter::once(prefer).chain((0..TIMED_SPEEDS).map(|i| lo * ratio.powi(i as i32))).collect()
        }
        (None, false, true) => KEEP_SPEEDS.iter().map(|r| prefer * r).collect(),
        _ => vec![prefer],
    };
    speeds.sort_by(f64::total_cmp);
    speeds.dedup_by(|a, b| (*a / *b - 1.0).abs() < 1e-6);
    let window = if adjust { (s.aim_turn.max(0.0) as f64).to_radians() } else { MOST_TWEAK };
    let hunt = Hunt { window, speeds, horizon, ceiling };
    let mut groups = scan(base, aim, s, shots, &hunt, limit);
    let mut group_of: Vec<Option<usize>> = vec![None; shots.len()];
    for (gi, g) in groups.iter().enumerate() {
        for &k in &g.balls {
            group_of[k] = Some(gi);
        }
    }
    let owns: Vec<f64> = shots.iter().map(|shot| shot.at(base).1.to_angle() as f64).collect();
    let mut firsts: Vec<(Vec<usize>, Option<Way>)> = in_parallel(shots.len(), |k| match group_of[k] {
        Some(gi) => soonest(base, aim, s, (&groups[gi], owns[k]), &hunt),
        None => (Vec::new(), None),
    });
    // Keeping its direction where the paths bend, the speeds scanned may all miss between them:
    // a ball with no way yet searches its own speeds closer (part 4's search), and what it finds
    // joins its group's leads.
    if !adjust && aim.bends() {
        let more = in_parallel(shots.len(), |k| match (group_of[k], &firsts[k].1) {
            (Some(_), None) => own_ways(base, aim, s, shots[k], &hunt, limit),
            _ => Vec::new(),
        });
        for (k, ways) in more.into_iter().enumerate() {
            let Some(gi) = group_of[k] else { continue };
            for way in ways {
                let g = &mut groups[gi];
                let lead =
                    Lead { arc: way.arc, speed: way.speed, a: (way.angle, 0.0), b: None, b_speed: way.speed, e: way.e };
                if firsts[k].1.as_ref().is_none_or(|w| way.e < w.e) {
                    firsts[k].1 = Some(way.clone());
                }
                g.leads.push(lead);
                g.closed.push(OnceLock::from((Some(way), 0)));
                g.sharers.push(1);
                firsts[k].0.push(g.leads.len() - 1);
            }
        }
    }
    // As soon as all can: when the last of them can be there, each by its soonest way.
    let t = limit.or_else(|| firsts.iter().filter_map(|f| f.1.as_ref().map(|w| w.e)).reduce(f64::max));
    let Some(t) = t else {
        let balls = shots.iter().map(|shot| unaimed(aim, s, shot.at(base).1, fire)).collect();
        return (earliest as f64, balls);
    };
    let arrive = earliest as f64 + t;
    if together {
        let ways = in_parallel(shots.len(), |k| {
            let gi = group_of[k]?;
            // With no time to arrive by, one with no soonest way has none at all.
            if limit.is_none() && firsts[k].1.is_none() {
                return None;
            }
            meet(base, aim, s, shots[k], (&groups[gi], &firsts[k].0), &hunt, t)
        });
        let balls: Vec<(Option<BallAim>, f64)> = ways
            .iter()
            .zip(shots)
            .map(|(way, shot)| {
                let u = shot.at(base).1;
                match way {
                    Some((d, f)) => {
                        let path = thin(&f.places);
                        (Some(BallAim { leave: fire, velocity: f.velocity(u), reaches: true, path }), *d)
                    }
                    None => (unaimed(aim, s, u, fire), f64::INFINITY),
                }
            })
            .collect();
        return (arrive, borrow_ways(base, window, shots, balls));
    }
    // Each by its way nearest Speed that is there by the round's moment (else its soonest).
    let balls = in_parallel(shots.len(), |k| {
        let g = match group_of[k] {
            Some(gi) => &groups[gi],
            None => return unaimed(aim, s, shots[k].at(base).1, fire),
        };
        let mut order: Vec<usize> = firsts[k].0.iter().copied().filter(|&li| g.leads[li].e <= t + 0.5).collect();
        let off = |li: usize| (g.leads[li].speed / prefer).ln().abs();
        order.sort_by(|&a, &b| off(a).total_cmp(&off(b)).then(g.leads[a].e.total_cmp(&g.leads[b].e)));
        let mut budget = BALL_STEPS;
        let way = order
            .iter()
            .take(SOONEST_TRIES)
            .find_map(|&li| close_lead(base, aim, s, (g, li), hunt.horizon, &mut budget).clone())
            .or_else(|| firsts[k].1.clone());
        match way {
            Some(way) => settle(base, aim, s, shots[k], way.turned(owns[k]), fire, arrive.round() as u64),
            None => unaimed(aim, s, shots[k].at(base).1, fire),
        }
    });
    (arrive, balls)
}

/// Leaving together, a ball that found no way of its own (or only a rough one) takes another
/// ball's that gets there from where it sets off too, when that needs a turn within `window`
/// (the smallest such): from one place, the ways that get there together are few, so they are
/// often shared.
fn borrow_ways(base: &Lone, window: f64, shots: &[Shot], balls: Vec<(Option<BallAim>, f64)>) -> Vec<Option<BallAim>> {
    let most_turn = window as f32 + 1e-6;
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
