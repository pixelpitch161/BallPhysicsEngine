//! Brushes: what a thing paints on a canvas as it moves and where it hits.

pub mod sim;

use bpe_core::{Choice, Color, ModuleKind, SettingDef, Settings, SlotId};

use crate::paint::{Follow, Paint};

pub use sim::MarksSim;

pub struct MarksBrushesKind;

/// What sets the width of a paint trail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum WidthBy {
    /// The same width everywhere.
    Fixed,
    /// The ball's size.
    #[default]
    Size,
    /// The ball's speed: faster is wider.
    Speed,
}

/// The tip of the brush that lays a trail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Tip {
    #[default]
    Round,
    Square,
}

/// When a ball paints its trail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum When {
    #[default]
    Always,
    /// Only after the ball has hit something.
    AfterFirstHit,
    /// Only while the ball moves faster than a set speed.
    FasterThan,
}

/// What a splat looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum SplatShape {
    #[default]
    Splat,
    Ring,
    Star,
    Crack,
    Burst,
    Dots,
}

/// How a splat is turned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Turn {
    /// Facing the way the hit came from.
    #[default]
    FaceHit,
    Random,
}

/// Where the colour of the marks comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MarkColour {
    /// The ball's own colour.
    #[default]
    Ball,
    /// The brush's own Colours.
    Own,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MarksBrushes {
    /// Whether the thing paints a trail behind it.
    #[setting(label = "Paint trail", group = "Paint trail", simple)]
    pub paint_on: bool,
    /// What sets the trail's width.
    #[setting(label = "Width from", when = "paint_on", group = "Paint trail", random)]
    pub paint_width_by: WidthBy,
    /// The trail's width. Size: half the width is this many ball radii. Fixed: the width is this
    /// many times 10 px. Speed: the width in px is this times the speed in px/s, over 100.
    #[setting(label = "Width", when = "paint_on", range(0.1, 4), group = "Paint trail", keyframe, random)]
    pub paint_width: f32,
    /// The shape of the brush tip.
    #[setting(label = "Tip", when = "paint_on", group = "Paint trail", random)]
    pub paint_tip: Tip,
    /// When the trail is painted.
    #[setting(label = "Paint when", when = "paint_on", group = "Paint trail")]
    pub paint_when: When,
    /// The slowest speed that still paints.
    #[setting(
        label = "Faster than",
        when = "paint_when=faster_than",
        range(0, 2000),
        unit = "px/s",
        group = "Paint trail"
    )]
    pub paint_min_speed: f32,
    /// How far the paint lasts, in pixels of travel; 0 never runs out.
    #[setting(label = "Paint load", when = "paint_on", range(0, 5000), unit = "px", group = "Paint trail")]
    pub paint_load: f32,
    /// Whether hits leave a splat.
    #[setting(label = "Splat", group = "Splat", simple)]
    pub splat_on: bool,
    /// What the splat looks like.
    #[setting(label = "Shape", when = "splat_on", group = "Splat", random)]
    pub splat_shape: SplatShape,
    /// How big the splat is.
    #[setting(label = "Size", when = "splat_on", range(2, 400), unit = "px", group = "Splat", keyframe, random)]
    pub splat_size: f32,
    /// How much the speed of the hit scales the size: 0 keeps it, 1 follows the speed fully.
    #[setting(label = "Size by speed", when = "splat_on", range(0, 1), group = "Splat", keyframe)]
    pub splat_by_speed: f32,
    /// How the splat is turned.
    #[setting(label = "Turn", when = "splat_on", group = "Splat")]
    pub splat_turn: Turn,
    /// How many different splats of the shape are mixed.
    #[setting(label = "Variants", when = "splat_on", range(1, 8), group = "Splat")]
    pub splat_variants: u32,
    /// The slowest hit that leaves a splat.
    #[setting(label = "Hit speed", when = "splat_on", range(0, 2000), unit = "px/s", group = "Splat")]
    pub splat_min_speed: f32,
    /// The ball's own colour, or the Colours below.
    #[setting(label = "Colour from", group = "Colour", simple, random)]
    pub colour_from: MarkColour,
    /// The marks' own colours, by where, in what order, how fast or when they were made.
    #[setting(
        paint,
        label = "Colours",
        prefix = "colour_",
        group = "Colour",
        when = "colour_from=own",
        follows = "place|order|speed|time|beat",
        simple
    )]
    pub colour: Paint,
}

impl Default for MarksBrushes {
    fn default() -> Self {
        Self {
            paint_on: true,
            paint_width_by: WidthBy::Size,
            paint_width: 1.0,
            paint_tip: Tip::Round,
            paint_when: When::Always,
            paint_min_speed: 100.0,
            paint_load: 0.0,
            splat_on: false,
            splat_shape: SplatShape::Splat,
            splat_size: 30.0,
            splat_by_speed: 0.5,
            splat_turn: Turn::FaceHit,
            splat_variants: 4,
            splat_min_speed: 50.0,
            colour_from: MarkColour::Ball,
            colour: Paint { follow: Follow::Place, ..Paint::solid(Color::WHITE) },
        }
    }
}

impl ModuleKind for MarksBrushesKind {
    fn id(&self) -> &'static str {
        "marks.brushes"
    }
    fn name(&self) -> &'static str {
        "Brushes"
    }
    fn help(&self) -> &'static str {
        "Paints a trail behind the thing and stamps a splat where it hits, on the canvases that ask for it."
    }
    fn slot(&self) -> SlotId {
        SlotId::Marks
    }
    fn settings(&self) -> &'static [SettingDef] {
        MarksBrushes::defs()
    }
}
