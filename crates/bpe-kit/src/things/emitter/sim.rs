//! The Emitter's simulation: rounds fired when their time comes, each ball launched on its own
//! step, earlier rounds kept, faded or removed, and balls caught as their centres come into the
//! catch circle (taken out, sent back out, or let through), in waves.

use std::collections::VecDeque;

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{Bodies, Event, EventKind, Log, Party, Pose, ThingSim, World, pop, state, state_mut};
use glam::Vec2;

use super::fire::{
    direction, first_count, launch_order, layout, layout_reach, next_count, planned, slot_step, speed_factor,
    start_offset,
};
use super::foci::sits_on_pose;
use super::{CatchFrom, Catches, Earlier, EmitterSettings, FireWhen, KIND, ReturnWay};
use crate::things::balls::rows::{BallRows, rows_mut};
use crate::things::balls::sim::size_factor;

pub struct EmitterSim;

/// How long after a wave's first arrival later arrivals of the same round still join it.
pub const WAVE_SECONDS: f64 = 0.5;

/// One ball still to be fired: on `step`, ball `k` of round `round`'s `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Launch {
    pub step: u64,
    pub round: u32,
    pub k: u32,
    pub n: u32,
}

/// A round fired: when, which, how many balls it has and how many have gone so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fired {
    pub step: u64,
    pub round: u32,
    pub size: u32,
    pub launched: u32,
}

/// A caught ball this emitter holds still until it sends it back out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Held {
    pub thing: ThingId,
    pub row: u32,
    pub release: u64,
    pub velocity: Vec2,
}

/// The arrivals at this emitter of one round's balls (a Balls thing's are round 0), from the
/// first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wave {
    pub owner: ThingId,
    pub round: u32,
    pub first: u64,
    pub arrivals: u32,
}

/// A wave's first arrival: when it landed and whose round it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Landing {
    pub step: u64,
    pub owner: ThingId,
    pub round: u32,
}

/// What an emitter keeps between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct EmitterState {
    pub settings: EmitterSettings,
    /// Its own balls' rows.
    pub rows: BallRows,
    /// One round's places, for the group gradient.
    pub layout: Vec<Vec2>,
    /// How many balls the group counts for the gradient's Order (0: those fired so far).
    pub planned: usize,
    pub rounds_fired: u32,
    /// The size of the next round.
    pub next_count: u32,
    /// The next ball's number.
    pub next_number: u32,
    /// The step the next round of Every goes on.
    pub next_every: Option<u64>,
    /// The balls still to be fired, soonest first (rounds in the order they fired on a tie).
    pub pending: VecDeque<Launch>,
    /// Every round fired so far, in order.
    pub fired: Vec<Fired>,
    /// The caught balls it holds.
    pub held: Vec<Held>,
    /// The waves still open to later arrivals.
    pub waves: Vec<Wave>,
    /// Every wave that has landed on it so far, in order.
    pub landings: Vec<Landing>,
}

impl EmitterState {
    fn new(settings: EmitterSettings) -> Self {
        Self {
            layout: layout(&settings),
            planned: planned(&settings),
            next_count: first_count(&settings),
            settings,
            rows: BallRows::default(),
            rounds_fired: 0,
            next_number: 0,
            next_every: None,
            pending: VecDeque::new(),
            fired: Vec::new(),
            held: Vec::new(),
            waves: Vec::new(),
            landings: Vec::new(),
        }
    }

    /// How many rounds may still fire.
    fn rounds_left(&self) -> u32 {
        self.settings.round_limit().map_or(u32::MAX, |n| n.saturating_sub(self.rounds_fired))
    }
}

/// `thing`'s emitter state, when it is an emitter in `world`.
pub fn emitter_state(world: &World, thing: ThingId) -> Option<&EmitterState> {
    world.states.get(&thing)?.as_any().downcast_ref::<EmitterState>()
}

/// One round on the timeline: when it fires (seconds), which it is, how many balls it has, and
/// whether it has fired yet or is still to come.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundMark {
    pub time: f64,
    pub round: u32,
    pub size: u32,
    pub fired: bool,
}

