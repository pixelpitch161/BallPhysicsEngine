//! Basic bounce: how much speed a hit keeps, and what else a hit does to the ball.

pub mod graph;
pub mod sim;

use bpe_core::{ModuleKind, SettingDef, Settings, SlotId};

pub struct BasicBounceKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BasicBounce {
    /// How much of its speed into the wall the ball keeps. Above 1 it speeds up.
    #[setting(label = "Bounciness", range(0, 1.5), simple, keyframe, random, pulse)]
    pub bounciness: f32,
    /// The whole speed is multiplied by this on every hit.
    #[setting(label = "Speed gain", range(0.5, 1.5), simple, keyframe, random)]
    pub speed_gain: f32,
    /// The ball never goes faster than this.
    #[setting(label = "Top speed", range(50, 20000), unit = "px/s", simple, keyframe)]
    pub top_speed: f32,
    /// How much bigger the ball gets on every hit. Negative shrinks it.
    #[setting(label = "Grow per hit", range(-20, 20), unit = "px", simple, keyframe, random)]
    pub grow: f32,
    /// The ball never grows past this.
    #[setting(label = "Biggest size", range(0.5, 400), unit = "px")]
    pub max_size: f32,
    /// Decides every bounce against this thing, even when the other thing is the one moving.
    #[setting(label = "Takes over")]
    pub takes_over: bool,
}

impl Default for BasicBounce {
    fn default() -> Self {
        Self { bounciness: 1.0, speed_gain: 1.0, top_speed: 3000.0, grow: 0.0, max_size: 60.0, takes_over: false }
    }
}

impl ModuleKind for BasicBounceKind {
    fn id(&self) -> &'static str {
        "bounce.basic"
    }
    fn name(&self) -> &'static str {
        "Basic"
    }
    fn help(&self) -> &'static str {
        "How much speed a hit keeps, and how the ball grows with each one."
    }
    fn slot(&self) -> SlotId {
        SlotId::Bounce
    }
    fn settings(&self) -> &'static [SettingDef] {
        BasicBounce::defs()
    }
    fn template(&self) -> Option<bpe_graph::Graph> {
        Some(graph::template())
    }
}
