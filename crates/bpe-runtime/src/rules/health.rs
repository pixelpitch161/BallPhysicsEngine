//! Health (rules part 4, spec section 7): a thing with a Health card keeps a health of its own,
//! each ball by its body row (a thing without balls, a shape, one for the whole thing). Hits take
//! health away in the Rules stage, before any rule runs; a rule's Change health moves it at the
//! next step's start. Each change is a [`HealthMoment`] the stage's When Health blocks read; at
//! zero the card's action (a Break) is queued. All of it is part of the world, cloned with it.

use std::collections::{BTreeMap, BTreeSet};

use bpe_core::vary::{self, Vary};
use bpe_core::{Params, Resolved, SlotId, Target, ThingId, Value};
use glam::Vec2;

use super::Action;
use crate::{Event, EventKind, Party, Sims, World};

/// How much health a hit takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Takes {
    /// The same amount every hit.
    EachHit(f64),
    /// The hit's strength (px/s) × this.
    ByStrength(f64),
    /// Hits take nothing: only rules change it.
    Nothing,
}

/// What a rule's Change health does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HealthChange {
    /// Takes this much away.
    Damage(f64),
    /// Gives this much back (past the starting health too).
    Heal(f64),
    /// Sets it.
    Set(f64),
    /// Back to its starting health.
    Refill,
}

/// A Health card as the world reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct HealthCard {
    pub start: f64,
    /// Starting health varied across the balls: its other end and how.
    pub start_vary: Option<(f64, Vary)>,
    pub takes: Takes,
    /// Softer hits (px/s) take nothing.
    pub least: f32,
    /// Only hits from these count.
    pub from: Target,
    /// What happens at zero: an action (a Break) whose thing, row, point, normal and strength are
    /// filled in when it is used ([`aimed`]); `None` when nothing happens.
    pub at_zero: Option<Action>,
}

impl HealthCard {
    /// The starting health of a ball at `spread` (the plain one for a thing without balls).
    pub fn start_at(&self, spread: Option<&bpe_core::BallSpread>) -> f64 {
        let start = match (&self.start_vary, spread) {
            (Some((to, v)), Some(spread)) => {
                let t = v.amount(spread.t(v.by, v.salt()));
                match vary::mixed(&Value::Number(self.start), &Value::Number(*to), t) {
                    Value::Number(x) => x,
                    _ => self.start,
                }
            }
            _ => self.start,
        };
        // A start of 0 is kept a hair above it, so the first hit that takes anything still
        // reaches zero (and breaks).
        if start.is_finite() { start.max(LEAST_START) } else { LEAST_START }
    }

    /// How much a hit of `strength` takes (before Least strength and Only hits from).
    fn damage(&self, strength: f32) -> f64 {
        match self.takes {
            Takes::EachHit(x) => x,
            Takes::ByStrength(k) => strength as f64 * k,
            Takes::Nothing => 0.0,
        }
    }
}

/// How a Health card kind reads its settings (the kit's `health`).
pub trait HealthSim: Send + Sync + 'static {
    /// The card kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    fn prepare(&self, params: &Params) -> HealthCard;
}

/// One ball's or thing's health.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hp {
    pub now: f64,
    pub start: f64,
}

impl Hp {
    /// Its health ÷ its starting health, 0 to 1.
    pub fn share(&self) -> f32 {
        let s = if self.start > 0.0 {
            self.now / self.start
        } else if self.now > 0.0 {
            1.0
        } else {
            0.0
        };
        if s.is_finite() { s.clamp(0.0, 1.0) as f32 } else { 0.0 }
    }
}

/// One change of health in a step: what the stage's When Health blocks read.
#[derive(Clone, Debug, PartialEq)]
pub struct HealthMoment {
    pub thing: ThingId,
    /// The ball's body row; `None` for a thing without balls.
    pub row: Option<u32>,
    pub before: f64,
    pub after: f64,
    pub start: f64,
    /// What hit it, when a hit took it.
    pub other: Option<Party>,
    pub point: Vec2,
    pub normal: Vec2,
    /// The hit's strength (0 for a rule's change).
    pub strength: f32,
}

#[derive(Clone, Debug)]
struct HealthOf {
    card: HealthCard,
    /// Whether it has balls (a health per body row) or is one thing.
    balls: bool,
    /// Per body row (one for a thing without balls).
    hp: Vec<Hp>,
}

