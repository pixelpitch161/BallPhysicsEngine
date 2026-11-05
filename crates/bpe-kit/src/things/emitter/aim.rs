//! Arrive together: aiming a round's balls so they all reach a target a set time after the round
//! fires. Straight in works out each ball's velocity; Keep my directions keeps each ball's own
//! direction and works out how fast it goes and when it leaves.
//!
//! The maths is exact in the world's own steps, not in continuous time: the world moves a ball
//! `v += a·dt` then `x += v·dt` each step (one substep a step; steps split only when some ball in
//! the scene goes faster than 3,840 px/s, and then a ball under a pull lands up to `a·dt·t/2` off),
//! so after `j` steps a ball is at `p0 + j·dt·v0 + a·dt²·j(j+1)/2`, and inside a step it moves in a
//! straight line. A ball launched in the Spawn stage of step `L` first moves in step `L`: it sets
//! off from the moment [`now`]`(L)`.

use bpe_core::{SlotId, ThingId};
use bpe_runtime::World;
use glam::Vec2;

use super::fire::{direction, first_count, launch_order, slot_step, start_offset};
use super::foci::index;
use super::sim::emitter_state;
use super::{Aim, CantReach, EmitterSettings, KIND};
use crate::modules::bounce::sim::BounceState;
use crate::modules::motion::sim::MotionState;
use crate::things::shape::{self, sim::ShapeState};

/// The most steps an aimed ball may fly: two minutes at 240 a second.
const MOST_STEPS: f64 = 120.0 * 240.0;
/// How far off the line through the target a direction may be and still count as through it,
/// when nothing pulls the ball across its way.
const THROUGH: f64 = 0.5;

/// The moment (in steps) a ball launched on step `step` sets off from, and the moment the world's
/// poses and walls stand at during that step's Spawn stage: the end of the step before (a launch
/// while building, step 0, sets off from 0 too).
pub fn now(step: u64) -> u64 {
    step.max(1) - 1
}

/// What moves an emitter's balls in flight: its steady pull (the scene's gravity × Gravity ×, plus
/// Wind, pixels per second²), its air drag, and whether its Motion card varies across the balls.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pull {
    pub accel: Vec2,
    pub drag: f32,
    pub varies: bool,
}

/// The pull on `thing`'s balls as the world has its Motion card now.
pub fn pull_in(world: &World, thing: ThingId) -> Pull {
    let Some(card) = world.card(thing, SlotId::Motion) else { return Pull::default() };
    let Some(m) = card.state.as_any().downcast_ref::<MotionState>() else {
        return Pull { varies: card.varies, ..Pull::default() };
    };
    let m = &m.settings;
    Pull { accel: world.gravity * m.gravity_scale + m.wind, drag: m.drag, varies: card.varies }
}

/// Whether `thing`'s balls bump into other balls (its Bounce card's Hit other balls).
fn hits_balls(world: &World, thing: ThingId) -> bool {
    world
        .card(thing, SlotId::Bounce)
        .and_then(|c| c.state.as_any().downcast_ref::<BounceState>())
        .is_some_and(|b| b.settings.hit_balls)
}

/// A focus of a shape as the shape's own Spin and Shrink will carry it: its middle goes on at its
/// velocity, the focus turns round it at the wall's spin and comes in as the shape shrinks (each
/// step by `keep`, down to `floor` of its start size).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusTrack {
    pub centre: Vec2,
    /// Pixels per second.
    pub velocity: Vec2,
    /// From the middle to the focus, now.
    pub offset: Vec2,
    /// Radians per second, clockwise.
    pub spin: f32,
    /// The shape's size now, as a fraction of its start size.
    pub scale: f32,
    /// What its size is multiplied by each step while it shrinks (1: it doesn't).
    pub keep: f32,
    pub floor: f32,
}

impl FocusTrack {
    /// Where the focus is `ahead` steps from now, at `dt` seconds a step.
    pub fn at(&self, ahead: f64, dt: f32) -> Vec2 {
        let t = (ahead * dt as f64) as f32;
        let size = if self.keep < 1.0 && self.scale > self.floor && ahead > 0.0 {
            (self.scale * self.keep.max(0.0).powf(ahead as f32)).max(self.floor)
        } else {
            self.scale
        };
        let shrunk = if self.scale > 0.0 { size / self.scale } else { 1.0 };
        self.centre + self.velocity * t + Vec2::from_angle(self.spin * t).rotate(self.offset * shrunk)
    }
}

