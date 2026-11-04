//! The world: every thing's simulation state, stepped at a fixed rate.
//!
//! Each step runs the spec's stages in order: Spawn, Scripted, Forces, Integrate, Collide,
//! Respond, Events. Things are built and scripted in tree order (parents first, so riders find
//! their parent's pose) and collide in `ThingId` order, so two runs of the same scene give the
//! same log.

use std::collections::{BTreeMap, BTreeSet};

use bpe_core::{Animation, Attach, Inputs, Params, Resolved, Scene, SlotId, Target, ThingId, Transform, Value};
use glam::Vec2;

use crate::camera::CameraView;
use crate::events::{HASH_OFFSET, crossing, step_fraction};
use crate::pairs::{Pairs, Support, collide_pairs, pop};
use crate::plugins::{Limits, ModuleSim, SimCx, Sims, ThingState};
use crate::pose::Pose;
use crate::{Bodies, Event, EventKind, History, Log, Party, Wall, keep_clear_from, reflect};

/// The furthest a ball may move in one substep without skipping a wall.
pub const SAFE_MOVE: f32 = 16.0;
/// The most substeps a step is split into.
pub const MAX_SUBSTEPS: u32 = 64;

/// One module a thing uses, prepared: its kind, the settings it was prepared from and its
/// state.
#[derive(Clone)]
pub(crate) struct Slotted {
    pub(crate) kind: String,
    pub(crate) params: Params,
    pub(crate) state: Box<dyn ThingState>,
}

