//! Glowing dots: how a swarm is drawn, as tiny additive points of light.

use bpe_core::{Choice, Color, ModuleKind, SettingDef, Settings, SlotId};

pub struct SwarmLookKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SwarmColors {
    /// Slow points take the first colour, fast ones the last.
    #[default]
    Speed,
    /// Each point takes the colour of the ball nearest to it.
    NearestBall,
    /// The colours are spread across the points.
    Gradient,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SwarmLook {
    /// How big each point is.
    #[setting(label = "Size", range(0.5, 8), unit = "px", simple, keyframe, random)]
    pub size: f32,
    /// How much light each point gives.
    #[setting(label = "Brightness", range(0, 2), simple, keyframe, random, pulse)]
    pub brightness: f32,
    /// What picks a point's colour from the list.
    #[setting(label = "Colour by", simple, random)]
    pub colors_by: SwarmColors,
    /// The colours to pick from.
    #[setting(label = "Colours", simple, random)]
    pub colors: Vec<Color>,
}

impl Default for SwarmLook {
    fn default() -> Self {
        Self {
            size: 1.5,
            brightness: 0.25,
            colors_by: SwarmColors::Speed,
            colors: vec![Color::rgb(0x20, 0x40, 0xff), Color::rgb(0x2e, 0xd8, 0xe6), Color::rgb(0xff, 0xf2, 0xc0)],
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
        "Tiny points of light, coloured by speed, by the nearest ball, or along a gradient."
    }
    fn slot(&self) -> SlotId {
        SlotId::Look
    }
    fn settings(&self) -> &'static [SettingDef] {
        SwarmLook::defs()
    }
}
