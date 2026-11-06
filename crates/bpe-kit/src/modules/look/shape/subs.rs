//! The settings of a shape's sub-layers that differ from a ball's: Glow and Shadow measure in
//! pixels (a shape's walk works in world pixels), and Shading has no brush and stays with the
//! shape or the screen. Each turns into the ball kind's settings for drawing, sizes kept in
//! pixels.

use std::sync::OnceLock;

use bpe_core::{Choice, Color, Gradient, SettingDef, Settings, Strokes};
use glam::Vec2;

use crate::modules::look::layers::{
    GlowWhere, ShadingKind, ShadingLayer, ShadowMoves, Space, SubColourFrom, SubGlow, SubShadow,
};

/// Glow on a shape's layer: a ball's sub-layer Glow, its size in pixels.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShapeSubGlow {
    /// How brightly it shines.
    #[setting(label = "Strength", range(0, 2), simple, keyframe, pulse)]
    pub strength: f32,
    /// How far the light reaches from the edge.
    #[setting(label = "Size", range(1, 400), unit = "px", simple, keyframe, pulse)]
    pub size: f32,
    /// Outside the piece, inside it, or both.
    #[setting(label = "Where", simple)]
    pub side: GlowWhere,
    /// Same as the layer glows in the layer's own colour.
    #[setting(label = "Colour from", simple)]
    pub colour_from: SubColourFrom,
    /// Its own colour, or colours.
    #[setting(area, label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Gradient,
    /// 0 is invisible, 1 is full.
    #[setting(label = "Opacity", range(0, 1), simple, keyframe, pulse)]
    pub opacity: f32,
}

impl Default for ShapeSubGlow {
    fn default() -> Self {
        let g = SubGlow::default();
        Self {
            strength: g.strength,
            size: 40.0,
            side: g.side,
            colour_from: g.colour_from,
            colour: g.colour,
            opacity: g.opacity,
        }
    }
}

impl From<ShapeSubGlow> for SubGlow {
    fn from(g: ShapeSubGlow) -> Self {
        SubGlow {
            strength: g.strength,
            size: g.size,
            side: g.side,
            colour_from: g.colour_from,
            colour: g.colour,
            opacity: g.opacity,
        }
    }
}

/// What a shape's shadow stays with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShapeShadowMoves {
    /// The screen: the shadow falls the same way however the shape turns.
    #[default]
    Screen,
    /// The shape: the shadow turns as the shape turns.
    Shape,
}

/// Shadow on a shape's layer: a ball's sub-layer Shadow, measured in pixels.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShapeSubShadow {
    /// The way the shadow falls, in degrees: 0 to the right, 90 straight down.
    #[setting(label = "Angle", span(-180, 180), unit = "°", simple, keyframe, pulse)]
    pub angle: f32,
    /// How far it falls.
    #[setting(label = "Distance", range(0, 200), unit = "px", simple, keyframe, pulse)]
    pub distance: f32,
    /// How soft its edge is.
    #[setting(label = "Softness", range(0, 200), unit = "px", keyframe, pulse)]
    pub softness: f32,
    /// Its colour.
    #[setting(label = "Colour", simple, keyframe)]
    pub colour: Color,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), simple, keyframe, pulse)]
    pub opacity: f32,
    /// Whether it falls the same way on the screen or turns with the shape.
    #[setting(label = "Moves with")]
    pub moves: ShapeShadowMoves,
}

impl Default for ShapeSubShadow {
    fn default() -> Self {
        let s = SubShadow::default();
        Self {
            angle: s.angle,
            distance: 12.0,
            softness: 10.0,
            colour: s.colour,
            opacity: s.opacity,
            moves: ShapeShadowMoves::Screen,
        }
    }
}

impl From<ShapeSubShadow> for SubShadow {
    fn from(s: ShapeSubShadow) -> Self {
        SubShadow {
            angle: s.angle,
            distance: s.distance,
            softness: s.softness,
            colour: s.colour,
            opacity: s.opacity,
            moves: match s.moves {
                ShapeShadowMoves::Screen => ShadowMoves::Screen,
                ShapeShadowMoves::Shape => ShadowMoves::Ball,
            },
        }
    }
}

