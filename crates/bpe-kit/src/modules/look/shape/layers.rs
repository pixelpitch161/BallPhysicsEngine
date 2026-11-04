//! The settings of a shape's paint layers: Colour and gradient, Outline and Glow. Blend and
//! Shader reuse the balls' (see `super::kinds`).

use bpe_core::{Blend, Choice, Color, Gradient, Settings};

use crate::modules::look::layers::{ColourFrom, OutlineSits};

/// How a Colour and gradient layer's colours run across the shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum PaintRuns {
    /// Straight across the shape at the layer's Direction, from one edge to the other.
    #[default]
    Across,
    /// From the shape's middle out to its furthest point; across the band of a hollow shape.
    FromMiddle,
    /// Once round the outline, clockwise from the top.
    AlongEdge,
}

impl PaintRuns {
    /// Its number in shape_layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Across => 0.0,
            Self::FromMiddle => 1.0,
            Self::AlongEdge => 2.0,
        }
    }
}

/// How an outline's or glow's own colours run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum LineRuns {
    /// Straight across the shape at the layer's Direction.
    #[default]
    Across,
    /// From the shape's middle out; across the band of a hollow shape.
    FromMiddle,
    /// Once round the outline, clockwise from the top.
    AlongEdge,
    /// Across the line from its inner side out (an outline), or out from the edge (a glow).
    Outward,
}

impl LineRuns {
    /// Its number in shape_layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Across => 0.0,
            Self::FromMiddle => 1.0,
            Self::AlongEdge => 2.0,
            Self::Outward => 3.0,
        }
    }
}

/// The colour a new Colour and gradient layer starts with: a clear blue, so it shows on the
/// white body at once.
pub const START_COLOUR: Color = Color::rgb(64, 156, 255);

/// Colour and gradient: one colour, or a gradient, over the whole shape.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct PaintLayer {
    /// One colour, or several: they run across the shape as Gradient says.
    #[setting(label = "Colours", simple, keyframe)]
    pub colours: Gradient,
    /// How several colours run: across, from the middle, or along the edge.
    #[setting(label = "Gradient", when = "colours", simple)]
    pub runs: PaintRuns,
    /// The way colours run across, in degrees: 90 from top to bottom. Turns with the shape.
    #[setting(label = "Direction", when = "colours and runs=across", span(-180, 180), unit = "°", keyframe)]
    pub direction: f32,
    /// How fast the colours move along the gradient, wrapping round, in times per second.
    #[setting(label = "Flow", when = "colours", span(-2, 2), keyframe)]
    pub flow: f32,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), group = "Mixing", keyframe, pulse)]
    pub opacity: f32,
    /// How it mixes with the layers under it in the same shape.
    #[setting(label = "Blend", group = "Mixing")]
    pub blend: Blend,
}

impl Default for PaintLayer {
    fn default() -> Self {
        Self {
            colours: Gradient::solid(START_COLOUR),
            runs: PaintRuns::Across,
            direction: 90.0,
            flow: 0.0,
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}

/// Outline: a line along every edge of the shape (both edges of a hollow band).
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShapeOutline {
    /// How wide the line is.
    #[setting(label = "Width", range(0, 400), span(0, 60), unit = "px", simple, keyframe, pulse)]
    pub width: f32,
    /// Inside the shape's edge, centred on it, or outside it.
    #[setting(label = "Sits", simple)]
    pub sits: OutlineSits,
    /// Same as body takes the colour of the layers under it, a little darker, so it shows.
    #[setting(label = "Colour from", simple)]
    pub colour_from: ColourFrom,
    /// The line's own colour, or colours.
    #[setting(label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Gradient,
    /// How its colours run.
    #[setting(label = "Gradient", when = "colour_from=own and colour")]
    pub gradient: LineRuns,
    /// The way colours run across, in degrees: 90 from top to bottom.
    #[setting(
        label = "Direction",
        when = "colour_from=own and colour and gradient=across",
        span(-180, 180),
        unit = "°",
        keyframe
    )]
    pub direction: f32,
    /// How fast the colours move along the gradient, wrapping round, in times per second.
    #[setting(label = "Flow", when = "colour_from=own and colour", span(-2, 2), keyframe)]
    pub flow: f32,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), group = "Mixing", keyframe, pulse)]
    pub opacity: f32,
    /// How it mixes with the layers under it in the same shape.
    #[setting(label = "Blend", group = "Mixing")]
    pub blend: Blend,
}

impl Default for ShapeOutline {
    fn default() -> Self {
        Self {
            width: 6.0,
            sits: OutlineSits::Inside,
            colour_from: ColourFrom::SameAsBody,
            colour: Gradient::solid(Color::WHITE),
            gradient: LineRuns::Across,
            direction: 90.0,
            flow: 0.0,
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}

/// Glow: soft light outside every edge of the shape.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShapeGlow {
    /// How brightly it shines.
    #[setting(label = "Amount", range(0, 2), simple, keyframe, pulse)]
    pub strength: f32,
    /// How far the light reaches from the edge.
    #[setting(label = "Size", range(1, 2000), span(2, 200), unit = "px", simple, keyframe)]
    pub size: f32,
    /// Same as body glows in the colour of the layers under it.
    #[setting(label = "Colour from", simple)]
    pub colour_from: ColourFrom,
    /// The light's own colour, or colours.
    #[setting(label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Gradient,
    /// How its colours run.
    #[setting(label = "Gradient", when = "colour_from=own and colour")]
    pub gradient: LineRuns,
    /// The way colours run across, in degrees: 90 from top to bottom.
    #[setting(
        label = "Direction",
        when = "colour_from=own and colour and gradient=across",
        span(-180, 180),
        unit = "°",
        keyframe
    )]
    pub direction: f32,
    /// How fast the colours move along the gradient, wrapping round, in times per second.
    #[setting(label = "Flow", when = "colour_from=own and colour", span(-2, 2), keyframe)]
    pub flow: f32,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), group = "Mixing", keyframe, pulse)]
    pub opacity: f32,
    /// How it mixes with the layers under it in the same shape.
    #[setting(label = "Blend", group = "Mixing")]
    pub blend: Blend,
}

impl Default for ShapeGlow {
    fn default() -> Self {
        Self {
            strength: 0.8,
            size: 30.0,
            colour_from: ColourFrom::SameAsBody,
            colour: Gradient::solid(Color::WHITE),
            gradient: LineRuns::Across,
            direction: 90.0,
            flow: 0.0,
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}
