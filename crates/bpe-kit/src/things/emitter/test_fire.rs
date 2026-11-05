//! Through bounces: aiming by test-firing. Each ball keeps its direction; its speed is found by
//! firing a lone copy of it in a stripped copy of the world (the walls and the ball's own Motion
//! and Bounce cards, nothing else: [`bpe_runtime::World::lone`]) at trial speeds until one
//! passes through the target, and it then leaves so it gets there when the others do.
//!
//! A test flight *is* the world's own stepping, from the very moment the real ball sets off, so a
//! way found is the way the real ball goes as long as nothing stripped away acts on it. What can
//! still differ: other balls it would bump into, a Motion card varied across the balls (the lone
//! ball has the card's own values), substeps the scene's fastest ball would cause (above 3,840
//! px/s), a physics setting following sound (it hears no hits here) and a wall riding a ball
//! (it stays put here).
//!
//! The search, per ball: fly at Speed, then at [`SCAN`] speeds evenly in ratio; group every pass
//! of the target by how many times the ball had bounced before it (its *arc*); a pass within
//! [`TOL`] is a way; else two neighbouring speeds whose passes on the same arc miss on opposite
//! sides bracket one, closed in on by the Illinois method. The fewest bounces win (at least
//! Bounces at least), then the speed nearest Speed. At most [`MOST_FLIGHTS`] flights a ball.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bpe_core::ThingId;
use bpe_runtime::{Bodies, EventKind, Log, World};
use glam::Vec2;

use super::EmitterSettings;
use super::aim::{Aimer, now};
use super::aim_cache::{What, cached};
use super::fire::{direction, start_offset};
use super::sim::{EmitterState, emitter_state, entry};
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
/// How many places along each found way are kept for the preview.
pub const PATH_POINTS: usize = 96;
/// How many times the way is checked again from when it would set off, at most.
const SETTLE_TRIES: usize = 6;
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

/// One test flight: its passes per arc and the places it went through, a step apart from where
/// it set off.
struct Flight {
    passes: Vec<Option<Pass>>,
    places: Vec<Vec2>,
}

impl Flight {
    fn on(&self, arc: usize) -> Option<Pass> {
        self.passes.get(arc).copied().flatten()
    }
}