/// Emitter `thing`'s rounds up to `until` seconds: those fired so far in `world`, then those still
/// to come when they come at known times (At start's one, Every's), none for a thing that isn't an
/// emitter.
pub fn round_marks(world: &World, thing: ThingId, until: f64) -> Vec<RoundMark> {
    let Some(st) = emitter_state(world, thing) else { return Vec::new() };
    let rate = world.steps_per_second as f64;
    let s = &st.settings;
    let mut out: Vec<RoundMark> = st
        .fired
        .iter()
        .map(|f| RoundMark { time: f.step as f64 / rate, round: f.round, size: f.size, fired: true })
        .collect();
    if !s.emits {
        return out;
    }
    let mut left = st.rounds_left();
    let mut round = st.rounds_fired + 1;
    let mut size = if st.rounds_fired == 0 { first_count(s) } else { st.next_count };
    let every = s.every.max(0.0) as f64;
    let mut at = match (st.rounds_fired, s.fire_when) {
        (0, FireWhen::WhenAWaveLands) if !s.wave_first => return out,
        (0, _) => s.start_delay.max(0.0) as f64,
        (_, FireWhen::Every) => st.next_every.map_or(f64::INFINITY, |n| n as f64 / rate),
        _ => return out,
    };
    // Every's rounds can be many: no more than the strip can show.
    while left > 0 && at <= until && out.len() < 1000 {
        out.push(RoundMark { time: at, round, size, fired: false });
        left -= 1;
        round += 1;
        size = next_count(s, size);
        if s.fire_when != FireWhen::Every || every <= 0.0 {
            break;
        }
        at += every;
    }
    out
}

/// Where the emitter is this step: on its focus when it sits on one, else where its transform (or
/// what it rides) puts it.
fn pose_now(world: &World, thing: ThingId, s: &EmitterSettings) -> Pose {
    let own = world.anchor(thing);
    match sits_on_pose(world, s) {
        Some(on) => Pose { position: on.position, angle: own.angle, ..Pose::default() },
        None => own,
    }
}

/// Everything a launch or a re-fire reads about the moment.
struct Moment {
    step: u64,
    rate: u32,
    seed: u64,
    thing: ThingId,
    pose: Pose,
}

/// Fires round `rounds_fired + 1`: the earlier rounds' balls are kept, faded or removed, and its
/// balls are queued on their steps.
fn fire_round(st: &mut EmitterState, bodies: &mut Bodies, at: &Moment, log: Option<&mut Log>) {
    let round = st.rounds_fired + 1;
    let s = &st.settings;
    let n = if round == 1 { first_count(s) } else { st.next_count };
    if round > 1 {
        match s.earlier {
            Earlier::Keep => {}
            Earlier::FadeOut => {
                for row in 0..st.rows.len() {
                    if st.rows.round[row] < round && bodies.alive[row] && st.rows.fade_from[row].is_none() {
                        st.rows.fade_from[row] = Some(at.step);
                    }
                }
            }
            Earlier::Remove => {
                if let Some(log) = log {
                    for row in 0..st.rows.len() {
                        if st.rows.round[row] < round && bodies.alive[row] {
                            pop(bodies, row, at.thing, at.step, log);
                        }
                    }
                } else {
                    // Only at the start, when nothing came before.
                }
            }
        }
    }
    for (j, k) in launch_order(s, n, round, at.seed, at.thing.0).into_iter().enumerate() {
        st.pending.push_back(Launch { step: at.step + slot_step(s, j as u32, at.rate), round, k, n });
    }
    // Stable: launches on the same step keep the order they were queued in.
    st.pending.make_contiguous().sort_by_key(|l| l.step);
    st.fired.push(Fired { step: at.step, round, size: n, launched: 0 });
    st.rounds_fired = round;
    st.next_count = next_count(s, n);
    if s.fire_when == FireWhen::Every {
        let every = ((s.every.max(0.0) as f64 * at.rate as f64).round() as u64).max(1);
        st.next_every = Some(at.step + every);
    }
}

/// How many rounds are due on `at.step`, `landed` waves having landed on the emitters it listens
/// to the step before.
fn rounds_due(st: &EmitterState, at: &Moment, landed: u32) -> u32 {
    let s = &st.settings;
    if !s.emits {
        return 0;
    }
    let mut due = 0u32;
    let first = ((s.start_delay.max(0.0) as f64 * at.rate as f64).round() as u64).max(0);
    let starts = s.fire_when != FireWhen::WhenAWaveLands || s.wave_first;
    if st.rounds_fired == 0 && starts && at.step >= first {
        due += 1;
    }
    match s.fire_when {
        FireWhen::AtStart => {}
        FireWhen::Every => {
            if st.rounds_fired > 0 && st.next_every.is_some_and(|n| n <= at.step) {
                due += 1;
            }
        }
        FireWhen::WhenAWaveLands => due += landed,
    }
    due.min(st.rounds_left())
}