/// A thing's prepared modules, per slot.
pub(crate) type Prepared = BTreeMap<SlotId, Slotted>;

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
    /// Where every placed thing is right now. Sims write it in `build` and `scripted`; riders
    /// read their parent's through [`World::anchor`].
    pub poses: BTreeMap<ThingId, Pose>,
    /// Every built thing's name, as the scene names it.
    pub names: BTreeMap<ThingId, String>,
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
    /// Bytes each thing's state takes beyond its own struct, for the states that hold a lot
    /// Their sims note it; [`World::memory`] counts it for playback budgets.
    pub state_memory: BTreeMap<ThingId, usize>,
    /// The kind of every built thing, in id order.
    pub(crate) kinds: BTreeMap<ThingId, String>,
    /// The built things in tree order: parents before children.
    pub(crate) order: Vec<ThingId>,
    /// Scratch space for the ball-to-ball stage.
    pairs: Pairs,
    /// Every wall's outline as the step began, before keys and scripts moved it: which side of
    /// a wall a ball is on is judged against it, so a wall that jumps keeps its balls.
    walls_before: BTreeMap<ThingId, crate::Outline>,
    /// Every built thing's parent, how it hangs off it and its own transform.
    attach: BTreeMap<ThingId, (ThingId, Attach, Transform)>,
    /// Per thing and slot, the module kind and its prepared state (Motion and Bounce).
    modules: BTreeMap<ThingId, Prepared>,
    /// The keyframes and pulses that move physics settings, with the resolved scene they are
    /// written into step by step. `None` when nothing physical is animated.
    animated: Option<Animated>,
    /// Which substep of how many is running, so a hit knows where in the step it happened.
    substep: (u32, u32),
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
    /// Builds the world: every resolved thing in tree order through its sim, then the Motion
    /// and Bounce modules it uses. Modules without a sim are left out; a thing without one
    /// builds nothing, but can still be named by a target.
    pub fn build(scene: &Scene, resolved: &Resolved, sims: &Sims) -> World {
        let mut world = World {
            step: 0,
            steps_per_second: 240,
            gravity: Vec2::ZERO,
            bodies: BTreeMap::new(),
            walls: BTreeMap::new(),
            states: BTreeMap::new(),
            poses: BTreeMap::new(),
            names: BTreeMap::new(),
            tags: BTreeMap::new(),
            history: BTreeMap::new(),
            seed: 0,
            camera: CameraView::default(),
            video: Vec2::new(1080.0, 1920.0),
            state_memory: BTreeMap::new(),
            kinds: BTreeMap::new(),
            order: Vec::new(),
            pairs: Pairs::default(),
            walls_before: BTreeMap::new(),
            substep: (0, 1),
            attach: BTreeMap::new(),
            modules: BTreeMap::new(),
            animated: None,
        };
        // Things that need the walls (none of today's kinds) are built in a second pass, so
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
                world.names.insert(*id, thing.name.clone());
                if !thing.tags.is_empty() {
                    world.tags.insert(*id, thing.tags.clone());
                }
                let mut prepared = Prepared::new();
                for slot in SIM_SLOTS {
                    let Some(s) = r.slots.get(&slot) else { continue };
                    if let Some(msim) = sims.module(&s.kind) {
                        let state = msim.prepare(&s.params);
                        prepared.insert(slot, Slotted { kind: s.kind.clone(), params: s.params.clone(), state });
                    }
                }
                if !prepared.is_empty() {
                    world.modules.insert(*id, prepared);
                }
            }
        }
        // Everything is there now: things that depend on the whole scene finish building.
        for id in world.order.clone() {
            if let Some(sim) = world.kinds.get(&id).and_then(|kind| sims.thing(kind)) {
                sim.after_build(id, &mut world);
            }
        }
        world.animated = physics_animation(scene, resolved, sims)
            .map(|animation| Animated { animation, resolved: resolved.clone() });
        world
    }

    /// Writes this step's keyframed physics settings and hands them to the sims: Motion and
    /// Bounce modules are prepared again, things hear `settings_changed`. Hits never move the
    /// physics, so pulses add nothing here.
    fn animate(&mut self, sims: &Sims) {
        let Some(mut animated) = self.animated.take() else { return };
        let time = self.time();
        animated.animation.apply(&mut animated.resolved, time, &Inputs::NONE);
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
                        let prepared = self.modules.entry(thing).or_default();
                        let state = msim.prepare(&s.params);
                        prepared.insert(slot, Slotted { kind: s.kind.clone(), params: s.params.clone(), state });
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

    /// The things the world steps, in the order it steps them: the tree order, parents before
    /// children, with the things that need the walls last.
    pub fn order(&self) -> &[ThingId] {
        &self.order
    }

    /// The kind of a thing the world built.
    pub fn kind_of(&self, id: ThingId) -> Option<&str> {
        self.kinds.get(&id).map(String::as_str)
    }

    /// A built thing's tags.
    pub fn tags_of(&self, id: ThingId) -> Option<&BTreeSet<String>> {
        self.tags.get(&id)
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

    /// Whether `target`, seen from `me`, names `other`, by the physics' rules: "me" is the
    /// thing itself.
    pub fn names(&self, target: &Target, me: ThingId, other: ThingId) -> bool {
        matches(&self.kinds, &self.tags, target, me, other)
    }

    /// The built things that `target` names, seen from `me`, in id order, by the physics'
    /// rules: "me" is the thing itself. `Anything` is every one of them, `me` included.
    pub fn matching(&self, target: &Target, me: ThingId) -> Vec<ThingId> {
        self.kinds.keys().copied().filter(|&other| matches(&self.kinds, &self.tags, target, me, other)).collect()
    }

    /// The module kind and prepared state a thing uses in a slot, when the runtime tracks it.
    pub fn module_state(&self, thing: ThingId, slot: SlotId) -> Option<(&str, &dyn ThingState)> {
        self.modules.get(&thing)?.get(&slot).map(|s| (s.kind.as_str(), s.state.as_ref()))
    }

    /// Bytes the world takes, for playback budgets.
    pub fn memory(&self) -> usize {
        self.bodies.values().map(Bodies::memory).sum::<usize>()
            + self.history.values().map(History::memory).sum::<usize>()
            + self.state_memory.values().sum::<usize>()
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
        self.walls_before.clear();
        self.walls_before.extend(self.walls.iter().map(|(id, w)| (*id, w.outline.clone())));
        self.animate(sims);
        let limits = self.limits();
        let dt = self.dt();
        // Things take their turns in tree order: parents go first, so riders see where their
        // parent moved to.
        let order = self.order.clone();

        // Spawn: things add what they release this step.
        for id in &order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
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
                fastest = fastest.max(speed.min(limits.ceiling));
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

        for k in 0..substeps {
            self.substep = (k, substeps);
            self.substep(sims, &order, sub_dt, &limits, log);
        }

        // Events: things log what happened to them this step, in tree order.
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
    }

    fn substep(&mut self, sims: &Sims, order: &[ThingId], dt: f32, limits: &Limits, log: &mut Log) {
        // Forces from things, before the Motion modules.
        for id in order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.forces(*id, self, dt);
            }
        }
        self.move_bodies(sims, dt);

        // Settle: things relax what they hold against each other and the walls.
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
    /// balls against each other. What each ball touched holds it up in the pair solve.
    fn collide(&mut self, sims: &Sims, dt: f32, limits: &Limits, log: &mut Log) {
        let World {
            step,
            steps_per_second,
            bodies,
            walls,
            walls_before,
            modules,
            kinds,
            tags,
            pairs,
            seed,
            substep,
            ..
        } = self;
        let step = *step;
        let (sub_index, sub_count) = *substep;
        let seed = *seed;
        let time = step as f32 / *steps_per_second as f32;
        let matcher = |t: &Target, me: ThingId, other: ThingId| matches(kinds, tags, t, me, other);
        let mut supports: Vec<Support> = Vec::new();
        for (mover, bodies) in bodies.iter_mut() {
            let mover_bounce = modules.get(mover).and_then(|m| m.get(&SlotId::Bounce));
            let own = mover_bounce.and_then(|s| Some((sims.module(&s.kind)?, s.state.as_ref())));
            let cx = SimCx { thing: *mover, time, seed };
            for (wall_id, wall) in walls.iter() {
                let before = walls_before.get(wall_id).unwrap_or(&wall.outline);
                let wall_bounce = modules.get(wall_id).and_then(|m| m.get(&SlotId::Bounce));
                let decider = decide(sims, wall_bounce, mover_bounce);
                let special = is_special(own, &matcher, *mover, *wall_id);
                for i in 0..bodies.len() {
                    if !bodies.alive[i] {
                        continue;
                    }
                    let velocity = bodies.velocity[i];
                    let radius = bodies.radius[i];
                    let previous = bodies.previous[i];
                    let start = bodies.start[i];
                    let at = &mut bodies.position[i];
                    let moved_to = *at;
                    let Some(contact) = keep_clear_from(at, previous, before, velocity, radius, wall) else {
                        continue;
                    };
                    // The ball was moved back to just touching: the gap to there along the
                    // normal, at the start of the substep and where it had moved to.
                    let touching = *at;
                    let gap = |p: Vec2| (p - touching).dot(contact.normal);
                    let within = crossing(gap(start), gap(moved_to));
                    let hit_at = step_fraction(sub_index, sub_count, within);
                    supports.push(Support {
                        thing: *mover,
                        item: i as u32,
                        normal: contact.normal,
                        wall_velocity: contact.wall_velocity,
                    });
                    respond(decider, &contact, bodies, i, limits, special, &cx);
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
                            at: hit_at,
                        });
                        if let Some((sim, state)) = own
                            && sim.effects(state, bodies, i, contact.speed, limits)
                        {
                            pop(bodies, i, *mover, step, log);
                        }
                    }
                }
            }
        }

        // Balls against balls.
        let substep = (sub_index, sub_count);
        collide_pairs(bodies, modules, &matcher, sims, pairs, &supports, step, substep, dt, limits, log);
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
/// in how they look fingerprint the same, so the editor can keep playing through
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
        // Only modules the processor simulates count.
        for slot in SIM_SLOTS {
            if let Some(s) = r.slots.get(&slot)
                && sims.module(&s.kind).is_some()
            {
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
            Value::Gradient(g) => {
                self.bytes(&[5]);
                for s in &g.stops {
                    self.f32(s.at);
                    self.bytes(&[s.color.r, s.color.g, s.color.b, s.color.a]);
                }
                self.bytes(&[g.transition as u8]);
            }
            Value::Choice(s) | Value::Text(s) => {
                self.bytes(&[6]);
                self.bytes(s.as_bytes());
            }
            Value::Target(t) => {
                self.bytes(&[7]);
                self.bytes(format!("{t:?}").as_bytes());
            }
            Value::Routes(r) => {
                self.bytes(&[11]);
                self.bytes(format!("{r:?}").as_bytes());
            }
            Value::Slots(n) => {
                self.bytes(&[12]);
                self.bytes(format!("{n:?}").as_bytes());
            }
            Value::Strokes(s) => {
                self.bytes(&[8]);
                self.bytes(&s.hash64().to_le_bytes());
            }
            Value::Points(ps) => {
                self.bytes(&[9]);
                self.bytes(&(ps.len() as u64).to_le_bytes());
                for [x, y] in ps {
                    self.f32(*x);
                    self.f32(*y);
                }
            }
            Value::Path(p) => {
                self.bytes(&[10]);
                self.bytes(&(p.points.len() as u64).to_le_bytes());
                for [x, y] in &p.points {
                    self.f32(*x);
                    self.f32(*y);
                }
                for b in &p.bends {
                    match b {
                        None => self.bytes(&[0]),
                        Some(hs) => {
                            self.bytes(&[1]);
                            for [x, y] in hs {
                                self.f32(*x);
                                self.f32(*y);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::Sims;
    use crate::testing::{
        DOTS, DOTS2, TestDots, TestRoot, WALL, dots_in_ring, dots_in_sticky_ring, dots_pair, dots_pair_popping,
        dots_pile, reg, ring_with_rider,
    };
    use bpe_core::{Address, Attach, Ease, SlotId, Target, ThingId, Value, resolve};

    fn root_setting(key: &str) -> Address {
        Address::own(ThingId::ROOT, key)
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
    fn hit_fraction_matches_where_the_ball_crossed() {
        // A ring of radius 100 (2 px thick): a 5 px dot touches it with its centre at 94. It starts
        // 4 px short of that and moves 10 px a step, so it touches 0.4 of the way through.
        let (mut scene, _, sims) = dots_in_ring(1);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
        let dots = scene.things.get_mut(&DOTS).unwrap();
        dots.transform.position = glam::Vec2::new(90.0, 0.0);
        dots.params.set("velocity", Value::Vec2([2400.0, 0.0]));
        let resolved = resolve(&scene, &reg());
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.step(&sims, &mut log);
        let hit = log.events().iter().find(|e| e.kind == EventKind::Hit).expect("it hits in the first step");
        assert!((hit.at - 0.4).abs() < 0.02, "at = {}", hit.at);
    }

    #[test]
    fn a_ball_pair_hit_knows_where_in_the_step_it_was() {
        // Two 5 px dots with centres 31 px apart, closing at 200 px/s: they touch 21 px of
        // closing later, 25.2 steps in (each step closes 200/240 px).
        let (scene, resolved, sims) = dots_pair(15.5, Target::Anything);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(1.0, &sims, &mut log);
        let hit = log.events().iter().find(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == DOTS2));
        let hit = hit.expect("the dots meet");
        let closing = 200.0 / 240.0;
        let exact = 21.0 / closing;
        let expected = exact - (hit.step - 1) as f32;
        assert!((hit.at - expected).abs() < 0.05, "step {} at {} expected {expected}", hit.step, hit.at);
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
        let counts: Vec<&Event> =
            log.events().iter().filter(|e| e.kind == EventKind::Hit && e.a.thing == counter).collect();
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
        assert_eq!(w.matching(&Target::Tag("soft".into()), DOTS), vec![DOTS2]);
        assert_eq!(w.matching(&Target::Kind("dots".into()), DOTS), vec![DOTS, DOTS2]);
        assert!(w.matching(&Target::Thing(ThingId(99)), DOTS).is_empty());
        // Both sides pointing at the other's thing: they bump.
        let (scene, resolved, sims) = dots_pair(30.0, Target::Kind("dots".into()));
        let mut w = World::build(&scene, &resolved, &sims);
        w.advance_to(1.0, &sims, &mut Log::default());
        assert!(w.bodies[&DOTS].position[0].x < 0.0);
    }

    #[test]
    fn a_fast_ball_never_crosses_a_thin_band_from_either_side() {
        for start_outside in [false, true] {
            let (mut scene, _, sims) = dots_in_ring(1);
            scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
            scene.things.get_mut(&WALL).unwrap().params.set("radius", Value::Number(100.0));
            let dots = scene.things.get_mut(&DOTS).unwrap();
            let velocity = if start_outside { [-6000.0, 1300.0] } else { [6000.0, 2500.0] };
            dots.params.set("velocity", Value::Vec2(velocity));
            if start_outside {
                dots.transform.position = Vec2::new(300.0, 0.0);
            }
            let resolved = resolve(&scene, &reg());
            let mut w = World::build(&scene, &resolved, &sims);
            let mut log = Log::default();
            let mut hits = 0;
            for k in 1..=480 {
                w.advance_to(k as f64 / 240.0, &sims, &mut log);
                let p = w.bodies[&DOTS].position[0];
                // The ring is 2 px thick at radius 100 and the dot's radius is 5.
                if start_outside {
                    assert!(p.length() >= 106.0 - 1e-2, "step {k}: the dot got in, at {p}");
                } else {
                    assert!(p.length() <= 94.0 + 1e-2, "step {k}: the dot got out, at {p}");
                }
                hits = log.events().len();
            }
            assert!(hits > 0, "it hit the band (outside {start_outside})");
        }
    }

    #[test]
    fn pair_collisions_are_deterministic_in_a_pile() {
        let (mut scene, _, sims) = dots_pile(400);
        // Wide enough that the whole block of dots starts inside it: a dot that starts outside a
        // hollow wall stays outside.
        scene.things.get_mut(&WALL).unwrap().params.set("radius", Value::Number(175.0));
        let resolved = resolve(&scene, &reg());
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
        assert!(w1.bodies[&DOTS].position.iter().all(|p| p.is_finite() && p.length() < 175.0));
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
        crate::testing::give(&mut scene, DOTS, SlotId::Trail, "trail.test", &[]);
        let resolved = resolve(&scene, &reg());
        // Keeping a history is a rebuild, but how the trail looks is not.
        let with_trail = fingerprint(&scene, &resolved, &sims);
        assert_ne!(with_trail, before);
        scene
            .things
            .get_mut(&DOTS)
            .unwrap()
            .slots
            .get_mut(&SlotId::Trail)
            .unwrap()
            .params
            .set("width", Value::Number(3.0));
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
