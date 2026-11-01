//! The liquid surface look: a coloured, lit, see-through body with a smoothed edge.

use bpe_core::{Blend, Color, ModuleKind, SettingDef, Settings, SlotId};

use crate::paint::Paint;

pub struct LiquidLookKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LiquidLook {
    /// The liquid's colours: by place (how deep or high), by how fast it flows there, or over
    /// time or the beat.
    #[setting(paint, label = "Colours", prefix = "", follows = "place|speed|time|beat", simple, no_vary)]
    pub colour: Paint,
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
    /// How it mixes with what is under it: Normal paints over it, the others mix with it.
    /// Everything with the same Blend and amount is mixed in once, as one picture, so where its
    /// parts overlap they do not stack. Mixed any other way than Normal at full amount, the
    /// liquid does not bend what is under it: the clear part shows it as it is.
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    /// How much of the blend shows: 0 is as if it weren't there, 1 is the whole blend.
    #[setting(label = "Blend amount", range(0, 1), group = "Blending", keyframe)]
    pub blend_amount: f32,
}

impl Default for LiquidLook {
    fn default() -> Self {
        Self {
            colour: Paint::solid(Color::rgb(0x2e, 0x8c, 0xff)),
            clarity: 0.45,
            refraction: 0.5,
            highlight: 0.8,
            smoothing: 6.0,
            glow: 0.1,
            blend: Blend::Normal,
            blend_amount: 1.0,
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
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "liquid"
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        LiquidLook::defs()
    }
}