/// Fires the ball `launch` from where the emitter is, unless Most balls are out already.
fn launch_ball(st: &mut EmitterState, bodies: &mut Bodies, launch: Launch, at: &Moment, log: Option<&mut Log>) {
    let s = &st.settings;
    let Launch { round, k, n, .. } = launch;
    let way = direction(s, k, n, round, at.seed, at.thing.0);
    let offset = start_offset(s, k, n, way);
    let turn = Vec2::from_angle(at.pose.angle);
    let position = at.pose.position + turn.rotate(offset);
    let dir = way + at.pose.angle;
    let speed = s.speed.max(0.0) * speed_factor(s, round, k, at.seed, at.thing.0);
    let velocity = Vec2::from_angle(dir) * speed;
    let number = st.next_number;
    st.next_number += 1;
    let factor = size_factor(&s.variety(), number);
    let radius = if factor == 1.0 { s.radius } else { s.radius * factor };
    let row = bodies.push(position, velocity, radius) as usize;
    bodies.number[row] = number;
    let place = offset + Vec2::from_angle(way) * layout_reach(s);
    st.rows.push(number, factor, round, dir, place, k);
    if let Some(f) = st.fired.iter_mut().rev().find(|f| f.round == round) {
        f.launched += 1;
    }
    if let Some(log) = log {
        log.push(Event {
            step: at.step,
            kind: EventKind::Spawn,
            a: Party { thing: at.thing, item: row as u32 },
            b: Some(Party { thing: at.thing, item: round }),
            point: position,
            normal: Vec2::from_angle(dir),
            speed,
            at: 0.0,
        });
    }
}

/// Fires the rounds due and launches the balls whose step has come.
fn fire_and_launch(st: &mut EmitterState, bodies: &mut Bodies, at: &Moment, landed: u32, mut log: Option<&mut Log>) {
    for _ in 0..rounds_due(st, at, landed) {
        fire_round(st, bodies, at, log.as_deref_mut());
    }
    if st.pending.front().is_none_or(|l| l.step > at.step) {
        return;
    }
    let mut live = bodies.alive_count();
    let most = st.settings.most_balls.max(1) as usize;
    while let Some(&launch) = st.pending.front() {
        if launch.step > at.step {
            break;
        }
        st.pending.pop_front();
        // Past Most balls the round is cut short.
        if live >= most {
            continue;
        }
        launch_ball(st, bodies, launch, at, log.as_deref_mut());
        live += 1;
    }
}

/// How many waves landed on the step before on the emitters `me` fires on.
fn landed_before(world: &World, me: ThingId, s: &EmitterSettings) -> u32 {
    if s.fire_when != FireWhen::WhenAWaveLands || world.step == 0 {
        return 0;
    }
    let before = world.step - 1;
    let mut landed = 0;
    for id in world.matching(&s.wave_from, me) {
        if world.kind(id) != Some(KIND) {
            continue;
        }
        let Some(other) = emitter_state(world, id) else { continue };
        landed += other.landings.iter().rev().take_while(|l| l.step >= before).filter(|l| l.step == before).count();
    }
    landed as u32
}

/// One ball that came into the catch circle this step.
struct Arrival {
    thing: ThingId,
    row: usize,
    point: Vec2,
    speed: f32,
}

/// Where the segment `a` → `b` first comes within `r` of `c`, when it does and `a` is outside.
fn entry(a: Vec2, b: Vec2, c: Vec2, r: f32) -> Option<Vec2> {
    let r2 = r * r;
    if (a - c).length_squared() <= r2 {
        return None;
    }
    let d = b - a;
    let f = a - c;
    let dd = d.length_squared();
    if dd <= 1e-12 {
        return None;
    }
    // |f + t d|² = r²: dd t² + 2 (f·d) t + |f|² − r² = 0, the smaller root in [0, 1].
    let half_b = f.dot(d);
    let disc = half_b * half_b - dd * (f.length_squared() - r2);
    if disc < 0.0 {
        return None;
    }
    let t = (-half_b - disc.sqrt()) / dd;
    (0.0..=1.0).contains(&t).then(|| a + d * t)
}

