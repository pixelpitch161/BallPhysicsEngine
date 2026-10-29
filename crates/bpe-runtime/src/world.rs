//! The world: every thing's simulation state, stepped at a fixed rate.
//!
//! Each step runs the spec's stages in order: Spawn, Scripted, Forces, Integrate, Collide,
//! Respond, Events, Rules. Everything iterates in `ThingId` order, so two runs of the same scene
//! give the same log. Rules queue actions; the next step's Spawn stage applies them.

use std::collections::{BTreeMap, BTreeSet};

use bpe_core::{Attach, ModuleGraph, Params, Resolved, Scene, SlotId, Target, ThingId, Value};
use glam::Vec2;

use crate::events::HASH_OFFSET;
use crate::plugins::{Limits, RuleCx, SimCx, Sims, ThingState};
use crate::{Action, Bodies, Event, EventKind, History, Log, Party, Wall, keep_inside, reflect};

/// The furthest a ball may move in one substep without skipping a wall.
pub const SAFE_MOVE: f32 = 16.0;
/// The most substeps a step is split into.
pub const MAX_SUBSTEPS: u32 = 64;

/// One module a thing uses, prepared: its kind, the settings it was prepared from, its graph
/// and its state.
#[derive(Clone)]
struct Slotted {
    kind: String,
    params: Params,
    graph: Option<ModuleGraph>,
    state: Box<dyn ThingState>,
}

/// A thing's prepared modules, per slot.
type Prepared = BTreeMap<SlotId, Slotted>;

