//! Glow: a soft halo of light around the ball.

use bpe_core::{Blend, Color, Gradient, Settings};

use super::ColourFrom;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct GlowLayer {
    /// How brightly the halo shines.
    #[setting(label = "Strength", range(0, 2), simple, keyframe, pulse)]
    pub strength: f32,
    /// How far the halo reaches, in ball radii.
    #[setting(label = "Size", range(0.05, 4), simple, keyframe)]
    pub size: f32,
    /// Same as body glows in the body's colour.
    #[setting(label = "Colour from", simple)]
    pub colour_from: ColourFrom,
    /// The halo's own colour, or colours: several run across each ball's halo.
    #[setting(area, label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Gradient,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), group = "Mixing", keyframe, pulse)]
    pub opacity: f32,
    /// How it mixes with the layers under it in the same ball.
    #[setting(label = "Blend", group = "Mixing")]
    pub blend: Blend,
}

impl Default for GlowLayer {
    fn default() -> Self {
        Self {
            strength: 0.8,
            size: 0.8,
            colour_from: ColourFrom::SameAsBody,
            colour: Gradient::solid(Color::WHITE),
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}
