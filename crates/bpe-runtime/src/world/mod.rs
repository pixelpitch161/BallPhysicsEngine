//! The world: every thing's simulation state, stepped at a fixed rate.
//!
//! Each step runs the spec's stages in order: Spawn, Scripted, Forces, Integrate, Collide,
//! Respond, Events. Things are built and scripted in tree order (parents first, so riders find
//! their parent's pose) and collide in `ThingId` order, so two runs of the same scene give the
//! same log.
//!
//! This file holds the [`World`] and what it tells the outside. Building it is `build.rs`, one
//! step `step.rs`, the keys, pulses and rules worked out before each step `animate.rs`, lone
//! copies and lending `lend.rs`, and fingerprints and the scene's parts `prints.rs`.

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

mod animate;
mod build;
mod lend;
mod prints;
mod step;
#[cfg(test)]
mod tests;

pub use animate::*;
pub use prints::*;
use step::*;

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
    /// Where the first beat is, in seconds (the scene's First beat at): beats count from there.
    pub first_beat: f64,
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
    pub(crate) modules: BTreeMap<ThingId, Prepared>,
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
    /// Whether some ball follows the scene's air ([`crate::air_felt`]): the air then wakes at the
    /// build and steps with the physics, at the start of every step.
    pub air_felt: bool,
    /// The air each ball that follows it is in this step and how hard it follows (px/s, per s), by
    /// thing and body row: worked out once a step after Spawn, used in every substep.
    pub(crate) air_at: BTreeMap<ThingId, Vec<(Vec2, f32)>>,
    /// The scene's physics fingerprint (the one without the history, [`sound_fingerprint`]'s),
    /// when some thing's sim asked for it ([`crate::ThingSim::needs_physics_print`]). Two worlds
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

const NO_TAGS: &BTreeSet<String> = &BTreeSet::new();

impl World {
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

    /// Bytes the world takes, for playback budgets. The balls' paths it shares with copies (jump
    /// points) count their share, so the copies together count them once.
    pub fn memory(&self) -> usize {
        self.memory_beside(None)
    }

    /// [`World::memory`] beside `earlier` (an earlier copy of this world, a jump point): the ball
    /// paths it still shares with it are counted there.
    pub fn memory_beside(&self, earlier: Option<&World>) -> usize {
        let history = self.history.iter().map(|(id, h)| h.memory_beside(earlier.and_then(|e| e.history.get(id))));
        self.bodies.values().map(Bodies::memory).sum::<usize>()
            + history.sum::<usize>()
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
}

/// Follow health: a thing's share of its starting health (rules part 4).
impl bpe_core::HealthLevels for World {
    fn share(&self, thing: ThingId) -> f32 {
        self.rules.as_ref().map_or(0.0, |r| r.health.thing_share(self, thing))
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
