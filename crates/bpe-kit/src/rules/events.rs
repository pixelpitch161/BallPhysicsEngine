//! When blocks for what happens to balls and things: Enters / Leaves, Fired, Removed / Broken.

use std::collections::{BTreeMap, BTreeSet};

use bpe_core::{BlockSection, Choice, Params, Settings, ThingId, Who};
use bpe_runtime::rules::WHOLE;
use bpe_runtime::{
    BlockSim, EventKind, Firing, Party, PrepareCx, StageCx, ThingState, Values, World, state, state_mut,
};

use super::block_kind;
use super::when::ball_values;
use super::who::{matches, matches_thing};
use crate::things::emitter::sim::emitter_state;

/// Which way Enters / Leaves watches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum EnterWay {
    /// The ball comes into the shape.
    #[default]
    Enters,
    /// The ball leaves it.
    Leaves,
    /// Either.
    Either,
}

/// When a ball comes into a shape (or an emitter's catcher) or leaves a shape.
pub struct Enter;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct EnterSettings {
    /// The balls watched.
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    /// The shapes (or emitters) they come into or leave. A shape set to let balls pass through is
    /// a zone; one that bounces them can only be come into by a ball put there.
    #[setting(label = "Shape", simple, group = "Rule")]
    pub zone: Who,
    #[setting(label = "When it", simple, group = "Rule")]
    pub way: EnterWay,
}

impl Default for EnterSettings {
    fn default() -> Self {
        EnterSettings { who: Who::Anything, zone: Who::Anything, way: EnterWay::Enters }
    }
}

block_kind!(
    Enter,
    "when.enter",
    "Enters / Leaves",
    When,
    EnterSettings,
    "Fires when a ball comes into a shape (or an emitter's catcher), or leaves a shape."
);

/// A ball in a zone: (zone, the ball's thing, its body row).
type Inside = (ThingId, ThingId, u32);

#[derive(Clone, Debug)]
struct EnterState {
    s: EnterSettings,
    /// Every live ball inside a zone watched at the last stage (whoever it is: a ball that
    /// starts or stops matching Who hasn't moved).
    inside: BTreeSet<Inside>,
    /// The zones watched at the last stage: one that starts matching only notes who is in it.
    zones: BTreeSet<ThingId>,
    /// How many rows each thing had at the last stage: a ball made since (fired, released,
    /// spawned or split off) inside a zone was born there and doesn't come in.
    rows: BTreeMap<ThingId, usize>,
}

/// Every live ball inside one of `zones`, in order.
fn inside_now(world: &World, zones: &BTreeSet<ThingId>) -> BTreeSet<Inside> {
    let mut out = BTreeSet::new();
    for &zone in zones {
        let Some(wall) = world.walls.get(&zone) else { continue };
        for (&thing, bodies) in &world.bodies {
            for row in 0..bodies.len() {
                if bodies.alive[row] && wall.outline.inside(bodies.position[row]) {
                    out.insert((zone, thing, row as u32));
                }
            }
        }
    }
    out
}

fn rows_of(world: &World) -> BTreeMap<ThingId, usize> {
    world.bodies.iter().map(|(&t, b)| (t, b.len())).collect()
}

impl BlockSim for Enter {
    fn kind(&self) -> &'static str {
        "when.enter"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = EnterSettings::from_params(params);
        // The balls inside at the start are there already: they never "come in".
        let w = cx.world;
        let zones = w.walls.keys().copied().filter(|&z| matches_thing(w, &s.zone, Party { thing: z, item: WHOLE }));
        let zones: BTreeSet<ThingId> = zones.collect();
        Ok(Box::new(EnterState { inside: inside_now(w, &zones), zones, rows: rows_of(w), s }))
    }
    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        let e = state_mut::<EnterState>(st);
        let world = cx.world;
        let zones: BTreeSet<ThingId> = world
            .walls
            .keys()
            .copied()
            .filter(|&z| !cx.is_broken(z) && matches(cx, &e.s.zone, Party { thing: z, item: WHOLE }))
            .collect();
        let now = inside_now(world, &zones);
        let live = |thing: ThingId, row: u32| world.bodies.get(&thing).is_some_and(|b| b.alive[row as usize]);
        let fire = |(zone, thing, row): Inside, entered: bool, out: &mut Vec<Firing>| {
            let ball = Party { thing, item: row };
            let mut values = Values::default();
            ball_values(world, ball, &mut values);
            values.set("way", if entered { 1.0 } else { 0.0 });
            out.push(Firing {
                step: cx.step,
                subject: Some(ball),
                other: Some(Party { thing: zone, item: WHOLE }),
                point: world.bodies[&thing].position[row as usize],
                normal: glam::Vec2::ZERO,
                values,
            });
        };
        let (enters, leaves) = match e.s.way {
            EnterWay::Enters => (true, false),
            EnterWay::Leaves => (false, true),
            EnterWay::Either => (true, true),
        };
        // In (zone, thing, row) order: comings in and goings out together.
        let changed: BTreeSet<Inside> = now.symmetric_difference(&e.inside).copied().collect();
        for k in changed {
            let (zone, thing, row) = k;
            let entered = now.contains(&k);
            // A zone that starts or stops being watched moves nobody.
            if !(zones.contains(&zone) && e.zones.contains(&zone)) {
                continue;
            }
            // A ball born inside never came in; a ball that is gone leaves nothing.
            let born = e.rows.get(&thing).is_none_or(|&n| row as usize >= n);
            if (entered && born) || (!entered && !live(thing, row)) {
                continue;
            }
            if !matches(cx, &e.s.who, Party { thing, item: row }) {
                continue;
            }
            if (entered && enters) || (!entered && leaves) {
                fire(k, entered, out);
            }
        }
        // An emitter's catcher: the balls it saw come in this step.
        if enters {
            for ev in cx.events.iter().filter(|ev| ev.kind == EventKind::Enter) {
                let Some(catcher) = ev.b else { continue };
                if cx.is_broken(catcher.thing)
                    || !matches(cx, &e.s.zone, Party { thing: catcher.thing, item: WHOLE })
                    || !matches(cx, &e.s.who, ev.a)
                {
                    continue;
                }
                let mut values = Values::default();
                ball_values(world, ev.a, &mut values);
                values.set("way", 1.0);
                values.set("round", catcher.item as f64);
                out.push(Firing {
                    step: cx.step,
                    subject: Some(ev.a),
                    other: Some(Party { thing: catcher.thing, item: WHOLE }),
                    point: ev.point,
                    normal: glam::Vec2::ZERO,
                    values,
                });
            }
        }
        e.inside = now;
        e.zones = zones;
        e.rows = rows_of(world);
    }
}

