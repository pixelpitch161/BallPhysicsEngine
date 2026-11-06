//! What Do blocks produce: actions, queued in the Rules stage and applied at the start of the
//! next step, in the order queued. Applying an action never runs rules.

use bpe_core::ThingId;
use glam::Vec2;

use super::shows::{FULL_FLASH_SPEED, Kick, MOST_KICKS, SceneOp, ShownScene, next_scene};
use super::{Change, ChangeKey};
use crate::{Event, EventKind, Log, Party, World, pop};

/// A change to a ball's motion, right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MotionChange {
    /// Adds speed along its direction (straight up when it is still); never below 0.
    AddSpeed(f32),
    /// Sets its speed along its direction.
    SetSpeed(f32),
    MulSpeed(f32),
    /// Adds a velocity.
    Push(Vec2),
    Stop,
    /// Turns it toward a point, keeping its speed.
    AimAt(Vec2),
    /// Holds it still for this many seconds, then lets it go on as it was going.
    Freeze(f32),
    /// Moves it there at once.
    Teleport(Vec2),
}

/// One thing a rule does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Changes the motion of one ball (its body row) or of every live ball of the thing.
    Motion { thing: ThingId, row: Option<u32>, motion: MotionChange },
    /// Changes a setting, for the whole thing or one ball.
    Change { key: ChangeKey, change: Box<Change> },
    /// Breaks one ball (its body row) or the whole thing.
    Break { thing: ThingId, row: Option<u32>, point: Vec2, normal: Vec2, strength: f32 },
    /// Shakes a camera or punches its zoom (its `start` is set when it applies).
    Kick(Kick),
    /// Changes how fast the simulation runs (a Set on a pace of 1; its `start` is set when it
    /// applies).
    Pace(Box<Change>),
    /// Flashes one ball (its body row), every live ball of the thing, or a thing without balls,
    /// as if hit; `strength` 1 is a full-strength hit.
    Flash { thing: ThingId, row: Option<u32>, strength: f32 },
    /// Shows a lighting scene among `count`; `fade` the crossfade's seconds (`None`: the card's).
    LightScene { op: SceneOp, count: u32, fade: Option<f32> },
    /// Plays a note: the `Note` event to log (its step is set when it applies).
    Note(Event),
}

/// A ball a rule holds still.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Frozen {
    pub thing: ThingId,
    pub row: u32,
    /// The step it is let go at.
    pub until: u64,
    /// How it was moving, given back when it is let go.
    pub velocity: Vec2,
}

/// The `Break` event's item for a whole thing.
pub const WHOLE: u32 = u32::MAX;

