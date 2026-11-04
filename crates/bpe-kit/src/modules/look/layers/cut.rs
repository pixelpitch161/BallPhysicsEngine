//! Cut: a sub-layer that limits its layer to part of the ball, and gives the piece its shape for
//! the sub-layers after it. A layer's cuts multiply: the layer shows only where every one of them
//! lets it.

use bpe_core::{Choice, Settings, Strokes};

/// What a cut lets the layer show through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CutFrom {
    /// Where the layer directly under it covers, as clipping in a paint program.
    #[default]
    LayerBelow,
    /// Where another layer of the same ball, under this one, covers.
    Layer,
    /// A shape: half, a wedge, quarters, a ring, a circle or a band.
    Shape,
    /// Where it is painted on the big ball.
    Brush,
    /// Where the layers under it are lighter than the threshold.
    Light,
    /// Where the layers under it are darker than the threshold.
    Dark,
}

impl CutFrom {
    /// Its number in layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::LayerBelow => 0.0,
            Self::Layer => 1.0,
            Self::Shape => 2.0,
            Self::Brush => 3.0,
            Self::Light => 4.0,
            Self::Dark => 5.0,
        }
    }
}

/// The shapes a Shape cut can be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CutShape {
    #[default]
    Half,
    Wedge,
    Quarters,
    Ring,
    Circle,
    Band,
}

impl CutShape {
    /// Its number in layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Half => 0.0,
            Self::Wedge => 1.0,
            Self::Quarters => 2.0,
            Self::Ring => 3.0,
            Self::Circle => 4.0,
            Self::Band => 5.0,
        }
    }
}

/// What a cut's shape or strokes stay with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CutMoves {
    /// The layer it limits: turns when the layer's pattern turns.
    #[default]
    Layer,
    /// The ball: turns as the ball turns.
    Ball,
    /// The screen: stays upright whatever the ball does.
    Screen,
}

impl CutMoves {
    /// Its number in layers.wgsl.
    pub fn code(self) -> f32 {
        match self {
            Self::Layer => 0.0,
            Self::Ball => 1.0,
            Self::Screen => 2.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CutLayer {
    /// What lets the layer show.
    #[setting(label = "From", simple)]
    pub from: CutFrom,
    /// The layer whose cover lets this one show (its id; -1 for none).
    #[setting(label = "Layer", when = "from=layer", simple)]
    pub target: i32,
    /// The shape that lets the layer show.
    #[setting(label = "Shape", when = "from=shape", simple)]
    pub shape: CutShape,
    /// Which way the shape points.
    #[setting(label = "Angle", span(-180, 180), unit = "°", when = "from=shape", keyframe, pulse)]
    pub angle: f32,
    /// How big the shape is: a wedge's share of the turn, a ring's or band's width, a circle's
    /// radius.
    #[setting(label = "Size", range(0, 1), when = "from=shape", simple, keyframe, pulse)]
    pub size: f32,
    /// The painted strokes: painted on the big ball with the brush.
    #[setting(label = "Strokes", when = "from=brush", simple)]
    pub strokes: Strokes,
    /// How light (or dark) the layers under it must be.
    #[setting(label = "Threshold", range(0, 1), when = "from=light|dark", simple, keyframe, pulse)]
    pub threshold: f32,
    /// Show the layer where the cut would hide it, and hide it where it would show.
    #[setting(label = "Invert")]
    pub invert: bool,
    /// How gently the cut's edge fades.
    #[setting(label = "Softness", range(0, 1), keyframe)]
    pub softness: f32,
    /// What the shape or strokes stay with.
    #[setting(label = "Moves with", when = "from=shape|brush")]
    pub moves: CutMoves,
}

impl Default for CutLayer {
    fn default() -> Self {
        Self {
            from: CutFrom::LayerBelow,
            target: -1,
            shape: CutShape::Half,
            angle: 0.0,
            size: 0.5,
            strokes: Strokes::default(),
            threshold: 0.5,
            invert: false,
            softness: 0.05,
            moves: CutMoves::Layer,
        }
    }
}

/// What a shape's cut lets the layer show through: a cut's choices without Brush (shapes have no
/// brush view).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShapeCutFrom {
    /// Where the layer directly under it covers.
    #[default]
    LayerBelow,
    /// Where another layer of the same shape, under this one, covers.
    Layer,
    /// A shape: half, a wedge, quarters, a ring, a circle or a band.
    Shape,
    /// Where the layers under it are lighter than the threshold.
    Light,
    /// Where the layers under it are darker than the threshold.
    Dark,
}

/// What a shape's cut stays with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ShapeCutMoves {
    /// The shape: turns as the shape turns.
    #[default]
    Shape,
    /// The screen: stays upright whatever the shape does.
    Screen,
}

/// A Cut on a shape's layer: a ball's Cut without the brush, its shape in the shape's own units
/// (its middle at 0, its size 1).
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShapeCut {
    /// What lets the layer show.
    #[setting(label = "From", simple)]
    pub from: ShapeCutFrom,
    /// The layer whose cover lets this one show (its id; -1 for none).
    #[setting(label = "Layer", when = "from=layer", simple)]
    pub target: i32,
    /// The shape that lets the layer show.
    #[setting(label = "Shape", when = "from=shape", simple)]
    pub shape: CutShape,
    /// Which way the shape points.
    #[setting(label = "Angle", span(-180, 180), unit = "°", when = "from=shape", keyframe, pulse)]
    pub angle: f32,
    /// How big the shape is: a wedge's share of the turn, a ring's or band's width, a circle's
    /// radius.
    #[setting(label = "Size", range(0, 1), when = "from=shape", simple, keyframe, pulse)]
    pub size: f32,
    /// How light (or dark) the layers under it must be.
    #[setting(label = "Threshold", range(0, 1), when = "from=light|dark", simple, keyframe, pulse)]
    pub threshold: f32,
    /// Show the layer where the cut would hide it, and hide it where it would show.
    #[setting(label = "Invert")]
    pub invert: bool,
    /// How gently the cut's edge fades.
    #[setting(label = "Softness", range(0, 1), keyframe)]
    pub softness: f32,
    /// What the shape stays with.
    #[setting(label = "Moves with", when = "from=shape")]
    pub moves: ShapeCutMoves,
}

impl Default for ShapeCut {
    fn default() -> Self {
        let c = CutLayer::default();
        Self {
            from: ShapeCutFrom::LayerBelow,
            target: c.target,
            shape: c.shape,
            angle: c.angle,
            size: c.size,
            threshold: c.threshold,
            invert: c.invert,
            softness: c.softness,
            moves: ShapeCutMoves::Shape,
        }
    }
}

impl From<ShapeCut> for CutLayer {
    /// The ball cut that draws the same: a shape's own frame is a ball's (moving with the layer
    /// is moving with the shape, as a shape's layers don't turn on their own).
    fn from(c: ShapeCut) -> Self {
        CutLayer {
            from: match c.from {
                ShapeCutFrom::LayerBelow => CutFrom::LayerBelow,
                ShapeCutFrom::Layer => CutFrom::Layer,
                ShapeCutFrom::Shape => CutFrom::Shape,
                ShapeCutFrom::Light => CutFrom::Light,
                ShapeCutFrom::Dark => CutFrom::Dark,
            },
            target: c.target,
            shape: c.shape,
            angle: c.angle,
            size: c.size,
            strokes: Strokes::default(),
            threshold: c.threshold,
            invert: c.invert,
            softness: c.softness,
            moves: match c.moves {
                ShapeCutMoves::Shape => CutMoves::Ball,
                ShapeCutMoves::Screen => CutMoves::Screen,
            },
        }
    }
}
