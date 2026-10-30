//! The world: every thing's simulation state, stepped at a fixed rate.
//!
//! Each step runs the spec's stages in order: Spawn, Scripted, Forces, Integrate, Collide,
//! Respond, Events, Rules. Things are built and scripted in tree order (parents first, so riders
//! find their parent's pose) and collide in `ThingId` order, so two runs of the same scene give
//! the same log. Rules queue actions; the next step's Spawn stage applies them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bpe_core::{
    Animation, Attach, Inputs, ModuleGraph, Params, PulseInputs, Resolved, Scene, SlotId, Target, ThingId, Transform,
    Value,
};
use glam::Vec2;

use crate::camera::CameraView;
use crate::events::HASH_OFFSET;
use crate::pairs::{Pairs, Support, collide_pairs, pop};
use crate::plugins::{Limits, ModuleSim, RuleCx, SimCx, Sims, ThingState};
use crate::pose::Pose;
use crate::solids::Solids;
use crate::{Action, Bodies, Event, EventKind, History, Log, Party, Wall, keep_inside, keep_outside, reflect};

/// The furthest a ball may move in one substep without skipping a wall.
pub const SAFE_MOVE: f32 = 16.0;
/// The most substeps a step is split into.
pub const MAX_SUBSTEPS: u32 = 64;

/// One module a thing uses, prepared: its kind, the settings it was prepared from, its graph
/// and its state.
#[derive(Clone)]
pub(crate) struct Slotted {
    pub(crate) kind: String,
    pub(crate) params: Params,
    pub(crate) graph: Option<ModuleGraph>,
    pub(crate) state: Box<dyn ThingState>,
}

/// A thing's prepared modules, per slot.
pub(crate) type Prepared = BTreeMap<SlotId, Slotted>;

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
    /// The solid pieces of every thing that has some: pegs, bars, rings balls bounce off.
    pub solids: BTreeMap<ThingId, Solids>,
    /// Whatever a thing's sim keeps between steps.
    pub states: BTreeMap<ThingId, Box<dyn ThingState>>,
    /// Where every placed thing is right now. Sims write it in `build` and `scripted`; riders
    /// read their parent's through [`World::anchor`].
    pub poses: BTreeMap<ThingId, Pose>,
    /// Every built thing's tags, for matching targets.
    pub tags: BTreeMap<ThingId, BTreeSet<String>>,
    /// Where the balls of a thing have been, for the things whose sim asked for it (trails).
    pub history: BTreeMap<ThingId, History>,
    /// The scene's seed, for anything random; the scene's sim sets it.
    pub seed: u64,
    /// What the video shows right now; the camera's sim sets it, the identity without one.
    pub camera: CameraView,
    /// The video's size: how much of the world the shot covers at Zoom 1. The scene's sim sets
    /// it.
    pub video: Vec2,
    /// What the rules asked for in the last step, applied at the start of the next.
    pub pending: Vec<Action>,
    /// The kind of every built thing, in id order.
    kinds: BTreeMap<ThingId, String>,
    /// The built things in tree order: parents before children.
    order: Vec<ThingId>,
    /// Scratch space for the ball-to-ball stage.
    pairs: Pairs,
    /// Every built thing's parent, how it hangs off it and its own transform.
    attach: BTreeMap<ThingId, (ThingId, Attach, Transform)>,
    /// Per thing and slot, the module kind and its prepared state (Motion, Bounce and Rules).
    modules: BTreeMap<ThingId, Prepared>,
    /// Per thing that links a Rules module, the things whose Rules slot comes from it: what
    /// "me" means to that module.
    scope: BTreeMap<ThingId, BTreeSet<ThingId>>,
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

const NO_TAGS: &BTreeSet<String> = &BTreeSet::new();

