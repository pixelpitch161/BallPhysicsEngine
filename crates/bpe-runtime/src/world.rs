//! The world: every thing's simulation state, stepped at a fixed rate.
//!
//! Each step runs the spec's stages in order: Spawn, Scripted, Forces, Integrate, Collide,
//! Respond, Events, Rules. Everything iterates in `ThingId` order, so two runs of the same scene
//! give the same log.

use std::collections::{BTreeMap, BTreeSet};

use bpe_core::{Attach, Resolved, Scene, SlotId, Target, ThingId, Value};
use glam::Vec2;

use crate::events::HASH_OFFSET;
use crate::plugins::{Limits, Sims, ThingState};
use crate::{Bodies, Event, EventKind, Log, Party, Wall, keep_inside, reflect};

/// The furthest a ball may move in one substep without skipping a wall.
pub const SAFE_MOVE: f32 = 16.0;
/// The most substeps a step is split into.
pub const MAX_SUBSTEPS: u32 = 64;

/// A thing's prepared modules: per slot, the module kind and its state.
type Prepared = BTreeMap<SlotId, (String, Box<dyn ThingState>)>;

/// Which slots the runtime asks a `ModuleSim` about.
const SIM_SLOTS: [SlotId; 2] = [SlotId::Motion, SlotId::Bounce];

/// Everything that moves, and what it moves against.
#[derive(Clone)]
pub struct World {
    /// Steps taken so far.
    pub step: u64,
    pub steps_per_second: u32,
    /// Pixels per second squared, +y down.
    pub gravity: Vec2,
    /// The balls of every thing that has some.
    pub bodies: BTreeMap<ThingId, Bodies>,
    /// The closed wall of every thing that is one.
    pub walls: BTreeMap<ThingId, Wall>,
    /// Whatever a thing's sim keeps between steps.
    pub states: BTreeMap<ThingId, Box<dyn ThingState>>,
    /// The kind of every thing that has a sim, in id order.
    kinds: BTreeMap<ThingId, String>,
    /// The tags of every thing that has a sim, for targets.
    tags: BTreeMap<ThingId, BTreeSet<String>>,
    /// Per thing and slot, the module kind and its prepared state (Motion and Bounce only).
    modules: BTreeMap<ThingId, Prepared>,
}

const NO_TAGS: &BTreeSet<String> = &BTreeSet::new();

impl World {
    /// Builds the world: every resolved thing in id order through its sim, then the Motion and
    /// Bounce modules it uses. Things and modules without a sim are left out.
    pub fn build(scene: &Scene, resolved: &Resolved, sims: &Sims) -> World {
        let mut world = World {
            step: 0,
            steps_per_second: 240,
            gravity: Vec2::ZERO,
            bodies: BTreeMap::new(),
            walls: BTreeMap::new(),
            states: BTreeMap::new(),
            kinds: BTreeMap::new(),
            tags: BTreeMap::new(),
            modules: BTreeMap::new(),
        };
        for (id, r) in &resolved.things {
            let Some(thing) = scene.things.get(id) else { continue };
            let Some(sim) = sims.thing(&r.kind) else { continue };
            sim.build(thing, r, &mut world);
            world.kinds.insert(*id, r.kind.clone());
            if !thing.tags.is_empty() {
                world.tags.insert(*id, thing.tags.clone());
            }
            let mut prepared = Prepared::new();
            for slot in SIM_SLOTS {
                if let Some(s) = r.slots.get(&slot)
                    && let Some(msim) = sims.module(&s.kind)
                {
                    prepared.insert(slot, (s.kind.clone(), msim.prepare(&s.params)));
                }
            }
            if !prepared.is_empty() {
                world.modules.insert(*id, prepared);
            }
        }
        world
    }

    /// Seconds since the start.
    pub fn time(&self) -> f64 {
        self.step as f64 / self.steps_per_second as f64
    }

    /// Seconds per step.
    pub fn dt(&self) -> f32 {
        1.0 / self.steps_per_second as f32
    }