/// Focus `which` (0 or 1) of shape `shape` as it will move, when it is a shape with foci.
pub fn focus_track(world: &World, shape: ThingId, which: usize) -> Option<FocusTrack> {
    let foci = shape::foci(world, shape)?;
    let wall = world.walls.get(&shape)?;
    let st = world.states.get(&shape)?.as_any().downcast_ref::<ShapeState>()?;
    let centre = wall.outline.center();
    let shrink = st.settings.shrink.max(0.0);
    Some(FocusTrack {
        centre,
        velocity: wall.velocity,
        offset: foci[which.min(1)] - centre,
        spin: wall.spin,
        scale: st.scale,
        keep: 1.0 - shrink * world.dt(),
        floor: st.floor(),
    })
}

/// Where an aim's target goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Track {
    Still(Vec2),
    /// Carried on at a steady velocity (pixels per second).
    Moving {
        at: Vec2,
        velocity: Vec2,
    },
    Focus(FocusTrack),
    /// A ball: carried on by its velocity and pulled by its own steady pull, as the world moves it
    /// (until it bounces).
    Thrown {
        at: Vec2,
        velocity: Vec2,
        accel: Vec2,
    },
}

impl Track {
    /// Where the target is `ahead` steps from now.
    pub fn at(&self, ahead: f64, dt: f32) -> Vec2 {
        match *self {
            Track::Still(p) => p,
            Track::Moving { at, velocity } => at + velocity * (ahead * dt as f64) as f32,
            Track::Focus(f) => f.at(ahead, dt),
            Track::Thrown { at, velocity, accel } => path_at(at, velocity, accel, ahead, dt),
        }
    }

    /// Whether the target goes its own way, so where it will be is only a guess from how it moves
    /// now (it may turn or bounce).
    pub fn guessed(&self) -> bool {
        matches!(self, Track::Moving { .. } | Track::Thrown { .. })
    }
}

/// What an emitter with settings `s` aims at and how it moves: the other focus of the shape it
/// sits on when Target is 0; else the thing it names (an emitter that sits on a focus as that
/// focus, a Balls thing its first ball, anything else its middle carried on at its pose's
/// velocity). `None` for no target.
pub fn target_track(world: &World, s: &EmitterSettings) -> Option<Track> {
    if s.aim_target <= 0 {
        if s.sits_on <= 0 {
            return None;
        }
        return focus_track(world, ThingId(s.sits_on as u32), 1 - index(s.sits_focus)).map(Track::Focus);
    }
    let id = ThingId(s.aim_target as u32);
    world.kind(id)?;
    if world.kind(id) == Some(KIND)
        && let Some(other) = emitter_state(world, id)
        && other.settings.sits_on > 0
        && let Some(f) = focus_track(world, ThingId(other.settings.sits_on as u32), index(other.settings.sits_focus))
    {
        return Some(Track::Focus(f));
    }
    // A Balls thing: its first ball still there.
    if world.kind(id) == Some("balls")
        && let Some(b) = world.bodies.get(&id)
        && let Some(row) = (0..b.len()).find(|&i| b.alive[i])
    {
        return Some(Track::Thrown { at: b.position[row], velocity: b.velocity[row], accel: pull_in(world, id).accel });
    }
    let pose = world.poses.get(&id).copied().unwrap_or_else(|| world.anchor(id));
    Some(if pose.velocity == Vec2::ZERO {
        Track::Still(pose.position)
    } else {
        Track::Moving { at: pose.position, velocity: pose.velocity }
    })
}

/// Straight in: the velocity that takes a ball `delta` away to it in exactly `n` steps under the
/// pull `accel`.
pub fn straight_in(delta: Vec2, accel: Vec2, n: u64, dt: f32) -> Vec2 {
    let (n, dt) = (n.max(1) as f64, dt as f64);
    let x = delta.x as f64 / (n * dt) - accel.x as f64 * dt * (n + 1.0) * 0.5;
    let y = delta.y as f64 / (n * dt) - accel.y as f64 * dt * (n + 1.0) * 0.5;
    Vec2::new(x as f32, y as f32)
}