/// Fires the lone ball from `p0` at `v` in a copy of `lone`, for at most `most` steps.
fn fly(lone: &Lone, aim: &Aimer, p0: Vec2, v: Vec2, most: u64) -> Flight {
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
        let previous = *places.last().unwrap_or(&p0);
        let target = aim.target_at(w.step as f64);
        places.push(p);
        rel.push(p - target);
        hits.push(h as u32);
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
    if let Some(last) = last_arc {
        passes.truncate(last + 1);
    }
    Flight { passes, places }
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
    fn of(speed: f64, tweak: f64, arc: usize, pass: Pass, flight: Flight) -> Found {
        let upto = (pass.e.ceil() as usize + 1).min(flight.places.len());
        let mut places = flight.places;
        places.truncate(upto);
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
    prefer: f64,
    near: f64,
    tweak: f64,
    left: u32,
}

/// A way to try: a flight already through, or a bracket to close in on (flight index, arc).
enum Try {
    Through(usize, usize),
    Bracket(usize, usize),
}

impl Search<'_> {
    fn fly_at(&mut self, speed: f64, tweak: f64, most: u64) -> Option<Flight> {
        // A background run told to stop gives up: its answer is never kept.
        if self.left == 0 || super::aim_cache::stopped() {
            self.left = 0;
            return None;
        }
        self.left -= 1;
        let v = Vec2::from_angle(tweak as f32).rotate(self.u) * speed as f32;
        Some(fly(self.lone, self.aim, self.p0, v, most))
    }

    fn fly(&mut self, speed: f64, most: u64) -> Option<Flight> {
        self.fly_at(speed, self.tweak, most)
    }

    /// How far a speed is from the one it would rather have, in ratio.
    fn off(&self, speed: f64) -> f64 {
        (speed.max(1e-9) / self.prefer.max(1e-9)).ln().abs()
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
                return Some(Found { slope, ..Found::of(speed, tweak, arc, pass, flight) });
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
        if pass.miss.abs() <= GOOD {
            return Some(Found::of(speed, tweak, arc, pass, flight));
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
                    return Some(Found::of(speed, tweak, arc, pass, flight));
                }
                if pass.miss.signum() != miss.signum() {
                    return self.close_in(arc, Knob::Tweak(speed), (own, miss), (tweak, pass.miss), most);
                }
            }
        }
        None
    }

    /// The best way among the flights at `speeds` (flown here), closing in on brackets as needed;
    /// with none yet, the speeds between the two flights that came nearest are tried, until the
    /// flights run out.
    fn best(&mut self, speeds: &[f64], most: u64) -> Option<Found> {
        let mut flown: Vec<(f64, Flight)> = Vec::new();
        for &s in speeds {
            if s > 0.0 && s.is_finite() && !flown.iter().any(|(f, _)| *f == s) {
                let Some(flight) = self.fly(s, most) else { break };
                flown.push((s, flight));
            }
        }
        flown.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Brackets that broke (a wall changed between their ends), by their ends' speeds and arc.
        let mut broken: Vec<(f64, f64, usize)> = Vec::new();
        loop {
            // Ways already through, then brackets, best first: fewest bounces, then nearest the
            // speed it would rather have.
            let mut tries: Vec<(usize, f64, Try)> = Vec::new();
            for (i, (s, f)) in flown.iter().enumerate() {
                for arc in self.least_arc..f.passes.len() {
                    if f.on(arc).is_some_and(|p| p.miss.abs() <= GOOD) {
                        tries.push((arc, self.off(*s), Try::Through(i, arc)));
                    }
                }
            }
            for i in 1..flown.len() {
                let (lo, hi) = (&flown[i - 1], &flown[i]);
                let arcs = lo.1.passes.len().min(hi.1.passes.len());
                for arc in self.least_arc..arcs {
                    if let (Some(p), Some(q)) = (lo.1.on(arc), hi.1.on(arc))
                        && p.miss.signum() != q.miss.signum()
                        && p.miss.abs() > GOOD
                        && q.miss.abs() > GOOD
                        && p.miss.abs().max(q.miss.abs()) <= self.near
                        && !broken.contains(&(lo.0, hi.0, arc))
                    {
                        tries.push((arc, self.off((lo.0 * hi.0).sqrt()), Try::Bracket(i - 1, arc)));
                    }
                }
            }
            tries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
            for (_, _, t) in tries {
                match t {
                    Try::Through(i, arc) => {
                        let (s, f) = flown.swap_remove(i);
                        let pass = f.on(arc)?;
                        return Some(Found::of(s, self.tweak, arc, pass, f));
                    }
                    Try::Bracket(i, arc) => {
                        let (a, fa) = (flown[i].0, flown[i].1.on(arc)?.miss);
                        let (b, fb) = (flown[i + 1].0, flown[i + 1].1.on(arc)?.miss);
                        if let Some(found) = self.close_in(arc, Knob::Speed, (a, fa), (b, fb), most) {
                            return Some(found);
                        }
                        broken.push((a, b, arc));
                    }
                }
            }
            if self.left == 0 {
                return None;
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
                if b <= a * (1.0 + 1e-6) {
                    continue;
                }
                let score = nearest(&flown[i - 1].1).min(nearest(&flown[i].1));
                let score = if score.is_finite() { score } else { 1e12 / (b / a).ln() };
                if pick.is_none_or(|(best, _)| score < best) {
                    pick = Some((score, i));
                }
            }
            let (_, i) = pick?;
            let mid = (flown[i - 1].0 * flown[i].0).sqrt();
            let flight = self.fly(mid, most)?;
            flown.insert(i, (mid, flight));
        }
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
            return Some(Found::of(speed, self.tweak, a, pass, flight));
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
                    return Some(Found::of(next, self.tweak, arc, pass, flight));
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

/// Aims one ball: shot `shot` from the lone copy `base` (standing at the earliest moment it can
/// set off from, `now(fire)`), for a round fired on step `fire` that arrives `aim.arrive` steps
/// later. `None` when it is skipped.
pub fn aim_ball(base: &Lone, aim: &Aimer, s: &EmitterSettings, shot: Shot, fire: u64) -> Option<BallAim> {
    let earliest = now(fire);
    let arrive = fire + aim.arrive;
    let (p0, u) = shot.at(base);
    let unaimed = || {
        aim.fire_anyway.then(|| BallAim {
            leave: fire,
            velocity: u * s.speed.max(0.0),
            reaches: false,
            path: Vec::new(),
        })
    };
    let most = arrive.saturating_sub(earliest);
    if most == 0 || u == Vec2::ZERO {
        return unaimed();
    }
    let reach = (aim.target_at(arrive as f64) - p0).length().max(1.0) as f64;
    let quickest = reach / (most as f64 * aim.dt as f64);
    let prefer = if s.speed > 0.0 { s.speed as f64 } else { quickest };
    let ceiling = aim.ceiling as f64 * 0.999;
    let near = (0.3 * reach).max(40.0);
    let mut search = Search {
        lone: base,
        aim,
        p0,
        u,
        least_arc: s.aim_bounces as usize,
        prefer,
        near,
        tweak: 0.0,
        left: MOST_FLIGHTS,
    };
    // Speed first, then the scan.
    let (lo, hi) = (0.5 * quickest.min(prefer), (4.0 * quickest.max(prefer)).min(ceiling));
    let mut speeds = vec![prefer.min(ceiling)];
    if hi > lo {
        let ratio = (hi / lo).powf(1.0 / (SCAN - 1) as f64);
        speeds.extend((0..SCAN).map(|i| lo * ratio.powi(i as i32)));
    }
    let Some(mut found) = search.best(&speeds, most) else { return unaimed() };
    // When it sets off, and the way checked from then (the walls, the target or the emitter may
    // have moved by then, changing the way and so when it must set off): a secant on how late it
    // would arrive against when it sets off.
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
    search.left += SETTLE_FLIGHTS;
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
        search.left = again.left;
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
    unaimed()
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
        let balls = in_parallel(n as usize, |k| {
            let k = k as u32;
            let way = direction(s, k, n, round, seed, me.0);
            let shot = Shot::Emitter { offset: start_offset(s, k, n, way), way };
            aim_ball(&base, aim, s, shot, fire)
        });
        let arrive = fire + aim.arrive;
        Arc::new(RoundAim { round, fire, arrive, meet: aim.target_at(arrive as f64), balls })
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