/// How a shape's Shading decides where to darken: a ball's, without the brush.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShapeShadingKind {
    /// Dark at one end of a line, fading to nothing at the other.
    #[default]
    Linear,
    /// Dark outside a circle, like a vignette.
    Radial,
}

/// What a shape's positions are read against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShapeSpace {
    /// The shape: turns as the shape turns.
    #[default]
    Shape,
    /// The screen: stays upright whatever the shape does.
    Screen,
}

/// Shading on a shape's layer: a ball's Shading without the brush, its points across the shape
/// (-1 to 1 over its size).
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShapeShading {
    /// How it decides where to darken.
    #[setting(label = "Kind", simple)]
    pub kind: ShapeShadingKind,
    /// The dark end of the line, across the shape (-1 to 1, left to right and top to bottom).
    #[setting(label = "Dark end", span(-1, 1), when = "kind=linear", keyframe)]
    pub from: Vec2,
    /// Where the darkening has faded to nothing.
    #[setting(label = "Light end", span(-1, 1), when = "kind=linear", keyframe)]
    pub to: Vec2,
    /// The middle of the light circle.
    #[setting(label = "Centre", span(-1, 1), when = "kind=radial", keyframe)]
    pub centre: Vec2,
    /// How big the light circle is, in the shape's sizes.
    #[setting(label = "Size", range(0.1, 2), when = "kind=radial", keyframe, pulse)]
    pub size: f32,
    /// Darken inside the circle instead of outside it.
    #[setting(label = "Darken the middle", when = "kind=radial")]
    pub invert: bool,
    /// The colour it darkens towards, or colours.
    #[setting(area, label = "Colour", simple, keyframe)]
    pub colour: Gradient,
    /// How dark it gets at its darkest.
    #[setting(label = "Amount", range(0, 1), simple, keyframe, pulse)]
    pub amount: f32,
    /// How gently the dark part fades into the light part.
    #[setting(label = "Softness", range(0, 1), keyframe)]
    pub softness: f32,
    /// Whether it turns with the shape or stays upright on the screen.
    #[setting(label = "Stays with")]
    pub space: ShapeSpace,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), keyframe, pulse)]
    pub opacity: f32,
}

impl Default for ShapeShading {
    fn default() -> Self {
        let s = ShadingLayer::default();
        Self {
            kind: ShapeShadingKind::Linear,
            from: s.from,
            to: s.to,
            centre: s.centre,
            size: s.size,
            invert: s.invert,
            colour: s.colour,
            amount: s.amount,
            softness: s.softness,
            space: ShapeSpace::Shape,
            opacity: s.opacity,
        }
    }
}

impl From<ShapeShading> for ShadingLayer {
    fn from(s: ShapeShading) -> Self {
        ShadingLayer {
            kind: match s.kind {
                ShapeShadingKind::Linear => ShadingKind::Linear,
                ShapeShadingKind::Radial => ShadingKind::Radial,
            },
            from: s.from,
            to: s.to,
            centre: s.centre,
            size: s.size,
            invert: s.invert,
            strokes: Strokes::default(),
            colour: s.colour,
            amount: s.amount,
            softness: s.softness,
            space: match s.space {
                ShapeSpace::Shape => Space::Ball,
                ShapeSpace::Screen => Space::Screen,
            },
            opacity: s.opacity,
            ..ShadingLayer::default()
        }
    }
}

/// A shape's Shading sub-layer settings.
pub fn shading_defs() -> &'static [SettingDef] {
    static MADE: OnceLock<Vec<SettingDef>> = OnceLock::new();
    MADE.get_or_init(|| ShapeShading::defs().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_sub_layers_keep_their_values_as_ball_ones() {
        let g = SubGlow::from(ShapeSubGlow { size: 55.0, ..ShapeSubGlow::default() });
        assert_eq!(g.size, 55.0, "sizes stay in pixels");
        let s = SubShadow::from(ShapeSubShadow { moves: ShapeShadowMoves::Shape, ..ShapeSubShadow::default() });
        assert_eq!((s.moves, s.distance), (ShadowMoves::Ball, 12.0));
        let sh = ShadingLayer::from(ShapeShading { kind: ShapeShadingKind::Radial, ..ShapeShading::default() });
        assert_eq!((sh.kind, sh.space), (ShadingKind::Radial, Space::Ball));
    }
}