/// `G(e)`: how far, in `accel·dt²`, a pull has carried a ball `e` steps after it set off.
fn pulled(e: f64) -> f64 {
    let k = e.floor();
    k * (k + 1.0) * 0.5 + (e - k) * (k + 1.0)
}

/// Where a ball that set off from `p0` at `v0` under `accel` is `e` steps later.
pub fn path_at(p0: Vec2, v0: Vec2, accel: Vec2, e: f64, dt: f32) -> Vec2 {
    let dt = dt as f64;
    let g = pulled(e.max(0.0)) * dt * dt;
    let e = e.max(0.0) * dt;
    Vec2::new(
        (p0.x as f64 + v0.x as f64 * e + accel.x as f64 * g) as f32,
        (p0.y as f64 + v0.y as f64 * e + accel.y as f64 * g) as f32,
    )
}

/// A way to the target keeping a direction: how many steps (and part of one) the ball flies to
/// pass through it, and how fast it leaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keep {
    pub steps: f64,
    pub speed: f32,
}

/// Keep my directions: how fast a ball must leave the way `u` (and how long it then flies) to pass
/// through the point `delta` away under the pull `accel`. With a pull across `u` the time comes
/// from how far across the target is and the speed from how far along; with none, only a way
/// through the target reaches, at `speed`. `None` when it can't (the pull carries it away, or it
/// would have to go backwards).
pub fn keep_way(delta: Vec2, accel: Vec2, u: Vec2, speed: f32, dt: f32) -> Option<Keep> {
    let u = u.normalize_or_zero();
    if u == Vec2::ZERO {
        return None;
    }
    let n = u.perp();
    let dt = dt as f64;
    let (across_d, along_d) = (n.dot(delta) as f64, u.dot(delta) as f64);
    let (across_a, along_a) = (n.dot(accel) as f64, u.dot(accel) as f64);
    if across_a.abs() < 1e-3 {
        if across_d.abs() > THROUGH || along_d <= 0.0 || speed < 0.0 {
            return None;
        }
        return along(along_d, speed as f64, along_a, dt).map(|steps| Keep { steps, speed });
    }
    // Across: n·Δ = (n·a)·dt²·G(e).
    let q = across_d / (across_a * dt * dt);
    if q <= 0.0 || !q.is_finite() {
        return None;
    }
    let mut k = (((8.0 * q + 1.0).sqrt() - 1.0) * 0.5).floor().max(0.0);
    while k > 0.0 && k * (k + 1.0) * 0.5 > q {
        k -= 1.0;
    }
    while (k + 1.0) * (k + 2.0) * 0.5 <= q {
        k += 1.0;
    }
    let e = k + (q - k * (k + 1.0) * 0.5) / (k + 1.0);
    if e <= 0.0 || e > MOST_STEPS {
        return None;
    }
    // Along: u·Δ = s·dt·e + (u·a)·dt²·G(e).
    let s = (along_d - along_a * dt * dt * q) / (e * dt);
    (s > 0.0 && s.is_finite()).then_some(Keep { steps: e, speed: s as f32 })
}