/// The balls that came into `me`'s catch circle this step, in thing then row order.
fn arrivals(world: &World, me: ThingId, s: &EmitterSettings, centre: Vec2) -> Vec<Arrival> {
    let r = s.catch_size.max(0.0);
    let mut out = Vec::new();
    for (&thing, bodies) in &world.bodies {
        let caught = match s.catch_from {
            CatchFrom::AnyBalls => true,
            CatchFrom::ItsOwn => thing == me,
            CatchFrom::Chosen => world.names(&s.catch_things, me, thing),
        };
        if !caught {
            continue;
        }
        for row in 0..bodies.len() {
            if !bodies.alive[row] || bodies.held[row] {
                continue;
            }
            if let Some(point) = entry(bodies.previous[row], bodies.position[row], centre, r) {
                out.push(Arrival { thing, row, point, speed: bodies.velocity[row].length() });
            }
        }
    }
    out
}

/// The round ball `row` of `thing` was fired in (0 for a ball that isn't an emitter's), and how
/// many balls that round (or that thing) has.
fn round_of(world: &World, thing: ThingId, row: usize) -> (u32, u32) {
    if let Some(st) = emitter_state(world, thing) {
        let round = st.rows.round.get(row).copied().unwrap_or(0);
        let size = st.fired.iter().rev().find(|f| f.round == round).map_or(1, |f| f.size);
        return (round, size);
    }
    (0, world.bodies.get(&thing).map_or(1, |b| b.len() as u32))
}

/// Catches what came into `me`'s circle this step.
fn catch(me: ThingId, world: &mut World, log: &mut Log) {
    let Some(st) = emitter_state(world, me) else { return };
    let s = st.settings.clone();
    if s.catches == Catches::Nothing {
        return;
    }
    let pose = world.poses.get(&me).copied().unwrap_or_default();
    let found = arrivals(world, me, &s, pose.position);
    if found.is_empty() {
        return;
    }
    let step = world.step;
    let rate = world.steps_per_second;
    let wave_steps = (WAVE_SECONDS * rate as f64).round() as u64;
    let at = Moment { step, rate, seed: world.seed, thing: me, pose };
    for a in found {
        let (round, size) = round_of(world, a.thing, a.row);
        log.push(Event {
            step,
            kind: EventKind::Enter,
            a: Party { thing: a.thing, item: a.row as u32 },
            b: Some(Party { thing: me, item: round }),
            point: a.point,
            normal: Vec2::ZERO,
            speed: a.speed,
            at: 0.0,
        });
        // Its wave: joining one of the same round's that landed less than half a second ago, or
        // landing a new one.
        let k = {
            let st = state_mut::<EmitterState>(world.states.get_mut(&me).expect("an emitter").as_mut());
            st.waves.retain(|w| step - w.first <= wave_steps);
            match st.waves.iter_mut().find(|w| w.owner == a.thing && w.round == round) {
                Some(w) => {
                    w.arrivals += 1;
                    w.arrivals - 1
                }
                None => {
                    st.waves.push(Wave { owner: a.thing, round, first: step, arrivals: 1 });
                    st.landings.push(Landing { step, owner: a.thing, round });
                    0
                }
            }
        };
        if let Some(rows) = rows_mut(world, a.thing)
            && let Some(n) = rows.arrivals.get_mut(a.row)
        {
            *n += 1;
        }
        let Some(bodies) = world.bodies.get_mut(&a.thing) else { continue };
        match s.catches {
            Catches::Nothing | Catches::PassThrough => {}
            Catches::Remove => pop(bodies, a.row, a.thing, step, log),
            Catches::SendBackOut => {
                let (place, velocity) = match s.return_way {
                    ReturnWay::SameWayBack => (a.point, -bodies.velocity[a.row]),
                    ReturnWay::OutMyWay => {
                        let n = size.max(k + 1);
                        let way = direction(&s, k, n, round.max(1), at.seed, me.0);
                        let offset = Vec2::from_angle(pose.angle).rotate(start_offset(&s, k, n, way));
                        let speed = s.speed.max(0.0) * speed_factor(&s, round.max(1), k, at.seed, me.0);
                        (pose.position + offset, Vec2::from_angle(way + pose.angle) * speed)
                    }
                };
                bodies.position[a.row] = place;
                let delay = (s.return_delay.max(0.0) as f64 * rate as f64).round() as u64;
                if delay == 0 {
                    bodies.velocity[a.row] = velocity;
                } else {
                    bodies.velocity[a.row] = Vec2::ZERO;
                    bodies.held[a.row] = true;
                    let st = state_mut::<EmitterState>(world.states.get_mut(&me).expect("an emitter").as_mut());
                    st.held.push(Held { thing: a.thing, row: a.row as u32, release: step + delay, velocity });
                }
            }
        }
    }
}

