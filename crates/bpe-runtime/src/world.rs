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
use crate::rows::Rows;
use crate::walls::middle_crossing;
use crate::{Bodies, Event, EventKind, History, Log, Party, Touch, Wall, keep_clear_from, on_own_side, reflect};

/// The furthest a ball may move in one substep without skipping a wall.
pub const SAFE_MOVE: f32 = 16.0;
/// The most substeps a step is split into.
pub const MAX_SUBSTEPS: u32 = 64;
/// How far into a wall a ball that touches it with its edge may begin a substep and still count
/// as coming in from its own side (its bounce then carries on for the rest of the substep), in
/// pixels: rounding, not a push from another ball.
const TOUCHING: f32 = 1e-3;

/// One module a thing uses, prepared: its kind, the settings it was prepared from and its
/// state.
#[derive(Clone)]
pub(crate) struct Slotted {
    pub(crate) kind: String,
    pub(crate) params: Params,
    pub(crate) state: Box<dyn ThingState>,
    /// A state per ball, when the card's settings vary across the balls.
    pub(crate) rows: Option<Rows>,
    /// The states of the balls (by body row) a rule changed this card for alone.
    pub(crate) own: BTreeMap<u32, std::sync::Arc<dyn ThingState>>,
}

impl Slotted {
    /// `sim`'s card of kind `kind`, prepared from `params`.
    pub(crate) fn new(sim: &dyn ModuleSim, kind: &str, params: &Params) -> Slotted {
        Slotted {
            kind: kind.to_owned(),
            params: params.clone(),
            state: sim.prepare(params),
            rows: Rows::of(sim, params),
            own: BTreeMap::new(),
        }
    }

    /// The state body row `row` uses: its own when the card varies across the balls, else the card's.
    pub(crate) fn state_for(&self, row: usize) -> &dyn ThingState {
        if let Some(own) = self.own.get(&(row as u32)) {
            return own.as_ref();
        }
        self.rows.as_ref().and_then(|r| r.state(row)).unwrap_or(self.state.as_ref())
    }

    /// Whether the card varies across the balls (by its own spreads, or for balls a rule changed
    /// it for alone).
    pub(crate) fn varies(&self) -> bool {
        self.rows.is_some() || !self.own.is_empty()
    }

    /// The card's own values, its spreads and rules aside: what a lone copy of the world uses.
    fn plain(&self) -> Slotted {
        Slotted { rows: None, own: BTreeMap::new(), ..self.clone() }
    }
}

/// A thing's prepared modules, per slot.
pub(crate) type Prepared = BTreeMap<SlotId, Slotted>;

/// Which slots the runtime asks a `ModuleSim` about.
const SIM_SLOTS: [SlotId; 2] = [SlotId::Motion, SlotId::Bounce];

/// The scene's cards that only things stepped beside the physics read (the Air card, which only
/// Particles things follow): their keys are animated in every world, but they count towards
/// [`beside_fingerprint`], never the physics fingerprint.
pub const BESIDE_SLOTS: [SlotId; 1] = [SlotId::Air];

/// A run's number (see [`World::run`]). A copy is a run of its own: cloning gives a fresh one.
#[derive(Debug, PartialEq, Eq)]
pub struct RunId(pub u64);

