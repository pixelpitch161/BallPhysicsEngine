//! Basic motion: gravity, air drag and wind.

pub mod sim;

use bpe_core::{ModuleKind, SettingDef, Settings, SlotId};
use glam::Vec2;

pub struct BasicMotionKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BasicMotion {
    /// How strongly the scene's gravity pulls these, as a multiple.
    #[setting(label = "Gravity ×", range(-5, 5), group = "Basics", simple, keyframe, pulse)]
    pub gravity_scale: f32,
    /// How quickly the air slows them.
    #[setting(label = "Air drag", range(0, 5), unit = "per s", group = "Basics", simple, keyframe)]
    pub drag: f32,
    /// A steady push on these, in pixels per second squared.
    #[setting(label = "Wind", group = "Basics", unit = "px/s²", keyframe, pulse)]
    pub wind: Vec2,
}

impl Default for BasicMotion {
    fn default() -> Self {
        Self { gravity_scale: 0.0, drag: 0.0, wind: Vec2::ZERO }
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
}