    pub fn limits(&self) -> Limits {
        Limits {
            ceiling: SAFE_MOVE * MAX_SUBSTEPS as f32 * self.steps_per_second as f32,
            resting: self.gravity.length() * self.dt() * 4.0 + 1.0,
        }
    }

    /// The kind of a thing that has a sim.
    pub fn kind(&self, thing: ThingId) -> Option<&str> {
        self.kinds.get(&thing).map(String::as_str)
    }

    /// A thing's tags (empty for a thing without any, or without a sim).
    pub fn tags(&self, thing: ThingId) -> &BTreeSet<String> {
        self.tags.get(&thing).unwrap_or(NO_TAGS)
    }

    /// The things with a sim that `target` names, in id order. `Anything` is every one of them,
    /// `me` included.
    pub fn matches(&self, target: &Target, me: ThingId) -> Vec<ThingId> {
        match target {
            Target::Me => self.kinds.contains_key(&me).then_some(me).into_iter().collect(),
            Target::Thing(id) => self.kinds.contains_key(id).then_some(*id).into_iter().collect(),
            Target::Tag(tag) => self.tags.iter().filter(|(_, t)| t.contains(tag)).map(|(id, _)| *id).collect(),
            Target::Kind(kind) => self.kinds.iter().filter(|(_, k)| *k == kind).map(|(id, _)| *id).collect(),
            Target::Anything => self.kinds.keys().copied().collect(),
        }
    }

    /// The module kind and prepared state a thing uses in a slot, when the runtime tracks it.
    pub fn module_state(&self, thing: ThingId, slot: SlotId) -> Option<(&str, &dyn ThingState)> {
        self.modules.get(&thing)?.get(&slot).map(|(k, s)| (k.as_str(), s.as_ref()))
    }

    /// Bytes the world takes, for playback budgets.
    pub fn memory(&self) -> usize {
        self.bodies.values().map(Bodies::memory).sum::<usize>() + std::mem::size_of::<World>()
    }

    /// Steps until `time` seconds have passed.
    pub fn advance_to(&mut self, time: f64, sims: &Sims, log: &mut Log) {
        let target = (time * self.steps_per_second as f64).floor() as u64;
        while self.step < target {
            self.step(sims, log);
        }
    }

    /// One step: every stage, in order.
    pub fn step(&mut self, sims: &Sims, log: &mut Log) {
        self.step += 1;
        let limits = self.limits();
        let dt = self.dt();
        let kinds: Vec<(ThingId, String)> = self.kinds.iter().map(|(id, k)| (*id, k.clone())).collect();

        // Spawn: emitters add what they release this step.
        for (id, kind) in &kinds {
            if let Some(sim) = sims.thing(kind) {
                sim.spawn(*id, self, log);
            }
        }

        // Clamp speeds, remember where everything started and find the fastest ball.
        let mut fastest: f32 = 0.0;
        for bodies in self.bodies.values_mut() {
            for i in 0..bodies.len() {
                if !bodies.alive[i] {
                    continue;
                }
                let speed = bodies.velocity[i].length();
                if speed > limits.ceiling {
                    bodies.velocity[i] *= limits.ceiling / speed;
                }
                // Silent bodies (liquid particles) are held inside the walls by their own solver,
                // so they never decide how finely everything else is stepped.
                if !bodies.silent {
                    fastest = fastest.max(speed.min(limits.ceiling));
                }
                bodies.previous[i] = bodies.position[i];
            }
        }
        let travel = (fastest + self.gravity.length() * dt) * dt;
        let substeps = ((travel / SAFE_MOVE).ceil() as u32).clamp(1, MAX_SUBSTEPS);
        let sub_dt = dt / substeps as f32;

        // Scripted: walls spin and shrink, things follow their scripts.
        for (id, kind) in &kinds {
            if let Some(sim) = sims.thing(kind) {
                sim.scripted(*id, self);
            }
        }

        for _ in 0..substeps {
            self.substep(sims, &kinds, sub_dt, &limits, log);
        }

        // Events: zone entries and leaves, and whatever else a thing notices at the end.
        for (id, kind) in &kinds {
            if let Some(sim) = sims.thing(kind) {
                sim.after_step(*id, self, log);
            }
        }
    }