impl RunId {
    pub fn fresh() -> RunId {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        RunId(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

impl Clone for RunId {
    fn clone(&self) -> RunId {
        RunId::fresh()
    }
}

/// The balls' places a [`World::set_between`] moved, to put back.
#[must_use = "put the balls back with World::restore_between"]
pub struct Between {
    saved: Vec<(ThingId, Vec<Vec2>)>,
}

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
    /// Seconds in a beat of the scene's tempo, and beats in a bar; the scene's sim sets them
    /// (120 beats a minute, four in a bar, without one). Only things stepped beside the physics
    /// read them (particles born on the beat).
    pub beat_seconds: f64,
    pub beats_in_bar: u32,
    /// Bytes each thing's state takes beyond its own struct, for the states that hold a lot
    /// Their sims note it; [`World::memory`] counts it for playback budgets.
    pub state_memory: BTreeMap<ThingId, usize>,
    /// Bytes each thing's state keeps in video memory (a Huge swarm's particles), noted by its
    /// sim; [`World::gpu_memory`] counts it for playback budgets.
    pub gpu_memory: BTreeMap<ThingId, usize>,
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
    /// The scene's sound, when a physics setting follows it.
    sound: Option<std::sync::Arc<dyn crate::SoundAt>>,
    /// The scene's light probe, when something physical follows light (lighting part 7).
    light: Option<std::sync::Arc<dyn crate::LightAt>>,
    /// Each ball's pull from the lights this step (Moths), by thing and body row, in pixels per
    /// second squared: worked out once a step after Spawn, added in every substep.
    pulls: BTreeMap<ThingId, Vec<Vec2>>,
    /// The scene's physics fingerprint (the one without the history, [`sound_fingerprint`]'s),
    /// when some thing's sim asked for it ([`crate::ThingSim::needs_fingerprint`]). Two worlds
    /// with the same one run alike step for step, so work keyed by it can be shared.
    pub fingerprint: Option<u64>,
    /// The scene's rules as they run (none without an enabled rule).
    pub(crate) rules: Option<crate::rules::RulesState>,
    /// How fast the simulation runs this step: 1, unless a rule's Slow motion changed it. The
    /// physics moves by `dt × pace` ([`World::sim_dt`]) while the video clock runs on.
    pace: f32,
    /// Which run this is: fresh for every built world and every copy of one (a snapshot, a test
    /// flight), so a cache keyed by a moment of a run (the light probe's) never mixes two runs
    /// that are at the same step.
    pub run: RunId,
    /// The built things stepped beside the physics ([`is_beside`]: their sim's
    /// [`crate::ThingSim::in_main`] is false and no rule reads them).
    pub beside: BTreeSet<ThingId>,
    /// The built things whose sim never steps them with the physics, beside it or kept in it by
    /// a rule that reads them: left out of test flights too.
    pub decoration: BTreeSet<ThingId>,
    /// The parts of every thing that has some (a Text's letters, Text part 3): what rules name one
    /// by one, as they name a group's balls.
    pub parts: BTreeMap<ThingId, crate::Parts>,
    /// The scene as saved, kept when a built thing's sim reads it ([`crate::ThingSim::reads_tree`]:
    /// a Text lays its letters out from the scene's fonts and names). Shared by every clone.
    pub tree: Option<std::sync::Arc<Scene>>,
}

/// States lent to a world for drawing ([`World::lend_states`]), with what they replaced.
#[must_use = "give the states back with World::take_back"]
pub struct Lent {
    replaced: Vec<(ThingId, Option<Box<dyn ThingState>>)>,
}

/// A thing's Motion or Bounce card as the world has it: see [`World::card`].
#[derive(Clone, Copy)]
pub struct Card<'a> {
    pub kind: &'a str,
    pub state: &'a dyn ThingState,
    pub varies: bool,
}

/// See [`World::animated`].
#[derive(Clone)]
struct Animated {
    animation: Animation,
    resolved: Resolved,
    /// The settings as built, before keys and rules, when rules can change settings: a setting a
    /// rule changed is put back from here before the keys and the rule's changes are worked out
    /// again, so changes never pile up step after step.
    pristine: Option<std::sync::Arc<Resolved>>,
}

/// The scene's tracks and pulses that change how it simulates: those on Motion and Bounce
/// modules, and those on a thing's own settings that its sim says affect the physics. `None`
/// when there are none.
pub fn physics_animation(scene: &Scene, resolved: &Resolved, sims: &Sims) -> Option<Animation> {
    let mut animation = Animation::new(scene, resolved);
    animation.retain(|thing, slot, key| match slot {
        Some(SlotId::Motion) | Some(SlotId::Bounce) => true,
        // Read by the things stepped beside the physics (the air): animated with it, never in the
        // physics fingerprint.
        Some(slot) if BESIDE_SLOTS.contains(&slot) => true,
        Some(_) => false,
        None => match (resolved.things.get(&thing).and_then(|r| sims.thing(&r.kind)), scene.things.get(&thing)) {
            (Some(s), Some(t)) => s.affects_physics_in(scene, t, bpe_core::vary::base_key(key)),
            (Some(s), None) => s.affects_physics(bpe_core::vary::base_key(key)),
            (None, _) => false,
        },
    });
    (!animation.is_empty()).then_some(animation)
}

/// `animation` without the keys on the cards only the things stepped beside the physics read
/// ([`BESIDE_SLOTS`]): what of it the physics itself reads. `None` when nothing is left.
fn main_animation(animation: &Animation) -> Option<Animation> {
    let mut main = animation.clone();
    main.retain(|_, slot, _| !slot.is_some_and(|s| BESIDE_SLOTS.contains(&s)));
    (!main.is_empty()).then_some(main)
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
            beat_seconds: 0.5,
            beats_in_bar: 4,
            state_memory: BTreeMap::new(),
            gpu_memory: BTreeMap::new(),
            kinds: BTreeMap::new(),
            order: Vec::new(),
            pairs: Pairs::default(),
            walls_before: BTreeMap::new(),
            substep: (0, 1),
            attach: BTreeMap::new(),
            modules: BTreeMap::new(),
            animated: None,
            sound: None,
            light: None,
            pulls: BTreeMap::new(),
            fingerprint: None,
            rules: None,
            pace: 1.0,
            run: RunId::fresh(),
            beside: BTreeSet::new(),
            decoration: BTreeSet::new(),
            parts: BTreeMap::new(),
            tree: None,
        };
        if resolved.things.values().any(|r| sims.thing(&r.kind).is_some_and(|s| s.reads_tree(r))) {
            world.tree = Some(std::sync::Arc::new(scene.clone()));
        }
        // Worked out first: a thing may cache work by it while it builds.
        let wants_fingerprint =
            resolved.things.values().any(|r| sims.thing(&r.kind).is_some_and(|s| s.needs_fingerprint(r)));
        if wants_fingerprint {
            world.fingerprint = Some(sound_fingerprint(scene, resolved, sims));
        }
        // Keys and pulses on physics settings hold from the very start: the world is built from
        // the settings as they are at 0 s (the sound, which no step has made yet, left out), so
        // the first frame and the first step's judgement of sides see them.
        let animation = physics_animation(scene, resolved, sims);
        let at_start = animation.as_ref().map(|animation| {
            let mut at_start = resolved.clone();
            // Health is full at the start.
            let full = FullHealth(resolved);
            animation.apply(&mut at_start, 0.0, &Inputs { health: Some(&full), ..Inputs::NONE });
            at_start
        });
        let built_from = at_start.as_ref().unwrap_or(resolved);
        // What the world senses for its keys: a world with nothing stepped beside the physics
        // (the editor's main playback) leaves out the keys only those things read, so an Air
        // key following sound or light gives it no sound or light to work out.
        // (A rule may keep such a thing in the main playback: it is built, so it senses too.)
        let decorated = resolved.things.values().any(|r| sims.thing(&r.kind).is_some_and(|s| !s.in_main(r)));
        let sensed = if decorated { animation.clone() } else { animation.as_ref().and_then(main_animation) };
        // Things that need the walls (none of today's kinds) are built in a second pass, so
        // an arena after them in the tree is there all the same; they come last in `order`. A
        // thing without a sim is still known by kind and tags, so a target can name it.
        for needs_walls in [false, true] {
            for id in &scene.order {
                let Some(r) = built_from.things.get(id) else { continue };
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
                    if is_beside(scene, sims, *id, r) {
                        world.beside.insert(*id);
                    }
                    if !sim.in_main(r) {
                        world.decoration.insert(*id);
                    }
                    if let Some(want) = sim.history(r) {
                        world.keep_history(*id, want);
                    }
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
                        prepared.insert(slot, Slotted::new(msim, &s.kind, &s.params));
                    }
                }
                if !prepared.is_empty() {
                    world.modules.insert(*id, prepared);
                }
            }
        }
        // The balls there from the start were there at step 0.
        for (id, history) in &mut world.history {
            if let Some(bodies) = world.bodies.get(id) {
                history.note(0, bodies.len());
            }
        }
        // Everything is there now: things that depend on the whole scene finish building.
        for id in world.order.clone() {
            if let Some(sim) = world.kinds.get(&id).and_then(|kind| sims.thing(kind)) {
                sim.after_build(id, &mut world);
            }
        }
        // Light sensing, only when something physical follows light: the probe is there before
        // the balls' places in light spreads are first worked out.
        if let Some(maker) = &sims.light
            && needs_light(scene, built_from, sims, sensed.as_ref())
        {
            world.light = Some(maker.make(scene, resolved));
        }
        world.refresh_rows(sims, &Log::default());
        world.rules = crate::rules::RulesState::build(scene, resolved, sims, &world);
        let pristine =
            world.rules.as_ref().filter(|r| r.changes_settings()).map(|_| std::sync::Arc::new(built_from.clone()));
        world.animated = match (animation.zip(at_start), pristine) {
            (Some((animation, resolved)), pristine) => Some(Animated { animation, resolved, pristine }),
            (None, Some(pristine)) => Some(Animated {
                animation: Animation::default(),
                resolved: (*pristine).clone(),
                pristine: Some(pristine),
            }),
            (None, None) => None,
        };
        if let (Some(maker), Some(_), Some(sensed)) = (&sims.sound, &world.animated, &sensed)
            && follows_sound(scene, sensed)
        {
            world.sound = Some(maker.make(scene, resolved));
        }
        world
    }

    /// Writes this step's keyframed physics settings and hands them to the sims: Motion and
    /// Bounce modules are prepared again, things hear `settings_changed`. Hits never move the
    /// physics; a setting that follows sound reads it as it was at the end of the step before
    /// (the world and `log` as they are now), so the physics never waits on itself.
    fn animate(&mut self, sims: &Sims, log: &Log) {
        let Some(mut animated) = self.animated.take() else { return };
        let time = self.time();
        // What the rules changed: the settings worked out again this step, as they were before.
        let (rule_keys, dirty) = self.rule_keys(sims, time);
        let before: Vec<Option<Value>> = rule_keys
            .iter()
            .map(|k| crate::rules::params_of(&animated.resolved, k.thing, k.slot).and_then(|p| p.get(&k.key)).cloned())
            .collect();
        if let Some(pristine) = animated.pristine.clone() {
            for k in &rule_keys {
                let was = crate::rules::params_of(&pristine, k.thing, k.slot).and_then(|p| p.get(&k.key)).cloned();
                if let (Some(was), Some(params)) =
                    (was, crate::rules::params_mut(&mut animated.resolved, k.thing, k.slot))
                {
                    params.set(&k.key, was);
                }
            }
        }
        {
            let levels = self.sound.as_deref().map(|sound| crate::LevelsAt { sound, world: self, log });
            let lit = self.light.as_deref().map(|light| crate::LightLevelsAt { light, world: self, log });
            let inputs = Inputs {
                sound: levels.as_ref().map(|l| l as &dyn bpe_core::SoundLevels),
                light: lit.as_ref().map(|l| l as &dyn bpe_core::LightLevels),
                // Health as the last Rules stage, and the actions applied since, left it.
                health: self.health_levels(),
                ..Inputs::NONE
            };
            // Decoration's pulses on hits move (a Particles thing's Rate); the physics' never do.
            let kinds = &self.kinds;
            let pulsed = |t: ThingId| kinds.get(&t).is_some_and(|k| sims.thing(k).is_some_and(|s| s.pulses_on_hits()));
            if !self.decoration.iter().any(|t| pulsed(*t)) {
                animated.animation.apply(&mut animated.resolved, time, &inputs);
            } else {
                let hits = Inputs { hits: crate::hit_strength(log, time, self.steps_per_second), ..inputs };
                animated.animation.apply_where(&mut animated.resolved, time, &inputs, |t| !pulsed(t));
                animated.animation.apply_where(&mut animated.resolved, time, &hits, pulsed);
            }
        }
        let mut destinations: Vec<(ThingId, Option<SlotId>)> =
            animated.animation.destinations().into_iter().map(|(thing, slot, _)| (thing, slot)).collect();
        if let Some(rules) = &self.rules {
            let keep = |k: &crate::rules::ChangeKey| self.is_physics(sims, k.thing, k.slot, &k.key);
            rules.apply_whole(&mut animated.resolved, time, keep);
            for (k, was) in rule_keys.iter().zip(&before) {
                let now = crate::rules::params_of(&animated.resolved, k.thing, k.slot).and_then(|p| p.get(&k.key));
                if now != was.as_ref() || dirty.contains(k) {
                    destinations.push((k.thing, k.slot));
                }
            }
            destinations.sort();
        }
        let mut done = BTreeSet::new();
        let mut prepared_afresh = BTreeSet::new();
        for (thing, slot) in destinations {
            if !done.insert((thing, slot)) {
                continue;
            }
            if self.broken(thing) {
                continue;
            }
            let Some(r) = animated.resolved.things.get(&thing) else { continue };
            match slot {
                Some(slot) => {
                    if let Some(s) = r.slots.get(&slot)
                        && let Some(msim) = sims.module(&s.kind)
                    {
                        let prepared = self.modules.entry(thing).or_default();
                        // A card whose settings didn't move this step keeps what it has: a card
                        // varied across thousands of balls isn't prepared afresh for nothing.
                        let same = prepared.get(&slot).is_some_and(|p| p.kind == s.kind && p.params == s.params);
                        if !same {
                            prepared.insert(slot, Slotted::new(msim, &s.kind, &s.params));
                            prepared_afresh.insert((thing, Some(slot)));
                        }
                    } else if let Some(sim) = sims.thing(&r.kind) {
                        // A card no module sim steps is the thing's own to read (a Particles
                        // thing's Motion and Meets things cards, with their layers).
                        sim.settings_changed(thing, r, self);
                    }
                }
                None => {
                    if let Some(sim) = sims.thing(&r.kind) {
                        sim.settings_changed(thing, r, self);
                    }
                }
            }
        }
        if self.rules.is_some() {
            self.refresh_ball_changes(sims, &animated.resolved, &dirty, &prepared_afresh, time);
        }
        self.animated = Some(animated);
    }

    /// The whole-thing physics settings the rules change now or changed until now (to be worked
    /// out again this step), after dropping the changes that are over; and the settings that
    /// gained or lost a change since the last step.
    fn rule_keys(
        &mut self,
        sims: &Sims,
        time: f64,
    ) -> (Vec<crate::rules::ChangeKey>, BTreeSet<crate::rules::ChangeKey>) {
        let Some(rules) = &mut self.rules else { return (Vec::new(), BTreeSet::new()) };
        rules.prune(time);
        let dirty = rules.take_dirty();
        let rules = self.rules.as_ref().expect("taken above");
        let mut keys: BTreeSet<crate::rules::ChangeKey> = rules.whole_keys().cloned().collect();
        keys.extend(dirty.iter().filter(|k| k.row.is_none()).cloned());
        let keys = keys.into_iter().filter(|k| self.is_physics(sims, k.thing, k.slot, &k.key)).collect();
        (keys, dirty)
    }

    /// Whether a thing's setting (its own, or on its card in `slot`) changes how it simulates:
    /// every Motion and Bounce setting, and a thing's own settings its sim says do.
    pub fn is_physics(&self, sims: &Sims, thing: ThingId, slot: Option<SlotId>, key: &str) -> bool {
        match slot {
            Some(slot) => SIM_SLOTS.contains(&slot),
            None => self
                .kinds
                .get(&thing)
                .and_then(|k| sims.thing(k))
                .is_some_and(|s| s.affects_physics(bpe_core::vary::base_key(key))),
        }
    }

    /// Works out again what a rule changed for single balls: each such ball's own Motion and Bounce
    /// state (for cards whose sims take a state per ball), and the things whose own settings a
    /// rule changed per ball hear `settings_changed`. Only balls whose changes came, went or move
    /// now, or all of a card's when the card was prepared afresh.
    fn refresh_ball_changes(
        &mut self,
        sims: &Sims,
        resolved: &Resolved,
        dirty: &BTreeSet<crate::rules::ChangeKey>,
        afresh: &BTreeSet<(ThingId, Option<SlotId>)>,
        time: f64,
    ) {
        let Some(rules) = self.rules.as_ref() else { return };
        // The previous step's time: a change that moved since then is worked out again.
        let since = time - self.dt() as f64;
        // (thing, slot) -> rows to work out again.
        let mut todo: BTreeMap<(ThingId, Option<SlotId>), BTreeSet<u32>> = BTreeMap::new();
        for k in dirty.iter().filter(|k| k.row.is_some()) {
            todo.entry((k.thing, k.slot)).or_default().insert(k.row.expect("filtered"));
        }
        let mut moving: Vec<(ThingId, Option<SlotId>, u32)> = Vec::new();
        for (thing, prepared) in &self.modules {
            for (slot, s) in prepared {
                let rows = rules.ball_rows(*thing, Some(*slot));
                if rows.is_empty() && s.own.is_empty() {
                    continue;
                }
                let all = afresh.contains(&(*thing, Some(*slot))) || s.rows.as_ref().is_some_and(|r| r.changes());
                for row in rows {
                    let moves = rules.ball_keys(*thing, Some(*slot), row).iter().any(|k| rules.moving(k, since, time));
                    if all || moves {
                        moving.push((*thing, Some(*slot), row));
                    }
                }
            }
        }
        for (thing, slot, row) in moving {
            todo.entry((thing, slot)).or_default().insert(row);
        }
        // Things' own settings changed per ball.
        let mut things = BTreeSet::new();
        for ((thing, slot), _) in todo.iter().filter(|((_, s), _)| s.is_none()) {
            things.insert((*thing, *slot));
        }
        for thing in self.kinds.keys() {
            let rows = rules.ball_rows(*thing, None);
            if rows.iter().any(|&row| rules.ball_keys(*thing, None, row).iter().any(|k| rules.moving(k, since, time))) {
                things.insert((*thing, None));
            }
        }
        for ((thing, slot), rows) in todo {
            let Some(slot) = slot else { continue };
            let Some(s) = self.modules.get(&thing).and_then(|m| m.get(&slot)) else { continue };
            let Some(sim) = sims.module(&s.kind) else { continue };
            if !sim.per_ball() {
                continue;
            }
            let mut made = Vec::new();
            for row in rows {
                if !self.rules.as_ref().is_some_and(|r| !r.ball_keys(thing, Some(slot), row).is_empty()) {
                    made.push((row, None));
                    continue;
                }
                let mut params = s.params.clone();
                if let Some(r) = &s.rows {
                    let spread = self.ball_spread(sims, thing, row as usize);
                    params = r.params_at(&params, &spread);
                }
                if let Some(rules) = &self.rules {
                    rules.apply_ball(thing, Some(slot), row, &mut params, time);
                }
                made.push((row, Some(std::sync::Arc::from(sim.prepare(&params)))));
            }
            let s = self.modules.get_mut(&thing).and_then(|m| m.get_mut(&slot)).expect("found above");
            for (row, state) in made {
                match state {
                    Some(state) => {
                        s.own.insert(row, state);
                    }
                    None => {
                        s.own.remove(&row);
                    }
                }
            }
        }
        for (thing, _) in things {
            if self.broken(thing) {
                continue;
            }
            let Some(r) = resolved.things.get(&thing) else { continue };
            if let Some(sim) = sims.thing(&r.kind) {
                sim.settings_changed(thing, r, self);
            }
        }
    }

    /// Where a thing sits this step: on its parent when it rides one (one ball of it by the ball's
    /// number, when it names one), else where its own transform puts it. A rider whose parent has
    /// no pose (or no such ball) uses its own transform too. A thing the world doesn't know is at
    /// the origin.
    pub fn anchor(&self, thing: ThingId) -> Pose {
        let Some((parent, attach, transform)) = self.attach.get(&thing) else { return Pose::default() };
        self.anchor_as(*parent, *attach, transform)
    }

    /// Where a thing under `parent`, attached by `attach` with its own `transform`, would be
    /// now: [`World::anchor`] for a thing this world doesn't step (one stepped beside it, as the
    /// editor's preview handles of a Particles thing need).
    pub fn anchor_as(&self, parent: ThingId, attach: Attach, transform: &Transform) -> Pose {
        let parent = &parent;
        let own = Pose::from_transform(transform);
        let Attach::Ride { offset, turn_with_parent, item } = attach else { return own };
        let parent_pose = match item {
            Some(number) => self.bodies.get(parent).and_then(|b| {
                let row = b.row_of(number)?;
                Some(Pose { position: b.position[row], velocity: b.velocity[row], ..Pose::default() })
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

    /// Seconds the physics moves by this step: a step's, times the pace a rule's Slow motion
    /// set (1 without one).
    pub fn sim_dt(&self) -> f32 {
        self.dt() * self.pace
    }

    /// How fast the simulation runs this step (Slow motion): 1 normally.
    pub fn pace(&self) -> f32 {
        self.pace
    }

    /// Seconds the simulation has run: the video's [`World::time`] less what slow motion held
    /// back.
    pub fn sim_time(&self) -> f64 {
        self.time() - self.rules.as_ref().map_or(0.0, |r| r.lag)
    }

    /// The simulation's clock for the cards (`SimCx::time`): exactly `step / rate` until a rule
    /// slowed the simulation.
    fn sim_clock(&self) -> f32 {
        let lag = self.rules.as_ref().map_or(0.0, |r| r.lag);
        if lag == 0.0 { self.step as f32 / self.steps_per_second as f32 } else { (self.time() - lag) as f32 }
    }

    pub fn limits(&self) -> Limits {
        Limits {
            // A faster pace moves a ball further each step, so it may go only so much faster.
            ceiling: SAFE_MOVE * MAX_SUBSTEPS as f32 * self.steps_per_second as f32 / self.pace,
            resting: self.gravity.length() * self.sim_dt() * 4.0 + 1.0,
            press: Vec2::ZERO,
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

    /// Bytes the world keeps in video memory, for playback budgets.
    pub fn gpu_memory(&self) -> usize {
        self.gpu_memory.values().sum()
    }

    /// Starts keeping `thing`'s ball positions as `want` asks, so a trail can draw where they
    /// have been. Asked again, it keeps the longer of the two.
    pub fn keep_history(&mut self, thing: ThingId, want: crate::HistoryWant) {
        let rate = self.steps_per_second;
        let new = History::new(rate, want);
        match self.history.get(&thing) {
            Some(h) if h.cap() >= new.cap() => {}
            _ => {
                self.history.insert(thing, new);
            }
        }
    }

    /// `thing`'s card in `slot` (Motion or Bounce) as the world steps it now: its kind, its
    /// prepared state (the card's own, not a ball's when it varies across the balls) and whether
    /// it varies. `None` when the thing has no such card with a sim.
    pub fn card(&self, thing: ThingId, slot: SlotId) -> Option<Card<'_>> {
        let s = self.modules.get(&thing)?.get(&slot)?;
        Some(Card { kind: &s.kind, state: s.state.as_ref(), varies: s.varies() })
    }

    /// The prepared state of `thing`'s card in `slot` that body row `row` uses: its own when the
    /// card varies across the balls or a rule changed it for that ball alone.
    pub fn card_for(&self, thing: ThingId, slot: SlotId, row: usize) -> Option<&dyn ThingState> {
        Some(self.modules.get(&thing)?.get(&slot)?.state_for(row))
    }

    /// A copy of the world for test-firing a ball: only the things `keep` names are stepped (their
    /// states, walls, cards and keys kept; a card varied across the balls with its own values),
    /// with no history. Every other thing is left out: not
    /// stepped, its balls gone, its keys dropped, its pose left where it is now (a thing riding it
    /// stays put). Kinds, names and tags stay whole, so targets still name what they named.
    /// Decoration ([`World::decoration`]: things stepped beside the physics, or kept in it by a
    /// rule that reads them) is always left out.
    pub fn lone(&self, keep: &dyn Fn(ThingId) -> bool) -> World {
        // Decoration (things whose sim steps beside the physics) never changes a flight, even when
        // a rule keeps it in this world.
        let keep = |id: ThingId| keep(id) && !self.decoration.contains(&id);
        let keep = &keep;
        let kept = |id: &ThingId| keep(*id);
        let animated = self.animated.as_ref().and_then(|a| {
            let mut animation = a.animation.clone();
            animation.retain(|thing, _, _| keep(thing));
            // Only the kept things' resolved settings: a flight is cloned many times.
            let resolved = Resolved {
                things: a.resolved.things.iter().filter(|(id, _)| kept(id)).map(|(id, r)| (*id, r.clone())).collect(),
                problems: Vec::new(),
            };
            // What the rules did so far stays; nothing more is changed, so nothing is put back.
            (!animation.is_empty()).then_some(Animated { animation, resolved, pristine: None })
        });
        // A thing riding one ball of a thing left out stays where it is now: its parent's balls
        // are gone, so it would fall back to its own place.
        let attach = self
            .attach
            .iter()
            .map(|(id, entry)| {
                let (parent, attach, _) = entry;
                if matches!(attach, Attach::Ride { item: Some(_), .. }) && !keep(*parent) {
                    let pose = self.anchor(*id);
                    let transform = Transform { position: pose.position, rotation: pose.angle };
                    (*id, (*parent, Attach::Group, transform))
                } else {
                    (*id, *entry)
                }
            })
            .collect();
        World {
            step: self.step,
            steps_per_second: self.steps_per_second,
            gravity: self.gravity,
            bodies: self.bodies.iter().filter(|(id, _)| kept(id)).map(|(id, b)| (*id, b.clone())).collect(),
            walls: self.walls.iter().filter(|(id, _)| kept(id)).map(|(id, w)| (*id, w.clone())).collect(),
            states: self.states.iter().filter(|(id, _)| kept(id)).map(|(id, s)| (*id, s.clone())).collect(),
            poses: self.poses.clone(),
            names: self.names.clone(),
            tags: self.tags.clone(),
            history: BTreeMap::new(),
            seed: self.seed,
            camera: self.camera,
            video: self.video,
            beat_seconds: self.beat_seconds,
            beats_in_bar: self.beats_in_bar,
            state_memory: BTreeMap::new(),
            gpu_memory: BTreeMap::new(),
            kinds: self.kinds.clone(),
            order: self.order.iter().copied().filter(kept).collect(),
            pairs: Pairs::default(),
            walls_before: BTreeMap::new(),
            attach,
            // A card varied across the balls answers with its own values: the lone ball is none of them.
            modules: self
                .modules
                .iter()
                .filter(|(id, _)| kept(id))
                .map(|(id, m)| (*id, m.iter().map(|(slot, s)| (*slot, s.plain())).collect()))
                .collect(),
            animated,
            substep: (0, 1),
            sound: self.sound.clone(),
            light: self.light.clone(),
            pulls: BTreeMap::new(),
            fingerprint: self.fingerprint,
            // A test flight finds its ways as if no rule acted from now on, health staying as it is.
            rules: self.rules.as_ref().and_then(crate::rules::RulesState::lone),
            pace: 1.0,
            run: RunId::fresh(),
            beside: BTreeSet::new(),
            decoration: BTreeSet::new(),
            parts: self.parts.iter().filter(|(id, _)| kept(id)).map(|(id, p)| (*id, p.clone())).collect(),
            tree: self.tree.clone(),
        }
    }

    /// Puts `states` in for drawing, by thing (the editor's particle playback lends its Particles
    /// things' states to the main world, which never built them). Draw-only: [`World::take_back`]
    /// must put back what they replaced before the world steps again. A state of a thing the world
    /// didn't build is never stepped.
    pub fn lend_states(&mut self, states: Vec<(ThingId, Box<dyn ThingState>)>) -> Lent {
        let replaced = states.into_iter().map(|(id, s)| (id, self.states.insert(id, s))).collect();
        Lent { replaced }
    }

    /// Puts back what [`World::lend_states`] replaced.
    pub fn take_back(&mut self, lent: Lent) {
        for (id, old) in lent.replaced {
            match old {
                Some(s) => self.states.insert(id, s),
                None => self.states.remove(&id),
            };
        }
    }

    /// Gives `to` the Motion and Bounce cards `owner` has in `from` (as prepared now, the card's
    /// own values when it varies across the balls), in place of its own: a lone copy firing
    /// another thing's ball. Keys on `owner`'s cards aren't carried over.
    pub fn lend_cards(&mut self, from: &World, owner: ThingId, to: ThingId) {
        let lent: Prepared = from
            .modules
            .get(&owner)
            .map(|m| m.iter().map(|(slot, s)| (*slot, s.plain())).collect())
            .unwrap_or_default();
        if let Some(a) = &mut self.animated {
            a.animation.retain(|thing, slot, _| thing != to || slot.is_none());
        }
        if lent.is_empty() {
            self.modules.remove(&to);
        } else {
            self.modules.insert(to, lent);
        }
    }

    /// The scene's rules as they run, when it has an enabled rule.
    pub fn rules(&self) -> Option<&crate::rules::RulesState> {
        self.rules.as_ref()
    }

    /// `thing`'s health (its ball in body row `row`, for a thing with balls), when it has a
    /// Health card (rules part 4).
    pub fn health(&self, thing: ThingId, row: Option<u32>) -> Option<crate::rules::Hp> {
        self.rules.as_ref()?.health.of(thing, row)
    }

    /// How much of the health of `thing`'s ball in body row `row` (or of `thing`, without balls)
    /// is gone: 0 at full health or without health, 1 with none left (the Health spread).
    pub fn hurt(&self, thing: ThingId, row: usize) -> f32 {
        let Some(health) = self.rules.as_ref().map(|r| &r.health).filter(|h| h.has(thing)) else { return 0.0 };
        let row = self.bodies.contains_key(&thing).then_some(row as u32);
        health.of(thing, row).map_or(0.0, |hp| 1.0 - hp.share())
    }

    /// [`World::hurt`] for every body row of `thing`, in row order (empty without health).
    pub fn hurts(&self, thing: ThingId) -> Vec<f32> {
        if !self.rules.as_ref().is_some_and(|r| r.health.has(thing)) {
            return Vec::new();
        }
        (0..self.bodies.get(&thing).map_or(0, Bodies::len)).map(|row| self.hurt(thing, row)).collect()
    }

    /// The things' health as pulses read it (Follow health), when some thing has health.
    pub fn health_levels(&self) -> Option<&dyn bpe_core::HealthLevels> {
        self.rules.as_ref().filter(|r| !r.health.is_empty()).map(|_| self as &dyn bpe_core::HealthLevels)
    }

    /// Whether a rule broke `thing`.
    pub fn broken(&self, thing: ThingId) -> bool {
        self.rules.as_ref().is_some_and(|r| r.is_broken(thing))
    }

    /// The balls' own changes on one of `thing`'s settings now, for its sim to fold over each
    /// ball's own value (see [`crate::rules::Ruled`]).
    pub fn ruled(&self, thing: ThingId, slot: Option<SlotId>, key: &str) -> Option<crate::rules::Ruled> {
        self.rules.as_ref()?.ruled(thing, slot, key, self.time())
    }

    /// The world's light probe, when something physical in its scene follows light.
    pub fn light(&self) -> Option<&dyn crate::LightAt> {
        self.light.as_deref()
    }

    /// Where body row `row` of `thing` sits in the spreads of its varied settings: as its sim
    /// says, else its number of all the rows; with its speed and hits as they are now.
    pub fn ball_spread(&self, sims: &Sims, thing: ThingId, row: usize) -> bpe_core::BallSpread {
        let Some(bodies) = self.bodies.get(&thing) else { return bpe_core::BallSpread::default() };
        let own = self.kinds.get(&thing).and_then(|k| sims.thing(k)).and_then(|s| s.ball_spread(thing, self, row));
        let mut spread = own.unwrap_or_else(|| bpe_core::BallSpread {
            number: bodies.number.get(row).copied().unwrap_or(row as u32),
            count: bodies.len() as u32,
            seed: self.seed as u32,
            ..bpe_core::BallSpread::default()
        });
        spread.speed = bodies.velocity.get(row).map_or(0.0, |v| v.length());
        spread.hits = bodies.hits.get(row).copied().unwrap_or(0);
        spread.hurt = self.hurt(thing, row);
        spread
    }

    /// Gives the balls of every card that varies across them the states for where they sit in its
    /// spreads: balls new since the last time, and every ball when a spread follows speed or hits.
    fn refresh_rows(&mut self, sims: &Sims, log: &Log) {
        let mut todo = Vec::new();
        for (id, prepared) in &self.modules {
            let count = self.bodies.get(id).map_or(0, Bodies::len);
            for (slot, s) in prepared {
                if let Some(rows) = &s.rows
                    && let Some(from) = rows.stale_from(count)
                {
                    todo.push((*id, *slot, from, count, rows.needs_light()));
                }
            }
        }
        let time = self.time();
        for (id, slot, from, count, lit) in todo {
            let spreads: Vec<bpe_core::BallSpread> = (from..count)
                .map(|row| {
                    let mut spread = self.ball_spread(sims, id, row);
                    // A spread by Light or Shadow asks the probe, as the world is now.
                    if let (true, Some(light)) = (lit, &self.light) {
                        let b = light.ball(self, log, id, row, time);
                        (spread.light, spread.shadow) = (b.light, b.shadow);
                    }
                    spread
                })
                .collect();
            let Some(s) = self.modules.get_mut(&id).and_then(|m| m.get_mut(&slot)) else { continue };
            let Some(sim) = sims.module(&s.kind) else { continue };
            let Slotted { params, rows, .. } = s;
            if let Some(rows) = rows {
                rows.fill(sim, params, from, &spreads);
            }
        }
    }

    /// Works out every ball's pull from the lights for this step (Moths), from the world as it is
    /// now: once a step, so a crowd of moths costs one probe reading a ball a step.
    fn work_out_pulls(&mut self, sims: &Sims, log: &Log) {
        self.pulls.clear();
        let Some(light) = self.light.clone() else { return };
        let time = self.time();
        let mut pulls = BTreeMap::new();
        for (id, prepared) in &self.modules {
            let Some(s) = prepared.get(&SlotId::Motion) else { continue };
            let Some(sim) = sims.module(&s.kind) else { continue };
            if sim.light_pull(s.state.as_ref()).is_none() {
                continue;
            }
            let Some(bodies) = self.bodies.get(id) else { continue };
            let each = (0..bodies.len())
                .map(|row| match sim.light_pull(s.state_for(row)) {
                    Some(p) if bodies.alive[row] && !bodies.held[row] => light.pull(self, log, *id, row, time, &p),
                    _ => Vec2::ZERO,
                })
                .collect();
            pulls.insert(*id, each);
        }
        self.pulls = pulls;
    }

    /// Steps until `time` seconds have passed.
    pub fn advance_to(&mut self, time: f64, sims: &Sims, log: &mut Log) {
        let target = step_at(time, self.steps_per_second);
        while self.step < target {
            self.step(sims, log);
        }
    }

    /// Moves every live ball back to `a` of the way through the last step (0 where it was when
    /// the step began, 1 where it is now), for drawing a moment between two steps (the bake's
    /// motion blur, lighting part 3). Draw-only: [`World::restore_between`] must put them back
    /// before the world steps again, so the physics never sees it.
    pub fn set_between(&mut self, a: f32) -> Between {
        let a = a.clamp(0.0, 1.0);
        let mut saved = Vec::with_capacity(self.bodies.len());
        for (id, bodies) in self.bodies.iter_mut() {
            saved.push((*id, bodies.position.clone()));
            for i in 0..bodies.len() {
                if bodies.alive[i] {
                    let (from, to) = (bodies.previous[i], bodies.position[i]);
                    bodies.position[i] = from + (to - from) * a;
                }
            }
        }
        Between { saved }
    }

    /// Puts the balls back where [`World::set_between`] found them.
    pub fn restore_between(&mut self, between: Between) {
        for (id, positions) in between.saved {
            if let Some(bodies) = self.bodies.get_mut(&id) {
                bodies.position = positions;
            }
        }
    }

    /// One step: every stage, in order.
    pub fn step(&mut self, sims: &Sims, log: &mut Log) {
        self.step += 1;
        let first = log.len();
        // What the rules did in the last step's Rules stage happens now, before anything moves.
        self.apply_rule_actions(sims, log);
        self.walls_before.clear();
        self.walls_before.extend(self.walls.iter().map(|(id, w)| (*id, w.outline.clone())));
        self.animate(sims, log);
        // Slow motion: the physics moves by a share of the step (the video clock runs on).
        let time = self.time();
        self.pace = self.rules.as_ref().map_or(1.0, |r| r.pace_at(time));
        let limits = self.limits();
        let dt = self.sim_dt();
        // Things take their turns in tree order: parents go first, so riders see where their
        // parent moved to.
        let order = self.order.clone();

        // Spawn: things add what they release this step.
        for id in &order {
            if let Some(sim) = self.kinds.get(id).and_then(|kind| sims.thing(kind)) {
                sim.spawn(*id, self, log);
            }
        }
        self.refresh_rows(sims, log);
        self.work_out_pulls(sims, log);

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
            let before = log.len();
            self.substep(sims, &order, sub_dt, &limits, log);
            if !self.history.is_empty() {
                let at = (self.step - 1) as f64 + (k + 1) as f64 / substeps as f64;
                self.note_corners(&log.events()[before..], at);
            }
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
        // Rules: what happened this step, and what to do about it at the next one's start.
        if let Some(mut rules) = self.rules.take() {
            if self.pace != 1.0 {
                rules.lag += self.dt() as f64 * (1.0 - self.pace as f64);
            }
            let events = log.events()[first..].to_vec();
            rules.run(self, sims, &events, log);
            self.rules = Some(rules);
        }
    }

    /// Notes in the histories where each ball hit in `events` was at the end of the substep it
    /// hit in (the moment `at`, in steps): just touching what it hit, where it turned. (A wall
    /// hit has noted the moment it touched already, in `collide`.)
    fn note_corners(&mut self, events: &[Event], at: f64) {
        for e in events.iter().filter(|e| e.kind == EventKind::Hit) {
            for side in std::iter::once(e.a).chain(e.b) {
                let (Some(history), Some(bodies)) = (self.history.get_mut(&side.thing), self.bodies.get(&side.thing))
                else {
                    continue;
                };
                if let Some(&place) = bodies.position.get(side.item as usize) {
                    history.corner(side.item as usize, at, place);
                }
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
        let time = self.sim_clock();
        let World { gravity, bodies, modules, seed, pulls, .. } = self;
        let gravity = *gravity;
        let seed = *seed;
        for (id, bodies) in bodies.iter_mut() {
            if let Some(s) = modules.get(id).and_then(|m| m.get(&SlotId::Motion))
                && let Some(sim) = sims.module(&s.kind)
            {
                let cx = SimCx { thing: *id, time, seed };
                if s.varies() {
                    for i in 0..bodies.len() {
                        if bodies.alive[i] {
                            sim.forces_one(s.state_for(i), bodies, i, gravity, dt, &cx);
                        }
                    }
                } else {
                    sim.forces_at(s.state.as_ref(), bodies, gravity, dt, &cx);
                }
            }
            // The lights' pull (Moths), worked out at the step's start.
            if let Some(pull) = pulls.get(id) {
                for (i, p) in pull.iter().enumerate().take(bodies.len()) {
                    if bodies.alive[i] && !bodies.held[i] {
                        bodies.velocity[i] += *p * dt;
                    }
                }
            }
            bodies.start.copy_from_slice(&bodies.position);
            for i in 0..bodies.len() {
                if bodies.alive[i] && !bodies.held[i] {
                    let v = bodies.velocity[i];
                    bodies.position[i] += v * dt;
                } else if bodies.held[i] {
                    // A held ball gathers no speed from the forces while it waits.
                    bodies.velocity[i] = Vec2::ZERO;
                }
            }
        }
    }

    /// Collide and Respond, then the Hit events: first the walls around the balls, then the
    /// balls against each other. What each ball touched holds it up in the pair solve.
    fn collide(&mut self, sims: &Sims, dt: f32, limits: &Limits, log: &mut Log) {
        let time = self.sim_clock();
        let World {
            step,
            steps_per_second,
            gravity,
            bodies,
            walls,
            walls_before,
            modules,
            kinds,
            tags,
            pairs,
            seed,
            substep,
            history,
            pace,
            ..
        } = self;
        let step = *step;
        let (sub_index, sub_count) = *substep;
        let seed = *seed;
        let step_dt = 1.0 / *steps_per_second as f32 * *pace;
        let matcher = |t: &Target, me: ThingId, other: ThingId| matches(kinds, tags, t, me, other);
        let press = |thing: ThingId, row: usize| press_of(modules, sims, thing, row, *gravity, step_dt);
        let mut supports: Vec<Support> = Vec::new();
        for (mover, bodies) in bodies.iter_mut() {
            let mover_bounce = modules.get(mover).and_then(|m| m.get(&SlotId::Bounce));
            let own = mover_bounce.and_then(|s| Some((sims.module(&s.kind)?, s.state.as_ref())));
            let cx = SimCx { thing: *mover, time, seed };
            let touch = own.map_or(Touch::Edge, |(sim, state)| sim.touch(state));
            // This mover's own: a ball pulled harder than the scene's gravity presses harder. A
            // Motion that varies across the balls presses each ball its own way.
            let pressed = modules.get(mover).and_then(|m| m.get(&SlotId::Motion)).is_some_and(Slotted::varies);
            let thing_limits = Limits { press: press(*mover, 0), ..*limits };
            // A wall balls pass through bounces nothing.
            for (wall_id, wall) in walls.iter().filter(|(_, w)| !w.passes) {
                let before = walls_before.get(wall_id).unwrap_or(&wall.outline);
                let wall_bounce = modules.get(wall_id).and_then(|m| m.get(&SlotId::Bounce));
                let decider = decide(sims, wall_bounce, mover_bounce);
                let special = is_special(own, &matcher, *mover, *wall_id);
                for i in 0..bodies.len() {
                    if !bodies.alive[i] || bodies.held[i] {
                        continue;
                    }
                    let row_limits;
                    let limits = if pressed {
                        row_limits = Limits { press: press(*mover, i), ..*limits };
                        &row_limits
                    } else {
                        &thing_limits
                    };
                    // The mover's own card answers with this ball's own state.
                    let decider =
                        decider.map(|(sim, s, mine)| (sim, if mine { s.state_for(i) } else { s.state.as_ref() }));
                    let own = match mover_bounce.filter(|s| s.varies()) {
                        Some(s) => own.map(|(sim, _)| (sim, s.state_for(i))),
                        None => own,
                    };
                    let velocity = bodies.velocity[i];
                    let radius = bodies.radius[i];
                    let previous = bodies.previous[i];
                    let start = bodies.start[i];
                    let at = &mut bodies.position[i];
                    let moved_to = *at;
                    let Some(contact) = keep_clear_from(at, previous, before, velocity, radius, touch, wall) else {
                        continue;
                    };
                    // The ball was moved back to just touching: the gap to there along the
                    // normal, at the start of the substep and where it had moved to.
                    let touching = *at;
                    let gap = |p: Vec2| (p - touching).dot(contact.normal);
                    // At the start against the wall where it was then: one coming at the ball
                    // has come this far nearer since.
                    let start_gap = gap(start) + contact.wall_velocity.dot(contact.normal) * dt;
                    let within = crossing(start_gap, gap(moved_to));
                    // A ball touching with its middle bounces exactly where its centre crossed
                    // the line, off the outline's own normal there.
                    let exact = match touch {
                        Touch::Middle => middle_crossing(start, moved_to, previous, before, velocity, wall),
                        Touch::Edge => None,
                    };
                    let (mut contact, within) = exact.unwrap_or((contact, within));
                    // Where its centre was as it touched, on the way it came in.
                    let crossed = start.lerp(moved_to, within);
                    let hit = contact.speed >= limits.resting_along(contact.normal);
                    // A ball that touched on its way in goes on from there for the rest of the
                    // substep. One that began the substep already in the wall (another ball
                    // pushed it there) only bounces where it was put back, so a pile settles.
                    let carries = hit
                        && match touch {
                            Touch::Middle => exact.is_some(),
                            Touch::Edge => start_gap > -TOUCHING,
                        };
                    if carries && touch == Touch::Edge {
                        // It touched there, not where it was slid back to along the wall.
                        contact.point += crossed - touching;
                    }
                    let hit_at = step_fraction(sub_index, sub_count, within);
                    supports.push(Support {
                        thing: *mover,
                        item: i as u32,
                        normal: contact.normal,
                        wall_velocity: contact.wall_velocity,
                    });
                    respond(decider, &contact, bodies, i, limits, special, &cx);
                    // And goes on from there for the rest of the substep, along its new velocity,
                    // so a bounce loses none of the step: balls that touch a hair apart in time
                    // leave a hair apart, not a step apart. A ball resting on a wall stays put.
                    if carries {
                        let mut carried = crossed + bodies.velocity[i] * ((1.0 - within) * dt);
                        match touch {
                            // Unless that would put it on the wrong side, near a corner.
                            Touch::Middle => {
                                if carried.is_finite() && on_own_side(carried, previous, before, wall) {
                                    bodies.position[i] = carried;
                                }
                            }
                            // Kept clear of the wall again where the rest of the way would reach
                            // into it (a curve or a corner); and not across any other wall's line
                            // (one already passed this substep wouldn't stop it).
                            Touch::Edge => {
                                if carried.is_finite() {
                                    let v = bodies.velocity[i];
                                    keep_clear_from(&mut carried, previous, before, v, radius, touch, wall);
                                    let crosses = |(id, w): (&ThingId, &Wall)| {
                                        let before = walls_before.get(id).unwrap_or(&w.outline);
                                        !w.passes && !on_own_side(carried, previous, before, w)
                                    };
                                    if !walls.iter().any(crosses) {
                                        bodies.position[i] = carried;
                                    }
                                }
                            }
                        }
                    }
                    if hit {
                        bodies.hits[i] += 1;
                        // Where its centre was as it touched, on the way it came in; the end of the
                        // substep notes where it went on to (`note_corners`).
                        if let Some(h) = history.get_mut(mover) {
                            // In f64, as the end-of-substep moments are, so the two stay in order.
                            let touched = (sub_index as f64 + within as f64) / sub_count.max(1) as f64;
                            h.corner(i, (step - 1) as f64 + touched, crossed);
                        }
                        log.push(Event {
                            step,
                            kind: EventKind::Hit,
                            a: Party { thing: *mover, item: i as u32 },
                            // The letter of a Text it hit; 0 for every other wall.
                            b: Some(Party { thing: *wall_id, item: contact.part }),
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
        collide_pairs(bodies, modules, &matcher, &press, sims, pairs, &supports, step, substep, dt, limits, log);

        // A ball touching with its middle sits a hair's breadth from the line, so a nudge from
        // another ball can carry it across: put it back on its own side (no bounce, no event).
        for (mover, bodies) in bodies.iter_mut() {
            let bounce = modules.get(mover).and_then(|m| m.get(&SlotId::Bounce));
            let touch = bounce.and_then(|s| Some(sims.module(&s.kind)?.touch(s.state.as_ref())));
            if touch != Some(Touch::Middle) {
                continue;
            }
            for (wall_id, wall) in walls.iter().filter(|(_, w)| !w.passes) {
                let before = walls_before.get(wall_id).unwrap_or(&wall.outline);
                for i in 0..bodies.len() {
                    if bodies.alive[i] {
                        let (previous, velocity) = (bodies.previous[i], bodies.velocity[i]);
                        let at = &mut bodies.position[i];
                        keep_clear_from(at, previous, before, velocity, 0.0, Touch::Middle, wall);
                    }
                }
            }
        }
    }
}

/// Follow health: a thing's share of its starting health (rules part 4).
impl bpe_core::HealthLevels for World {
    fn share(&self, thing: ThingId) -> f32 {
        self.rules.as_ref().map_or(0.0, |r| r.health.thing_share(self, thing))
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

/// Who answers a contact: the hit thing's Bounce when it takes over, else the mover's (`true`).
fn decide<'a>(sims: &'a Sims, hit: Bounce<'a>, mover: Bounce<'a>) -> Option<(&'a dyn ModuleSim, &'a Slotted, bool)> {
    hit.and_then(|s| {
        let sim = sims.module(&s.kind)?;
        sim.takes_over(s.state.as_ref()).then_some((sim, s, false))
    })
    .or_else(|| mover.and_then(|s| Some((sims.module(&s.kind)?, s, true))))
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

/// The step a moment `time` seconds in falls in, at `rate` steps a second: the last one taken
/// by then. A time worked out as a frame's (`index / fps`) that lands a hair under a step still
/// counts as that step, so the export and the editor draw every frame from the same step.
pub fn step_at(time: f64, rate: u32) -> u64 {
    // A millionth of a step: far below any real gap between frames, far above the rounding.
    (time * rate as f64 + 1e-6).floor().max(0.0) as u64
}

/// `thing`'s balls' own steady pull (Gravity ×, Wind) four steps' worth, as
/// [`Limits::press`]: how hard a ball lying on something presses into it. Zero for a thing whose
/// Motion doesn't pull steadily. `dt` is a whole step.
/// Body row `row`'s own, when the Motion varies across the balls.
pub(crate) fn press_of(
    modules: &BTreeMap<ThingId, Prepared>,
    sims: &Sims,
    thing: ThingId,
    row: usize,
    gravity: Vec2,
    dt: f32,
) -> Vec2 {
    modules
        .get(&thing)
        .and_then(|m| m.get(&SlotId::Motion))
        .and_then(|s| sims.module(&s.kind)?.pull(s.state_for(row), gravity))
        .map_or(Vec2::ZERO, |p| p * dt * 4.0)
}

/// Whether something physical in the scene follows light, so its worlds need the light probe: a
/// physics setting following light, a Motion or Bounce card whose settings spread by Light or
/// Shadow, or a Motion card drawing its balls to the lights.
pub fn needs_light(scene: &Scene, resolved: &Resolved, sims: &Sims, animation: Option<&Animation>) -> bool {
    let follows = animation.is_some_and(|animation| {
        animation.addresses().iter().any(|a| scene.pulses.get(*a).is_some_and(|p| p.source.follows_light()))
    });
    follows
        || resolved.things.values().any(|r| {
            SIM_SLOTS.iter().any(|slot| {
                let Some(s) = r.slots.get(slot) else { return false };
                let Some(sim) = sims.module(&s.kind) else { return false };
                let spreads = bpe_core::vary::varies_in(&s.params).iter().any(|v| v.to.is_some() && v.by.reads_light());
                spreads || (*slot == SlotId::Motion && sim.light_pull(sim.prepare(&s.params).as_ref()).is_some())
            })
        })
}

/// Whether any of `animation`'s pulses follows sound.
fn follows_sound(scene: &Scene, animation: &Animation) -> bool {
    animation
        .addresses()
        .iter()
        .any(|a| scene.pulses.get(*a).is_some_and(|p| matches!(p.source, bpe_core::PulseSource::Sound { .. })))
}

/// A hash of everything about a scene that changes how it simulates. Scenes that differ only
/// in how they look fingerprint the same, so the editor can keep playing through
/// those edits.
pub fn fingerprint(scene: &Scene, resolved: &Resolved, sims: &Sims) -> u64 {
    fingerprint_of(scene, resolved, sims, true)
}

/// [`fingerprint`] without the balls' history (how much of their past trails and particles
/// keep): what the sound hears of a run. A longer trail changes the run the editor keeps, not
/// its hits, so the sound plays on through it.
/// Things stepped beside the physics that the sound hears ([`crate::ThingSim::heard`]: a Particles
/// thing whose sound is on) count too: how they run and what of their sound card changes what they
/// log, so editing their particles runs the sound again while a volume change only patches it.
pub fn sound_fingerprint(scene: &Scene, resolved: &Resolved, sims: &Sims) -> u64 {
    let base = fingerprint_of(scene, resolved, sims, false);
    let heard = |r: &bpe_core::ResolvedThing| sims.thing(&r.kind).and_then(|s| s.heard(r));
    if !resolved.things.values().any(|r| heard(r).is_some()) {
        return base;
    }
    let mut h = Hasher(base);
    h.bytes(b"heard");
    beside_hash(&mut h, scene, resolved, sims, &|r| heard(r).is_some());
    // What of its sound card changes what it logs, beside the physics or in it (a rule reads it).
    for (id, r) in &resolved.things {
        if let Some(print) = heard(r) {
            h.bytes(&id.0.to_le_bytes());
            h.bytes(&print.to_le_bytes());
        }
    }
    h.0
}

/// Whether thing `id` (resolved as `r`) is stepped beside the physics: its sim says it doesn't
/// step with it ([`crate::ThingSim::in_main`] false), no enabled rule reads it
/// ([`crate::BlockSim::keeps`]: a rule that reads it changes the balls, so it steps with them), no
/// other thing reads it as it steps ([`crate::ThingSim::keeps`]: a camera following it) and no
/// setting follows its sound (W12).
pub fn is_beside(scene: &Scene, sims: &Sims, id: ThingId, r: &bpe_core::ResolvedThing) -> bool {
    let Some(sim) = sims.thing(&r.kind).filter(|s| !s.in_main(r)) else { return false };
    let tags = scene.things.get(&id).map_or(NO_TAGS, |t| &t.tags);
    let read_by_a_rule = scene
        .rules
        .list
        .iter()
        .filter(|rule| rule.enabled)
        .flat_map(|rule| &rule.blocks)
        .any(|b| sims.block(&b.kind).is_some_and(|s| s.keeps(&b.params, id, &r.kind, tags)));
    // Another thing reads it as it steps (a camera following it).
    let read_by_a_thing = scene
        .things
        .values()
        .filter(|t| t.enabled && t.id != id)
        .any(|t| sims.thing(&t.kind).is_some_and(|s| s.keeps(scene, t, id)));
    // A setting follows its sound (its own, or a bus while the sound hears it): its moments must
    // be in the log Follow sound reads.
    let heard = || sim.heard(r).is_some();
    let sound_followed = scene.pulses.values().any(|p| match &p.source {
        bpe_core::PulseSource::Sound { shape, source } => {
            *shape == id || (matches!(source, bpe_core::SoundSource::Bus(_)) && heard())
        }
        _ => false,
    });
    !(read_by_a_rule || read_by_a_thing || sound_followed)
}

/// `resolved` without the things `drop` says, borrowed when it drops none.
fn without<'a>(
    resolved: &'a Resolved,
    drop: impl Fn(ThingId, &bpe_core::ResolvedThing) -> bool,
) -> std::borrow::Cow<'a, Resolved> {
    if !resolved.things.iter().any(|(id, r)| drop(*id, r)) {
        return std::borrow::Cow::Borrowed(resolved);
    }
    let things = resolved.things.iter().filter(|(id, r)| !drop(**id, r)).map(|(id, r)| (*id, r.clone())).collect();
    std::borrow::Cow::Owned(Resolved { things, problems: resolved.problems.clone() })
}

/// `resolved` without the things stepped beside the physics ([`is_beside`]): what the editor's
/// main playback and the aiming build from. A world built from it steps every ball exactly as one
/// built from the whole scene.
pub fn main_part<'a>(scene: &Scene, resolved: &'a Resolved, sims: &Sims) -> std::borrow::Cow<'a, Resolved> {
    without(resolved, |id, r| is_beside(scene, sims, id, r))
}

/// [`main_part`] and the things beside it whose events the sound hears ([`crate::ThingSim::heard`]):
/// what the sound engine's physics builds from.
pub fn heard_part<'a>(scene: &Scene, resolved: &'a Resolved, sims: &Sims) -> std::borrow::Cow<'a, Resolved> {
    let heard = |r: &bpe_core::ResolvedThing| sims.thing(&r.kind).is_some_and(|s| s.heard(r).is_some());
    without(resolved, |id, r| is_beside(scene, sims, id, r) && !heard(r))
}

/// Whether some thing of `resolved` is stepped beside the physics.
pub fn has_beside(scene: &Scene, resolved: &Resolved, sims: &Sims) -> bool {
    resolved.things.iter().any(|(id, r)| is_beside(scene, sims, *id, r))
}

/// A hash of everything that changes how the things stepped beside the physics run (and the
/// physics they read), 0 when there are none: the editor's particle playback starts over when it
/// changes. Their Motion and Bounce cards count whole, as nothing but the thing reads them.
pub fn beside_fingerprint(scene: &Scene, resolved: &Resolved, sims: &Sims) -> u64 {
    if !has_beside(scene, resolved, sims) {
        return 0;
    }
    let mut h = Hasher(fingerprint_of(scene, resolved, sims, true));
    beside_hash(&mut h, scene, resolved, sims, &|_| true);
    h.0
}

/// Hashes into `h` the things stepped beside the physics that `pick` takes: their settings, place,
/// parent, Motion and Bounce cards whole, and their keys.
fn beside_hash(
    h: &mut Hasher,
    scene: &Scene,
    resolved: &Resolved,
    sims: &Sims,
    pick: &dyn Fn(&bpe_core::ResolvedThing) -> bool,
) {
    let chosen = |id: ThingId, r: &bpe_core::ResolvedThing| is_beside(scene, sims, id, r) && pick(r);
    // What the main things hold that only the beside things read (the tempo).
    for (id, r) in &resolved.things {
        let Some(sim) = sims.thing(&r.kind).filter(|s| s.in_main(r)) else { continue };
        for (key, value) in r.params.iter() {
            if sim.affects_beside(key) {
                h.bytes(&id.0.to_le_bytes());
                h.bytes(key.as_bytes());
                h.value(value);
            }
        }
    }
    for (id, r) in &resolved.things {
        if !chosen(*id, r) {
            continue;
        }
        let sim = sims.thing(&r.kind).expect("a beside thing has a sim");
        let Some(thing) = scene.things.get(id) else { continue };
        h.bytes(&id.0.to_le_bytes());
        h.bytes(&sim.more_fingerprint(r).to_le_bytes());
        // What else of the saved scene it reads (the Text a Particles thing spells).
        let more = sim.fingerprint_more(scene, thing, r);
        if more != 0 {
            h.bytes(&more.to_le_bytes());
        }
        h.bytes(r.kind.as_bytes());
        h.bytes(&thing.parent.map_or(u32::MAX, |p| p.0).to_le_bytes());
        h.f32(thing.transform.position.x);
        h.f32(thing.transform.position.y);
        h.f32(thing.transform.rotation);
        h.bytes(&[thing.enabled as u8]);
        for (key, value) in r.params.iter() {
            h.bytes(key.as_bytes());
            h.value(value);
        }
        for (slot, s) in r.slots.iter().filter(|(slot, _)| !SIM_SLOTS.contains(slot)) {
            for (key, value) in s.params.iter().filter(|(key, _)| sim.beside_card_key(*slot, key)) {
                h.bytes(&[*slot as u8]);
                h.bytes(key.as_bytes());
                h.value(value);
            }
        }
        for slot in SIM_SLOTS {
            if let Some(s) = r.slots.get(&slot) {
                h.bytes(&[slot as u8]);
                h.bytes(s.kind.as_bytes());
                h.bytes(format!("{:?}", s.layers).as_bytes());
                for (key, value) in s.params.iter() {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
        }
    }
    // The scene's cards only they read (the air).
    for (id, r) in &resolved.things {
        for slot in BESIDE_SLOTS {
            if let Some(s) = r.slots.get(&slot) {
                h.bytes(&id.0.to_le_bytes());
                h.bytes(&[slot as u8]);
                h.bytes(s.kind.as_bytes());
                for (key, value) in s.params.iter() {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
        }
    }
    // Their keys too, and those cards', with what of the sound or the light they follow.
    if let Some(animation) = physics_animation(scene, resolved, sims) {
        let theirs = |a: &bpe_core::Address| {
            a.slot.is_some_and(|s| BESIDE_SLOTS.contains(&s))
                || resolved.things.get(&a.thing).is_some_and(|r| chosen(a.thing, r))
        };
        let follows = |light: bool| {
            animation.addresses().iter().filter(|a| theirs(a)).any(|a| {
                scene.pulses.get(*a).is_some_and(|p| {
                    if light {
                        p.source.follows_light()
                    } else {
                        matches!(p.source, bpe_core::PulseSource::Sound { .. })
                    }
                })
            })
        };
        if let Some(maker) = &sims.sound
            && follows(false)
        {
            h.bytes(b"beside hear");
            h.bytes(&maker.fingerprint(scene).to_le_bytes());
        }
        if let Some(maker) = &sims.light
            && follows(true)
        {
            h.bytes(b"beside see");
            h.bytes(&maker.fingerprint(scene).to_le_bytes());
        }
        for a in animation.addresses() {
            if theirs(a) {
                h.bytes(format!("{a:?}").as_bytes());
                if let Some(track) = scene.tracks.get(a) {
                    h.bytes(format!("{track:?}").as_bytes());
                }
                if let Some(pulse) = scene.pulses.get(a) {
                    h.bytes(format!("{pulse:?}").as_bytes());
                }
            }
        }
    }
}

fn fingerprint_of(scene: &Scene, resolved: &Resolved, sims: &Sims, history: bool) -> u64 {
    // Things stepped beside the physics never change it.
    let resolved = &*main_part(scene, resolved, sims);
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
                if sim.affects_physics_in(scene, thing, bpe_core::vary::base_key(key)) {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
            let more = sim.fingerprint_more(scene, thing, r);
            if more != 0 {
                h.bytes(b"more");
                h.bytes(&more.to_le_bytes());
            }
        }
        if let Some(want) = sims.thing(&r.kind).and_then(|s| s.history(r)).filter(|_| history) {
            // Whether a history is kept and how long, not how its trails look.
            h.bytes(b"history");
            h.f32(want.rounded());
            h.bytes(&[want.forever as u8]);
        }
        for tag in &thing.tags {
            h.bytes(tag.as_bytes());
        }
        // Only modules the processor simulates count, and the cards of a thing that reads its own.
        let owns = sims.thing(&r.kind).is_some_and(|s| s.own_cards());
        for slot in SIM_SLOTS {
            if let Some(s) = r.slots.get(&slot)
                && (owns || sims.module(&s.kind).is_some())
            {
                h.bytes(&[slot as u8]);
                h.bytes(s.kind.as_bytes());
                if owns {
                    // What each layer is and does; not its name.
                    for l in &s.layers {
                        h.bytes(format!("{:?}", (l.id, &l.kind, l.parent, &l.only)).as_bytes());
                    }
                }
                for (key, value) in s.params.iter() {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
            }
        }
        // What else its stepping keeps (two-way particles' trails and marks), so a change rebuilds it.
        if let Some(more) = sims.thing(&r.kind).map(|s| s.more_fingerprint(r)).filter(|m| *m != 0) {
            h.bytes(b"more");
            h.bytes(&more.to_le_bytes());
        }
    }
    // Health cards change the run (a ball breaks at zero, rules read it): only scenes with one.
    for (id, r) in &resolved.things {
        if let Some(s) = r.slots.get(&SlotId::Health)
            && sims.health(&s.kind).is_some()
        {
            h.bytes(b"health");
            h.bytes(&id.0.to_le_bytes());
            h.bytes(s.kind.as_bytes());
            for (key, value) in s.params.iter() {
                h.bytes(key.as_bytes());
                h.value(value);
            }
        }
    }
    // A thing in the physics that reads its own cards (two-way particles) may read the air too:
    // then the air's cards and keys count.
    if resolved.things.values().any(|r| sims.thing(&r.kind).is_some_and(|s| s.own_cards())) {
        for (id, r) in &resolved.things {
            for slot in BESIDE_SLOTS {
                if let Some(s) = r.slots.get(&slot) {
                    h.bytes(b"beside card");
                    h.bytes(&id.0.to_le_bytes());
                    h.bytes(&[slot as u8]);
                    h.bytes(s.kind.as_bytes());
                    for (key, value) in s.params.iter() {
                        h.bytes(key.as_bytes());
                        h.value(value);
                    }
                }
            }
        }
        if let Some(animation) = physics_animation(scene, resolved, sims) {
            for a in animation.addresses() {
                if a.slot.is_some_and(|s| BESIDE_SLOTS.contains(&s)) {
                    h.bytes(format!("{a:?}").as_bytes());
                    if let Some(track) = scene.tracks.get(a) {
                        h.bytes(format!("{track:?}").as_bytes());
                    }
                    if let Some(pulse) = scene.pulses.get(a) {
                        h.bytes(format!("{pulse:?}").as_bytes());
                    }
                }
            }
        }
    }
    // Keyframes and pulses on physics settings: moving a key changes the run. Keys on the cards
    // only the things beside the physics read are theirs ([`beside_fingerprint`]).
    let animation = physics_animation(scene, resolved, sims).as_ref().and_then(main_animation);
    if let Some(animation) = &animation {
        for a in animation.addresses() {
            h.bytes(format!("{a:?}").as_bytes());
            if let Some(track) = scene.tracks.get(a) {
                h.bytes(format!("{track:?}").as_bytes());
            }
            if let Some(pulse) = scene.pulses.get(a) {
                h.bytes(format!("{pulse:?}").as_bytes());
            }
        }
        // A physics setting that follows sound moves with what of the sound it reads.
        if let Some(maker) = &sims.sound
            && follows_sound(scene, animation)
        {
            h.bytes(&maker.fingerprint(scene).to_le_bytes());
        }
    }
    // Rules change the run: every enabled rule's blocks and links (not names or switched-off ones),
    // and what of the sound and the light they read.
    if scene.rules.any_enabled() {
        let blocks = || {
            scene
                .rules
                .list
                .iter()
                .filter(|r| r.enabled)
                .flat_map(|r| &r.blocks)
                .filter_map(|b| Some((sims.block(&b.kind)?, b)))
        };
        if let Some(maker) = &sims.sound
            && blocks().any(|(sim, b)| sim.hears_sound(&b.params))
        {
            h.bytes(b"rules hear");
            h.bytes(&maker.fingerprint(scene).to_le_bytes());
        }
        if let Some(maker) = &sims.light
            && blocks().any(|(sim, b)| sim.sees_light(&b.params))
        {
            h.bytes(b"rules see");
            h.bytes(&maker.fingerprint(scene).to_le_bytes());
        }
        h.bytes(b"rules");
        for rule in scene.rules.list.iter().filter(|r| r.enabled) {
            h.bytes(&rule.id.0.to_le_bytes());
            for b in &rule.blocks {
                h.bytes(&b.id.0.to_le_bytes());
                h.bytes(b.kind.as_bytes());
                for i in &b.inputs {
                    h.bytes(&i.0.to_le_bytes());
                }
                for (key, value) in b.params.iter() {
                    h.bytes(key.as_bytes());
                    h.value(value);
                }
                h.bytes(&[0xff]);
            }
        }
    }
    // Physics that follows light moves with what the light probe reads.
    if let Some(maker) = &sims.light
        && needs_light(scene, resolved, sims, animation.as_ref())
    {
        h.bytes(b"light");
        h.bytes(&maker.fingerprint(scene).to_le_bytes());
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
                    self.bytes(&[s.color.r, s.color.g, s.color.b, s.color.a, s.blend as u8]);
                    self.f32(s.middle);
                }
                self.bytes(&[g.mix as u8, g.looped as u8]);
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
            Value::LightScenes(l) => {
                self.bytes(&[14]);
                self.bytes(format!("{l:?}").as_bytes());
            }
            Value::Curve(c) => {
                self.bytes(&[13]);
                self.bytes(&(c.len() as u64).to_le_bytes());
                for p in c {
                    self.f32(p.x);
                    self.f32(p.y);
                    self.f32(p.bend);
                }
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
            Value::Who(w) => {
                self.bytes(&[15]);
                self.bytes(format!("{w:?}").as_bytes());
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

/// Every thing with a Health card at full health: what Follow health reads while the world is built.
struct FullHealth<'a>(&'a Resolved);

impl bpe_core::HealthLevels for FullHealth<'_> {
    fn share(&self, thing: ThingId) -> f32 {
        let has = self.0.things.get(&thing).is_some_and(|r| r.slots.contains_key(&bpe_core::SlotId::Health));
        if has { 1.0 } else { 0.0 }
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
    fn a_moment_between_steps_is_drawn_and_undone_without_touching_the_physics() {
        let (scene, resolved, sims) = dots_pair(120.0, Target::Anything);
        let mut world = World::build(&scene, &resolved, &sims);
        let mut twin = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        let mut twin_log = Log::default();
        world.advance_to(0.1, &sims, &mut log);
        twin.advance_to(0.1, &sims, &mut twin_log);
        let before = world.bodies[&DOTS].position.clone();
        let previous = world.bodies[&DOTS].previous.clone();
        let between = world.set_between(0.25);
        for i in 0..before.len() {
            let want = previous[i] + (before[i] - previous[i]) * 0.25;
            assert!((world.bodies[&DOTS].position[i] - want).length() < 1e-4);
        }
        assert!(before.iter().zip(&previous).any(|(a, b)| a != b), "the balls moved in the step");
        world.restore_between(between);
        assert_eq!(world.bodies[&DOTS].position, before, "put back bit for bit");
        world.advance_to(0.5, &sims, &mut log);
        twin.advance_to(0.5, &sims, &mut twin_log);
        for (id, b) in &world.bodies {
            assert_eq!(b.position, twin.bodies[id].position);
            assert_eq!(b.velocity, twin.bodies[id].velocity);
        }
    }

    #[test]
    fn a_lone_copy_steps_only_what_it_keeps_as_the_world_does() {
        let (scene, resolved, sims) = dots_pair(120.0, Target::Anything);
        let mut world = World::build(&scene, &resolved, &sims);
        world.keep_history(DOTS, crate::HistoryWant { seconds: 2.0, forever: false });
        let mut log = Log::default();
        world.advance_to(0.1, &sims, &mut log);
        let mut lone = world.lone(&|id| id != DOTS2);
        assert!(lone.bodies.contains_key(&DOTS) && !lone.bodies.contains_key(&DOTS2));
        assert!(lone.walls.contains_key(&WALL) && !lone.states.contains_key(&DOTS2));
        assert!(lone.history.is_empty() && !lone.order().contains(&DOTS2));
        assert_eq!(lone.kind(DOTS2), Some("dots"), "still named");
        // Before the two meet, the kept ball goes exactly where it goes in the whole world.
        let mut lone_log = Log::default();
        for _ in 0..60 {
            world.step(&sims, &mut log);
            lone.step(&sims, &mut lone_log);
            assert_eq!(world.bodies[&DOTS].position, lone.bodies[&DOTS].position);
        }
        assert!(world.fingerprint.is_none(), "nothing asked for it");
    }

    #[test]
    fn lent_cards_move_a_ball_as_their_owner_does() {
        let (scene, resolved, sims) = dots_pair(120.0, Target::Anything);
        let world = World::build(&scene, &resolved, &sims);
        let mut lone = world.lone(&|id| id != DOTS2);
        lone.modules.remove(&DOTS);
        assert!(lone.card(DOTS, SlotId::Motion).is_none());
        lone.lend_cards(&world, DOTS2, DOTS);
        assert_eq!(lone.card(DOTS, SlotId::Motion).map(|c| c.kind), Some("motion.gravity"));
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

    /// A light probe that counts how often it is made.
    struct CountingLight(std::sync::atomic::AtomicUsize);

    impl crate::LightAt for CountingLight {
        fn level(&self, _: &World, _: &Log, _: ThingId, _: &bpe_core::LightSource, _: f64) -> f32 {
            1.0
        }
        fn ball(&self, _: &World, _: &Log, _: ThingId, _: usize, _: f64) -> crate::BallLight {
            crate::BallLight::default()
        }
        fn pull(&self, _: &World, _: &Log, _: ThingId, _: usize, _: f64, _: &crate::LightPull) -> Vec2 {
            Vec2::ZERO
        }
    }

    struct CountingMaker(std::sync::Arc<CountingLight>);

    impl crate::LightMaker for CountingMaker {
        fn make(&self, _: &Scene, _: &Resolved) -> std::sync::Arc<dyn crate::LightAt> {
            self.0.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.0.clone()
        }
        fn fingerprint(&self, _: &Scene) -> u64 {
            7
        }
    }

    #[test]
    fn a_world_makes_no_light_probe_unless_something_physical_follows_light() {
        let (scene, resolved, mut sims) = dots_in_ring(3);
        let counter = std::sync::Arc::new(CountingLight(std::sync::atomic::AtomicUsize::new(0)));
        let before = fingerprint(&scene, &resolved, &sims);
        sims.light = Some(std::sync::Arc::new(CountingMaker(counter.clone())));
        let mut world = World::build(&scene, &resolved, &sims);
        world.advance_to(0.5, &sims, &mut Log::default());
        assert!(world.light().is_none());
        assert_eq!(counter.0.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(fingerprint(&scene, &resolved, &sims), before, "light sensing off: the run is as it was");
        assert!(!needs_light(&scene, &resolved, &sims, None));
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

    /// A key at 0 s holds from the start: the world is built with it, not with the setting
    /// under it.
    #[test]
    fn a_key_at_the_start_is_there_before_the_first_step() {
        let (mut scene, _, sims) = dots_in_ring(1);
        let reg = reg();
        let gravity = root_setting("gravity");
        scene.set_key(&reg, &gravity, 0.0, Value::Vec2([0.0, 100.0])).unwrap();
        scene.set_key(&reg, &gravity, 1.0, Value::Vec2([0.0, 100.0])).unwrap();
        let resolved = resolve(&scene, &reg);
        let w = World::build(&scene, &resolved, &sims);
        assert_eq!(w.gravity, Vec2::new(0.0, 100.0));
        assert_eq!(w.limits().resting, 100.0 * w.dt() * 4.0 + 1.0);
    }

    /// A frame's time, `index / fps`, lands on its own step even when the product rounds just
    /// under it (2.05 s × 240 is 491.99999999999994), so the export and the editor agree.
    #[test]
    fn a_frame_time_lands_on_its_own_step() {
        let (scene, resolved, sims) = dots_in_ring(1);
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(123.0 / 60.0, &sims, &mut log);
        assert_eq!(w.step, 492);
        for index in 0..2000u64 {
            let time = index as f64 / 60.0;
            assert_eq!(crate::step_at(time, 240), index * 4, "frame {index}");
            let time = index as f64 / 30.0;
            assert_eq!(crate::step_at(time, 240), index * 8, "frame {index} at 30 fps");
        }
        let mut playback = crate::Playback::new(&scene, &resolved, std::sync::Arc::new(sims));
        playback.seek(2.05);
        assert_eq!(playback.world().step, 492);
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

    /// A held ball stays where it is through gravity, the wall it sits on and a ball heading
    /// into it, and logs nothing; let go, it moves again.
    #[test]
    fn a_held_ball_stays_put_through_gravity_walls_and_pairs() {
        let (scene, resolved, sims) = dots_pair_popping(100);
        let mut w = World::build(&scene, &resolved, &sims);
        let held = w.bodies[&DOTS2].position[0];
        w.bodies.get_mut(&DOTS2).unwrap().held[0] = true;
        let mut log = Log::default();
        w.advance_to(3.0, &sims, &mut log);
        assert_eq!(w.bodies[&DOTS2].position[0], held, "never moved");
        assert!(log.events().iter().all(|e| e.a.thing != DOTS2 && e.b.is_none_or(|b| b.thing != DOTS2)), "{log:?}");
        assert!(log.events().iter().any(|e| e.a.thing == DOTS), "the other ball still bounces");
        w.bodies.get_mut(&DOTS2).unwrap().held[0] = false;
        w.advance_to(3.5, &sims, &mut log);
        assert_ne!(w.bodies[&DOTS2].position[0], held, "let go, it falls");
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
        // A longer trail keeps a longer history, and that is a rebuild.
        fn trail(scene: &mut Scene) -> &mut bpe_core::Slot {
            scene.things.get_mut(&DOTS).unwrap().slots.get_mut(&SlotId::Trail).unwrap()
        }
        trail(&mut scene).params.set("seconds", Value::Number(5.0));
        let long = resolve(&scene, &reg());
        assert_ne!(fingerprint(&scene, &long, &sims), with_trail);
        assert_eq!(World::build(&scene, &long, &sims).history[&DOTS].cap(), 5 * 60 + 1);
        trail(&mut scene).params.set("seconds", Value::Number(0.5));
        let resolved = resolve(&scene, &reg());

        let mut world = World::build(&scene, &resolved, &sims);
        assert!(world.history.contains_key(&DOTS));
        assert!(world.history[&DOTS].is_empty());
        let mut log = Log::default();
        world.advance_to(1.0, &sims, &mut log);
        let history = &world.history[&DOTS];
        assert_eq!(history.len(), 60);
        assert_eq!(history.points(1).next(), Some(world.bodies[&DOTS].position[1]));
        assert!(history.points(0).nth(30).unwrap() != world.bodies[&DOTS].position[0]);
        assert!(world.memory() >= plain.memory() + 2 * 60 * 8);
        // The same scene without the history steps the same.
        let mut bare = World::build(&scene, &resolved, &sims);
        bare.history.clear();
        let mut bare_log = Log::default();
        bare.advance_to(1.0, &sims, &mut bare_log);
        assert_eq!(bare.bodies, world.bodies);
        assert_eq!(bare_log.events(), log.events());
    }

    /// A fast ball's history runs through every bounce: a trail drawn through it turns where the
    /// ball really turned instead of cutting the corner between two samples.
    #[test]
    fn a_history_runs_through_every_bounce() {
        let (mut scene, _, sims) = dots_in_ring(1);
        scene.things.get_mut(&DOTS).unwrap().params.set("velocity", Value::Vec2([2600.0, -1700.0]));
        crate::testing::give(&mut scene, DOTS, SlotId::Trail, "trail.test", &[("seconds", Value::Number(2.0))]);
        let resolved = resolve(&scene, &reg());
        let mut world = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        world.advance_to(1.5, &sims, &mut log);
        let history = &world.history[&DOTS];
        let radius = world.bodies[&DOTS].radius[0];
        let trace: Vec<(f64, Vec2)> = history.trace(0).collect();
        assert!(trace.windows(2).all(|w| w[0].0 > w[1].0), "newest first, in order");
        let oldest = trace.last().unwrap().0;
        let hits: Vec<&Event> = log
            .events()
            .iter()
            .filter(|e| e.kind == EventKind::Hit && e.a.thing == DOTS && e.step as f64 > oldest + 1.0)
            .collect();
        assert!(hits.len() >= 5, "{} bounces", hits.len());
        for e in hits {
            // Where the ball's centre was as it touched.
            let centre = e.point + e.normal * radius;
            let nearest = trace.iter().map(|(_, p)| (*p - centre).length()).fold(f32::INFINITY, f32::min);
            assert!(nearest < 1.0, "the bounce on step {} is {nearest} px from the history", e.step);
        }
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

    #[test]
    fn lent_states_are_there_for_drawing_and_gone_after() {
        let (scene, resolved, sims) = dots_in_ring(3);
        let mut w = World::build(&scene, &resolved, &sims);
        let before: Vec<ThingId> = w.states.keys().copied().collect();
        let lent = w.lend_states(vec![(ThingId(77), Box::new(5u32) as Box<dyn ThingState>)]);
        assert_eq!(*crate::state::<u32>(w.states[&ThingId(77)].as_ref()), 5);
        w.take_back(lent);
        assert_eq!(w.states.keys().copied().collect::<Vec<_>>(), before);
        assert!(w.beside.is_empty(), "every test kind steps with the physics");
        assert!(matches!(main_part(&scene, &resolved, &sims), std::borrow::Cow::Borrowed(_)));
    }
}
