//! Shine: light on the ball, so it looks round and shiny. At full strength with its spot where it
//! starts, it lights the ball exactly as the old Shiny style did.

use bpe_core::{Blend, Choice, Color, Gradient, Settings};
use glam::Vec2;

/// The colour of a Shine's light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShineLight {
    /// White light: the classic shine.
    #[default]
    White,
    /// Its own colour, or colours: they tint the bright spot and the sheen.
    Own,
}

/// What the ball seems to be made of, by how the light sits on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShineMaterial {
    /// A smooth, shiny ball: one bright spot.
    #[default]
    Glossy,
    /// Soft, even light and no sharp spot, like rubber or chalk.
    Matte,
    /// Strong contrast, a reflected horizon and a spot tinted by the ball's colour.
    Metal,
    /// A darker middle, a bright rim and a sharp spot with a faint glint opposite it.
    Glass,
}

impl ShineMaterial {
    /// Its number in layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Glossy => 0.0,
            Self::Matte => 1.0,
            Self::Metal => 2.0,
            Self::Glass => 3.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShineLayer {
    /// What the ball seems to be made of.
    #[setting(label = "Material", simple)]
    pub material: ShineMaterial,
    /// 0 leaves the layers under it as they are, 1 lights them fully.
    #[setting(label = "Strength", range(0, 1), simple, keyframe, pulse)]
    pub strength: f32,
    /// Where the light comes from across the ball: -1 to 1 left to right and top to bottom.
    #[setting(label = "Spot", span(-1, 1), simple, keyframe)]
    pub spot: Vec2,
    /// How big the bright spot is.
    #[setting(label = "Size", range(0.2, 3), simple, keyframe)]
    pub size: f32,
    /// How soft the bright spot's edge is: 0 sharp, 1 soft and wide; 0.5 is the classic shine.
    #[setting(label = "Softness", range(0, 1), simple, keyframe)]
    pub softness: f32,
    /// White light, or light of its own colour.
    #[setting(label = "Light", simple)]
    pub light: ShineLight,
    /// The light's colour, or colours: several run across each ball.
    #[setting(area, label = "Colour", when = "light=own", keyframe)]
    pub colour: Gradient,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), group = "Mixing", keyframe, pulse)]
    pub opacity: f32,
    /// How it mixes with the layers under it in the same ball.
    #[setting(label = "Blend", group = "Mixing")]
    pub blend: Blend,
}

/// Where the light comes from on a new Shine: the upper left, as the classic shiny ball.
pub const DEFAULT_SPOT: Vec2 = Vec2::new(-0.45, -0.55);

impl ShineLayer {
    /// How much sharper than the classic spot it is, for layers.wgsl: the spot's exponent is
    /// times `1 + sharpness`, which is 4 at Softness 0, 1 at 0.5 (the classic shine, 0 here) and
    /// a quarter at 1.
    pub fn sharpness(&self) -> f32 {
        let s = if self.softness.is_finite() { self.softness.clamp(0.0, 1.0) } else { 0.5 };
        4f32.powf(1.0 - 2.0 * s) - 1.0
    }
}

impl Default for ShineLayer {
    fn default() -> Self {
        Self {
            material: ShineMaterial::Glossy,
            strength: 1.0,
            spot: DEFAULT_SPOT,
            size: 1.0,
            softness: 0.5,
            light: ShineLight::White,
            colour: Gradient::solid(Color::rgb(0xff, 0xd8, 0x8a)),
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn softness_half_is_the_classic_spot() {
        let at = |softness| ShineLayer { softness, ..ShineLayer::default() }.sharpness();
        assert_eq!(at(0.5), 0.0, "the default draws as the classic shine");
        assert!((at(0.0) - 3.0).abs() < 1e-5 && (at(1.0) + 0.75).abs() < 1e-5);
        assert_eq!(at(f32::NAN), 0.0);
    }
}
