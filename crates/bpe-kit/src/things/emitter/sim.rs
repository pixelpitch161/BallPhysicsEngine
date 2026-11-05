//! The Emitter's simulation: rounds fired when their time comes, each ball launched on its own
//! step, earlier rounds kept, faded or removed, and balls caught as their centres come into the
//! catch circle (taken out, sent back out, or let through), in waves.

use std::collections::VecDeque;

use bpe_core::vary::{self, BallSpread, Vary, VaryBy};
use bpe_core::{Params, ResolvedThing, Settings, Thing, ThingId, Value};
use bpe_runtime::{Bodies, Event, EventKind, Log, Party, Pose, ThingSim, World, pop, state, state_mut};
use glam::Vec2;

use super::aim::{Aimer, Plan, now};
use super::fire::{
    direction, first_count, launch_order, layout, layout_reach, next_count, planned, slot_step, speed_factor,
    start_offset,
};
use super::foci::sits_on_pose;
use super::{Aim, CatchFrom, Catches, Earlier, EmitterSettings, FireWhen, KIND, ReturnWay};
use crate::things::balls::rows::{BallRows, Caught, rows_mut};
use crate::things::balls::sim::{resize_rows, size_factor};

pub struct EmitterSim;

/// How long after a wave's first arrival later arrivals of the same round still join it.
pub const WAVE_SECONDS: f64 = 0.5;

/// One ball still to be fired: on `step`, ball `k` of round `round`'s `n`, aimed to reach the
/// target at the moment `arrive` (in steps), or unaimed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Launch {
    pub step: u64,
    pub round: u32,
    pub k: u32,
    pub n: u32,
    pub arrive: Option<u64>,
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
    /// Its own settings that vary across the balls (Size, Speed), with a To.
    pub varies: Vec<Vary>,
    /// Each body row's own size: its Size, varied and with its Variety share, before any growth.
    pub own_size: Vec<f32>,
}