impl World {
    /// Builds the world: every resolved thing in tree order through its sim, then the Motion,
    /// Bounce and Rules modules it uses. Modules without a sim are left out; a thing without one
    /// builds nothing, but can still be named by a target and carry a Rules module.
    pub fn build(scene: &Scene, resolved: &Resolved, sims: &Sims) -> World {
        let mut world = World {
            step: 0,
            steps_per_second: 240,
            gravity: Vec2::ZERO,
            bodies: BTreeMap::new(),
            walls: BTreeMap::new(),
            solids: BTreeMap::new(),
            states: BTreeMap::new(),
            poses: BTreeMap::new(),
            tags: BTreeMap::new(),
            history: BTreeMap::new(),
            seed: 0,
            camera: CameraView::default(),
            video: Vec2::new(1080.0, 1920.0),
            pending: Vec::new(),
            kinds: BTreeMap::new(),
            order: Vec::new(),
            pairs: Pairs::default(),
            attach: BTreeMap::new(),
            modules: BTreeMap::new(),
            scope: BTreeMap::new(),
            pulse_inputs: None,
            animated: None,
        };
        // Things that need the walls (a liquid finding its pool) are built in a second pass, so
        // an arena after them in the tree is there all the same; they come last in `order`. A
        // thing without a sim is still known by kind and tags, so a target can name it.
        for needs_walls in [false, true] {
            for id in &scene.order {
                let Some(r) = resolved.things.get(id) else { continue };
                let Some(thing) = scene.things.get(id) else { continue };
                let sim = sims.thing(&r.kind);
                if sim.is_some_and(|s| s.needs_walls()) != needs_walls {
                    continue;
                }
                if let Some(parent) = thing.parent {
                    world.attach.insert(*id, (parent, thing.attach, thing.transform));
                }
                if let Some(sim) = sim {
                    sim.build(thing, r, &mut world);
                    world.order.push(*id);
                }
                world.kinds.insert(*id, r.kind.clone());
                if !thing.tags.is_empty() {
                    world.tags.insert(*id, thing.tags.clone());
                }
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
            &Inputs { pulse: self.pulse_inputs.as_deref(), hits: 0.0, video_time: None },
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
                        // The module keeps its graph; only its settings moved.
                        let prepared = self.modules.entry(thing).or_default();
                        let graph = prepared.get(&slot).and_then(|p| p.graph.clone());
                        let state = msim.prepare_graph(&s.params, graph.as_ref());
                        prepared.insert(slot, Slotted { kind: s.kind.clone(), params: s.params.clone(), graph, state });
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

    /// Where a thing sits this step: on its parent when it rides one, else where its own
    /// transform puts it. A rider whose parent has no pose (or no such ball) uses its own
    /// transform too. A thing the world doesn't know is at the origin.
    pub fn anchor(&self, thing: ThingId) -> Pose {
        let Some((parent, attach, transform)) = self.attach.get(&thing) else { return Pose::default() };
        let own = Pose::from_transform(transform);
        let Attach::Ride { offset, turn_with_parent, item } = *attach else { return own };
        let parent_pose = match item {
            Some(i) => self.bodies.get(parent).filter(|b| (i as usize) < b.len()).map(|b| Pose {
                position: b.position[i as usize],
                velocity: b.velocity[i as usize],
                ..Pose::default()
            }),
            None => self.poses.get(parent).copied(),
        };
        match parent_pose {
            Some(p) => {
                let mut pose = p.local(offset, turn_with_parent);
                pose.angle += transform.rotation;
                pose
            }
            None => own,
        }
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

    /// Whether a thing belongs to a target, as the rules on `me` see it: the same rules as
    /// `Scene::target_matches` over the things the world was built from, except that "me"
    /// reaches everything in `me`'s Rules scope.
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

    /// The kind of a thing that has a sim.
    pub fn kind(&self, thing: ThingId) -> Option<&str> {
        self.kinds.get(&thing).map(String::as_str)
    }

    /// A thing's tags (empty for a thing without any, or without a sim).
    pub fn tags(&self, thing: ThingId) -> &BTreeSet<String> {
        self.tags.get(&thing).unwrap_or(NO_TAGS)
    }

    /// The built things that `target` names, seen from `me`, in id order, by the physics'
    /// rules (where "me" is the thing itself, not its Rules scope): what fields and emitters
    /// use. `Anything` is every one of them, `me` included. Rules graphs use [`World::things_of`].
    pub fn matching(&self, target: &Target, me: ThingId) -> Vec<ThingId> {
        self.kinds.keys().copied().filter(|&other| matches(&self.kinds, &self.tags, target, me, other)).collect()
    }

    /// The module kind and prepared state a thing uses in a slot, when the runtime tracks it.
    pub fn module_state(&self, thing: ThingId, slot: SlotId) -> Option<(&str, &dyn ThingState)> {
        self.modules.get(&thing)?.get(&slot).map(|s| (s.kind.as_str(), s.state.as_ref()))
    }

    /// Changes one setting of the module a thing uses in a slot and prepares it again. A thing
    /// without that slot, or a setting the module doesn't have, is left alone.
    pub(crate) fn set_slot_param(&mut self, thing: ThingId, slot: SlotId, key: &str, value: Value, sims: &Sims) {
        let Some(s) = self.modules.get_mut(&thing).and_then(|m| m.get_mut(&slot)) else { return };
        // A rule sends a number; the setting keeps its own type.
        let value = match (s.params.get(key), value) {
            (None, _) => return,
            (Some(Value::Bool(_)), Value::Number(x)) => Value::Bool(x != 0.0),
            (Some(Value::Int(_)), Value::Number(x)) => Value::Int(x.round() as i64),
            (Some(old), new) if std::mem::discriminant(old) == std::mem::discriminant(&new) => new,
            // A number can't stand in for a colour, a choice or a target.
            _ => return,
        };
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
        self.animate(sims);
        let limits = self.limits();
        let dt = self.dt();
        // Things take their turns in tree order: parents go first, so riders see where their
        // parent moved to.
        let order = self.order.clone();

        // Spawn: emitters add what they release this step.
        for id in &order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.spawn(*id, self, log);
            }
        }

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
        for id in &order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.scripted(*id, self);
            }
        }

        for _ in 0..substeps {
            self.substep(sims, &order, sub_dt, &limits, log);
        }

        // Events: things log what happened to them this step, in tree order: zone entries and
        // leaves, a soft body's impacts, the scene's beats.
        for id in &order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.after_step(*id, self, log);
            }
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

