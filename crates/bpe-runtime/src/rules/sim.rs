//! What a rule block plugin gives the runtime: how it reads its settings and what it does in the
//! Rules stage.

use std::collections::{BTreeMap, BTreeSet};

use bpe_core::{BlockId, BlockSection, Params, Resolved, RuleId, Scene, ThingId};

use super::{Action, Firing, Signal};
use crate::{Event, Log, Sims, ThingState, World};

/// What a block knows while it reads its settings, as the world is built.
pub struct PrepareCx<'a> {
    pub scene: &'a Scene,
    /// The world, built and before any step.
    pub world: &'a World,
    pub resolved: &'a Resolved,
    pub sims: &'a Sims,
    pub rule: RuleId,
    pub block: BlockId,
}

/// What a block knows in the Rules stage: the world as it is after this step, and this step's
/// new events.
pub struct StageCx<'a> {
    pub world: &'a World,
    pub sims: &'a Sims,
    /// The events logged in this step, in log order.
    pub events: &'a [Event],
    pub step: u64,
    /// Seconds since the start, at the end of this step.
    pub time: f64,
    pub rule: RuleId,
    /// Every thing's health and this step's changes of it (the world's own answer reads nothing
    /// while the stage runs).
    pub health: &'a super::Health,
    pub block: BlockId,
    /// The things a rule broke (the world's own answer, [`World::broken`], reads nothing while
    /// the stage runs).
    pub broken: &'a BTreeSet<ThingId>,
    /// Every event so far (this step's last), for what reads the past: the sound and the light.
    pub log: &'a Log,
    /// The counters as they are this step (a Count block's change applies at the next step's
    /// start).
    pub counters: &'a BTreeMap<String, f64>,
    /// The tags rules gave single balls, by thing and body row.
    pub ball_tags: &'a BTreeMap<(ThingId, u32), BTreeSet<String>>,
}

impl StageCx<'_> {
    /// Whether a rule broke `thing`.
    pub fn is_broken(&self, thing: ThingId) -> bool {
        self.broken.contains(&thing)
    }

    /// A counter's value now (0 for one never set).
    pub fn counter(&self, name: &str) -> f64 {
        self.counters.get(name).copied().unwrap_or(0.0)
    }

    /// Whether body row `row` of `thing` has `tag` of its own (a rule tagged that ball).
    pub fn ball_has_tag(&self, thing: ThingId, row: u32, tag: &str) -> bool {
        self.ball_tags.get(&(thing, row)).is_some_and(|t| t.contains(tag))
    }

    /// A number from 0 to 1 for this block, this step and `subject`, the same on every run and
    /// never shared with another block or rule (see [`super::dice`]).
    pub fn dice(&self, subject: Option<crate::Party>, salt: u32) -> f32 {
        super::dice(self.world.seed, self.rule, self.block, self.step, subject, salt)
    }
}

/// How a block kind acts in the simulation. A block's state is one value holding its settings
/// and its memory, cloned with the world.
pub trait BlockSim: Send + Sync + 'static {
    /// The block kind's id, as registered in `bpe_core`.
    fn kind(&self) -> &'static str;
    fn section(&self) -> BlockSection;
    /// Reads the settings once; `Err` names a problem, and the rule is skipped.
    fn prepare(&self, params: &Params, cx: &PrepareCx) -> Result<Box<dyn ThingState>, String>;
    /// A When block: the firings this step makes.
    fn fire(&self, _state: &mut dyn ThingState, _cx: &StageCx, _out: &mut Vec<Firing>) {}
    /// An If block: whether `firing` goes on.
    fn pass(&self, _state: &mut dyn ThingState, _firing: &Firing, _cx: &StageCx) -> bool {
        true
    }
    /// A Logic block: `firing` came in on input `input` of `inputs`; the firings that go on.
    fn join(
        &self,
        _state: &mut dyn ThingState,
        _input: usize,
        _inputs: usize,
        _firing: &Firing,
        _cx: &StageCx,
        _out: &mut Vec<Firing>,
    ) {
    }
    /// A When block: the signals sent in the last round of this stage (or carried from the last
    /// step) that it hears, as firings.
    fn hear(&self, _state: &mut dyn ThingState, _signals: &[Signal], _cx: &StageCx, _out: &mut Vec<Firing>) {}
    /// A Logic block, once a stage before any When fires: firings whose wait has run out.
    fn tick(&self, _state: &mut dyn ThingState, _inputs: usize, _cx: &StageCx, _out: &mut Vec<Firing>) {}
    /// A Do block: the actions `firing` makes, applied at the next step's start.
    fn act(&self, _state: &mut dyn ThingState, _firing: &Firing, _cx: &StageCx, _out: &mut Vec<Action>) {}
    /// Whether a prepared block of this kind can change settings (Change setting): the world then
    /// keeps a copy of the settings to change.
    fn changes_settings(&self, _state: &dyn ThingState) -> bool {
        false
    }
    /// The card (`None`: a thing's own settings) and setting key a block with these settings
    /// changes, when it changes one (Change setting).
    fn changes(&self, _params: &Params) -> Option<(Option<bpe_core::SlotId>, String)> {
        None
    }
    /// Whether a block with these settings reads the scene's sound: the physics fingerprint then
    /// hashes what the sound reads, so editing it runs the scene again.
    fn hears_sound(&self, _params: &Params) -> bool {
        false
    }
    /// Whether a block with these settings reads the light (the probe): likewise.
    fn sees_light(&self, _params: &Params) -> bool {
        false
    }
    /// Whether a block with these settings reads thing `thing` (of `kind`, with the scene's
    /// `tags`), whose sim would step it beside the physics: it then steps with the physics, so
    /// the rule sees it in every world (particles part 7: When Particles).
    fn keeps(&self, _params: &Params, _thing: ThingId, _kind: &str, _tags: &BTreeSet<String>) -> bool {
        false
    }
}
