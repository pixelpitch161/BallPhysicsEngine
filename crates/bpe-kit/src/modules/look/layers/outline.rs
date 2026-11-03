//! Outline: a ring along the ball's edge.

use bpe_core::{Blend, Choice, Color, Settings};

use super::ColourFrom;

/// Where the ring sits on the ball's edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum OutlineSits {
    #[default]
    Inside,
    Centre,
    Outside,
}

impl OutlineSits {
    /// Its number in layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Inside => 0.0,
            Self::Centre => 1.0,
            Self::Outside => 2.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct OutlineLayer {
    /// How wide the ring is.
    #[setting(label = "Width", range(0, 60), unit = "px", simple, keyframe, pulse)]
    pub width: f32,
    /// Inside the edge, centred on it, or outside it.
    #[setting(label = "Sits", simple)]
    pub sits: OutlineSits,
    /// Same as body takes the body's colour a little darker, so a white ball still shows its ring.
    #[setting(label = "Colour from", simple)]
    pub colour_from: ColourFrom,
    /// The ring's own colour.
    #[setting(label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Color,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), group = "Mixing", keyframe, pulse)]
    pub opacity: f32,
    /// How it mixes with the layers under it in the same ball.
    #[setting(label = "Blend", group = "Mixing")]
    pub blend: Blend,
}

impl Default for OutlineLayer {
    fn default() -> Self {
        Self {
            width: 4.0,
            sits: OutlineSits::Inside,
            colour_from: ColourFrom::SameAsBody,
            colour: Color::WHITE,
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}