    fn substep(&mut self, sims: &Sims, order: &[ThingId], dt: f32, limits: &Limits, log: &mut Log) {
        // Forces from things: fields and the like, before the Motion modules.
        for id in order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.forces(*id, self, dt);
            }
        }
        self.move_bodies(sims, dt);

        // Settle: liquids relax their particles against each other and the walls.
        for id in order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.settle(*id, self, dt);
            }
        }
        self.collide(sims, dt, limits, log);
    }

    /// Forces from the Motion modules, then Integrate, per body thing.
    fn move_bodies(&mut self, sims: &Sims, dt: f32) {
        let World { step, steps_per_second, gravity, bodies, modules, seed, .. } = self;
        let gravity = *gravity;
        let seed = *seed;
        let time = *step as f32 / *steps_per_second as f32;
        for (id, bodies) in bodies.iter_mut() {
            if let Some(s) = modules.get(id).and_then(|m| m.get(&SlotId::Motion))
                && let Some(sim) = sims.module(&s.kind)
            {
                sim.forces_at(s.state.as_ref(), bodies, gravity, dt, &SimCx { thing: *id, time, seed });
            }
            bodies.start.copy_from_slice(&bodies.position);
            for i in 0..bodies.len() {
                if bodies.alive[i] {
                    let v = bodies.velocity[i];
                    bodies.position[i] += v * dt;
                }
            }
        }
    }

    /// Collide and Respond, then the Hit events: first the walls around the balls, then the
    /// solids among them, then the balls against each other. What each ball touched holds it
    /// up in the pair solve. Silent bodies (liquid particles) are kept in and bounced like the
    /// rest but never log a hit.
    fn collide(&mut self, sims: &Sims, dt: f32, limits: &Limits, log: &mut Log) {
        let World { step, steps_per_second, bodies, walls, solids, modules, kinds, tags, pairs, seed, .. } = self;
        let step = *step;
        let seed = *seed;
        let time = step as f32 / *steps_per_second as f32;
        let matcher = |t: &Target, me: ThingId, other: ThingId| matches(kinds, tags, t, me, other);
        let mut supports: Vec<Support> = Vec::new();
        for (mover, bodies) in bodies.iter_mut() {
            let mover_bounce = modules.get(mover).and_then(|m| m.get(&SlotId::Bounce));
            let own = mover_bounce.and_then(|s| Some((sims.module(&s.kind)?, s.state.as_ref())));
            let cx = SimCx { thing: *mover, time, seed };
            for (wall_id, wall) in walls.iter() {
                let wall_bounce = modules.get(wall_id).and_then(|m| m.get(&SlotId::Bounce));
                let decider = decide(sims, wall_bounce, mover_bounce);
                let special = is_special(own, &matcher, *mover, *wall_id);
                for i in 0..bodies.len() {
                    if !bodies.alive[i] {
                        continue;
                    }
                    let velocity = bodies.velocity[i];
                    let radius = bodies.radius[i];
                    let Some(contact) = keep_inside(&mut bodies.position[i], velocity, radius, wall) else { continue };
                    supports.push(Support {
                        thing: *mover,
                        item: i as u32,
                        normal: contact.normal,
                        wall_velocity: contact.wall_velocity,
                    });
                    respond(decider, &contact, bodies, i, limits, special, &cx);
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
                        if let Some((sim, state)) = own
                            && sim.effects(state, bodies, i, contact.speed, limits)
                        {
                            pop(bodies, i, *mover, step, log);
                        }
                    }
                }
            }
            for (solid_id, solids) in solids.iter() {
                if solid_id == mover {
                    continue;
                }
                let solid_bounce = modules.get(solid_id).and_then(|m| m.get(&SlotId::Bounce));
                let decider = decide(sims, solid_bounce, mover_bounce);
                let special = is_special(own, &matcher, *mover, *solid_id);
                for i in 0..bodies.len() {
                    if !bodies.alive[i] {
                        continue;
                    }
                    let radius = bodies.radius[i];
                    let near = solids.reach + radius + SAFE_MOVE;
                    if (bodies.position[i] - solids.pose.position).length_squared() > near * near {
                        continue;
                    }
                    let previous = bodies.start[i];
                    for (part, (solid, (center, bound))) in solids.parts.iter().zip(&solids.bounds).enumerate() {
                        let near = bound + radius + SAFE_MOVE;
                        if (bodies.position[i] - *center).length_squared() > near * near {
                            continue;
                        }
                        let velocity = bodies.velocity[i];
                        let Some(contact) =
                            keep_outside(&mut bodies.position[i], previous, velocity, radius, solid, &solids.pose)
                        else {
                            continue;
                        };
                        supports.push(Support {
                            thing: *mover,
                            item: i as u32,
                            normal: contact.normal,
                            wall_velocity: contact.wall_velocity,
                        });
                        respond(decider, &contact, bodies, i, limits, special, &cx);
                        if contact.speed >= limits.resting && !bodies.silent {
                            bodies.hits[i] += 1;
                            log.push(Event {
                                step,
                                kind: EventKind::Hit,
                                a: Party { thing: *mover, item: i as u32 },
                                b: Some(Party { thing: *solid_id, item: part as u32 }),
                                point: contact.point,
                                normal: contact.normal,
                                speed: contact.speed,
                            });
                            if let Some((sim, state)) = own
                                && sim.effects(state, bodies, i, contact.speed, limits)
                            {
                                pop(bodies, i, *mover, step, log);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Balls against balls.
        collide_pairs(bodies, modules, &matcher, sims, pairs, &supports, step, dt, limits, log);
    }
}

/// The target rules, over a world's kinds and tags.
fn matches(
    kinds: &BTreeMap<ThingId, String>,
    tags: &BTreeMap<ThingId, BTreeSet<String>>,
    target: &Target,
    me: ThingId,
    other: ThingId,
) -> bool {
    match target {
        Target::Anything => true,
        Target::Me => other == me && kinds.contains_key(&other),
        Target::Thing(t) => other == *t && kinds.contains_key(&other),
        Target::Tag(tag) => tags.get(&other).is_some_and(|t| t.contains(tag)),
        Target::Kind(kind) => kinds.get(&other).is_some_and(|k| k == kind),
    }
}

/// Whether the mover's own Bounce module calls hitting `other` special.
fn is_special(
    own: Option<(&dyn ModuleSim, &dyn ThingState)>,
    matcher: &dyn Fn(&Target, ThingId, ThingId) -> bool,
    mover: ThingId,
    other: ThingId,
) -> bool {
    own.and_then(|(sim, state)| sim.special_target(state)).is_some_and(|t| matcher(&t, mover, other))
}

/// A prepared Bounce module.
type Bounce<'a> = Option<&'a Slotted>;

/// Who answers a contact: the hit thing's Bounce when it takes over, else the mover's.
fn decide<'a>(sims: &'a Sims, hit: Bounce<'a>, mover: Bounce<'a>) -> Option<(&'a dyn ModuleSim, &'a dyn ThingState)> {
    hit.and_then(|s| {
        let sim = sims.module(&s.kind)?;
        sim.takes_over(s.state.as_ref()).then_some((sim, s.state.as_ref()))
    })
    .or_else(|| mover.and_then(|s| Some((sims.module(&s.kind)?, s.state.as_ref()))))
}

#[allow(clippy::too_many_arguments)]
fn respond(
    decider: Option<(&dyn ModuleSim, &dyn ThingState)>,
    contact: &crate::Contact,
    bodies: &mut Bodies,
    i: usize,
    limits: &Limits,
    special: bool,
    cx: &SimCx,
) {
    match decider {
        Some((sim, state)) => sim.respond_at(state, contact, bodies, i, limits, special, cx),
        None => bodies.velocity[i] = reflect(bodies.velocity[i], contact, 1.0),
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
        for tag in &thing.tags {
            h.bytes(b"#");
            h.bytes(tag.as_bytes());
        }
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
        if let Some(s) = r.slots.get(&SlotId::Trail) {
            // Whether a history is kept depends on the trail, not on how it looks.
            h.bytes(&[SlotId::Trail as u8]);
            h.bytes(s.kind.as_bytes());
        }
        for tag in &thing.tags {
            h.bytes(tag.as_bytes());
        }
        // Only modules the processor simulates count; a swarm's Flocking is the graphics card's.
        for slot in SIM_SLOTS {
            if let Some(s) = r.slots.get(&slot)
                && sims.module(&s.kind).is_some()
            {
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
                {
                    h.graph(g);
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

    /// What a graph does: its blocks by id with their kind, switch and settings, and its
    /// wires. Where the blocks sit, the notes, the name and what the cards show are the
    /// editor's, so moving a block does not rebuild the world.
    fn graph(&mut self, g: &bpe_core::ModuleGraph) {
        let g = &g.graph;
        self.bytes(&[g.enabled as u8]);
        let mut nodes: Vec<_> = g.nodes.iter().collect();
        nodes.sort_by_key(|n| n.id);
        for n in nodes {
            self.bytes(&n.id.to_le_bytes());
            self.bytes(n.kind.as_bytes());
            self.bytes(&[n.enabled as u8]);
            for (key, param) in &n.params {
                self.bytes(key.as_bytes());
                if let Ok(text) = ron::to_string(param) {
                    self.bytes(text.as_bytes());
                }
            }
        }
        let mut links: Vec<_> = g.links.iter().collect();
        links.sort();
        for l in links {
            self.bytes(&l.from.0.to_le_bytes());
            self.bytes(&[l.from.1]);
            self.bytes(&l.to.0.to_le_bytes());
            self.bytes(&[l.to.1]);
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
    use crate::testing::{
        DOTS, DOTS2, EMITTER, PEG, TestDots, TestRoot, WALL, dots_and_peg, dots_in_ring, dots_in_sticky_ring,
        dots_pair, dots_pair_popping, dots_pile, dots_with_emitter, dots_with_rules, reg, ring_with_rider,
    };
    use bpe_core::{Address, Attach, Ease, ModuleGraph, Owner, SlotId, Target, ThingId, Value, resolve};

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
    fn a_rider_starts_at_its_parents_pose_with_its_speed() {
        let (scene, resolved, sims) = ring_with_rider(Vec2::new(50.0, 0.0), true);
        let w = World::build(&scene, &resolved, &sims);
        assert_eq!(w.bodies[&DOTS].position[0], Vec2::new(50.0, 0.0));
        // The rim at (50, 0) of a ring turning clockwise at 1 rad/s moves down the screen at
        // 50 px/s, on top of the dots' own launch speed of (300, -200).
        let v = w.bodies[&DOTS].velocity[0];
        assert!((v.y - (-200.0 + 50.0)).abs() < 1e-3 && (v.x - 300.0).abs() < 1e-3, "{v:?}");
        assert_eq!(w.poses[&WALL].spin, 1.0);
        assert_eq!(w.anchor(DOTS).angle, 0.0);
    }

    #[test]
    fn a_rider_without_a_parent_pose_uses_its_own_transform() {
        let (mut scene, _, _) = ring_with_rider(Vec2::new(50.0, 0.0), true);
        scene.things.get_mut(&DOTS).unwrap().transform.position = Vec2::new(7.0, 8.0);
        let resolved = resolve(&scene, &reg());
        // Without a wall sim the parent builds nothing and has no pose.
        let mut thin = Sims::default();
        thin.add_thing(TestRoot);
        thin.add_thing(TestDots);
        let w = World::build(&scene, &resolved, &thin);
        assert_eq!(w.bodies[&DOTS].position[0], Vec2::new(7.0, 8.0));
        assert_eq!(w.anchor(ThingId(99)), Pose::default());
        assert_eq!(w.anchor(ThingId::ROOT), Pose::default());
    }

    #[test]
    fn build_and_scripted_follow_tree_order_not_id_order() {
        // A parent added after its child in id order must still be placed first.
        let (mut scene, _, sims) = ring_with_rider(Vec2::new(50.0, 0.0), false);
        let reg = reg();
        let late = scene.add_thing(&reg, "wall", ThingId::ROOT).unwrap();
        scene.things.get_mut(&late).unwrap().transform.position = Vec2::new(300.0, 0.0);
        scene.set_parent(DOTS, late, Attach::Ride { offset: Vec2::ZERO, turn_with_parent: false, item: None }).unwrap();
        let resolved = resolve(&scene, &reg);
        let mut w = World::build(&scene, &resolved, &sims);
        assert_eq!(w.bodies[&DOTS].position[0], Vec2::new(300.0, 0.0));
        assert_eq!(w.order, vec![ThingId::ROOT, WALL, late, DOTS]);
        w.step(&sims, &mut Log::default());
        assert_eq!(w.poses[&late].position, Vec2::new(300.0, 0.0));
    }

    #[test]
    fn a_wall_bounce_does_not_fake_a_path_through_a_peg_outside() {
        // A peg sits just outside the ring's top. A dot shot up at the top bounces off the ring in
        // the same substep as its sweep against the peg: the sweep must follow the path the dot
        // took (up to the ring), not one made up from its bounced-back speed, which would cross
        // the peg and throw the dot out of the ring.
        let (mut scene, _, sims) = dots_and_peg();
        let reg = reg();
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
        let inner = 100.0 - 1.0 - 5.0;
        scene.things.get_mut(&PEG).unwrap().transform.position = Vec2::new(0.0, -(inner + 5.0 + 10.0 + 2.0));
        scene.things.get_mut(&DOTS).unwrap().transform.position = Vec2::new(0.0, -(inner - 1.0));
        scene.things.get_mut(&DOTS).unwrap().params.set("velocity", Value::Vec2([0.0, -600.0]));
        let resolved = resolve(&scene, &reg);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        for _ in 0..30 {
            w.step(&sims, &mut log);
            let p = w.bodies[&DOTS].position[0];
            assert!(p.length() <= inner + 1e-3, "the dot stays inside the ring: {p:?}");
        }
        assert!(log.events().iter().all(|e| e.b.is_none_or(|b| b.thing != PEG)), "{:?}", log.events());
        assert!(log.events().iter().any(|e| e.b.is_some_and(|b| b.thing == WALL)), "it did bounce off the ring");
    }

    #[test]
    fn balls_bounce_off_a_peg_and_log_it_against_its_part() {
        let (scene, resolved, sims) = dots_and_peg();
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(1.0, &sims, &mut log);
        let hit = log.events().iter().find(|e| e.b.is_some_and(|b| b.thing == PEG)).expect("the dot lands on the peg");
        assert_eq!(hit.b, Some(Party { thing: PEG, item: 0 }));
        assert!(hit.normal.y < 0.0, "{hit:?}");
        assert_eq!(hit.a, Party { thing: DOTS, item: 0 });
        let p = w.bodies[&DOTS].position[0];
        assert!(p.is_finite() && (p - Vec2::new(0.0, 40.0)).length() >= 15.0 - 1e-3, "{p:?}");
        assert_eq!(w.solids[&PEG].reach, 10.0);
    }

    #[test]
    fn two_balls_heading_at_each_other_bounce_and_log_one_hit() {
        let (scene, resolved, sims) = dots_pair(30.0, Target::Anything);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(1.0, &sims, &mut log);
        let hits: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Hit).collect();
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!((hits[0].a.thing, hits[0].b.map(|b| b.thing)), (DOTS, Some(DOTS2)));
        assert!((hits[0].speed - 200.0).abs() < 5.0, "{:?}", hits[0]);
        assert!((hits[0].normal - Vec2::new(-1.0, 0.0)).length() < 1e-3, "from the second ball towards the first");
        let (a, b) = (w.bodies[&DOTS].velocity[0], w.bodies[&DOTS2].velocity[0]);
        assert!(a.x < -90.0 && b.x > 90.0, "{a:?} {b:?}");
        assert_eq!((w.bodies[&DOTS].hits[0], w.bodies[&DOTS2].hits[0]), (1, 1));
    }

    #[test]
    fn a_target_that_does_not_match_lets_balls_pass_through() {
        let (scene, resolved, sims) = dots_pair(30.0, Target::Tag("hard".into()));
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(1.0, &sims, &mut log);
        assert!(log.is_empty());
        assert!(w.bodies[&DOTS].position[0].x > 0.0);
        let (scene, resolved, sims) = dots_pair(30.0, Target::Tag("soft".into()));
        let mut w = World::build(&scene, &resolved, &sims);
        w.advance_to(1.0, &sims, &mut Log::default());
        // Only the second ball is tagged soft: the first wants it, it doesn't want the first.
        assert!(w.bodies[&DOTS].position[0].x > 0.0);
        assert!(w.matches(DOTS, DOTS2, &Target::Tag("soft".into())));
        assert!(!w.matches(DOTS2, DOTS, &Target::Tag("soft".into())));
        assert!(w.matches(DOTS, DOTS2, &Target::Kind("dots".into())) && w.matches(DOTS, DOTS, &Target::Me));
        assert!(!w.matches(DOTS, ThingId(99), &Target::Thing(ThingId(99))));
        // Both sides pointing at the other's thing: they bump.
        let (scene, resolved, sims) = dots_pair(30.0, Target::Kind("dots".into()));
        let mut w = World::build(&scene, &resolved, &sims);
        w.advance_to(1.0, &sims, &mut Log::default());
        assert!(w.bodies[&DOTS].position[0].x < 0.0);
    }

    #[test]
    fn pair_collisions_are_deterministic_in_a_pile() {
        let (scene, resolved, sims) = dots_pile(400);
        let run = || {
            let mut w = World::build(&scene, &resolved, &sims);
            let mut log = Log::default();
            w.advance_to(3.0, &sims, &mut log);
            (w, log)
        };
        let (w1, l1) = run();
        let (w2, l2) = run();
        assert_eq!(l1.hash(), l2.hash());
        assert_eq!(w1.bodies[&DOTS].position, w2.bodies[&DOTS].position);
        assert!(l1.events().iter().any(|e| e.b.is_some_and(|b| b.thing == DOTS)), "balls hit each other");
        assert!(w1.bodies[&DOTS].position.iter().all(|p| p.is_finite() && p.length() < 160.0));
        assert!(l1.events().windows(2).all(|p| p[0].step <= p[1].step));
    }

    #[test]
    fn a_pile_of_balls_settles_instead_of_boiling() {
        let (scene, resolved, sims) = dots_pile(60);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(4.0, &sims, &mut log);
        let bodies = &w.bodies[&DOTS];
        let average = bodies.velocity.iter().map(|v| v.length()).sum::<f32>() / bodies.len() as f32;
        let late = log.events().iter().filter(|e| e.step > 3 * w.steps_per_second as u64).count();
        assert!(average < 10.0, "a pile at rest barely moves, not {average} px/s");
        assert!(late < 10, "a pile at rest stops ticking, not {late} hits in its fourth second");
        // The ring is 2 px thick and the dots 5 px wide: none sinks into the floor.
        let farthest = bodies.position.iter().map(|p| p.length()).fold(0.0f32, f32::max);
        assert!(farthest <= 144.5, "nothing sinks through the floor: {farthest}");
    }

    #[test]
    fn a_ball_pops_after_its_hits_and_logs_a_remove() {
        let (scene, resolved, sims) = dots_pair_popping(2);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(5.0, &sims, &mut log);
        let removes: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Remove).collect();
        assert_eq!(removes.len(), 1, "{removes:?}");
        assert_eq!(removes[0].a.thing, DOTS);
        assert!(!w.bodies[&DOTS].alive[0]);
        assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Hit && e.a.thing == DOTS).count(), 2);
        assert!(w.bodies[&DOTS2].alive[0], "the other ball never pops");
    }

    #[test]
    fn tags_change_the_fingerprint() {
        let (mut scene, resolved, sims) = dots_in_ring(1);
        let before = fingerprint(&scene, &resolved, &sims);
        scene.things.get_mut(&DOTS).unwrap().tags.insert("x".into());
        assert_ne!(fingerprint(&scene, &resolve(&scene, &reg()), &sims), before);
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
    fn moving_a_block_keeps_the_fingerprint() {
        use bpe_graph::{Link, Node, Note, Param};
        let (mut scene, resolved, sims) = dots_with_rules(1, false);
        let rules = resolved.things[&DOTS].slots[&SlotId::Rules].module;
        let node = |id: u32, kind: &str, by: Option<f32>| Node {
            id,
            kind: kind.into(),
            pos: [0.0, 0.0],
            params: by.map(|b| ("by".to_owned(), Param::Number(b))).into_iter().collect(),
            enabled: true,
            exposed: Default::default(),
        };
        let mut g = bpe_graph::Graph::new("r");
        g.nodes.push(node(1, "when.hit", None));
        g.nodes.push(node(2, "do.grow", Some(2.0)));
        g.links.push(Link { from: (1, 0), to: (2, 0) });
        scene.library.modules.get_mut(&rules).unwrap().graph =
            Some(ModuleGraph { graph: g, template: "rules".into(), custom: true, simple: Default::default() });
        let before = fingerprint(&scene, &resolved, &sims);
        // Layout, notes, the name, the block order and what the cards show are editor-only.
        let mg = scene.library.modules.get_mut(&rules).unwrap().graph.as_mut().unwrap();
        mg.graph.node_mut(2).unwrap().pos = [500.0, 100.0];
        mg.graph.name = "moved".into();
        mg.graph.notes.push(Note { pos: [0.0; 2], size: [0.0; 2], text: "hi".into(), color: [1.0; 4] });
        mg.graph.nodes.swap(0, 1);
        mg.graph.node_mut(2).unwrap().exposed.insert("by".into());
        mg.simple.insert((2, "by".into()));
        assert_eq!(fingerprint(&scene, &resolved, &sims), before);
        // What the blocks do is not.
        let mg = scene.library.modules.get_mut(&rules).unwrap().graph.as_mut().unwrap();
        mg.graph.node_mut(2).unwrap().params.insert("by".into(), Param::Number(3.0));
        let changed = fingerprint(&scene, &resolved, &sims);
        assert_ne!(changed, before);
        let mg = scene.library.modules.get_mut(&rules).unwrap().graph.as_mut().unwrap();
        mg.graph.node_mut(2).unwrap().enabled = false;
        assert_ne!(fingerprint(&scene, &resolved, &sims), changed);
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
        assert_eq!(w.matching(&Target::Tag("red".into()), WALL), vec![DOTS]);
        assert_eq!(w.matching(&Target::Tag("blue".into()), WALL), vec![]);
        assert_eq!(w.matching(&Target::Kind("wall".into()), DOTS), vec![WALL]);
        assert_eq!(w.matching(&Target::Thing(DOTS), WALL), vec![DOTS]);
        assert_eq!(w.matching(&Target::Thing(ThingId(99)), WALL), vec![]);
        assert_eq!(w.matching(&Target::Me, WALL), vec![WALL]);
        assert_eq!(w.matching(&Target::Anything, WALL), vec![ThingId::ROOT, WALL, DOTS]);
        assert_eq!(w.kind(DOTS), Some("dots"));
        assert_eq!(w.kind(ThingId(99)), None);
        assert!(w.tags(DOTS).contains("red"));
        assert!(w.tags(WALL).is_empty());
    }
}