/// Every thing's health, and this step's changes.
#[derive(Clone, Debug, Default)]
pub struct Health {
    things: BTreeMap<ThingId, HealthOf>,
    pub(crate) moments: Vec<HealthMoment>,
    /// Held as it is: hits take nothing (a test flight's copy, `World::lone`).
    still: bool,
    /// The health a new ball row takes from the ball it split from, until `fill` gives it.
    inherited: BTreeMap<(ThingId, u32), Hp>,
}

/// The least starting health: a card's 0 counts as this.
pub const LEAST_START: f64 = 1e-9;

/// `template` (a card's action at zero) aimed at `thing` (or its ball `row`), with the place and
/// strength of what did it.
pub fn aimed(template: &Action, thing: ThingId, row: Option<u32>, point: Vec2, normal: Vec2, strength: f32) -> Action {
    match template.clone() {
        Action::Break { show, .. } => Action::Break { thing, row, point, normal, strength, show },
        Action::Motion { motion, .. } => Action::Motion { thing, row, motion },
        Action::Health { change, .. } => Action::Health { thing, row, change },
        other => other,
    }
}

impl Health {
    /// Every built thing of `world` with a Health card whose kind has a sim.
    pub fn build(resolved: &Resolved, world: &World, sims: &Sims) -> Health {
        let mut things = BTreeMap::new();
        for &id in &world.order {
            let Some(slot) = resolved.things.get(&id).and_then(|r| r.slots.get(&SlotId::Health)) else { continue };
            let Some(sim) = sims.health(&slot.kind) else { continue };
            let card = sim.prepare(&slot.params);
            let balls = world.bodies.contains_key(&id);
            let hp = if balls { Vec::new() } else { vec![Hp { now: card.start_at(None), start: card.start_at(None) }] };
            things.insert(id, HealthOf { card, balls, hp });
        }
        Health { things, moments: Vec::new(), still: false, inherited: BTreeMap::new() }
    }

    /// A copy whose health stays as it is now: hits take nothing (a test flight's world).
    pub(crate) fn held(&self) -> Health {
        Health { things: self.things.clone(), moments: Vec::new(), still: true, inherited: self.inherited.clone() }
    }

    /// Ball row `to` of `thing` (a ball split from row `from`) starts with `from`'s health.
    pub(crate) fn copy_row(&mut self, thing: ThingId, from: u32, to: u32) {
        if let Some(hp) = self.things.get(&thing).filter(|h| h.balls).and_then(|_| self.of(thing, Some(from))) {
            self.inherited.insert((thing, to), hp);
        }
    }

    /// Whether any thing has health.
    pub fn is_empty(&self) -> bool {
        self.things.is_empty()
    }

    /// Whether `thing` has a Health card.
    pub fn has(&self, thing: ThingId) -> bool {
        self.things.contains_key(&thing)
    }

