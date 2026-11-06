//! Do: Flash (rules part 6). Fires who's Flash on hit and Colour on hit layers as if hit, without
//! a hit: the physics, the sound and everything else that reacts to hits don't notice.

use bpe_core::{BlockSection, Params, Settings, ThingId, Who};
use bpe_runtime::{Action, BlockSim, Firing, PrepareCx, StageCx, ThingState, state};

use super::amount::Amount;
use super::block_kind;
use super::who::targets;

pub struct Flash;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FlashSettings {
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    /// 1 flashes as a full-strength hit does.
    #[setting(label = "Strength", range(0, 1.5), simple, group = "Rule")]
    pub strength: f32,
}

impl Default for FlashSettings {
    fn default() -> Self {
        FlashSettings { who: Who::That, strength: 1.0 }
    }
}

block_kind!(
    Flash,
    "do.flash",
    "Flash",
    Do,
    FlashSettings,
    "Fires the Flash on hit and Colour on hit layers of a ball or a thing as if it was hit."
);

#[derive(Clone, Debug)]
struct FlashState {
    who: Who,
    strength: Amount,
}

impl BlockSim for Flash {
    fn kind(&self) -> &'static str {
        "do.flash"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = FlashSettings::from_params(params);
        let strength = Amount::read(params, "strength", s.strength as f64);
        Ok(Box::new(FlashState { who: s.who, strength }))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let f = state::<FlashState>(st);
        let strength = (f.strength.or_fixed(firing) as f32).clamp(0.0, 1.5);
        for (thing, row) in targets(cx, &f.who, firing) {
            if thing != ThingId::ROOT {
                // A whole thing with balls flashes each of them as it applies.
                out.push(Action::Flash { thing, row, strength });
            }
        }
    }
}