impl EmitterState {
    fn new(settings: EmitterSettings, varies: Vec<Vary>) -> Self {
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
            varies,
            own_size: Vec::new(),
        }
    }

    /// Where ball `number` of round `round`, fired the way `launch` (radians) from `place` in its
    /// round's layout, sits in the spreads, going at `speed` with `hits` so far.
    pub fn spread_at(&self, number: u32, round: u32, launch: f32, place: Vec2, speed: f32, hits: u32) -> BallSpread {
        use crate::modules::look::layers::group::ENDLESS_ROUNDS;
        use crate::things::balls::rows::RoundSpan;
        let from_first = round.saturating_sub(1) as f32;
        let round = match self.settings.round_span() {
            RoundSpan::Finite(n) if n <= 1 => 0.0,
            RoundSpan::Finite(n) => from_first / (n - 1) as f32,
            RoundSpan::Endless => {
                let x = from_first / ENDLESS_ROUNDS;
                x - x.floor()
            }
        };
        let (lo, hi) =
            self.layout.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
        let place = if hi - lo > 1e-4 { ((place.x - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
        // Every ball fired so far counts, this one too.
        let count = if self.planned > 0 { self.planned } else { (self.next_number as usize).max(number as usize + 1) };
        BallSpread {
            number,
            count: count.max(1) as u32,
            round,
            launch: vary::launch_turn(Vec2::from_angle(launch)),
            place,
            speed,
            hits,
            seed: self.settings.seed as u32,
            alone: false,
        }
    }

    /// `key`'s value for a ball at `spread`: `base`, mixed towards its To when it varies.
    fn own_value(&self, key: &str, base: f32, spread: &BallSpread) -> f32 {
        let Some(v) = self.varies.iter().find(|v| v.key == key) else { return base };
        let to = v.to.as_ref().expect("only varies with a To are kept");
        match vary::mixed(&Value::Number(base as f64), to, v.amount(spread.t(v.by, v.salt()))) {
            Value::Number(x) => x as f32,
            _ => base,
        }
    }

    /// The Size of a ball at `spread`, varied (before its Variety share).
    pub fn size_at(&self, spread: &BallSpread) -> f32 {
        self.own_value("radius", self.settings.radius, spread)
    }

    /// The Speed a ball at `spread` is fired at, varied (before its Speed spread).
    pub fn speed_at(&self, spread: &BallSpread) -> f32 {
        self.own_value("speed", self.settings.speed, spread).max(0.0)
    }

    /// The bigger end of its Size, when the Size varies across the balls (before Variety).
    pub fn biggest_size(&self) -> f32 {
        let to = self.varies.iter().find(|v| v.key == "radius").and_then(|v| match v.to {
            Some(Value::Number(x)) => Some(x as f32),
            _ => None,
        });
        to.map_or(self.settings.radius, |to| to.max(self.settings.radius))
    }

    /// Whether the Size varies across the balls.
    fn size_varies(&self) -> bool {
        self.varies.iter().any(|v| v.key == "radius")
    }

    /// Each body row's own size as its settings and spreads say now.
    fn own_sizes(&self, bodies: &Bodies) -> Vec<f32> {
        (0..bodies.len().min(self.rows.len()))
            .map(|row| {
                let size = match self.size_varies() {
                    true => self.size_at(&self.spread_of(bodies, row)),
                    false => self.settings.radius,
                };
                let factor = self.rows.size_factor[row];
                if factor == 1.0 { size } else { size * factor }
            })
            .collect()
    }

    /// Where body row `row` of `bodies` sits in the spreads, as it moves now.
    fn spread_of(&self, bodies: &Bodies, row: usize) -> BallSpread {
        let r = &self.rows;
        self.spread_at(
            r.number(row),
            r.round[row],
            r.launch[row],
            r.place[row],
            bodies.velocity.get(row).map_or(0.0, |v| v.length()),
            bodies.hits.get(row).copied().unwrap_or(0) + r.arrivals.get(row).copied().unwrap_or(0),
        )
    }

    /// How many rounds may still fire.
    pub fn rounds_left(&self) -> u32 {
        self.settings.round_limit().map_or(u32::MAX, |n| n.saturating_sub(self.rounds_fired))
    }
}

/// `thing`'s emitter state, when it is an emitter in `world`.
pub fn emitter_state(world: &World, thing: ThingId) -> Option<&EmitterState> {
    world.states.get(&thing)?.as_any().downcast_ref::<EmitterState>()
}

/// Where body row `row` of emitter `thing` sits in the spreads a setting varies by, as it moves
/// now: its number among every ball it has fired, its round across the rounds (every ten for
/// endless ones, as the group gradient), its launch angle and its place across one round.
pub fn spread_in(world: &World, thing: ThingId, row: usize) -> Option<BallSpread> {
    let st = emitter_state(world, thing)?;
    let bodies = world.bodies.get(&thing)?;
    if row >= st.rows.len() {
        return None;
    }
    Some(st.spread_of(bodies, row))
}

/// Its own settings in `params` that vary across its balls: Size and Speed.
fn own_varies(params: &Params) -> Vec<Vary> {
    vary::varies_in(params)
        .into_iter()
        .filter(|v| matches!(v.key.as_str(), "radius" | "speed") && v.to.is_some())
        .collect()
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
    /// How it aims, when it does and has something to aim at.
    aim: Option<Aimer>,
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
                // Balls of earlier rounds still waiting on their Gap never go.
                st.pending.retain(|l| l.round >= round);
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
        let step = at.step + slot_step(s, j as u32, at.rate);
        let launch = match at.aim {
            None => Launch { step, round, k, n, arrive: None },
            // Each ball leaves when its own way takes it there in time (no Gap).
            Some(aim) if aim.style == Aim::KeepMyDirections => {
                let way = direction(s, k, n, round, at.seed, at.thing.0);
                let p0 = at.pose.position + Vec2::from_angle(at.pose.angle).rotate(start_offset(s, k, n, way));
                match aim.plan_keep(p0, Vec2::from_angle(way + at.pose.angle), s.speed.max(0.0), at.step) {
                    Plan::Go { leave, arrive, .. } => Launch { step: leave, round, k, n, arrive },
                    Plan::Skip => continue,
                }
            }
            Some(aim) => Launch { step, round, k, n, arrive: Some(at.step + aim.arrive) },
        };
        st.pending.push_back(launch);
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
    let first = (s.start_delay.max(0.0) as f64 * at.rate as f64).round() as u64;
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

/// The velocity an aimed ball leaving `position` the way `u` on step `leave` goes at to reach the
/// target at the moment `arrive`; `None` when it can't (Can't reach then says what happens).
fn aimed(aim: &Aimer, s: &EmitterSettings, position: Vec2, u: Vec2, leave: u64, arrive: u64) -> Option<Vec2> {
    match aim.style {
        Aim::Off => None,
        Aim::StraightIn => aim.straight(position, leave, arrive),
        Aim::KeepMyDirections => aim.keep_velocity(position, u, s.speed.max(0.0), leave, arrive),
    }
}

/// Fires the ball `launch` from where the emitter is, aimed when it is to be; false when it
/// doesn't go (it can't reach and is skipped).
fn launch_ball(st: &mut EmitterState, bodies: &mut Bodies, launch: Launch, at: &Moment, log: Option<&mut Log>) -> bool {
    let s = &st.settings;
    let Launch { round, k, n, .. } = launch;
    let way = direction(s, k, n, round, at.seed, at.thing.0);
    let offset = start_offset(s, k, n, way);
    let turn = Vec2::from_angle(at.pose.angle);
    let position = at.pose.position + turn.rotate(offset);
    let mut dir = way + at.pose.angle;
    let number = st.next_number;
    let place = offset + Vec2::from_angle(way) * layout_reach(s);
    let (velocity, speed) = match (at.aim, launch.arrive) {
        (Some(aim), Some(arrive)) => {
            let plain = Vec2::from_angle(dir) * s.speed.max(0.0);
            let v = match aimed(&aim, s, position, Vec2::from_angle(dir), at.step, arrive) {
                Some(v) => v,
                None if aim.fire_anyway => plain,
                None => return false,
            };
            if v.length() > 0.0 {
                // Its launch angle is the way it really leaves.
                dir = v.y.atan2(v.x);
            }
            (v, v.length())
        }
        // An aimed emitter's ball that goes unaimed (Fire anyway) goes at Speed, unspread.
        (Some(_), None) => (Vec2::from_angle(dir) * s.speed.max(0.0), s.speed.max(0.0)),
        _ => {
            let own = st.speed_at(&st.spread_at(number, round, dir, place, 0.0, 0));
            let speed = own * speed_factor(s, round, k, at.seed, at.thing.0);
            (Vec2::from_angle(dir) * speed, speed)
        }
    };
    let factor = size_factor(&s.variety(), number);
    let size = st.size_at(&st.spread_at(number, round, dir, place, speed, 0));
    let radius = if factor == 1.0 { size } else { size * factor };
    st.next_number += 1;
    let row = bodies.push(position, velocity, radius) as usize;
    bodies.number[row] = number;
    st.rows.push(number, factor, round, dir, place, k);
    st.own_size.push(radius);
    // A ball of an earlier round launched after a later round fired fades with its round.
    if s.earlier == Earlier::FadeOut
        && round < st.rounds_fired
        && let Some(later) = st.fired.iter().find(|f| f.round == round + 1)
    {
        let row = st.rows.len() - 1;
        st.rows.fade_from[row] = Some(later.step);
    }
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
    true
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
        if launch_ball(st, bodies, launch, at, log.as_deref_mut()) {
            live += 1;
        }
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
    let aim = if s.return_way == ReturnWay::OutMyWay && s.catches == Catches::SendBackOut {
        Aimer::of(world, me, &s, step)
    } else {
        None
    };
    let at = Moment { step, rate, seed: world.seed, thing: me, pose, aim };
    let delay = (s.return_delay.max(0.0) as f64 * rate as f64).round() as u64;
    for a in found {
        // A caught ball falls by its own thing's Motion card, not the catcher's.
        let aim = at.aim.map(|aim| Aimer { accel: super::aim::pull_in(world, a.thing).accel, ..aim });
        let (round, size) = round_of(world, a.thing, a.row);
        // One of its own balls goes back out at its own Speed when the Speed varies.
        let own_speed = match a.thing == me {
            true => emitter_state(world, me).zip(spread_in(world, me, a.row)).map(|(st, at)| st.speed_at(&at)),
            false => None,
        }
        .unwrap_or(s.speed.max(0.0));
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
            Catches::Remove => {
                pop(bodies, a.row, a.thing, step, log);
                // Who caught it and when, for its trail to stay behind (Leave trails).
                if let Some(rows) = rows_mut(world, a.thing)
                    && let Some(c) = rows.caught.get_mut(a.row)
                {
                    *c = Some(Caught { by: me, step });
                }
            }
            Catches::SendBackOut => {
                // Where it goes from, how, and the step it leaves (held still until then).
                let mut leave = if delay == 0 { step + 1 } else { step + delay };
                let (place, velocity) = match s.return_way {
                    ReturnWay::SameWayBack => (a.point, -bodies.velocity[a.row]),
                    ReturnWay::OutMyWay => {
                        let n = size.max(k + 1);
                        let way = direction(&s, k, n, round.max(1), at.seed, me.0);
                        let offset = Vec2::from_angle(pose.angle).rotate(start_offset(&s, k, n, way));
                        let place = pose.position + offset;
                        let u = Vec2::from_angle(way + pose.angle);
                        match aim {
                            None => {
                                let speed = own_speed * speed_factor(&s, round.max(1), k, at.seed, me.0);
                                (place, u * speed)
                            }
                            // Aimed as a round of its own fired when it would leave.
                            Some(aim) => match send_aimed(&aim, &s, place, u, leave) {
                                Some((go, v)) => {
                                    leave = go;
                                    (place, v)
                                }
                                None if aim.fire_anyway => (place, u * s.speed.max(0.0)),
                                None => {
                                    pop(bodies, a.row, a.thing, step, log);
                                    continue;
                                }
                            },
                        }
                    }
                };
                bodies.position[a.row] = place;
                // It has gone nowhere this step as far as any other catcher can tell, so a second
                // catcher over this one doesn't catch it again.
                bodies.previous[a.row] = place;
                if delay == 0 && leave <= step + 1 {
                    bodies.velocity[a.row] = velocity;
                } else {
                    bodies.velocity[a.row] = Vec2::ZERO;
                    bodies.held[a.row] = true;
                    let st = state_mut::<EmitterState>(world.states.get_mut(&me).expect("an emitter").as_mut());
                    st.held.push(Held { thing: a.thing, row: a.row as u32, release: leave, velocity });
                }
            }
        }
    }
}

/// A caught ball sent back out aimed, as a round of its own fired on step `fire` (when it would
/// leave): the step it leaves and its velocity, `None` when it can't reach.
fn send_aimed(aim: &Aimer, s: &EmitterSettings, place: Vec2, u: Vec2, fire: u64) -> Option<(u64, Vec2)> {
    let arrive = fire + aim.arrive;
    match aim.style {
        Aim::Off => None,
        Aim::StraightIn => aim.straight(place, fire, arrive).map(|v| (fire, v)),
        Aim::KeepMyDirections => match aim.plan_keep(place, u, s.speed.max(0.0), fire) {
            Plan::Go { leave, arrive: Some(arrive), .. } => {
                aim.keep_velocity(place, u, s.speed.max(0.0), leave, arrive).map(|v| (leave, v))
            }
            _ => None,
        },
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
        let aims = settings.aim != Aim::Off;
        let mut st = EmitterState::new(settings, own_varies(&resolved.params));
        let mut bodies = Bodies::default();
        // A round at 0 s is there from the first frame (no step logs it). An aimed one waits for
        // everything to be built (its target, its Motion card): `after_build`.
        if !aims {
            let at =
                Moment { step: 0, rate: world.steps_per_second, seed: world.seed, thing: thing.id, pose, aim: None };
            fire_and_launch(&mut st, &mut bodies, &at, 0, None);
        }
        world.bodies.insert(thing.id, bodies);
        world.states.insert(thing.id, Box::new(st));
    }

    /// An aimed round at 0 s fires once everything is built.
    fn after_build(&self, thing: ThingId, world: &mut World) {
        let Some(st) = emitter_state(world, thing) else { return };
        if st.settings.aim == Aim::Off {
            return;
        }
        let aim = Aimer::of(world, thing, &st.settings, 0);
        let pose = world.poses.get(&thing).copied().unwrap_or_default();
        let at = Moment { step: 0, rate: world.steps_per_second, seed: world.seed, thing, pose, aim };
        let (Some(st), Some(bodies)) = (world.states.get_mut(&thing), world.bodies.get_mut(&thing)) else { return };
        fire_and_launch(state_mut::<EmitterState>(st.as_mut()), bodies, &at, 0, None);
        // The balls there from the start were there at step 0.
        let rows = bodies.len();
        if let Some(history) = world.history.get_mut(&thing) {
            history.note(0, rows);
        }
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
        let aim = Aimer::of(world, thing, &st.settings, now(world.step));
        let at = Moment { step: world.step, rate: world.steps_per_second, seed: world.seed, thing, pose, aim };
        let (Some(st), Some(bodies)) = (world.states.get_mut(&thing), world.bodies.get_mut(&thing)) else { return };
        fire_and_launch(state_mut::<EmitterState>(st.as_mut()), bodies, &at, landed, Some(log));
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        let Some(st) = world.states.get(&thing) else { return };
        let st = state::<EmitterState>(st.as_ref());
        // An endless emitter's Number spread runs over the balls fired so far, so it moves too.
        let moves = |by: VaryBy| by.changes() || (by == VaryBy::Number && st.planned == 0);
        let follows = st.varies.iter().any(|v| v.key == "radius" && moves(v.by));
        let pose = pose_now(world, thing, &st.settings);
        world.poses.insert(thing, pose);
        // A Size that varies by the balls' speed or hits (or an endless Number) follows them every step.
        if follows && let (Some(st), Some(bodies)) = (world.states.get_mut(&thing), world.bodies.get_mut(&thing)) {
            let st = state_mut::<EmitterState>(st.as_mut());
            let own = st.own_sizes(bodies);
            resize_rows(&mut st.own_size, bodies, own);
        }
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
            let memory =
                st.rows.memory() + st.landings.capacity() * 24 + st.fired.capacity() * 24 + st.own_size.capacity() * 4;
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
        let varies = own_varies(&resolved.params);
        let resize =
            st.settings.radius != settings.radius || st.size_varies() || varies.iter().any(|v| v.key == "radius");
        st.layout = layout(&settings);
        st.planned = planned(&settings);
        st.settings = settings;
        st.varies = varies;
        // Each ball takes its own size from the new settings, keeping what it has grown by.
        if resize && let Some(bodies) = world.bodies.get_mut(&thing) {
            let own = st.own_sizes(bodies);
            resize_rows(&mut st.own_size, bodies, own);
        }
    }

    fn ball_spread(&self, thing: ThingId, world: &World, row: usize) -> Option<bpe_core::BallSpread> {
        spread_in(world, thing, row)
    }

    /// The marker, the look-only variety and Leave trails change only how it looks.
    fn affects_physics(&self, key: &str) -> bool {
        !(key.starts_with("marker")
            || key.starts_with("flash_")
            || key.starts_with("ripple")
            || matches!(key, "vary_colour" | "vary_turn" | "leave_trails"))
    }
}

#[cfg(test)]
#[path = "sim_tests.rs"]
mod tests;
