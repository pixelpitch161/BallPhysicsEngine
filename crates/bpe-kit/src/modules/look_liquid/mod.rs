//! The liquid surface look: a coloured, lit, see-through body with a smoothed edge.

use bpe_core::{Color, ModuleKind, SettingDef, Settings, SlotId};

pub struct LiquidLookKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LiquidLook {
    /// The liquid's colour.
    #[setting(label = "Colour", simple, keyframe, random)]
    pub color: Color,
    /// How see-through it is.
    #[setting(label = "Clarity", range(0, 1), simple, keyframe, random)]
    pub clarity: f32,
    /// How much it bends what is behind it.
    #[setting(label = "Refraction", range(0, 1), simple, keyframe, random)]
    pub refraction: f32,
    /// How bright the light on its surface is.
    #[setting(label = "Highlight", range(0, 2), simple, keyframe, random)]
    pub highlight: f32,
    /// How far the surface is smoothed, hiding the particles.
    #[setting(label = "Smoothing", range(0, 20), unit = "px", keyframe)]
    pub smoothing: f32,
    /// Light the liquid gives off.
    #[setting(label = "Glow", range(0, 2), keyframe, pulse)]
    pub glow: f32,
}

impl Default for LiquidLook {
    fn default() -> Self {
        Self {
            color: Color::rgb(0x2e, 0x8c, 0xff),
            clarity: 0.45,
            refraction: 0.5,
            highlight: 0.8,
            smoothing: 6.0,
            glow: 0.1,
        }
    }
}

impl ModuleKind for LiquidLookKind {
    fn id(&self) -> &'static str {
        "look.liquid"
    }
    fn name(&self) -> &'static str {
        "Liquid surface"
    }
    fn help(&self) -> &'static str {
        "A smooth, lit, see-through surface over the particles."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        LiquidLook::defs()
    }
}
