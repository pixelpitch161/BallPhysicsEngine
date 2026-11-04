//! Outline: a ring along the ball's edge.

use bpe_core::{Blend, Choice, Color, Gradient, Settings};

use super::{ColourFrom, GradientShape};

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
    /// The ring's own colour, or colours: several run across each ball's ring.
    #[setting(label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Gradient,
    /// How its colours run: straight across the ball, or across the ring from its inner edge out.
    #[setting(label = "Gradient", when = "colour_from=own and colour")]
    pub gradient: GradientShape,
    /// The way straight colours run across the ball, in degrees: 90 from top to bottom.
    #[setting(label = "Direction", when = "colour_from=own and colour and gradient=straight", span(-180, 180), unit = "°", keyframe)]
    pub direction: f32,
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
            colour: Gradient::solid(Color::WHITE),
            gradient: GradientShape::Straight,
            direction: 90.0,
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}
