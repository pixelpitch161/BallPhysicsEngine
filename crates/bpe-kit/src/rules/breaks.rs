//! Do: Break.

use bpe_core::{BlockSection, Choice, Params, Settings, ThingId, Who};
use bpe_runtime::{Action, BlockSim, Firing, PrepareCx, StageCx, ThingState, state};

use super::block_kind;
use super::who::targets;

/// How a broken thing goes (part 3 adds Fade, Pop and Shatter).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BreakLook {
    /// Gone at once.
    #[default]
    Vanish,
}

pub struct Break;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BreakSettings {
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    #[setting(label = "Look", group = "Rule")]
    pub look: BreakLook,
}

impl Default for BreakSettings {
    fn default() -> Self {
        BreakSettings { who: Who::That, look: BreakLook::Vanish }
    }
}

block_kind!(
    Break,
    "do.break",
    "Break",
    Do,
    BreakSettings,
    "Breaks a ball or a whole thing: it is gone for the rest of the clip."
);

impl BlockSim for Break {
    fn kind(&self) -> &'static str {
        "do.break"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let s = BreakSettings::from_params(params);
        if s.who.names_thing() == Some(ThingId::ROOT) {
            return Err("The scene settings can't break".into());
        }
        Ok(Box::new(s))
    }
    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let s = state::<BreakSettings>(st);
        let mut each: Vec<(ThingId, Option<u32>)> = Vec::new();
        for (thing, row) in targets(cx, &s.who, firing) {
            match (&s.who, cx.world.bodies.get(&thing)) {
                // The group is its balls: each of them breaks, and the thing (an emitter, say)
                // carries on.
                (Who::Group, Some(b)) => {
                    each.extend((0..b.len() as u32).filter(|&r| b.alive[r as usize]).map(|r| (thing, Some(r))))
                }
                _ => each.push((thing, row)),
            }
        }
        for (thing, row) in each {
            if thing == ThingId::ROOT {
                continue;
            }
            out.push(Action::Break {
                thing,
                row,
                point: firing.point,
                normal: firing.normal,
                strength: firing.values.get("strength").unwrap_or(0.0) as f32,
            });
        }
    }
}
