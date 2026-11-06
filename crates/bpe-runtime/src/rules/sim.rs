//! What a rule block plugin gives the runtime: how it reads its settings and what it does in the
//! Rules stage.

use bpe_core::{BlockId, BlockSection, Params, Resolved, RuleId, Scene};

use super::{Action, Firing};
use crate::{Event, Sims, ThingState, World};

/// What a block knows while it reads its settings, as the world is built.
pub struct PrepareCx<'a> {
    pub scene: &'a Scene,
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
    pub block: BlockId,
}

impl StageCx<'_> {
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
    /// A Logic block, once a stage before any When fires: firings whose wait has run out.
    fn tick(&self, _state: &mut dyn ThingState, _inputs: usize, _cx: &StageCx, _out: &mut Vec<Firing>) {}
    /// A Do block: the actions `firing` makes, applied at the next step's start.
    fn act(&self, _state: &mut dyn ThingState, _firing: &Firing, _cx: &StageCx, _out: &mut Vec<Action>) {}
    /// Whether a prepared block of this kind can change settings (Change setting): the world then
    /// keeps a copy of the settings to change.
    fn changes_settings(&self, _state: &dyn ThingState) -> bool {
        false
    }
}
