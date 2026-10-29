//! The world: every thing's simulation state, stepped at a fixed rate.
//!
//! Each step runs the spec's stages in order: Spawn, Scripted, Forces, Integrate, Collide,
//! Respond, Events, Rules. Everything iterates in `ThingId` order, so two runs of the same scene
//! give the same log.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bpe_core::{Animation, Attach, Inputs, PulseInputs, Resolved, Scene, SlotId, ThingId, Value};
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
    /// Per thing and slot, the module kind and its prepared state (Motion and Bounce only).
    modules: BTreeMap<ThingId, Prepared>,
    /// What pulses follow: the beat grid and the song's bands. The scene's sim sets it.
    pub pulse_inputs: Option<Arc<PulseInputs>>,
    /// The keyframes and pulses that move physics settings, with the resolved scene they are
    /// written into step by step. `None` when nothing physical is animated.
    animated: Option<Animated>,
}

/// See [`World::animated`].
#[derive(Clone)]
struct Animated {
    animation: Animation,
    resolved: Resolved,
}

/// The scene's tracks and pulses that change how it simulates: those on Motion and Bounce
/// modules, and those on a thing's own settings that its sim says affect the physics. `None`
/// when there are none.
pub fn physics_animation(scene: &Scene, resolved: &Resolved, sims: &Sims) -> Option<Animation> {
    let mut animation = Animation::new(scene, resolved);
    animation.retain(|thing, slot, key| match slot {
        Some(SlotId::Motion) | Some(SlotId::Bounce) => true,
        Some(_) => false,
        None => resolved.things.get(&thing).and_then(|r| sims.thing(&r.kind)).is_some_and(|s| s.affects_physics(key)),
    });
    (!animation.is_empty()).then_some(animation)
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
            kinds: BTreeMap::new(),
            modules: BTreeMap::new(),
            pulse_inputs: None,
            animated: None,
        };
        for (id, r) in &resolved.things {
            let Some(thing) = scene.things.get(id) else { continue };
            let Some(sim) = sims.thing(&r.kind) else { continue };
            sim.build(thing, r, &mut world);
            world.kinds.insert(*id, r.kind.clone());
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
        world.animated = physics_animation(scene, resolved, sims)
            .map(|animation| Animated { animation, resolved: resolved.clone() });
        world
    }

    /// Writes this step's keyframed and pulsed physics settings and hands them to the sims:
    /// Motion and Bounce modules are prepared again, things hear `settings_changed`. Pulses
    /// follow the beat and the song only; hits never move the physics.
    fn animate(&mut self, sims: &Sims) {
        let Some(mut animated) = self.animated.take() else { return };
        let time = self.time();
        animated.animation.apply(
            &mut animated.resolved,
            time,
            &Inputs { pulse: self.pulse_inputs.as_deref(), hits: 0.0 },
        );
        let mut done = BTreeSet::new();
        for (thing, slot, _) in animated.animation.destinations() {
            if !done.insert((thing, slot)) {
                continue;
            }
            let Some(r) = animated.resolved.things.get(&thing) else { continue };
            match slot {
                Some(slot) => {
                    if let Some(s) = r.slots.get(&slot)
                        && let Some(msim) = sims.module(&s.kind)
                    {
                        self.modules.entry(thing).or_default().insert(slot, (s.kind.clone(), msim.prepare(&s.params)));
                    }
                }
                None => {
                    if let Some(sim) = sims.thing(&r.kind) {
                        sim.settings_changed(thing, r, self);
                    }
                }
            }
        }
        self.animated = Some(animated);
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
        self.animate(sims);
        let limits = self.limits();
        let dt = self.dt();

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

        // Events: things log what happened to them this step.
        for (id, kind) in &kinds {
            if let Some(sim) = sims.thing(kind) {
                sim.after_step(*id, self, log);
            }
        }
    }

    fn substep(&mut self, sims: &Sims, dt: f32, limits: &Limits, log: &mut Log) {
        let World { step, gravity, bodies, walls, modules, .. } = self;
        let step = *step;
        let gravity = *gravity;

        // Forces, then Integrate, per body thing.
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
                        .and_then(|(kind, state)| {
                            let sim = sims.module(kind)?;
                            sim.takes_over(state.as_ref()).then_some((sim, state))
                        })
                        .or_else(|| mover_bounce.and_then(|(kind, state)| Some((sims.module(kind)?, state))));
                    match decider {
                        Some((sim, state)) => sim.respond(state.as_ref(), &contact, bodies, i, limits),
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
    // Keyframes and pulses on physics settings: moving a key changes the run.
    if let Some(animation) = physics_animation(scene, resolved, sims) {
        for a in animation.addresses() {
            h.bytes(format!("{a:?}").as_bytes());
            if let Some(track) = scene.tracks.get(a) {
                h.bytes(format!("{track:?}").as_bytes());
            }
            if let Some(pulse) = scene.pulses.get(a) {
                h.bytes(format!("{pulse:?}").as_bytes());
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
    use crate::testing::{DOTS, TestDots, WALL, dots_in_ring, dots_in_sticky_ring, reg};
    use bpe_core::{Address, Ease, Owner, SlotId, ThingId, Value, resolve};

    fn root_setting(key: &str) -> Address {
        Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: key.into() }
    }

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
    fn a_gravity_track_changes_the_world_at_its_time() {
        let (mut scene, resolved, sims) = dots_in_ring(1);
        let reg = reg();
        let before = fingerprint(&scene, &resolved, &sims);
        let gravity = root_setting("gravity");
        let first = scene.set_key(&reg, &gravity, 0.0, Value::Vec2([0.0, 500.0])).unwrap();
        scene.set_key(&reg, &gravity, 1.0, Value::Vec2([0.0, -500.0])).unwrap();
        assert!(scene.set_ease(&gravity, first, Ease::Hold));
        let resolved = resolve(&scene, &reg);
        assert_ne!(fingerprint(&scene, &resolved, &sims), before, "a physics key changes the run");
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(0.5, &sims, &mut log);
        assert_eq!(w.gravity, Vec2::new(0.0, 500.0));
        w.advance_to(1.5, &sims, &mut log);
        assert_eq!(w.gravity, Vec2::new(0.0, -500.0));
        // The resolved scene the world was built from is untouched.
        assert_eq!(resolved.things[&ThingId::ROOT].params.get("gravity"), Some(&Value::Vec2([0.0, 500.0])));
    }

    #[test]
    fn a_look_track_leaves_the_fingerprint_alone() {
        let (mut scene, resolved, sims) = dots_in_ring(1);
        let reg = reg();
        let before = fingerprint(&scene, &resolved, &sims);
        let tint = root_setting("tint");
        scene.set_key(&reg, &tint, 0.0, Value::Number(1.0)).unwrap();
        scene.set_key(&reg, &tint, 2.0, Value::Number(9.0)).unwrap();
        let resolved = resolve(&scene, &reg);
        assert_eq!(fingerprint(&scene, &resolved, &sims), before);
        assert!(physics_animation(&scene, &resolved, &sims).is_none());
        let mut w = World::build(&scene, &resolved, &sims);
        w.advance_to(1.0, &sims, &mut Log::default());
        assert_eq!(w.gravity, Vec2::new(0.0, 500.0));
    }

    #[test]
    fn after_step_runs_once_per_step_with_the_log() {
        let (mut scene, _, sims) = dots_in_ring(1);
        let reg = reg();
        let counter = scene.add_thing(&reg, "counter", ThingId::ROOT).unwrap();
        let resolved = resolve(&scene, &reg);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(1.0, &sims, &mut log);
        let counts: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Custom("tick".into())).collect();
        assert_eq!(counts.len(), 240);
        assert_eq!(counts.first().unwrap().step, 1);
        assert_eq!(counts.last().unwrap().step, 240);
        assert!(counts.iter().all(|e| e.a == Party { thing: counter, item: 0 }));
        assert!(log.events().windows(2).all(|p| p[0].step <= p[1].step), "after_step events keep the log sorted");
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
