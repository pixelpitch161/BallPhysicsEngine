//! The settings of a shape's paint layers: Colour and gradient, Outline and Glow. Blend and
//! Shader reuse the balls' (see `super::kinds`).

use bpe_core::{Blend, Choice, Color, Gradient, Settings};

use crate::modules::look::layers::{ColourFrom, OutlineSits};

/// The colour a new Colour and gradient layer starts with: a clear blue, so it shows on the
/// white body at once.
pub const START_COLOUR: Color = Color::rgb(64, 156, 255);

/// What a Text's layer lays its own colours over (Text part 4); a shape is always one whole.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Across {
    /// One gradient across every letter.
    #[default]
    WholeText,
    /// The gradient again on each letter, turning with it.
    EachLetter,
    /// The gradient again on each line.
    EachLine,
}

/// Colour and gradient: one colour, or a gradient, over the whole shape.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct PaintLayer {
    /// One colour, or several: they run across the shape as Gradient says.
    #[setting(area, label = "Colours", simple, keyframe)]
    pub colours: Gradient,
    /// On a Text: the gradient across the whole text, or again on each letter or each line.
    #[setting(label = "Across", when = "colours")]
    pub across: Across,
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
            across: Across::WholeText,
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
    #[setting(area, label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Gradient,
    /// On a Text: the gradient across the whole text, or again on each letter or each line.
    #[setting(label = "Across", when = "colour_from=own and colour")]
    pub across: Across,
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
            across: Across::WholeText,
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
    #[setting(area, label = "Colour", when = "colour_from=own", simple, keyframe)]
    pub colour: Gradient,
    /// On a Text: the gradient across the whole text, or again on each letter or each line.
    #[setting(label = "Across", when = "colour_from=own and colour")]
    pub across: Across,
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
            across: Across::WholeText,
            flow: 0.0,
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}
