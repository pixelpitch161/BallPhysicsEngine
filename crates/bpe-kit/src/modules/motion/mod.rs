//! Basic motion: gravity, air drag and wind.

pub mod graph;
pub mod sim;

use bpe_core::{ModuleKind, SettingDef, Settings, SlotId};
use glam::Vec2;

pub struct BasicMotionKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BasicMotion {
    /// How much of the scene's gravity pulls on these. Negative floats them.
    #[setting(label = "Gravity", range(-5, 5), simple, keyframe, random, pulse)]
    pub gravity_scale: f32,
    /// How quickly the air slows them.
    #[setting(label = "Air drag", range(0, 5), unit = "per s", simple, keyframe, random(0, 1.5))]
    pub drag: f32,
    /// A steady push on these, in pixels per second squared.
    #[setting(label = "Wind", unit = "px/s²", keyframe, random, pulse)]
    pub wind: Vec2,
}

impl Default for BasicMotion {
    fn default() -> Self {
        Self { gravity_scale: 1.0, drag: 0.0, wind: Vec2::ZERO }
    }
}

impl ModuleKind for BasicMotionKind {
    fn id(&self) -> &'static str {
        "motion.basic"
    }
    fn name(&self) -> &'static str {
        "Basic"
    }
    fn help(&self) -> &'static str {
        "Falls under gravity, is slowed by the air and pushed by the wind."
    }
    fn slot(&self) -> SlotId {
        SlotId::Motion
    }
    fn settings(&self) -> &'static [SettingDef] {
        BasicMotion::defs()
    }
    fn template(&self) -> Option<bpe_graph::Graph> {
        Some(graph::template())
    }
}
