//! The cards of a Water scene's solid things: **Material** (the Course's Rock card too),
//! a Piece's **Shape**, a Wall's **Line**, and **Moves**.

use bpe_core::{Choice, Color, Settings, ShapePath};
use glam::Vec2;

/// What a solid is made of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum MaterialKind {
    #[default]
    Rock,
    Glass,
    Paper,
    Basalt,
    /// One flat colour, with no texture.
    Flat,
}

/// The Material card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MaterialCard {
    #[setting(label = "Material", simple, group = "Material")]
    pub material: MaterialKind,
    #[setting(label = "Colour", simple, keyframe, group = "Material")]
    pub colour: Color,
    /// The colour its texture is mixed with (the veins in rock, the fibres in paper).
    #[setting(label = "Second colour", when = "material!=flat", keyframe, group = "Material")]
    pub second_colour: Color,
    /// How big its texture's grain is.
    #[setting(
        label = "Texture size",
        when = "material!=flat",
        range(1, 1e9),
        span(10, 400),
        unit = "px",
        group = "Material"
    )]
    pub texture_size: f32,
    /// How far its edges round back from the front, which the lamps pick out.
    #[setting(label = "Depth", range(0, 1e9), span(0, 40), unit = "px", group = "Material")]
    pub depth: f32,
    /// 0 is polished and shines, 1 is matt.
    #[setting(label = "Roughness", range(0, 1), span(0, 1), group = "Material")]
    pub roughness: f32,
    /// A drawn line round its edge (0: none).
    #[setting(label = "Outline", range(0, 1e9), span(0, 10), unit = "px", group = "Material")]
    pub outline: f32,
    #[setting(label = "Outline colour", when = "outline", group = "Material")]
    pub outline_colour: Color,
    /// How much it darkens where liquid has touched it.
    #[setting(label = "Wet marks", range(0, 1), span(0, 1), group = "Material")]
    pub wet_marks: f32,
}

impl Default for MaterialCard {
    fn default() -> Self {
        Self {
            material: MaterialKind::Rock,
            colour: Color::rgb(0x6b, 0x5e, 0x52),
            second_colour: Color::rgb(0x3d, 0x35, 0x2f),
            texture_size: 120.0,
            depth: 12.0,
            roughness: 0.6,
            outline: 0.0,
            outline_colour: Color::rgb(0, 0, 0),
            wet_marks: 0.5,
        }
    }
}

/// The ready-made pieces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum PieceKind {
    #[default]
    Ramp,
    Funnel,
    Pipe,
    Sieve,
    Basin,
    Step,
    Pillar,
    Wheel,
    Bucket,
    Gate,
    Float,
    /// An outline you draw yourself.
    Custom,
}

/// A Piece's Shape card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct PieceShape {
    #[setting(label = "Piece", simple, group = "Shape")]
    pub piece: PieceKind,
    /// How wide and how tall it is.
    #[setting(label = "Size", unit = "px", simple, group = "Shape")]
    pub size: Vec2,
    /// Mirrors it left to right.
    #[setting(label = "Flip", group = "Shape")]
    pub flip: bool,
    /// Cuts its shape out of the course instead of adding to it.
    #[setting(label = "Cuts", group = "Shape")]
    pub cuts: bool,
    #[setting(label = "Outline", when = "piece=custom", group = "Shape")]
    pub path: ShapePath,
}

impl Default for PieceShape {
    fn default() -> Self {
        Self {
            piece: PieceKind::Ramp,
            size: Vec2::new(300.0, 200.0),
            flip: false,
            cuts: false,
            path: ShapePath::circle(),
        }
    }
}

/// How a wall's line ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum LineEnds {
    #[default]
    Round,
    Square,
}

/// A Wall's Line card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WallLine {
    /// The line's points, round the wall's place.
    #[setting(label = "Points", group = "Shape")]
    pub points: Vec<[f32; 2]>,
    #[setting(label = "Thickness", range(1, 1e9), span(2, 200), unit = "px", simple, group = "Shape")]
    pub thickness: f32,
    /// Runs a smooth curve through the points instead of straight lines.
    #[setting(label = "Curved", group = "Shape")]
    pub curved: bool,
    #[setting(label = "Ends", group = "Shape")]
    pub ends: LineEnds,
    /// Cuts the line out of the course instead of adding to it: a channel.
    #[setting(label = "Cuts", group = "Shape")]
    pub cuts: bool,
}

