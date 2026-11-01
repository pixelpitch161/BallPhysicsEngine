//! Flowing: how a liquid moves. Gravity pulls it; viscosity, stiffness and the ball push shape
//! how its particles settle against each other.

pub mod sim;

use bpe_core::{ModuleKind, SettingDef, Settings, SlotId};

pub struct FlowingKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct Flowing {
    /// How much of the scene's gravity pulls on the liquid. Negative floats it.
    #[setting(label = "Gravity", range(-5, 5), simple, keyframe, random, pulse)]
    pub gravity_scale: f32,
    /// How thick and slow it flows.
    #[setting(label = "Viscosity", range(0, 1), simple, keyframe, random)]
    pub viscosity: f32,
    /// How hard it resists being squashed.
    #[setting(label = "Stiffness", range(0, 1), simple, keyframe, random)]
    pub stiffness: f32,
    /// How strongly balls shove the liquid aside.
    #[setting(label = "Ball push", range(0, 3), simple, keyframe, random)]
    pub ball_push: f32,
}

impl Default for Flowing {
    fn default() -> Self {
        Self { gravity_scale: 1.0, viscosity: 0.2, stiffness: 0.5, ball_push: 1.0 }
    }
}

impl ModuleKind for FlowingKind {
    fn id(&self) -> &'static str {
        "motion.liquid"
    }
    fn name(&self) -> &'static str {
        "Flowing"
    }
    fn help(&self) -> &'static str {
        "Falls under gravity and flows like a liquid."
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "liquid"
    }
    fn slot(&self) -> SlotId {
        SlotId::Motion
    }
    fn settings(&self) -> &'static [SettingDef] {
        Flowing::defs()
    }
}