/// Which slots the runtime asks a `ModuleSim` about.
const SIM_SLOTS: [SlotId; 3] = [SlotId::Motion, SlotId::Bounce, SlotId::Rules];

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
    /// Where the balls of a thing have been, for the things whose sim asked for it (trails).
    pub history: BTreeMap<ThingId, History>,
    /// The scene's seed, for anything random; the scene's sim sets it.
    pub seed: u64,
    /// What the rules asked for in the last step, applied at the start of the next.
    pub pending: Vec<Action>,
    /// The kind of every thing that has a sim, in id order.
    kinds: BTreeMap<ThingId, String>,
    /// Every thing's tags.
    tags: BTreeMap<ThingId, BTreeSet<String>>,
    /// Per thing and slot, the module kind and its prepared state (Motion, Bounce and Rules).
    modules: BTreeMap<ThingId, Prepared>,
    /// Per thing that links a Rules module, the things whose Rules slot comes from it: what
    /// "me" means to that module.
    scope: BTreeMap<ThingId, BTreeSet<ThingId>>,
}

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
            history: BTreeMap::new(),
            seed: 0,
            pending: Vec::new(),
            kinds: BTreeMap::new(),
            tags: BTreeMap::new(),
            modules: BTreeMap::new(),
            scope: BTreeMap::new(),
        };
        for (id, r) in &resolved.things {
            let Some(thing) = scene.things.get(id) else { continue };
            // A thing without a sim (a folder, say) builds nothing, but it can still be named
            // by a target and carry a Rules module.
            if let Some(sim) = sims.thing(&r.kind) {
                sim.build(thing, r, &mut world);
            }
            world.kinds.insert(*id, r.kind.clone());
            world.tags.insert(*id, thing.tags.clone());
            let mut prepared = Prepared::new();
            for slot in SIM_SLOTS {
                let Some(s) = r.slots.get(&slot) else { continue };
                if slot == SlotId::Rules {
                    // A Rules module runs once, on the thing that links it, for every thing
                    // that inherits it.
                    world.scope.entry(s.from).or_default().insert(*id);
                    if s.from != *id {
                        continue;
                    }
                }
                if let Some(msim) = sims.module(&s.kind) {
                    let graph = scene.library.modules.get(&s.module).and_then(|m| m.graph.clone());
                    let state = msim.prepare_graph(&s.params, graph.as_ref());
                    prepared.insert(slot, Slotted { kind: s.kind.clone(), params: s.params.clone(), graph, state });
                }
            }
            if !prepared.is_empty() {
                world.modules.insert(*id, prepared);
            }
        }
        world
    }

    /// The kind of a thing the world built.
    pub fn kind_of(&self, id: ThingId) -> Option<&str> {
        self.kinds.get(&id).map(String::as_str)
    }

    /// A built thing's tags.
    pub fn tags_of(&self, id: ThingId) -> Option<&BTreeSet<String>> {
        self.tags.get(&id)
    }

    /// Whether `thing` uses the Rules module linked on `owner`. A thing is always in its own
    /// scope, so "me" means at least the thing itself.
    pub fn in_scope(&self, owner: ThingId, thing: ThingId) -> bool {
        (owner == thing && self.kinds.contains_key(&thing))
            || self.scope.get(&owner).is_some_and(|s| s.contains(&thing))
    }

    /// Whether a thing belongs to a target, as the rules on `me` see it.
    pub fn matches(&self, me: ThingId, thing: ThingId, target: &Target) -> bool {
        match target {
            Target::Me => self.in_scope(me, thing),
            Target::Thing(id) => *id == thing && self.kinds.contains_key(&thing),
            Target::Tag(tag) => self.tags.get(&thing).is_some_and(|t| t.contains(tag)),
            Target::Kind(kind) => self.kinds.get(&thing).is_some_and(|k| k == kind),
            Target::Anything => self.kinds.contains_key(&thing),
        }
    }

    /// Every built thing a target names, as the rules on `me` see it, in id order.
    pub fn things_of(&self, me: ThingId, target: &Target) -> Vec<ThingId> {
        self.kinds.keys().copied().filter(|&t| self.matches(me, t, target)).collect()
    }

    /// The settings a thing's module in a slot was prepared from, when the runtime tracks it.
    pub fn slot_params(&self, thing: ThingId, slot: SlotId) -> Option<&Params> {
        self.modules.get(&thing)?.get(&slot).map(|s| &s.params)
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

    /// The module kind and prepared state a thing uses in a slot, when the runtime tracks it.
    pub fn module_state(&self, thing: ThingId, slot: SlotId) -> Option<(&str, &dyn ThingState)> {
        self.modules.get(&thing)?.get(&slot).map(|s| (s.kind.as_str(), s.state.as_ref()))
    }

    /// Changes one setting of the module a thing uses in a slot and prepares it again. A thing
    /// without that slot, or a setting the module doesn't have, is left alone.
    pub(crate) fn set_slot_param(&mut self, thing: ThingId, slot: SlotId, key: &str, value: Value, sims: &Sims) {
        let Some(s) = self.modules.get_mut(&thing).and_then(|m| m.get_mut(&slot)) else { return };
        if s.params.get(key).is_none() {
            return;
        }
        let Some(sim) = sims.module(&s.kind) else { return };
        s.params.set(key, value);
        s.state = sim.prepare_graph(&s.params, s.graph.as_ref());
    }

    /// Bytes the world takes, for playback budgets.
    pub fn memory(&self) -> usize {
        self.bodies.values().map(Bodies::memory).sum::<usize>()
            + self.history.values().map(History::memory).sum::<usize>()
            + std::mem::size_of::<World>()
    }

    /// Starts keeping `thing`'s ball positions, so a trail can draw where they have been.
    pub fn keep_history(&mut self, thing: ThingId) {
        let rate = self.steps_per_second;
        self.history.entry(thing).or_insert_with(|| History::new(rate));
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

        // Spawn: what the rules asked for last step.
        if !self.pending.is_empty() {
            let pending = std::mem::take(&mut self.pending);
            self.apply(pending, sims, log);
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
                fastest = fastest.max(speed.min(limits.ceiling));
                bodies.previous[i] = bodies.position[i];
            }
        }
        let travel = (fastest + self.gravity.length() * dt) * dt;
        let substeps = ((travel / SAFE_MOVE).ceil() as u32).clamp(1, MAX_SUBSTEPS);
        let sub_dt = dt / substeps as f32;

        // Scripted: walls spin and shrink, things follow their scripts.
        let kinds: Vec<(ThingId, String)> = self.kinds.iter().map(|(id, k)| (*id, k.clone())).collect();
        for (id, kind) in &kinds {
            if let Some(sim) = sims.thing(kind) {
                sim.scripted(*id, self);
            }
        }

        for _ in 0..substeps {
            self.substep(sims, sub_dt, &limits, log);
        }

        for (id, history) in &mut self.history {
            if let Some(bodies) = self.bodies.get(id) {
                history.record(self.step, &bodies.position);
            }
        }
        // Rules: every Rules module in owner order, on this step's events.
        let owners: Vec<ThingId> =
            self.modules.iter().filter(|(_, m)| m.contains_key(&SlotId::Rules)).map(|(id, _)| *id).collect();
        let mut actions = Vec::new();
        for owner in owners {
            let Some(mut slotted) = self.modules.get_mut(&owner).and_then(|m| m.remove(&SlotId::Rules)) else {
                continue;
            };
            if let Some(sim) = sims.module(&slotted.kind) {
                let mut cx = RuleCx { thing: owner, world: self, events: log.since(self.step), actions: &mut actions };
                sim.rules(slotted.state.as_mut(), &mut cx);
            }
            self.modules.entry(owner).or_default().insert(SlotId::Rules, slotted);
        }
        self.pending = actions;
    }

    fn substep(&mut self, sims: &Sims, dt: f32, limits: &Limits, log: &mut Log) {
        let World { step, steps_per_second, gravity, bodies, walls, modules, seed, .. } = self;
        let step = *step;
        let gravity = *gravity;
        let time = step as f32 / *steps_per_second as f32;

        // Forces, then Integrate, per body thing.
        for (id, bodies) in bodies.iter_mut() {
            if let Some(s) = modules.get(id).and_then(|m| m.get(&SlotId::Motion))
                && let Some(sim) = sims.module(&s.kind)
            {
                sim.forces_at(s.state.as_ref(), bodies, gravity, dt, &SimCx { thing: *id, time, seed: *seed });
            }
            for i in 0..bodies.len() {
                if bodies.alive[i] {
                    let v = bodies.velocity[i];
                    bodies.position[i] += v * dt;
                }
            }
        }

        // Collide and Respond, then the Hit events.
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
                        .and_then(|s| {
                            let sim = sims.module(&s.kind)?;
                            sim.takes_over(s.state.as_ref()).then_some((sim, &s.state))
                        })
                        .or_else(|| mover_bounce.and_then(|s| Some((sims.module(&s.kind)?, &s.state))));
                    match decider {
                        Some((sim, state)) => {
                            let cx = SimCx { thing: *mover, time, seed: *seed };
                            sim.respond_at(state.as_ref(), &contact, bodies, i, limits, &cx)
                        }
                        None => bodies.velocity[i] = reflect(velocity, &contact, 1.0),
                    }
                    if contact.speed >= limits.resting {
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
        if let Some(sim) = sims.thing(&r.kind) {
            for (key, value) in r.params.iter() {
                if sim.affects_physics(key) {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
        }
        if let Some(s) = r.slots.get(&SlotId::Trail) {
            // Whether a history is kept depends on the trail, not on how it looks.
            h.bytes(&[SlotId::Trail as u8]);
            h.bytes(s.kind.as_bytes());
        }
        for tag in &thing.tags {
            h.bytes(tag.as_bytes());
        }
        for slot in SIM_SLOTS {
            if let Some(s) = r.slots.get(&slot) {
                h.bytes(&[slot as u8]);
                h.bytes(s.kind.as_bytes());
                h.bytes(&s.from.0.to_le_bytes());
                for (key, value) in s.params.iter() {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
                // An edited graph runs instead of the native path, and a rules graph always
                // runs, so both change the physics.
                if let Some(g) = scene.library.modules.get(&s.module).and_then(|m| m.graph.as_ref())
                    && (g.custom || slot == SlotId::Rules)
                    && let Ok(text) = ron::to_string(g)
                {
                    h.bytes(text.as_bytes());
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
    use crate::Action;
    use crate::plugins::Sims;
    use crate::testing::{DOTS, TestDots, WALL, dots_in_ring, dots_in_sticky_ring, dots_with_rules, reg};
    use bpe_core::{ModuleGraph, SlotId, Target, ThingId, Value, resolve};

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
    fn a_trail_keeps_a_history_that_follows_the_balls() {
        let (mut scene, resolved, sims) = dots_in_ring(2);
        let plain = World::build(&scene, &resolved, &sims);
        assert!(plain.history.is_empty());
        let before = fingerprint(&scene, &resolved, &sims);
        let trail = scene.add_module(&reg(), "trail.test", "Trail").unwrap();
        scene.link(&reg(), DOTS, SlotId::Trail, trail).unwrap();
        let resolved = resolve(&scene, &reg());
        // Keeping a history is a rebuild, but how the trail looks is not.
        let with_trail = fingerprint(&scene, &resolved, &sims);
        assert_ne!(with_trail, before);
        scene.library.modules.get_mut(&trail).unwrap().params.set("width", Value::Number(3.0));
        let resolved = resolve(&scene, &reg());
        assert_eq!(fingerprint(&scene, &resolved, &sims), with_trail);

        let mut world = World::build(&scene, &resolved, &sims);
        assert!(world.history.contains_key(&DOTS));
        assert!(world.history[&DOTS].is_empty());
        let mut log = Log::default();
        world.advance_to(1.0, &sims, &mut log);
        let history = &world.history[&DOTS];
        assert_eq!(history.len(), 60);
        assert_eq!(history.points(1).next(), Some(world.bodies[&DOTS].position[1]));
        assert!(history.points(0).nth(30).unwrap() != world.bodies[&DOTS].position[0]);
        assert!(world.memory() >= plain.memory() + 2 * 120 * 8);
        // The same scene without the history steps the same.
        let mut bare = World::build(&scene, &resolved, &sims);
        bare.history.clear();
        let mut bare_log = Log::default();
        bare.advance_to(1.0, &sims, &mut bare_log);
        assert_eq!(bare.bodies, world.bodies);
        assert_eq!(bare_log.events(), log.events());
    }

    #[test]
    fn rules_run_after_the_step_and_act_on_the_next_one() {
        let (scene, resolved, sims) = dots_with_rules(1, false);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(5.0, &sims, &mut log);
        let hits = log.events().iter().filter(|e| e.kind == EventKind::Hit).count() as f32;
        assert!(hits >= 3.0);
        let grown = w.bodies[&DOTS].radius[0] - 5.0;
        // Every hit but any in the very last step has been applied.
        assert!(grown == hits || grown == hits - 1.0, "grown {grown} after {hits} hits");
        assert_eq!(w.pending.len() as f32, hits - grown);
        w.step(&sims, &mut log);
        assert!(w.pending.is_empty() || log.since(w.step).iter().any(|e| e.kind == EventKind::Hit));
    }

    #[test]
    fn a_root_rules_module_runs_once_with_every_thing_in_scope() {
        let (scene, resolved, sims) = dots_with_rules(1, true);
        let w = World::build(&scene, &resolved, &sims);
        assert!(w.module_state(ThingId::ROOT, SlotId::Rules).is_some());
        assert!(w.module_state(DOTS, SlotId::Rules).is_none());
        assert!(w.in_scope(ThingId::ROOT, DOTS));
        assert!(w.in_scope(ThingId::ROOT, ThingId::ROOT));
        assert!(w.matches(ThingId::ROOT, DOTS, &Target::Me));
        assert!(!w.matches(DOTS, WALL, &Target::Me));
        assert!(w.matches(DOTS, WALL, &Target::Kind("wall".into())));
        assert!(w.matches(DOTS, WALL, &Target::Thing(WALL)));
        assert!(w.matches(DOTS, WALL, &Target::Anything));
        assert_eq!(w.kind_of(DOTS), Some("dots"));
        assert_eq!(w.things_of(ThingId::ROOT, &Target::Me), vec![ThingId::ROOT, WALL, DOTS]);
        // The root's rule still grows the dots: hits by things in its scope count.
        let mut w = w;
        let mut log = Log::default();
        w.advance_to(5.0, &sims, &mut log);
        assert!(w.bodies[&DOTS].radius[0] > 5.0);
        // On the dots alone, the scope is just the dots.
        let (scene, resolved, sims) = dots_with_rules(1, false);
        let w = World::build(&scene, &resolved, &sims);
        assert!(!w.in_scope(DOTS, WALL));
        assert_eq!(w.things_of(DOTS, &Target::Me), vec![DOTS]);
    }

    #[test]
    fn a_missing_target_matches_nothing() {
        let (mut scene, _, sims) = dots_in_ring(1);
        scene.things.get_mut(&DOTS).unwrap().tags.insert("red".into());
        let resolved = resolve(&scene, &reg());
        let w = World::build(&scene, &resolved, &sims);
        assert!(w.matches(DOTS, DOTS, &Target::Tag("red".into())));
        assert_eq!(w.tags_of(DOTS).map(|t| t.len()), Some(1));
        for t in [Target::Thing(ThingId(99)), Target::Tag("nope".into()), Target::Kind("ghost".into())] {
            assert!(!w.matches(DOTS, DOTS, &t));
            assert!(!w.matches(DOTS, ThingId(99), &t));
            assert!(w.things_of(DOTS, &t).is_empty());
        }
        assert!(!w.in_scope(ThingId(99), DOTS));
    }

    #[test]
    fn set_changes_a_motion_setting_live() {
        let (scene, resolved, sims) = dots_in_ring(1);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        assert_eq!(w.module_state(DOTS, SlotId::Motion).map(|(_, s)| *crate::state::<f32>(s)), Some(1.0));
        w.apply(
            vec![Action::Set { thing: DOTS, slot: SlotId::Motion, key: "scale".into(), value: Value::Number(0.0) }],
            &sims,
            &mut log,
        );
        assert_eq!(w.module_state(DOTS, SlotId::Motion).map(|(_, s)| *crate::state::<f32>(s)), Some(0.0));
        assert_eq!(w.slot_params(DOTS, SlotId::Motion).and_then(|p| p.get("scale")), Some(&Value::Number(0.0)));
        let v = w.bodies[&DOTS].velocity[0];
        w.step(&sims, &mut log);
        assert_eq!(w.bodies[&DOTS].velocity[0], v, "no gravity once the scale is 0");
        // A thing without that slot, or a key the kind doesn't have, is left alone.
        w.apply(
            vec![Action::Set { thing: WALL, slot: SlotId::Motion, key: "scale".into(), value: Value::Number(2.0) }],
            &sims,
            &mut log,
        );
        w.apply(
            vec![Action::Set { thing: DOTS, slot: SlotId::Motion, key: "nope".into(), value: Value::Number(2.0) }],
            &sims,
            &mut log,
        );
        assert_eq!(w.slot_params(DOTS, SlotId::Motion).and_then(|p| p.get("nope")), None);
    }

    #[test]
    fn custom_graphs_tags_and_rules_change_the_fingerprint() {
        let (mut scene, resolved, sims) = dots_with_rules(1, false);
        let before = fingerprint(&scene, &resolved, &sims);
        let gravity = resolved.things[&DOTS].slots[&SlotId::Motion].module;
        scene.library.modules.get_mut(&gravity).unwrap().graph = Some(ModuleGraph {
            graph: bpe_graph::Graph::new("g"),
            template: "x".into(),
            custom: false,
            simple: Default::default(),
        });
        assert_eq!(fingerprint(&scene, &resolved, &sims), before, "an unedited graph runs natively");
        scene.library.modules.get_mut(&gravity).unwrap().graph.as_mut().unwrap().custom = true;
        let custom = fingerprint(&scene, &resolved, &sims);
        assert_ne!(custom, before);
        scene.things.get_mut(&DOTS).unwrap().tags.insert("red".into());
        let resolved = resolve(&scene, &reg());
        let tagged = fingerprint(&scene, &resolved, &sims);
        assert_ne!(tagged, custom);
        let rules = resolved.things[&DOTS].slots[&SlotId::Rules].module;
        scene.library.modules.get_mut(&rules).unwrap().graph = Some(ModuleGraph {
            graph: bpe_graph::Graph::new("r"),
            template: "rules".into(),
            custom: false,
            simple: Default::default(),
        });
        assert_ne!(fingerprint(&scene, &resolved, &sims), tagged, "a rules graph always counts");
    }

    #[test]
    fn the_pending_queue_survives_a_clone_and_a_replay() {
        let (scene, resolved, sims) = dots_with_rules(1, false);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        assert_eq!(w.seed, 0);
        // Step until a hit leaves something pending.
        while w.pending.is_empty() && w.step < 5000 {
            w.step(&sims, &mut log);
        }
        assert!(!w.pending.is_empty());
        let copy = w.clone();
        assert_eq!(copy.pending, w.pending);
        let mut log2 = log.clone();
        let mut copy = copy;
        w.advance_to(6.0, &sims, &mut log);
        copy.advance_to(6.0, &sims, &mut log2);
        assert_eq!(log.hash(), log2.hash());
        assert_eq!(w.bodies, copy.bodies);
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
}