impl World {
    /// Lets go the balls whose freeze is over, then applies the actions queued in the last Rules
    /// stage, in order.
    pub(crate) fn apply_rule_actions(&mut self, log: &mut Log) {
        let Some(mut rules) = self.rules.take() else { return };
        let step = self.step;
        let rate = self.steps_per_second as f64;
        rules.frozen.retain(|f| {
            if f.until > step {
                return true;
            }
            if let Some(b) = self.bodies.get_mut(&f.thing)
                && let Some(i) = Some(f.row as usize).filter(|&i| i < b.len())
                && b.alive[i]
                && b.held[i]
            {
                b.held[i] = false;
                b.velocity[i] = f.velocity;
            }
            false
        });
        let time = self.time();
        rules.prune_shows(step, self.steps_per_second, time);
        for action in std::mem::take(&mut rules.pending) {
            match action {
                Action::Motion { thing, row, motion } => {
                    if rules.broken.contains(&thing) {
                        continue;
                    }
                    let Some(bodies) = self.bodies.get_mut(&thing) else { continue };
                    let rows: Vec<usize> = match row {
                        Some(r) => vec![r as usize],
                        None => (0..bodies.len()).collect(),
                    };
                    for i in rows {
                        if i >= bodies.len() || !bodies.alive[i] {
                            continue;
                        }
                        let frozen = rules.frozen.iter().position(|f| f.thing == thing && f.row == i as u32);
                        let v = match frozen {
                            Some(k) => rules.frozen[k].velocity,
                            None => bodies.velocity[i],
                        };
                        let dir = if v.length_squared() > 0.0 { v.normalize() } else { Vec2::NEG_Y };
                        let new_v = match motion {
                            MotionChange::AddSpeed(a) => dir * (v.length() + a).max(0.0),
                            MotionChange::SetSpeed(s) => dir * s,
                            MotionChange::MulSpeed(m) => v * m,
                            MotionChange::Push(p) => v + p,
                            MotionChange::Stop => Vec2::ZERO,
                            MotionChange::AimAt(at) => {
                                let to = at - bodies.position[i];
                                if to.length_squared() > 0.0 { to.normalize() * v.length() } else { v }
                            }
                            MotionChange::Freeze(seconds) => {
                                let steps = (seconds.max(0.0) as f64 * rate).round().clamp(1.0, 1e15) as u64;
                                let until = step.saturating_add(steps);
                                match frozen {
                                    Some(k) => rules.frozen[k].until = rules.frozen[k].until.max(until),
                                    // A ball something else holds (an emitter waiting to send it
                                    // back) is left to it.
                                    None if bodies.held[i] => {}
                                    None => {
                                        rules.frozen.push(Frozen { thing, row: i as u32, until, velocity: v });
                                        bodies.held[i] = true;
                                        bodies.velocity[i] = Vec2::ZERO;
                                    }
                                }
                                continue;
                            }
                            MotionChange::Teleport(p) => {
                                if p.is_finite() {
                                    bodies.position[i] = p;
                                    bodies.previous[i] = p;
                                    bodies.start[i] = p;
                                }
                                continue;
                            }
                        };
                        if !new_v.is_finite() {
                            continue;
                        }
                        match rules.frozen.iter_mut().find(|f| f.thing == thing && f.row == i as u32) {
                            Some(f) => f.velocity = new_v,
                            None => bodies.velocity[i] = new_v,
                        }
                    }
                }
                Action::Kick(mut kick) => {
                    if rules.broken.contains(&kick.thing)
                        || !(kick.amount.is_finite() && kick.seconds.is_finite() && kick.rate.is_finite())
                        || kick.seconds <= 0.0
                    {
                        continue;
                    }
                    kick.start = step;
                    if rules.kicks.len() >= MOST_KICKS {
                        rules.kicks.remove(0);
                    }
                    rules.kicks.push(kick);
                }
                Action::Pace(mut change) => {
                    change.start = time;
                    rules.add_pace(*change);
                }
                Action::Flash { thing, row, strength } => {
                    if rules.broken.contains(&thing) || !strength.is_finite() {
                        continue;
                    }
                    let flash = |item: u32, point: Vec2| Event {
                        step,
                        kind: EventKind::Flash,
                        a: Party { thing, item },
                        b: None,
                        point,
                        normal: Vec2::ZERO,
                        speed: strength.max(0.0) * FULL_FLASH_SPEED,
                        at: 0.0,
                    };
                    match self.bodies.get(&thing) {
                        Some(bodies) => {
                            let rows: Vec<usize> = match row {
                                Some(r) => vec![r as usize],
                                None => (0..bodies.len()).collect(),
                            };
                            for i in rows {
                                if i < bodies.len() && bodies.alive[i] {
                                    log.push(flash(i as u32, bodies.position[i]));
                                    *rules.flashes.entry((thing, i as u32)).or_default() += 1;
                                }
                            }
                        }
                        None if self.names.contains_key(&thing) => {
                            let point = match self.poses.get(&thing) {
                                Some(p) => p.position,
                                None => self.anchor(thing).position,
                            };
                            log.push(flash(0, point));
                            *rules.flashes.entry((thing, 0)).or_default() += 1;
                        }
                        None => {}
                    }
                }
                Action::LightScene { op, count, fade } => {
                    let dice = super::dice(self.seed, bpe_core::RuleId(u32::MAX), bpe_core::BlockId(0), step, None, 7);
                    if let Some(to) = next_scene(rules.scene.as_ref(), op, count, dice) {
                        let from = rules.scene.as_ref().and_then(|s| s.to);
                        let fade = fade.filter(|f| f.is_finite()).map(|f| f.max(0.0));
                        rules.scene = Some(ShownScene { step, from, to, fade });
                    }
                }
                Action::Note(mut e) => {
                    let shape = e.b.map(|b| b.thing);
                    if shape.is_none_or(|s| rules.broken.contains(&s))
                        || !(e.speed.is_finite() && e.normal.is_finite() && e.point.is_finite())
                    {
                        continue;
                    }
                    e.step = step;
                    e.kind = EventKind::Note;
                    e.at = 0.0;
                    log.push(e);
                }
                Action::Change { key, mut change } => {
                    if rules.broken.contains(&key.thing) {
                        continue;
                    }
                    change.start = time;
                    rules.add_change(key, *change);
                }
                Action::Break { thing, row, point, normal, strength } => {
                    if rules.broken.contains(&thing) {
                        continue;
                    }
                    let event = |item: u32| Event {
                        step,
                        kind: EventKind::Break,
                        a: Party { thing, item },
                        b: None,
                        point,
                        normal,
                        speed: if strength.is_finite() { strength } else { 0.0 },
                        at: 0.0,
                    };
                    match row {
                        Some(r) => {
                            let Some(bodies) = self.bodies.get_mut(&thing) else { continue };
                            let i = r as usize;
                            if i >= bodies.len() || !bodies.alive[i] {
                                continue;
                            }
                            log.push(event(r));
                            pop(bodies, i, thing, step, log);
                            bodies.held[i] = false;
                            rules.frozen.retain(|f| !(f.thing == thing && f.row == r));
                            rules.forget_row(thing, r);
                        }
                        None => {
                            log.push(event(WHOLE));
                            if let Some(bodies) = self.bodies.get_mut(&thing) {
                                for i in 0..bodies.len() {
                                    if bodies.alive[i] {
                                        pop(bodies, i, thing, step, log);
                                    }
                                    bodies.held[i] = false;
                                }
                            }
                            self.walls.remove(&thing);
                            self.order.retain(|t| *t != thing);
                            rules.broken.insert(thing);
                            rules.frozen.retain(|f| f.thing != thing);
                            rules.forget(thing);
                        }
                    }
                }
            }
        }
        self.rules = Some(rules);
    }
}
