//! Glowing dots: how a swarm is drawn, as tiny additive points of light.

use bpe_core::{Blend, Color, Gradient, ModuleKind, SettingDef, Settings, SlotId};

use crate::paint::{Follow, Paint};

pub struct SwarmLookKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SwarmLook {
    /// How big each point is.
    #[setting(label = "Size", range(0.5, 8), unit = "px", simple, keyframe, random)]
    pub size: f32,
    /// How much light each point gives.
    #[setting(label = "Brightness", range(0, 2), simple, keyframe, random, pulse)]
    pub brightness: f32,
    /// The points' colours: slow points take the first and points at the span's speed the last;
    /// or each takes the colour of the ball nearest to it, a place along the colours at random
    /// (Order), its place, or the time or the beat.
    #[setting(paint, label = "Colours", prefix = "", follows = "speed|nearest_ball|place|order|time|beat", simple)]
    pub colours: Paint,
    /// How it mixes with what is under it: Normal paints over it, the others mix with it.
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    /// How much of the blend shows: 0 is as if it weren't there, 1 is the whole blend.
    #[setting(label = "Blend amount", range(0, 1), group = "Blending", keyframe)]
    pub blend_amount: f32,
}

impl Default for SwarmLook {
    fn default() -> Self {
        Self {
            size: 1.5,
            brightness: 0.25,
            colours: Paint {
                colours: Gradient::even(&[
                    Color::rgb(0x20, 0x40, 0xff),
                    Color::rgb(0x2e, 0xd8, 0xe6),
                    Color::rgb(0xff, 0xf2, 0xc0),
                ]),
                follow: Follow::Speed,
                // The default flocking's top speed: the fastest points take the last colour.
                span: 260.0,
                ..Paint::default()
            },
            blend: Blend::Normal,
            blend_amount: 1.0,
        }
    }
}

impl ModuleKind for SwarmLookKind {
    fn id(&self) -> &'static str {
        "look.swarm"
    }
    fn name(&self) -> &'static str {
        "Glowing dots"
    }
    fn help(&self) -> &'static str {
        "Tiny points of light, coloured by speed, by the nearest ball, by place, or at random along the colours."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        SwarmLook::defs()
    }
}