    fn substep(&mut self, sims: &Sims, kinds: &[(ThingId, String)], dt: f32, limits: &Limits, log: &mut Log) {
        // Forces from things: fields and the like, before the Motion modules.
        for (id, kind) in kinds {
            if let Some(sim) = sims.thing(kind) {
                sim.forces(*id, self, dt);
            }
        }
        self.move_bodies(sims, dt);

        // Settle: liquids relax their particles against each other and the walls.
        for (id, kind) in kinds {
            if let Some(sim) = sims.thing(kind) {
                sim.settle(*id, self, dt);
            }
        }
        self.collide(sims, limits, log);
    }

    /// Forces from the Motion modules, then Integrate, per body thing.
    fn move_bodies(&mut self, sims: &Sims, dt: f32) {
        let World { gravity, bodies, modules, .. } = self;
        let gravity = *gravity;
        for (id, bodies) in bodies.iter_mut() {
            if let Some((kind, state)) = modules.get(id).and_then(|m| m.get(&SlotId::Motion))
                && let Some(sim) = sims.module(kind)
            {
                sim.forces(state.as_ref(), bodies, gravity, dt);
            }
            for i in 0..bodies.len() {
                if bodies.alive[i] {
                    let v = bodies.velocity[i];
                    bodies.position[i] += v * dt;
                }
            }
        }
    }

    /// Collide and Respond, then the Hit events.
    fn collide(&mut self, sims: &Sims, limits: &Limits, log: &mut Log) {
        let World { step, bodies, walls, modules, .. } = self;
        let step = *step;
        for (mover, bodies) in bodies.iter_mut() {
            let mover_bounce = modules.get(mover).and_then(|m| m.get(&SlotId::Bounce));
            for (wall_id, wall) in walls.iter() {
                let wall_bounce = modules.get(wall_id).and_then(|m| m.get(&SlotId::Bounce));
                for i in 0..bodies.len() {
                    if !bodies.alive[i] {
                        continue;
                    }
                    let velocity = bodies.velocity[i];
                    let radius = bodies.radius[i];
                    let Some(contact) = keep_inside(&mut bodies.position[i], velocity, radius, wall) else { continue };
                    let decider = wall_bounce
                        .and_then(|(kind, state)| {
                            let sim = sims.module(kind)?;
                            sim.takes_over(state.as_ref()).then_some((sim, state))
                        })
                        .or_else(|| mover_bounce.and_then(|(kind, state)| Some((sims.module(kind)?, state))));
                    match decider {
                        Some((sim, state)) => sim.respond(state.as_ref(), &contact, bodies, i, limits),
                        None => bodies.velocity[i] = reflect(velocity, &contact, 1.0),
                    }
                    if contact.speed >= limits.resting && !bodies.silent {
                        bodies.hits[i] += 1;
                        log.push(Event {
                            step,
                            kind: EventKind::Hit,
                            a: Party { thing: *mover, item: i as u32 },
                            b: Some(Party { thing: *wall_id, item: 0 }),
                            point: contact.point,
                            normal: contact.normal,
                            speed: contact.speed,
                        });
                    }
                }
            }
        }
    }
}