/// How many steps a ball leaving at `speed` along a line, pulled `pull` along it, takes to cover
/// `dist`: the first time it gets there.
fn along(dist: f64, speed: f64, pull: f64, dt: f64) -> Option<f64> {
    // At whole steps it has gone p(k) = speed·dt·k + pull·dt²·k(k+1)/2, and in between straight on.
    let at = |k: f64| speed * dt * k + pull * dt * dt * k * (k + 1.0) * 0.5;
    let (a2, b1) = (pull * dt * dt * 0.5, speed * dt + pull * dt * dt * 0.5);
    let k_star = if a2.abs() < 1e-12 {
        if b1 <= 0.0 {
            return None;
        }
        dist / b1
    } else {
        let disc = b1 * b1 + 4.0 * a2 * dist;
        if disc < 0.0 {
            return None;
        }
        let r = disc.sqrt();
        let roots = [(-b1 + r) / (2.0 * a2), (-b1 - r) / (2.0 * a2)];
        roots.into_iter().filter(|x| *x >= 0.0).fold(f64::INFINITY, f64::min)
    };
    if !k_star.is_finite() || k_star > MOST_STEPS {
        return None;
    }
    let mut k = k_star.floor().max(0.0);
    for _ in 0..4 {
        if k > 0.0 && at(k) > dist {
            k -= 1.0;
        } else if at(k + 1.0) < dist {
            k += 1.0;
        } else {
            break;
        }
    }
    let (from, to) = (at(k), at(k + 1.0));
    if !(from <= dist && dist <= to) || to <= from {
        return None;
    }
    Some(k + (dist - from) / (to - from))
}

/// When a Keep ball leaves: on `leave`, aimed to arrive at the moment `arrive` (or, Fire anyway,
/// unaimed with no `arrive`); `late` when it can only get there after the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    Go { leave: u64, arrive: Option<u64>, late: bool },
    Skip,
}

/// How an emitter aims one round's balls, worked out from the world at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aimer {
    pub style: Aim,
    pub track: Track,
    pub accel: Vec2,
    pub dt: f32,
    /// The moment (in steps) the world stands at: where the target and the poses are now.
    pub now: u64,
    /// Steps from a round firing to its balls arriving.
    pub arrive: u64,
    pub ceiling: f32,
    pub fire_anyway: bool,
}

/// How many steps Arrive after is, at `rate` steps a second.
pub fn arrive_steps(s: &EmitterSettings, rate: u32) -> u64 {
    ((s.arrive_after.max(0.0) as f64 * rate as f64).round() as u64).max(1)
}

impl Aimer {
    /// How emitter `me` with settings `s` aims, the world standing at the moment `now`. `None`
    /// with Aim off or nothing to aim at.
    pub fn of(world: &World, me: ThingId, s: &EmitterSettings, now: u64) -> Option<Aimer> {
        if s.aim == Aim::Off {
            return None;
        }
        Some(Aimer {
            style: s.aim,
            track: target_track(world, s)?,
            accel: pull_in(world, me).accel,
            dt: world.dt(),
            now,
            arrive: arrive_steps(s, world.steps_per_second),
            ceiling: world.limits().ceiling,
            fire_anyway: s.cant_reach == CantReach::FireAnyway,
        })
    }

    /// Where the target is at the moment `moment` (in steps).
    pub fn target_at(&self, moment: f64) -> Vec2 {
        self.track.at(moment - self.now as f64, self.dt)
    }

    /// Whether a ball leaving at `v0` stays under the world's speed ceiling for `steps` steps
    /// (the fastest it goes is at one end).
    fn under_ceiling(&self, v0: Vec2, steps: f64) -> bool {
        let end = v0 + self.accel * self.dt * (steps.ceil() - 1.0).max(0.0) as f32;
        v0.length() <= self.ceiling && end.length() <= self.ceiling
    }

    /// Straight in: the velocity that takes a ball leaving `p0` on step `leave` to the target at
    /// the end of step `arrive`. `None` when it leaves too late or would go too fast.
    pub fn straight(&self, p0: Vec2, leave: u64, arrive: u64) -> Option<Vec2> {
        let n = arrive.checked_sub(now(leave)).filter(|&n| n >= 1)?;
        let v = straight_in(self.target_at(arrive as f64) - p0, self.accel, n, self.dt);
        (v.is_finite() && self.under_ceiling(v, n as f64)).then_some(v)
    }

    /// Keep my directions, setting off at the moment `start`: the way through the target where it
    /// will be when the ball gets there (worked out at `arrive`, then again at the arrival it
    /// gives, for a moving target).
    fn keep_from(&self, p0: Vec2, u: Vec2, speed: f32, start: f64, arrive: f64) -> Option<Keep> {
        let first = keep_way(self.target_at(arrive) - p0, self.accel, u, speed, self.dt)?;
        if matches!(self.track, Track::Still(_)) {
            return Some(first);
        }
        keep_way(self.target_at(start + first.steps) - p0, self.accel, u, speed, self.dt)
    }