impl Default for WallLine {
    fn default() -> Self {
        Self {
            points: vec![[-150.0, 0.0], [150.0, 0.0]],
            thickness: 24.0,
            curved: false,
            ends: LineEnds::Round,
            cuts: false,
        }
    }
}

/// How a Piece or a Wall moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum MovesKind {
    /// It stays where it is, as part of the course.
    #[default]
    Fixed,
    /// It follows its keyframes, and pushes the water as it goes.
    Keyframed,
    /// The water and gravity move it.
    Driven,
}

/// What holds a driven piece.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Joint {
    /// Nothing: it floats or sinks.
    #[default]
    Free,
    /// It turns round its place (a wheel, a tipping bucket, a gate).
    Pin,
    /// It slides along a line.
    Slide,
}

/// The Moves card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MovesCard {
    #[setting(label = "Moves", simple, group = "Motion")]
    pub moves: MovesKind,
    /// How far it is from its place.
    #[setting(label = "Position", when = "moves=keyframed", unit = "px", keyframe, group = "Motion")]
    pub offset: Vec2,
    #[setting(label = "Turn", when = "moves=keyframed", span(-360, 360), unit = "°", keyframe, group = "Motion")]
    pub turn: f32,
    #[setting(
        label = "Scale",
        when = "moves=keyframed",
        range(0.01, 100),
        span(0.1, 4),
        unit = "×",
        keyframe,
        group = "Motion"
    )]
    pub scale: f32,
    #[setting(label = "Joint", when = "moves=driven", simple, group = "Joint")]
    pub joint: Joint,
    /// How heavy it is next to water: under 1 floats, over 1 sinks.
    #[setting(label = "Weight", when = "moves=driven", range(0.01, 1e9), span(0.1, 10), simple, group = "Joint")]
    pub weight: f32,
    /// How quickly it loses its speed.
    #[setting(label = "Drag", when = "moves=driven", range(0, 1e9), span(0, 1), group = "Joint")]
    pub drag: f32,
    /// How strongly it is pulled back to where it started (0: not at all).
    #[setting(label = "Spring", when = "moves=driven", range(0, 1e9), span(0, 1), group = "Joint")]
    pub spring: f32,
    /// Stops it going past two limits.
    #[setting(label = "Stops", when = "moves=driven and joint!=free", group = "Joint")]
    pub stops: bool,
    #[setting(label = "Stop from", when = "moves=driven and joint!=free and stops", span(-360, 360), group = "Joint")]
    pub stop_low: f32,
    #[setting(label = "Stop to", when = "moves=driven and joint!=free and stops", span(-360, 360), group = "Joint")]
    pub stop_high: f32,
    /// Drives it round or along by itself (0: no motor).
    #[setting(label = "Motor", when = "moves=driven and joint!=free", span(-4, 4), keyframe, group = "Joint")]
    pub motor: f32,
    /// It holds the liquid that falls into it, like a bucket.
    #[setting(label = "Catches liquid", when = "moves=driven", group = "Joint")]
    pub catches: bool,
    /// How full it gets before it tips: 1 is to the brim.
    #[setting(label = "Tips when", when = "moves=driven and catches", range(0, 1), span(0, 1), group = "Joint")]
    pub tips_when: f32,
}

impl Default for MovesCard {
    fn default() -> Self {
        Self {
            moves: MovesKind::Fixed,
            offset: Vec2::ZERO,
            turn: 0.0,
            scale: 1.0,
            joint: Joint::Free,
            weight: 1.0,
            drag: 0.1,
            spring: 0.0,
            stops: false,
            stop_low: -90.0,
            stop_high: 90.0,
            motor: 0.0,
            catches: false,
            tips_when: 0.8,
        }
    }
}
