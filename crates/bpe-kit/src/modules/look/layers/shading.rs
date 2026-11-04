//! Shading: darkens part of the ball, along a line (Linear), outside a circle (Radial) or where
//! it is painted (Brush).

use bpe_core::{Blend, Choice, Color, Gradient, Settings, Strokes};

use super::GradientShape;
use glam::Vec2;

/// How a Shading layer decides where to darken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShadingKind {
    /// Dark at one end of a line, fading to nothing at the other.
    #[default]
    Linear,
    /// Dark outside a circle, like a vignette.
    Radial,
    /// Dark where it is painted on the big ball.
    Brush,
}

impl ShadingKind {
    /// Its number in layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Linear => 0.0,
            Self::Radial => 1.0,
            Self::Brush => 2.0,
        }
    }
}

/// What a layer's positions are read against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Space {
    /// The ball: turns as the ball turns.
    #[default]
    Ball,
    /// The screen: stays upright whatever the ball does.
    Screen,
}

impl Space {
    /// Its number in layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Ball => 0.0,
            Self::Screen => 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShadingLayer {
    /// How it decides where to darken.
    #[setting(label = "Kind", simple)]
    pub kind: ShadingKind,
    /// The dark end of the line, across the ball (-1 to 1, left to right and top to bottom).
    #[setting(label = "Dark end", span(-1, 1), when = "kind=linear", keyframe)]
    pub from: Vec2,
    /// Where the darkening has faded to nothing.
    #[setting(label = "Light end", span(-1, 1), when = "kind=linear", keyframe)]
    pub to: Vec2,
    /// The middle of the light circle.
    #[setting(label = "Centre", span(-1, 1), when = "kind=radial", keyframe)]
    pub centre: Vec2,
    /// How big the light circle is, in ball radii.
    #[setting(label = "Size", range(0.1, 2), when = "kind=radial", keyframe, pulse)]
    pub size: f32,
    /// Darken inside the circle instead of outside it.
    #[setting(label = "Darken the middle", when = "kind=radial")]
    pub invert: bool,
    /// The painted strokes: painted on the big ball with the brush.
    #[setting(label = "Strokes", when = "kind=brush", simple)]
    pub strokes: Strokes,
    /// The colour it darkens towards, or colours: several run across each ball.
    #[setting(label = "Colour", simple, keyframe)]
    pub colour: Gradient,
    /// How its colours run: straight across the ball, or from its middle to its edge.
    #[setting(label = "Gradient", when = "colour")]
    pub gradient: GradientShape,
    /// The way straight colours run across the ball, in degrees: 90 from top to bottom.
    #[setting(label = "Direction", when = "colour and gradient=straight", span(-180, 180), unit = "°", keyframe)]
    pub direction: f32,
    /// How dark it gets at its darkest.
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How gently the dark part fades into the light part.
    #[setting(label = "Softness", range(0, 1), when = "kind!=brush", keyframe)]
    pub softness: f32,
    /// Whether it turns with the ball or stays upright on the screen.
    #[setting(label = "Stays with")]
    pub space: Space,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), group = "Mixing", keyframe, pulse)]
    pub opacity: f32,
    /// How it mixes with the layers under it in the same ball.
    #[setting(label = "Blend", group = "Mixing")]
    pub blend: Blend,
}

impl Default for ShadingLayer {
    fn default() -> Self {
        // Light from the upper left, as a new Shine's spot, so the two agree.
        Self {
            kind: ShadingKind::Linear,
            from: Vec2::new(0.7, 0.7),
            to: Vec2::new(-0.7, -0.7),
            centre: Vec2::new(-0.3, -0.3),
            size: 0.9,
            invert: false,
            strokes: Strokes::default(),
            colour: Gradient::solid(Color::rgb(0, 0, 0)),
            gradient: GradientShape::Straight,
            direction: 90.0,
            amount: 0.6,
            softness: 0.5,
            space: Space::Ball,
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}
