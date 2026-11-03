//! Mask: limits the drawn layer it sits under to part of the ball. A layer's masks multiply: the
//! layer shows only where every one of them lets it.

use bpe_core::{Choice, Settings, Strokes};

/// What a mask lets the layer show through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MaskFrom {
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

impl MaskFrom {
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

/// The shapes a Shape mask can be.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MaskShape {
    #[default]
    Half,
    Wedge,
    Quarters,
    Ring,
    Circle,
    Band,
}

impl MaskShape {
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

/// What a mask's shape or strokes stay with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum MaskMoves {
    /// The layer it limits: turns when the layer's pattern turns.
    #[default]
    Layer,
    /// The ball: turns as the ball turns.
    Ball,
    /// The screen: stays upright whatever the ball does.
    Screen,
}

impl MaskMoves {
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
pub struct MaskLayer {
    /// What lets the layer show.
    #[setting(label = "From", simple)]
    pub from: MaskFrom,
    /// The layer whose cover lets this one show (its id; -1 for none).
    #[setting(label = "Layer", when = "from=layer", simple)]
    pub target: i32,
    /// The shape that lets the layer show.
    #[setting(label = "Shape", when = "from=shape", simple)]
    pub shape: MaskShape,
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
    /// Show the layer where the mask would hide it, and hide it where it would show.
    #[setting(label = "Invert")]
    pub invert: bool,
    /// How gently the mask's edge fades.
    #[setting(label = "Softness", range(0, 1), keyframe)]
    pub softness: f32,
    /// What the shape or strokes stay with.
    #[setting(label = "Moves with", when = "from=shape|brush")]
    pub moves: MaskMoves,
}

impl Default for MaskLayer {
    fn default() -> Self {
        Self {
            from: MaskFrom::LayerBelow,
            target: -1,
            shape: MaskShape::Half,
            angle: 0.0,
            size: 0.5,
            strokes: Strokes::default(),
            threshold: 0.5,
            invert: false,
            softness: 0.05,
            moves: MaskMoves::Layer,
        }
    }
}
