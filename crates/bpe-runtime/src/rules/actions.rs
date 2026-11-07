//! What Do blocks produce: actions, queued in the Rules stage and applied at the start of the
//! next step, in the order queued. Applying an action never runs rules.

use std::sync::Arc;

use bpe_core::ThingId;
use glam::Vec2;

use super::shows::{FULL_FLASH_SPEED, Kick, MOST_KICKS, SceneOp, ShownScene, next_scene};
use super::{BallAt, BreakShow, Change, ChangeKey, Firing, HealthChange, MAX_BALLS, MAX_ROWS, Remains};
use crate::{Event, EventKind, Log, NewBall, Party, Sims, World, on_own_side, pop};

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
    /// Breaks one ball (its body row) or the whole thing, `show` saying how its look plays out
    /// (drawn only; the default is Vanish).
    Break { thing: ThingId, row: Option<u32>, point: Vec2, normal: Vec2, strength: f32, show: Arc<BreakShow> },
    /// Changes the health of one ball (its body row), of every live ball of the thing (`None`
    /// on a thing with balls) or of a thing without balls (rules part 4).
    Health { thing: ThingId, row: Option<u32>, change: HealthChange },
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
    /// Splits one ball into `into` balls fanned `spread` radians across its way, going `speed`
    /// times as fast; `share` shares its area out between them, else each keeps its size.
    Split { thing: ThingId, row: u32, into: u32, spread: f32, speed: f32, share: bool },
    /// Adds balls to a thing: each one's place, velocity and size (`None`: the thing's own),
    /// fanned out from `from`; a ball whose place is across a wall from there starts at `from`.
    Spawn { thing: ThingId, from: Vec2, balls: Vec<(Vec2, Vec2, Option<f32>)> },
    /// Asks the thing to fire now (an emitter's round).
    Fire { thing: ThingId },
    /// Adds (or removes) a tag on a whole thing, or on one ball (its body row).
    Tag { thing: ThingId, row: Option<u32>, tag: String, add: bool },
    /// Changes a named counter.
    Count { name: String, how: CountHow },
    /// Sends a signal. Never queued: the stage hears it in its next round.
    Signal { name: String, firing: Firing },
    /// Changes a Text's words (Set text, Text part 2): drawn only.
    Text { thing: ThingId, op: TextOp },
}

/// What a Set text action does to a Text's words.
#[derive(Clone, Debug, PartialEq)]
pub enum TextOp {
    /// These words in place of its own.
    Replace(String),
    /// A line after its words.
    AddLine(String),
    /// Shows its next line alone.
    NextLine,
    /// No words.
    Clear,
}