/// A hash of everything about a scene that changes how it simulates. Scenes that differ only
/// in how they look or sound fingerprint the same, so the editor can keep playing through
/// those edits.
pub fn fingerprint(scene: &Scene, resolved: &Resolved, sims: &Sims) -> u64 {
    let mut h = Hasher(HASH_OFFSET);
    for (id, r) in &resolved.things {
        let Some(thing) = scene.things.get(id) else { continue };
        h.bytes(&id.0.to_le_bytes());
        h.bytes(r.kind.as_bytes());
        h.bytes(&thing.parent.map_or(u32::MAX, |p| p.0).to_le_bytes());
        h.f32(thing.transform.position.x);
        h.f32(thing.transform.position.y);
        h.f32(thing.transform.rotation);
        match thing.attach {
            Attach::Group => h.bytes(&[0]),
            Attach::Ride { offset, turn_with_parent, item } => {
                h.bytes(&[1, turn_with_parent as u8]);
                h.f32(offset.x);
                h.f32(offset.y);
                h.bytes(&item.map_or(u32::MAX, |i| i).to_le_bytes());
            }
        }
        for tag in &thing.tags {
            h.bytes(tag.as_bytes());
            h.bytes(&[0]);
        }
        if let Some(sim) = sims.thing(&r.kind) {
            for (key, value) in r.params.iter() {
                if sim.affects_physics(key) {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
        }
        for slot in SIM_SLOTS {
            if let Some(s) = r.slots.get(&slot) {
                h.bytes(&[slot as u8]);
                h.bytes(s.kind.as_bytes());
                for (key, value) in s.params.iter() {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
        }
    }
    h.0
}

struct Hasher(u64);

impl Hasher {
    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }

    fn f32(&mut self, x: f32) {
        self.bytes(&x.to_bits().to_le_bytes());
    }

    fn value(&mut self, v: &Value) {
        match v {
            Value::Number(x) => {
                self.bytes(&[0]);
                self.bytes(&x.to_bits().to_le_bytes());
            }
            Value::Int(x) => {
                self.bytes(&[1]);
                self.bytes(&x.to_le_bytes());
            }
            Value::Bool(b) => self.bytes(&[2, *b as u8]),
            Value::Vec2([x, y]) => {
                self.bytes(&[3]);
                self.f32(*x);
                self.f32(*y);
            }
            Value::Color(c) => self.bytes(&[4, c.r, c.g, c.b, c.a]),
            Value::Colors(cs) => {
                self.bytes(&[5]);
                for c in cs {
                    self.bytes(&[c.r, c.g, c.b, c.a]);
                }
            }
            Value::Choice(s) | Value::Text(s) => {
                self.bytes(&[6]);
                self.bytes(s.as_bytes());
            }
            Value::Target(t) => {
                self.bytes(&[7]);
                self.bytes(format!("{t:?}").as_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::Sims;
    use crate::testing::{DOTS, EMITTER, TestDots, WALL, dots_in_ring, dots_in_sticky_ring, dots_with_emitter, reg};
    use bpe_core::{SlotId, Target, ThingId, Value, resolve};

    #[test]
    fn two_runs_give_the_same_log_and_a_ball_bounces() {
        let (scene, resolved, sims) = dots_in_ring(1);
        let run = || {
            let mut w = World::build(&scene, &resolved, &sims);
            let mut log = Log::default();
            w.advance_to(5.0, &sims, &mut log);
            (w, log)
        };
        let (w1, log1) = run();
        let (_, log2) = run();
        assert_eq!(log1, log2);
        assert!(log1.len() >= 3, "a launched ball under gravity should hit the ring a few times in 5 s");
        assert_eq!(log1.events()[0].kind, EventKind::Hit);
        assert_eq!(log1.events()[0].a, Party { thing: DOTS, item: 0 });
        assert_eq!(log1.events()[0].b, Some(Party { thing: WALL, item: 0 }));
        assert!(w1.bodies[&DOTS].position[0].length() < 100.0);
        assert_eq!(w1.step, 1200);
        assert_eq!(w1.time(), 5.0);
        assert_eq!(w1.bodies[&DOTS].hits[0] as usize, log1.len());
    }

    #[test]
    fn things_run_in_id_order_and_events_are_step_sorted() {
        let (scene, resolved, sims) = dots_in_ring(50);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(3.0, &sims, &mut log);
        assert!(log.len() > 50);
        assert!(log.events().windows(2).all(|p| p[0].step <= p[1].step));
        assert!(log.events().windows(2).filter(|p| p[0].step == p[1].step).all(|p| p[0].a <= p[1].a));
    }

    #[test]
    fn a_resting_touch_is_not_a_hit() {
        let (scene, resolved, sims) = dots_in_ring(1);
        let mut w = World::build(&scene, &resolved, &sims);
        // Put the ball on the floor of the ring, at rest.
        w.bodies.get_mut(&DOTS).unwrap().position[0] = Vec2::new(0.0, 94.0);
        w.bodies.get_mut(&DOTS).unwrap().velocity[0] = Vec2::ZERO;
        let mut log = Log::default();
        w.advance_to(1.0, &sims, &mut log);
        assert!(log.is_empty(), "{:?}", log.events());
        assert!(w.bodies[&DOTS].position[0].y <= 94.0 + 1e-3);
    }

    #[test]
    fn the_wall_can_take_over_the_bounce() {
        let (scene, resolved, sims) = dots_in_sticky_ring(true);
        let mut w = World::build(&scene, &resolved, &sims);
        w.advance_to(2.0, &sims, &mut Log::default());
        assert!(w.bodies[&DOTS].velocity[0].length() < 50.0);
    }

    #[test]
    fn without_taking_over_the_mover_decides() {
        let (scene, resolved, sims) = dots_in_sticky_ring(false);
        let mut w = World::build(&scene, &resolved, &sims);
        w.advance_to(2.0, &sims, &mut Log::default());
        assert!(w.bodies[&DOTS].velocity[0].length() > 50.0);
        assert_eq!(w.module_state(DOTS, SlotId::Bounce).map(|(k, _)| k), Some("bounce.bouncy"));
        assert_eq!(w.module_state(WALL, SlotId::Bounce).map(|(k, _)| k), Some("bounce.sticky"));
        assert!(w.module_state(WALL, SlotId::Look).is_none());
    }

    #[test]
    fn speeds_are_clamped_to_the_ceiling() {
        let (scene, resolved, sims) = dots_in_ring(1);
        let mut w = World::build(&scene, &resolved, &sims);
        w.bodies.get_mut(&DOTS).unwrap().velocity[0] = Vec2::new(1.0e9, 0.0);
        let mut log = Log::default();
        w.advance_to(1.0, &sims, &mut log);
        let p = w.bodies[&DOTS].position[0];
        assert!(p.is_finite() && p.length() < 100.0);
        assert!(w.bodies[&DOTS].velocity[0].length() <= w.limits().ceiling + 1.0);
        assert_eq!(w.limits().ceiling, 16.0 * 64.0 * 240.0);
    }

    #[test]
    fn look_changes_keep_the_fingerprint() {
        let (mut scene, resolved, sims) = dots_in_ring(1);
        let before = fingerprint(&scene, &resolved, &sims);
        scene.things.get_mut(&DOTS).unwrap().params.set("tint", Value::Number(3.0));
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("tint", Value::Number(3.0));
        let resolved = resolve(&scene, &reg());
        assert_eq!(fingerprint(&scene, &resolved, &sims), before);
        scene.things.get_mut(&DOTS).unwrap().transform.position.x += 1.0;
        let resolved = resolve(&scene, &reg());
        let moved = fingerprint(&scene, &resolved, &sims);
        assert_ne!(moved, before);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 1.0]));
        let resolved = resolve(&scene, &reg());
        assert_ne!(fingerprint(&scene, &resolved, &sims), moved);
    }

    #[test]
    fn a_thing_without_a_sim_is_skipped() {
        let (mut scene, _, sims) = dots_in_ring(1);
        let reg = reg();
        let extra = scene.add_thing(&reg, "wall", ThingId::ROOT).unwrap();
        scene.things.get_mut(&extra).unwrap().kind = "scene".into();
        let mut thin = Sims::default();
        thin.add_thing(TestDots);
        let resolved = resolve(&scene, &reg);
        let w = World::build(&scene, &resolved, &thin);
        assert!(w.walls.is_empty());
        assert_eq!(w.bodies.len(), 1);
        assert_eq!(w.steps_per_second, 240);
        drop(sims);
    }
    #[test]
    fn hooks_run_in_spec_order_and_can_add_bodies() {
        let (scene, resolved, sims) = dots_with_emitter();
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(0.5, &sims, &mut log);
        assert_eq!(w.bodies[&DOTS].len(), 1 + 120);
        assert_eq!(log.events()[0].kind, EventKind::Spawn, "spawn comes first in a step");
        assert_eq!(log.events()[0].a, Party { thing: DOTS, item: 1 });
        assert_eq!(log.events()[0].b, Some(Party { thing: EMITTER, item: 0 }));
        let ticks = log.events().iter().filter(|e| e.kind == EventKind::Custom("tick".into())).count();
        assert_eq!(ticks, 120, "after_step once per step");
        // Within a step: spawn, then any hits, then the tick.
        let first_step: Vec<_> = log.events().iter().filter(|e| e.step == 1).map(|e| e.kind.tag()).collect();
        assert_eq!(first_step.first(), Some(&1));
        assert_eq!(first_step.last(), Some(&7));
        let settled = *crate::state::<u32>(w.states[&EMITTER].as_ref());
        assert!(settled > 100, "settle saw the move after integrate on most substeps: {settled}");
        // One step from the start, before any wall is met: the forces hook has added its push.
        let mut w = World::build(&scene, &resolved, &sims);
        w.step(&sims, &mut Log::default());
        let vx = w.bodies[&DOTS].velocity[0].x;
        assert!((vx - (300.0 + 1000.0 / 240.0)).abs() < 1e-3, "the forces hook pushed the ball: {vx}");
    }

    #[test]
    fn silent_bodies_log_no_hits() {
        let (scene, resolved, sims) = dots_in_ring(1);
        let mut w = World::build(&scene, &resolved, &sims);
        w.bodies.get_mut(&DOTS).unwrap().silent = true;
        let mut log = Log::default();
        w.advance_to(5.0, &sims, &mut log);
        assert!(log.is_empty(), "{:?}", log.events());
        assert_eq!(w.bodies[&DOTS].hits[0], 0);
        assert!(w.bodies[&DOTS].position[0].length() < 100.0, "it still bounces off the wall");
    }

    #[test]
    fn silent_bodies_do_not_add_substeps() {
        let (scene, resolved, sims) = dots_in_ring(1);
        let plain = {
            let mut w = World::build(&scene, &resolved, &sims);
            let mut log = Log::default();
            w.advance_to(3.0, &sims, &mut log);
            log
        };
        let mut w = World::build(&scene, &resolved, &sims);
        let mut fast = Bodies { silent: true, ..Bodies::default() };
        fast.push(Vec2::ZERO, Vec2::new(50_000.0, 0.0), 2.0);
        w.bodies.insert(ThingId(9), fast);
        let mut log = Log::default();
        w.advance_to(3.0, &sims, &mut log);
        assert_eq!(log, plain, "a liquid's speed never changes how the balls are stepped");
        assert!(w.bodies[&ThingId(9)].position[0].is_finite() && w.bodies[&ThingId(9)].position[0].length() < 100.0);
    }

    #[test]
    fn targets_resolve_by_tag_kind_thing_and_me() {
        let (mut scene, _, sims) = dots_in_ring(1);
        scene.things.get_mut(&DOTS).unwrap().tags.insert("red".into());
        let resolved = resolve(&scene, &reg());
        let w = World::build(&scene, &resolved, &sims);
        assert_eq!(w.matches(&Target::Tag("red".into()), WALL), vec![DOTS]);
        assert_eq!(w.matches(&Target::Tag("blue".into()), WALL), vec![]);
        assert_eq!(w.matches(&Target::Kind("wall".into()), DOTS), vec![WALL]);
        assert_eq!(w.matches(&Target::Thing(DOTS), WALL), vec![DOTS]);
        assert_eq!(w.matches(&Target::Thing(ThingId(99)), WALL), vec![]);
        assert_eq!(w.matches(&Target::Me, WALL), vec![WALL]);
        assert_eq!(w.matches(&Target::Anything, WALL), vec![ThingId::ROOT, WALL, DOTS]);
        assert_eq!(w.kind(DOTS), Some("dots"));
        assert_eq!(w.kind(ThingId(99)), None);
        assert!(w.tags(DOTS).contains("red"));
        assert!(w.tags(WALL).is_empty());
    }

    #[test]
    fn tags_change_the_fingerprint() {
        let (mut scene, resolved, sims) = dots_in_ring(1);
        let before = fingerprint(&scene, &resolved, &sims);
        scene.things.get_mut(&DOTS).unwrap().tags.insert("red".into());
        let resolved = resolve(&scene, &reg());
        assert_ne!(fingerprint(&scene, &resolved, &sims), before);
    }
}
