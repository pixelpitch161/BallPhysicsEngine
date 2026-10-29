//! Basic motion: gravity and air drag.

pub mod sim;

use bpe_core::{ModuleKind, SettingDef, Settings, SlotId};

pub struct BasicMotionKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BasicMotion {
    /// How much of the scene's gravity pulls on these. Negative floats them.
    #[setting(label = "Gravity", range(-5, 5), simple, keyframe, random, pulse)]
    pub gravity_scale: f32,
    /// How quickly the air slows them.
    #[setting(label = "Air drag", range(0, 5), unit = "per s", simple, keyframe, random)]
    pub drag: f32,
}

impl Default for BasicMotion {
    fn default() -> Self {
        Self { gravity_scale: 1.0, drag: 0.0 }
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
        "Falls under gravity and is slowed by the air."
    }
    fn slot(&self) -> SlotId {
        SlotId::Motion
    }
    fn settings(&self) -> &'static [SettingDef] {
        BasicMotion::defs()
    }
}