/// How a Count action changes its counter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CountHow {
    Add(f64),
    Set(f64),
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
    pub(crate) fn apply_rule_actions(&mut self, sims: &Sims, log: &mut Log) {
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
        rules.prune_remains(time);
        rules.prune_shows(step, self.steps_per_second, time);
        let gravity = self.gravity;
        // Walked as a queue, so a health that reaches 0 here can add its Break for this same pass.
        let mut queue: std::collections::VecDeque<Action> = std::mem::take(&mut rules.pending).into();
        while let Some(action) = queue.pop_front() {
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
                Action::Health { thing, row, change } => {
                    if rules.broken.contains(&thing) || !rules.health.has(thing) {
                        continue;
                    }
                    let rows: Vec<Option<u32>> = match (row, self.bodies.get(&thing)) {
                        (Some(r), Some(b)) => {
                            if (r as usize) < b.len() && b.alive[r as usize] {
                                vec![Some(r)]
                            } else {
                                Vec::new()
                            }
                        }
                        (None, Some(b)) => (0..b.len() as u32).filter(|&r| b.alive[r as usize]).map(Some).collect(),
                        (_, None) => vec![None],
                    };
                    for row in rows {
                        let point = match (row, self.bodies.get(&thing)) {
                            (Some(r), Some(b)) => b.position[r as usize],
                            _ => self.poses.get(&thing).map_or_else(|| self.anchor(thing).position, |p| p.position),
                        };
                        let at_zero = rules.health.card(thing).and_then(|c| c.at_zero.clone());
                        if let Some((_, true)) = rules.health.change(thing, row, change, (point, Vec2::ZERO, None, 0.0))
                            && let Some(template) = at_zero
                        {
                            queue.push_back(super::aimed(&template, thing, row, point, Vec2::ZERO, 0.0));
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
                Action::Break { thing, row, point, normal, strength, show } => {
                    if rules.broken.contains(&thing) {
                        continue;
                    }
                    let strength = if strength.is_finite() { strength } else { 0.0 };
                    let remains = |wall, ball| Remains {
                        step,
                        time,
                        point,
                        normal,
                        strength,
                        show: show.clone(),
                        gravity,
                        wall,
                        ball,
                    };
                    let ball_at = |b: &crate::Bodies, i: usize| {
                        Some(BallAt { position: b.position[i], velocity: b.velocity[i], radius: b.radius[i] })
                    };
                    let event = |item: u32| Event {
                        step,
                        kind: EventKind::Break,
                        a: Party { thing, item },
                        b: None,
                        point,
                        normal,
                        speed: strength,
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
                            if show.shows() {
                                rules.remains.insert((thing, r), remains(None, ball_at(bodies, i)));
                            }
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
                                        if show.shows() {
                                            let at = ball_at(bodies, i);
                                            rules.remains.insert((thing, i as u32), remains(None, at));
                                        }
                                        pop(bodies, i, thing, step, log);
                                    }
                                    bodies.held[i] = false;
                                }
                            }
                            let wall = self.walls.remove(&thing);
                            if show.shows() {
                                rules.remains.insert((thing, WHOLE), remains(wall, None));
                            }
                            self.order.retain(|t| *t != thing);
                            rules.broken.insert(thing);
                            rules.frozen.retain(|f| f.thing != thing);
                            rules.forget(thing);
                        }
                    }
                }
                Action::Split { thing, row, into, spread, speed, share } => {
                    if !rules.broken.contains(&thing) {
                        self.split(sims, &mut rules, thing, row, into, spread, speed, share);
                    }
                }
                Action::Spawn { thing, from, balls } => {
                    if rules.broken.contains(&thing) {
                        continue;
                    }
                    let Some(sim) = self.kinds.get(&thing).and_then(|k| sims.thing(k)) else { continue };
                    for (position, velocity, radius) in balls {
                        if !room_for(self, thing) {
                            break;
                        }
                        if !(position.is_finite() && velocity.is_finite() && radius.is_none_or(f32::is_finite)) {
                            continue;
                        }
                        let radius = radius.map(|r| r.max(0.0));
                        let position = if self.clear_way(from, position) { position } else { from };
                        sim.add_ball(thing, self, &NewBall { position, velocity, radius, like: None });
                    }
                }
                Action::Fire { thing } => {
                    if rules.broken.contains(&thing) {
                        continue;
                    }
                    if let Some(sim) = self.kinds.get(&thing).and_then(|k| sims.thing(k)) {
                        sim.fire_now(thing, self);
                    }
                }
                Action::Tag { thing, row, tag, add } => {
                    if rules.broken.contains(&thing) || tag.is_empty() {
                        continue;
                    }
                    match row {
                        Some(r) => {
                            let live = self.bodies.get(&thing).is_some_and(|b| b.alive.get(r as usize) == Some(&true));
                            if add && live {
                                rules.ball_tags.entry((thing, r)).or_default().insert(tag);
                            } else if !add && let Some(tags) = rules.ball_tags.get_mut(&(thing, r)) {
                                tags.remove(&tag);
                                if tags.is_empty() {
                                    rules.ball_tags.remove(&(thing, r));
                                }
                            }
                        }
                        None if add => {
                            self.tags.entry(thing).or_default().insert(tag);
                        }
                        None => {
                            if let Some(tags) = self.tags.get_mut(&thing) {
                                tags.remove(&tag);
                                if tags.is_empty() {
                                    self.tags.remove(&thing);
                                }
                            }
                        }
                    }
                }
                Action::Count { name, how } => {
                    let now = rules.counters.get(&name).copied().unwrap_or(0.0);
                    let new = match how {
                        CountHow::Add(x) => now + x,
                        CountHow::Set(x) => x,
                    };
                    if new.is_finite() {
                        rules.counters.insert(name, new);
                    }
                }
                // Heard in the stage that sent it; never queued.
                Action::Signal { .. } => {}
                Action::Text { thing, op } => {
                    let t = rules.texts.entry(thing).or_default();
                    match op {
                        TextOp::Replace(words) => *t = super::RuleText { words: Some(words), ..Default::default() },
                        TextOp::AddLine(line) => t.added.push(line),
                        TextOp::NextLine => t.nexts.push(time),
                        TextOp::Clear => *t = super::RuleText { words: Some(String::new()), ..Default::default() },
                    }
                }
            }
        }
        self.rules = Some(rules);
    }

    /// Splits ball `row` of `thing` (see [`Action::Split`]): it becomes the middle one of the fan
    /// and the others are added beside it, inside where it was, through the thing's sim.
    #[allow(clippy::too_many_arguments)]
    fn split(
        &mut self,
        sims: &Sims,
        rules: &mut super::RulesState,
        thing: ThingId,
        row: u32,
        into: u32,
        spread: f32,
        speed: f32,
        share: bool,
    ) {
        let Some(sim) = self.kinds.get(&thing).and_then(|k| sims.thing(k)) else { return };
        let Some(bodies) = self.bodies.get(&thing) else { return };
        let i = row as usize;
        if i >= bodies.len() || !bodies.alive[i] || into < 2 || !spread.is_finite() || !speed.is_finite() {
            return;
        }
        // How many more balls there is room for.
        let room = MAX_BALLS.saturating_sub(bodies.alive_count()).min(MAX_ROWS.saturating_sub(bodies.len()));
        let n = (into as usize).min(room + 1);
        if n < 2 {
            return;
        }
        let frozen = rules.frozen.iter().position(|f| f.thing == thing && f.row == row);
        let v = frozen.map_or(bodies.velocity[i], |k| rules.frozen[k].velocity);
        let (centre, r, hits) = (bodies.position[i], bodies.radius[i], bodies.hits[i]);
        let dir = if v.length_squared() > 0.0 { v.normalize() } else { Vec2::NEG_Y };
        let speed = v.length() * speed;
        let small = if share { r / (n as f32).sqrt() } else { r };
        let reach = (r - small).max(0.05 * r);
        let middle = (n - 1) / 2;
        let ball = |k: usize| {
            let t = k as f32 / (n - 1) as f32 - 0.5;
            let way = Vec2::from_angle(spread * t).rotate(dir);
            let place = centre + dir.perp() * (t * 2.0 * reach);
            // Never across a wall from where the ball was (a Middle touch runs its centre right
            // along a wall's line).
            (if self.clear_way(centre, place) { place } else { centre }, way * speed)
        };
        let fan: Vec<(Vec2, Vec2)> = (0..n).map(ball).collect();
        for (k, (position, velocity)) in fan.into_iter().enumerate() {
            if !(position.is_finite() && velocity.is_finite()) {
                continue;
            }
            if k == middle {
                let b = self.bodies.get_mut(&thing).expect("checked");
                b.position[i] = position;
                b.previous[i] = position;
                b.start[i] = position;
                b.radius[i] = small;
                match frozen {
                    Some(f) => rules.frozen[f].velocity = velocity,
                    None => b.velocity[i] = velocity,
                }
                continue;
            }
            let new = NewBall { position, velocity, radius: Some(small), like: Some(row) };
            let Some(child) = sim.add_ball(thing, self, &new) else { return };
            if let Some(b) = self.bodies.get_mut(&thing)
                && let Some(h) = b.hits.get_mut(child as usize)
            {
                *h = hits;
            }
            rules.copy_row(thing, row, child);
        }
    }
}

impl World {
    /// Whether `to` is on the same side as `from` of every wall balls bounce off: a ball whose
    /// centre was at `from` may be put at `to`.
    pub fn clear_way(&self, from: Vec2, to: Vec2) -> bool {
        to.is_finite() && self.walls.values().all(|w| w.passes || on_own_side(to, from, &w.outline, w))
    }
}

/// Whether `thing` may have one more ball: fewer than [`MAX_BALLS`] live and [`MAX_ROWS`] rows.
fn room_for(world: &World, thing: ThingId) -> bool {
    world.bodies.get(&thing).is_none_or(|b| b.alive_count() < MAX_BALLS && b.len() < MAX_ROWS)
}