/// What Fired fires on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FiredEach {
    /// Each ball the emitter fires.
    #[default]
    ABall,
    /// Each round, as it fires.
    ARound,
}

/// When an emitter fires.
pub struct Fired;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FiredSettings {
    /// The emitters watched.
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Each", simple, group = "Rule")]
    pub each: FiredEach,
}

impl Default for FiredSettings {
    fn default() -> Self {
        FiredSettings { who: Who::Anything, each: FiredEach::ABall }
    }
}

block_kind!(Fired, "when.fired", "Fired", When, FiredSettings, "Fires when an emitter fires a ball, or a round.");

impl BlockSim for Fired {
    fn kind(&self) -> &'static str {
        "when.fired"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(FiredSettings::from_params(params)))
    }
    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        let s = state::<FiredSettings>(st);
        let world = cx.world;
        let emitter_ok = |t: ThingId| !cx.is_broken(t) && matches(cx, &s.who, Party { thing: t, item: WHOLE });
        match s.each {
            FiredEach::ABall => {
                for ev in cx.events.iter().filter(|ev| ev.kind == EventKind::Spawn) {
                    let Some(by) = ev.b else { continue };
                    if !emitter_ok(by.thing) {
                        continue;
                    }
                    let mut values = Values::default();
                    ball_values(world, ev.a, &mut values);
                    values.set("round", by.item as f64);
                    values.set("speed", ev.speed as f64);
                    out.push(Firing {
                        step: cx.step,
                        subject: Some(ev.a),
                        other: Some(Party { thing: by.thing, item: WHOLE }),
                        point: ev.point,
                        normal: glam::Vec2::ZERO,
                        values,
                    });
                }
            }
            FiredEach::ARound => {
                for &thing in world.states.keys() {
                    let Some(em) = emitter_state(world, thing) else { continue };
                    if !emitter_ok(thing) {
                        continue;
                    }
                    let place = world.poses.get(&thing).map_or(glam::Vec2::ZERO, |p| p.position);
                    // Rounds fired this step (the latest are last).
                    let first = em.fired.partition_point(|f| f.step < cx.step);
                    for f in &em.fired[first..] {
                        let mut values = Values::default();
                        values.set("round", f.round as f64);
                        values.set("balls", f.size as f64);
                        values.set("count", em.rounds_fired as f64);
                        out.push(Firing {
                            step: cx.step,
                            subject: Some(Party { thing, item: WHOLE }),
                            other: None,
                            point: place,
                            normal: glam::Vec2::ZERO,
                            values,
                        });
                    }
                }
            }
        }
    }
}

/// What Removed / Broken fires on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Gone {
    /// A ball taken out for any reason: caught, faded, popped by its Bounce card, broken.
    #[default]
    Removed,
    /// A ball or a whole thing a rule broke.
    Broken,
}

/// When a ball is removed, or a ball or thing is broken.
pub struct Removed;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct RemovedSettings {
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Is", simple, group = "Rule")]
    pub what: Gone,
}

impl Default for RemovedSettings {
    fn default() -> Self {
        RemovedSettings { who: Who::Anything, what: Gone::Removed }
    }
}

block_kind!(
    Removed,
    "when.removed",
    "Removed / Broken",
    When,
    RemovedSettings,
    "Fires when a ball is taken out, or a ball or a thing is broken."
);

impl BlockSim for Removed {
    fn kind(&self) -> &'static str {
        "when.removed"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(RemovedSettings::from_params(params)))
    }
    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        let s = state::<RemovedSettings>(st);
        let kind = match s.what {
            Gone::Removed => EventKind::Remove,
            Gone::Broken => EventKind::Break,
        };
        for ev in cx.events.iter().filter(|ev| ev.kind == kind) {
            if !matches(cx, &s.who, ev.a) {
                continue;
            }
            let mut values = Values::default();
            ball_values(cx.world, ev.a, &mut values);
            if kind == EventKind::Break {
                values.set("strength", ev.speed as f64);
            }
            out.push(Firing {
                step: cx.step,
                subject: Some(ev.a),
                other: None,
                point: ev.point,
                normal: ev.normal,
                values,
            });
        }
    }
}