/// Lets go the balls it holds whose time has come.
fn release(me: ThingId, world: &mut World) {
    let step = world.step;
    let Some(st) = world.states.get_mut(&me) else { return };
    let st = state_mut::<EmitterState>(st.as_mut());
    if st.held.iter().all(|h| h.release > step) {
        return;
    }
    let (due, kept): (Vec<Held>, Vec<Held>) = st.held.drain(..).partition(|h| h.release <= step);
    st.held = kept;
    for h in due {
        if let Some(bodies) = world.bodies.get_mut(&h.thing)
            && bodies.alive.get(h.row as usize).copied().unwrap_or(false)
        {
            bodies.held[h.row as usize] = false;
            bodies.velocity[h.row as usize] = h.velocity;
        }
    }
}

impl ThingSim for EmitterSim {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = EmitterSettings::from_params(&resolved.params);
        let pose = pose_now(world, thing.id, &settings);
        world.poses.insert(thing.id, pose);
        let mut st = EmitterState::new(settings);
        let mut bodies = Bodies::default();
        // A round at 0 s is there from the first frame (no step logs it).
        let at = Moment { step: 0, rate: world.steps_per_second, seed: world.seed, thing: thing.id, pose };
        fire_and_launch(&mut st, &mut bodies, &at, 0, None);
        world.bodies.insert(thing.id, bodies);
        world.states.insert(thing.id, Box::new(st));
    }

    /// Built (and stepped) after the shapes, so it can sit on one as it moves.
    fn needs_walls(&self) -> bool {
        true
    }

    fn history(&self, resolved: &ResolvedThing) -> Option<bpe_runtime::HistoryWant> {
        crate::modules::look::layers::around_history(resolved)
    }

    fn spawn(&self, thing: ThingId, world: &mut World, log: &mut Log) {
        release(thing, world);
        let Some(st) = world.states.get(&thing) else { return };
        let st = state::<EmitterState>(st.as_ref());
        let landed = landed_before(world, thing, &st.settings);
        let pose = world.poses.get(&thing).copied().unwrap_or_default();
        let at = Moment { step: world.step, rate: world.steps_per_second, seed: world.seed, thing, pose };
        let (Some(st), Some(bodies)) = (world.states.get_mut(&thing), world.bodies.get_mut(&thing)) else { return };
        fire_and_launch(state_mut::<EmitterState>(st.as_mut()), bodies, &at, landed, Some(log));
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        let Some(st) = world.states.get(&thing) else { return };
        let pose = pose_now(world, thing, &state::<EmitterState>(st.as_ref()).settings);
        world.poses.insert(thing, pose);
    }

    fn after_step(&self, thing: ThingId, world: &mut World, log: &mut Log) {
        let step = world.step;
        let rate = world.steps_per_second;
        // Faded rounds go once their fade is over.
        if let (Some(st), Some(bodies)) = (world.states.get_mut(&thing), world.bodies.get_mut(&thing)) {
            let st = state_mut::<EmitterState>(st.as_mut());
            let fade = st.settings.fade_steps(rate);
            if fade > 0 {
                for row in 0..st.rows.len() {
                    if bodies.alive[row] && st.rows.fade_from[row].is_some_and(|f| step >= f + fade) {
                        pop(bodies, row, thing, step, log);
                    }
                }
            }
            let memory = st.rows.memory() + st.landings.capacity() * 24 + st.fired.capacity() * 24;
            world.state_memory.insert(thing, memory);
        }
        catch(thing, world, log);
    }

    /// A key moved one of its settings: the next launches and rounds read it. A new Size resizes
    /// the balls as a Balls thing's do.
    fn settings_changed(&self, thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let settings = EmitterSettings::from_params(&resolved.params);
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<EmitterState>(st.as_mut());
        let (old, new) = (st.settings.radius, settings.radius);
        if new != old
            && let Some(bodies) = world.bodies.get_mut(&thing)
        {
            let sized = |size: f32, factor: f32| if factor == 1.0 { size } else { size * factor };
            for (row, r) in bodies.radius.iter_mut().enumerate() {
                let factor = st.rows.size_factor.get(row).copied().unwrap_or(1.0);
                *r = if *r == sized(old, factor) || old <= 0.0 { sized(new, factor) } else { *r * (new / old) };
            }
        }
        st.settings = settings;
    }

    /// The marker and the look-only variety change only how it looks.
    fn affects_physics(&self, key: &str) -> bool {
        !(key.starts_with("marker")
            || key.starts_with("flash_")
            || key.starts_with("ripple")
            || matches!(key, "vary_colour" | "vary_turn"))
    }
}

#[cfg(test)]
#[path = "sim_tests.rs"]
mod tests;