    /// The things with health, in id order.
    pub fn things(&self) -> impl Iterator<Item = ThingId> + '_ {
        self.things.keys().copied()
    }

    /// `thing`'s card, as read.
    pub fn card(&self, thing: ThingId) -> Option<&HealthCard> {
        self.things.get(&thing).map(|h| &h.card)
    }

    /// This stage's changes of health, in the order made.
    pub fn moments(&self) -> &[HealthMoment] {
        &self.moments
    }

    /// Gives the balls new since the last time their starting health, at their places in its
    /// spread.
    pub(crate) fn fill(&mut self, world: &World, sims: &Sims) {
        for (id, h) in self.things.iter_mut().filter(|(_, h)| h.balls) {
            let count = world.bodies.get(id).map_or(0, |b| b.len());
            for row in h.hp.len()..count {
                if let Some(hp) = self.inherited.remove(&(*id, row as u32)) {
                    h.hp.push(hp);
                    continue;
                }
                let start = match &h.card.start_vary {
                    Some(_) => h.card.start_at(Some(&world.ball_spread(sims, *id, row))),
                    None => h.card.start_at(None),
                };
                h.hp.push(Hp { now: start, start });
            }
        }
    }

    /// `thing`'s health (its ball `row`'s, for a thing with balls). A ball whose health isn't
    /// worked out yet has its card's plain starting health.
    pub fn of(&self, thing: ThingId, row: Option<u32>) -> Option<Hp> {
        let h = self.things.get(&thing)?;
        if !h.balls {
            return h.hp.first().copied();
        }
        let row = row? as usize;
        Some(h.hp.get(row).copied().unwrap_or_else(|| {
            let s = h.card.start_at(None);
            Hp { now: s, start: s }
        }))
    }

    /// `thing`'s share of its starting health (0 to 1): its own for a thing without balls, else
    /// the mean over its live balls (0 when they are all gone, 1 before it has any). 0 without a
    /// Health card.
    pub fn thing_share(&self, world: &World, thing: ThingId) -> f32 {
        let Some(h) = self.things.get(&thing) else { return 0.0 };
        if !h.balls {
            return h.hp.first().map_or(0.0, Hp::share);
        }
        let Some(b) = world.bodies.get(&thing).filter(|b| !b.is_empty()) else { return 1.0 };
        let (mut sum, mut n) = (0.0f32, 0u32);
        for row in (0..b.len()).filter(|&r| b.alive[r]) {
            sum += h.hp.get(row).map_or(1.0, Hp::share);
            n += 1;
        }
        if n == 0 { 0.0 } else { sum / n as f32 }
    }

    /// Moves the health of `thing` (its ball `row`) by `change`: never below 0, a non-finite
    /// result dropped. Returns the moment when it moved, and whether it reached 0 just now.
    pub(crate) fn change(
        &mut self,
        thing: ThingId,
        row: Option<u32>,
        change: HealthChange,
        at: (Vec2, Vec2, Option<Party>, f32),
    ) -> Option<(HealthMoment, bool)> {
        let h = self.things.get_mut(&thing)?;
        let i = if h.balls { row? as usize } else { 0 };
        let hp = h.hp.get_mut(i)?;
        let before = hp.now;
        let after = match change {
            HealthChange::Damage(x) => before - x,
            HealthChange::Heal(x) => before + x,
            HealthChange::Set(x) => x,
            HealthChange::Refill => hp.start,
        };
        if after.is_nan() {
            return None;
        }
        let after = after.clamp(0.0, f64::MAX);
        if after == before {
            return None;
        }
        hp.now = after;
        let (point, normal, other, strength) = at;
        let moment = HealthMoment {
            thing,
            row: h.balls.then_some(i as u32),
            before,
            after,
            start: hp.start,
            other,
            point,
            normal,
            strength,
        };
        self.moments.push(moment.clone());
        Some((moment, before > 0.0 && after <= 0.0))
    }

    /// What this step's hits take: each hit in log order, each side with health takes its card's
    /// damage (when hard enough and from what the card counts); at zero the card's action goes
    /// into `out`.
    pub(crate) fn damage(
        &mut self,
        world: &World,
        events: &[Event],
        broken: &BTreeSet<ThingId>,
        out: &mut Vec<Action>,
    ) {
        if self.things.is_empty() || self.still {
            return;
        }
        for e in events.iter().filter(|e| e.kind == EventKind::Hit) {
            let sides = [(Some(e.a), e.b, e.normal), (e.b, Some(e.a), -e.normal)];
            for (me, other, normal) in sides {
                let Some(me) = me else { continue };
                let Some(h) = self.things.get(&me.thing) else { continue };
                if broken.contains(&me.thing) {
                    continue;
                }
                let row = if h.balls {
                    let Some(b) = world.bodies.get(&me.thing) else { continue };
                    let i = me.item as usize;
                    if i >= b.len() || !b.alive[i] {
                        continue;
                    }
                    Some(me.item)
                } else {
                    None
                };
                let card = &h.card;
                if e.speed < card.least || matches!(card.takes, Takes::Nothing) {
                    continue;
                }
                let counts = match other {
                    Some(o) => world.names(&card.from, me.thing, o.thing),
                    None => card.from == Target::Anything,
                };
                if !counts {
                    continue;
                }
                let damage = card.damage(e.speed);
                let at_zero = card.at_zero.clone();
                if let Some((_, zero)) =
                    self.change(me.thing, row, HealthChange::Damage(damage), (e.point, normal, other, e.speed))
                    && zero
                    && let Some(template) = at_zero
                {
                    out.push(aimed(&template, me.thing, row, e.point, normal, e.speed));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(start: f64) -> HealthCard {
        HealthCard {
            start,
            start_vary: None,
            takes: Takes::EachHit(1.0),
            least: 0.0,
            from: Target::Anything,
            at_zero: None,
        }
    }

    fn one_thing(start: f64) -> Health {
        let mut things = BTreeMap::new();
        things.insert(ThingId(5), HealthOf { card: card(start), balls: false, hp: vec![Hp { now: start, start }] });
        Health { things, moments: Vec::new(), still: false, inherited: BTreeMap::new() }
    }

    const AT: (Vec2, Vec2, Option<Party>, f32) = (Vec2::ZERO, Vec2::ZERO, None, 0.0);

    #[test]
    fn a_share_is_health_over_its_start_from_0_to_1() {
        assert_eq!(Hp { now: 1.0, start: 4.0 }.share(), 0.25);
        assert_eq!(Hp { now: 9.0, start: 4.0 }.share(), 1.0, "healed past its start: still full");
        assert_eq!(Hp { now: 0.0, start: 0.0 }.share(), 0.0);
        assert_eq!(Hp { now: 2.0, start: 0.0 }.share(), 1.0);
    }

    #[test]
    fn changes_never_go_below_zero_and_note_a_moment_each() {
        let mut h = one_thing(3.0);
        let t = ThingId(5);
        let (m, zero) = h.change(t, None, HealthChange::Damage(1.0), AT).unwrap();
        assert_eq!((m.before, m.after, zero), (3.0, 2.0, false));
        let (m, zero) = h.change(t, None, HealthChange::Damage(5.0), AT).unwrap();
        assert_eq!((m.before, m.after, zero), (2.0, 0.0, true));
        assert!(h.change(t, None, HealthChange::Damage(1.0), AT).is_none(), "nothing to take: no moment");
        assert!(h.change(t, None, HealthChange::Damage(f64::NAN), AT).is_none());
        let (m, zero) = h.change(t, None, HealthChange::Heal(10.0), AT).unwrap();
        assert_eq!((m.after, zero), (10.0, false), "healing has no top");
        h.change(t, None, HealthChange::Set(1.5), AT).unwrap();
        let (m, _) = h.change(t, None, HealthChange::Refill, AT).unwrap();
        assert_eq!((m.before, m.after), (1.5, 3.0));
        assert_eq!(h.moments().len(), 5);
        assert!(h.change(ThingId(6), None, HealthChange::Refill, AT).is_none(), "no Health card");
        let (m, _) = h.change(t, None, HealthChange::Heal(f64::INFINITY), AT).unwrap();
        assert_eq!(m.after, f64::MAX, "an endless heal is capped");
        let (m, zero) = h.change(t, None, HealthChange::Damage(f64::INFINITY), AT).unwrap();
        assert_eq!((m.after, zero), (0.0, true), "an endless hit empties it");
    }

    #[test]
    fn a_start_of_zero_still_reaches_zero_on_the_first_hit() {
        let c = card(0.0);
        let start = c.start_at(None);
        assert_eq!(start, LEAST_START);
        let mut things = BTreeMap::new();
        things.insert(ThingId(5), HealthOf { card: c, balls: false, hp: vec![Hp { now: start, start }] });
        let mut h = Health { things, moments: Vec::new(), still: false, inherited: BTreeMap::new() };
        let (_, zero) = h.change(ThingId(5), None, HealthChange::Damage(1.0), AT).unwrap();
        assert!(zero);
    }

    #[test]
    fn starting_health_varies_by_the_spread_and_never_below_zero() {
        let mut c = card(2.0);
        let mut p = Params::default();
        p.set("start", Value::Number(2.0));
        p.set("start~to", Value::Number(10.0));
        p.set("start~by", Value::Choice("number".into()));
        c.start_vary = Some((10.0, vary::vary_of(&p, "start").unwrap()));
        let at = |number| c.start_at(Some(&bpe_core::BallSpread { number, count: 5, ..Default::default() }));
        assert_eq!((at(0), at(2), at(4)), (2.0, 6.0, 10.0));
        assert_eq!(c.start_at(None), 2.0);
        assert_eq!(card(-3.0).start_at(None), LEAST_START);
    }

    #[test]
    fn a_zero_template_is_aimed_at_what_reached_zero() {
        let show = std::sync::Arc::new(super::super::BreakShow::default());
        let template = Action::Break {
            thing: ThingId::ROOT,
            row: None,
            point: Vec2::ZERO,
            normal: Vec2::ZERO,
            strength: 0.0,
            show: show.clone(),
        };
        let a = aimed(&template, ThingId(3), Some(7), Vec2::new(1.0, 2.0), Vec2::Y, 300.0);
        assert_eq!(
            a,
            Action::Break {
                thing: ThingId(3),
                row: Some(7),
                point: Vec2::new(1.0, 2.0),
                normal: Vec2::Y,
                strength: 300.0,
                show
            }
        );
    }
}
