//! The rules' When block **Particles** (`when.particles`, particles part 7): fires when a Particles
//! thing's particles did something this step, at least so many times: hit a ball, hit a shape,
//! landed, were born in a burst, or a burst went off. A Particles thing a rule reads steps with
//! the physics ([`BlockSim::keeps`]), so the rule sees it in every world.

use std::collections::BTreeSet;

use bpe_core::{BlockSection, Choice, Params, Settings, ThingId, Who};
use bpe_runtime::rules::WHOLE;
use bpe_runtime::{BlockSim, Firing, Party, PrepareCx, StageCx, ThingState, Values, state};

use super::KIND;
use super::moments::{MOMENT_KINDS, Moment, MomentKind};
use super::sim::state_of;
use crate::rules::block_kind;
use crate::rules::who::matches;

/// Which moments the block counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MomentPick {
    /// Every kind of moment.
    #[default]
    Any,
    /// Particles hitting a ball (bouncing off it or vanishing at it).
    HitABall,
    /// Particles hitting a shape (bouncing off it or vanishing at it).
    HitAShape,
    /// Particles landing on a shape (stopping on it).
    Land,
    /// Particles born in a burst.
    Born,
    /// A burst going off.
    Burst,
}

impl MomentPick {
    /// Whether it counts moments of `kind`.
    pub fn takes(self, kind: MomentKind) -> bool {
        match self {
            MomentPick::Any => true,
            MomentPick::HitABall => kind == MomentKind::HitBall,
            MomentPick::HitAShape => kind == MomentKind::HitShape,
            MomentPick::Land => kind == MomentKind::Land,
            MomentPick::Born => kind == MomentKind::Born,
            MomentPick::Burst => kind == MomentKind::Burst,
        }
    }

    /// How many of `counts` (by [`MomentKind`] place) it counts.
    pub fn count(self, counts: &[u32; MOMENT_KINDS]) -> u64 {
        MomentKind::ALL.iter().filter(|k| self.takes(**k)).map(|k| counts[*k as usize] as u64).sum()
    }
}

/// When a Particles thing's particles did something.
pub struct ParticlesWhen;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ParticlesWhenSettings {
    /// The Particles things watched.
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    /// What their particles did.
    #[setting(label = "Moment", simple, group = "Rule")]
    pub moment: MomentPick,
    /// How many times in a step, at least.
    #[setting(label = "At least", range(1, 1000000), simple, group = "Rule")]
    pub at_least: i32,
}

impl Default for ParticlesWhenSettings {
    fn default() -> Self {
        ParticlesWhenSettings { who: Who::Anything, moment: MomentPick::Any, at_least: 1 }
    }
}

block_kind!(
    ParticlesWhen,
    "when.particles",
    "Particles",
    When,
    ParticlesWhenSettings,
    "Fires when a Particles thing's particles hit a ball or a shape, land, or burst, at least so many times in a step.",
    values = ["count", "speed", "size"],
    from = []
);

impl BlockSim for ParticlesWhen {
    fn kind(&self) -> &'static str {
        "when.particles"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(ParticlesWhenSettings::from_params(params)))
    }

    /// A Particles thing its Who names steps with the physics: one by id, or every one for any
    /// other Who (a rule may tag one at any time).
    fn keeps(&self, params: &Params, thing: ThingId, kind: &str, _tags: &BTreeSet<String>) -> bool {
        if kind != KIND {
            return false;
        }
        match ParticlesWhenSettings::from_params(params).who {
            Who::Thing { thing: named, .. } => named == thing,
            _ => true,
        }
    }

    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        let s = state::<ParticlesWhenSettings>(st);
        let least = s.at_least.max(1) as u64;
        for &thing in cx.world.states.keys() {
            let Some(p) = state_of(cx.world, thing) else { continue };
            if cx.is_broken(thing) || !matches(cx, &s.who, Party { thing, item: WHOLE }) {
                continue;
            }
            let (moments, counts) = p.moments_at(cx.step);
            let count = s.moment.count(&counts);
            if count < least {
                continue;
            }
            let kept: Vec<&Moment> = moments.iter().filter(|m| s.moment.takes(m.kind)).collect();
            // The strongest kept moment is where it fired.
            let strongest = kept.iter().copied().max_by(|a, b| a.speed.total_cmp(&b.speed));
            let mut values = Values::default();
            values.set("count", count as f64);
            values.set("speed", strongest.map_or(0.0, |m| m.speed as f64));
            let sizes: Vec<f32> = kept.iter().filter(|m| m.kind != MomentKind::Burst).map(|m| m.size).collect();
            let size = if sizes.is_empty() { 0.0 } else { sizes.iter().sum::<f32>() / sizes.len() as f32 };
            values.set("size", size as f64);
            let place = strongest
                .map_or_else(|| cx.world.poses.get(&thing).map_or(glam::Vec2::ZERO, |p| p.position), |m| m.place);
            out.push(Firing {
                step: cx.step,
                subject: Some(Party { thing, item: WHOLE }),
                other: strongest.and_then(|m| m.with).map(|with| Party { thing: with, item: WHOLE }),
                point: place,
                normal: strongest.and_then(|m| m.velocity.try_normalize()).unwrap_or(glam::Vec2::ZERO),
                values,
            });
        }
    }
}