    /// When a Keep ball of a round fired on step `fire` leaves `p0` the way `u` (at `speed` when
    /// nothing pulls across its way), so it arrives `self.arrive` steps after the round fired.
    pub fn plan_keep(&self, p0: Vec2, u: Vec2, speed: f32, fire: u64) -> Plan {
        let arrive = fire + self.arrive;
        let earliest = now(fire);
        let unaimed = if self.fire_anyway { Plan::Go { leave: fire, arrive: None, late: false } } else { Plan::Skip };
        let Some(first) = keep_way(self.target_at(arrive as f64) - p0, self.accel, u, speed, self.dt) else {
            return unaimed;
        };
        let mut start = (arrive as f64 - first.steps).round();
        if !matches!(self.track, Track::Still(_))
            && let Some(again) = self.keep_from(p0, u, speed, start.max(earliest as f64), arrive as f64)
        {
            start = (arrive as f64 - again.steps).round();
        }
        if start < earliest as f64 {
            return if self.fire_anyway {
                Plan::Go { leave: fire, arrive: Some(arrive), late: true }
            } else {
                Plan::Skip
            };
        }
        let start = start as u64;
        let leave = if start == earliest { fire } else { start + 1 };
        match self.keep_velocity(p0, u, speed, leave, arrive) {
            Some(_) => Plan::Go { leave, arrive: Some(arrive), late: false },
            None => unaimed,
        }
    }

    /// Keep my directions at the launch: the velocity of a ball leaving `p0` the way `u` on step
    /// `leave`, aimed at the target at about the moment `arrive`. `None` when it can't reach or
    /// would go too fast.
    pub fn keep_velocity(&self, p0: Vec2, u: Vec2, speed: f32, leave: u64, arrive: u64) -> Option<Vec2> {
        let k = self.keep_from(p0, u, speed, now(leave) as f64, arrive as f64)?;
        let v = u.normalize_or_zero() * k.speed;
        self.under_ceiling(v, k.steps).then_some(v)
    }
}

/// One ball of a round as it would be aimed: where it starts, how it leaves, when (steps), when it
/// arrives (the moment, in steps) and whether it reaches the target then.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimedBall {
    pub start: Vec2,
    pub velocity: Vec2,
    pub leave: u64,
    pub arrive: u64,
    pub reaches: bool,
}

/// Emitter `me`'s next round as it would be aimed if it fired on the next step: at most `most`
/// of its balls, picked evenly, and how it aims. Empty with Aim off or no target.
pub fn next_round(world: &World, me: ThingId, most: usize) -> (Option<Aimer>, Vec<AimedBall>) {
    let Some(st) = emitter_state(world, me) else { return (None, Vec::new()) };
    let s = &st.settings;
    let Some(aim) = Aimer::of(world, me, s, world.step) else { return (None, Vec::new()) };
    // No round is coming.
    if !s.emits || st.rounds_left() == 0 {
        return (Some(aim), Vec::new());
    }
    let fire = world.step + 1;
    let round = st.rounds_fired + 1;
    let n = if st.rounds_fired == 0 { first_count(s) } else { st.next_count };
    if n == 0 {
        return (Some(aim), Vec::new());
    }
    let pose = world.poses.get(&me).copied().unwrap_or_default();
    let turn = Vec2::from_angle(pose.angle);
    let rate = world.steps_per_second;
    // Each ball's place in the firing order, for its Gap slot.
    let mut slot = vec![0u32; n as usize];
    for (j, k) in launch_order(s, n, round, world.seed, me.0).into_iter().enumerate() {
        slot[k as usize] = j as u32;
    }
    let shown = (n as usize).min(most.max(1));
    let arrive = fire + aim.arrive;
    let mut out = Vec::with_capacity(shown);
    for j in 0..shown {
        let k = if shown == n as usize { j as u32 } else { (j as u64 * n as u64 / shown as u64) as u32 };
        let way = direction(s, k, n, round, world.seed, me.0);
        let start = pose.position + turn.rotate(start_offset(s, k, n, way));
        let u = Vec2::from_angle(way + pose.angle);
        let plain = u * s.speed.max(0.0);
        let ball = match aim.style {
            Aim::KeepMyDirections => match aim.plan_keep(start, u, s.speed.max(0.0), fire) {
                Plan::Go { leave, arrive: Some(at), late } => {
                    let v = aim.keep_velocity(start, u, s.speed.max(0.0), leave, at);
                    AimedBall { start, velocity: v.unwrap_or(plain), leave, arrive: at, reaches: v.is_some() && !late }
                }
                Plan::Go { .. } | Plan::Skip => {
                    AimedBall { start, velocity: plain, leave: fire, arrive, reaches: false }
                }
            },
            _ => {
                let leave = fire + slot_step(s, slot[k as usize], rate);
                let v = aim.straight(start, leave, arrive);
                AimedBall { start, velocity: v.unwrap_or(plain), leave, arrive, reaches: v.is_some() }
            }
        };
        out.push(ball);
    }
    (Some(aim), out)
}

