//! The rules' When block **Liquid** (`when.liquid`, liquid part 2): fires when a ball goes into a
//! Liquid thing's liquid, comes out of it, or splashes into it (its Liquid events). A Liquid thing a
//! rule reads steps with the physics ([`BlockSim::keeps`]), so its events are in every world's log.

use std::collections::BTreeSet;

use bpe_core::{BlockSection, Choice, Params, Settings, ThingId, Who};
use bpe_runtime::rules::WHOLE;
use bpe_runtime::{Action, BlockSim, EventKind, Firing, Party, PrepareCx, StageCx, ThingState, Values, state};

use super::KIND;
use super::feel::{ENTER, LEAVE, SPLASH};
use crate::rules::block_kind;
use crate::rules::when::ball_values;
use crate::rules::who::{matches, targets};

/// What the block watches for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LiquidWay {
    /// A ball goes in.
    #[default]
    GoesIn,
    /// A ball comes out.
    ComesOut,
    /// A ball lands in it fast (at least the liquid's Splashes from), as it first touches it.
    Splashes,
    /// A ball goes in or comes out.
    GoesInOrOut,
}

impl LiquidWay {
    /// Whether it fires on an event whose item is `code`.
    fn takes(self, code: u32) -> bool {
        match self {
            LiquidWay::GoesIn => code == ENTER,
            LiquidWay::ComesOut => code == LEAVE,
            LiquidWay::Splashes => code == SPLASH,
            LiquidWay::GoesInOrOut => code == ENTER || code == LEAVE,
        }
    }
}

/// When a ball goes into a liquid, comes out of it or splashes.
pub struct LiquidWhen;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LiquidWhenSettings {
    /// The balls watched.
    #[setting(label = "Who", simple, group = "Rule")]
    pub who: Who,
    /// The Liquid things they go into.
    #[setting(label = "Liquid", simple, group = "Rule")]
    pub liquid: Who,
    #[setting(label = "When it", simple, group = "Rule")]
    pub way: LiquidWay,
}

impl Default for LiquidWhenSettings {
    fn default() -> Self {
        LiquidWhenSettings { who: Who::Anything, liquid: Who::Anything, way: LiquidWay::GoesIn }
    }
}

block_kind!(
    LiquidWhen,
    "when.liquid",
    "Liquid",
    When,
    LiquidWhenSettings,
    "Fires when a ball goes into a liquid, comes out of it, or splashes into it.",
    values = ["speed", "size", "hits", "ball", "way"],
    from = []
);

impl BlockSim for LiquidWhen {
    fn kind(&self) -> &'static str {
        "when.liquid"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(LiquidWhenSettings::from_params(params)))
    }

    /// Every Liquid thing steps with the physics while a rule reads liquid events: the scene's
    /// liquids share one card host, so they must all be in the same world (keeping only the named
    /// one would split them between the main world and the one beside it).
    fn keeps(&self, _params: &Params, _thing: ThingId, kind: &str, _tags: &BTreeSet<String>) -> bool {
        kind == KIND
    }

    fn fire(&self, st: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        let s = state::<LiquidWhenSettings>(st);
        for ev in cx.events.iter().filter(|ev| ev.kind == EventKind::Liquid) {
            let Some(liquid) = ev.b else { continue };
            if !s.way.takes(liquid.item)
                || cx.is_broken(liquid.thing)
                || !matches(cx, &s.liquid, Party { thing: liquid.thing, item: WHOLE })
                || !matches(cx, &s.who, ev.a)
            {
                continue;
            }
            let mut values = Values::default();
            ball_values(cx.world, ev.a, &mut values);
            values.set("way", if liquid.item == LEAVE { 0.0 } else { 1.0 });
            values.set("speed", ev.speed as f64);
            out.push(Firing {
                step: cx.step,
                subject: Some(ev.a),
                other: Some(Party { thing: liquid.thing, item: WHOLE }),
                point: ev.point,
                normal: ev.normal,
                values,
            });
        }
    }
}

/// Do: **Un-mix** (liquid part 4): the liquid plays back what it remembers (the first Liquid
/// thing's Remember for Un-mix and Seconds), colours un-mixing. All Liquid things are one body of
/// liquid, so naming one plays back all of them.
pub struct LiquidUnmix;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LiquidUnmixSettings {
    /// The Liquid things to play back (they share one liquid: naming any plays it all back).
    #[setting(label = "Liquid", simple, group = "Rule")]
    pub liquid: Who,
}

impl Default for LiquidUnmixSettings {
    fn default() -> Self {
        LiquidUnmixSettings { liquid: Who::Anything }
    }
}

block_kind!(
    LiquidUnmix,
    "do.unmix",
    "Un-mix",
    Do,
    LiquidUnmixSettings,
    "Plays a liquid's last seconds backwards, colours un-mixing (its Liquid card's Remember for Un-mix must be on).",
    values = [],
    from = []
);

impl BlockSim for LiquidUnmix {
    fn kind(&self) -> &'static str {
        "do.unmix"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(LiquidUnmixSettings::from_params(params)))
    }

    /// A rule that plays the liquid back keeps every Liquid thing with the physics (they share one
    /// body, which plays back in the world the rule runs in).
    fn keeps(&self, _params: &Params, _thing: ThingId, kind: &str, _tags: &BTreeSet<String>) -> bool {
        kind == KIND
    }

    fn act(&self, st: &mut dyn ThingState, firing: &Firing, cx: &StageCx, out: &mut Vec<Action>) {
        let s = state::<LiquidUnmixSettings>(st);
        let mut named: Vec<ThingId> = targets(cx, &s.liquid, firing)
            .into_iter()
            .map(|(thing, _)| thing)
            .filter(|t| cx.world.kind_of(*t) == Some(KIND))
            .collect();
        named.dedup();
        // One is enough: the liquid is one body.
        if let Some(&thing) = named.first() {
            out.push(Action::Fire { thing });
        }
    }
}