/// `points` places along `ball`'s way under `aim`'s pull, from where it sets off to its arrival.
pub fn path_points(aim: &Aimer, ball: &AimedBall, points: usize) -> Vec<Vec2> {
    let from = now(ball.leave);
    let steps = ball.arrive.saturating_sub(from).max(1) as f64;
    let last = points.max(2) - 1;
    (0..=last).map(|i| path_at(ball.start, ball.velocity, aim.accel, steps * i as f64 / last as f64, aim.dt)).collect()
}

/// What the Emitter card says under its Aim rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AimCheck {
    /// Nothing to aim at.
    pub no_target: bool,
    /// Something acts on the balls in flight that the aim leaves out: they may miss.
    pub in_the_way: bool,
    /// The target goes its own way: the aim follows how it moves now.
    pub target_moves: bool,
    /// How many of the next round's balls can't reach in time, of how many.
    pub cant_reach: u32,
    pub of: u32,
    /// They are skipped (else they go anyway).
    pub skip: bool,
}

/// How many balls of a round the checks and the preview look at.
pub const MOST_CHECKED: usize = 400;
/// How many places along each ball's way the wall check looks at.
pub const PATH_POINTS: usize = 24;

/// What emitter `me`'s aim should warn about in `world`; `None` when it isn't an emitter or
/// doesn't aim.
pub fn check(world: &World, me: ThingId) -> Option<AimCheck> {
    let st = emitter_state(world, me)?;
    let s = &st.settings;
    if s.aim == Aim::Off {
        return None;
    }
    let (aim, balls) = next_round(world, me, MOST_CHECKED);
    let Some(aim) = aim else { return Some(AimCheck { no_target: true, ..AimCheck::default() }) };
    let pull = pull_in(world, me);
    let mut in_the_way = pull.drag > 0.0 || pull.varies || hits_balls(world, me);
    if !in_the_way {
        let r = st.biggest_size().max(0.0);
        // A hundred balls are plenty to see a wall in the way.
        in_the_way = balls.iter().filter(|b| b.reaches).step_by(balls.len().div_ceil(100).max(1)).any(|b| {
            let path = path_points(&aim, b, PATH_POINTS);
            world.walls.values().any(|wall| {
                let reach = r + wall.outline.thickness() * 0.5;
                let side = |p: Vec2| wall.outline.inside(p);
                path.iter().any(|&p| (wall.outline.nearest(p).0 - p).length() < reach)
                    || path.windows(2).any(|w| side(w[0]) != side(w[1]))
            })
        });
    }
    let cant = balls.iter().filter(|b| !b.reaches).count();
    // The balls looked at stand for the whole round.
    let n = if st.rounds_fired == 0 { first_count(s) } else { st.next_count };
    let cant_reach = if balls.is_empty() { 0 } else { (cant as u64 * n as u64 / balls.len() as u64) as u32 };
    Some(AimCheck {
        no_target: false,
        in_the_way,
        target_moves: aim.track.guessed(),
        cant_reach,
        of: if balls.is_empty() { 0 } else { n },
        skip: !aim.fire_anyway,
    })
}

#[cfg(test)]
#[path = "aim_tests.rs"]
mod tests;
