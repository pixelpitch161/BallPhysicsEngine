//! Body: the ball itself, always the bottom layer: its shape and its colours. Its size is the
//! Balls thing's own Size, shown with it in the editor.

use bpe_core::Settings;

use crate::modules::look::BallShape;
use crate::paint::{Follow, Paint};

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BodyLayer {
    /// The outline the ball is drawn with; it still bounces as a circle.
    #[setting(label = "Shape", simple)]
    pub ball_shape: BallShape,
    /// How rounded the shape's corners are; circles ignore it.
    #[setting(label = "Roundness", when = "ball_shape!=circle", range(0, 1), keyframe)]
    pub roundness: f32,
    /// The colours and what spreads them across the balls.
    #[setting(
        paint,
        label = "Colours",
        prefix = "",
        group = "Colours",
        follows = "each_ball|place|start|order|speed|hits|step|time|path",
        simple
    )]
    pub fill: Paint,
    /// 0 is invisible, 1 is solid: a see-through body leaves its outline, glow and the rest on
    /// their own. Mixing it with what is behind the ball is the Blend layer's job, right above it.
    #[setting(label = "Opacity", range(0, 1), group = "Mixing", keyframe, pulse)]
    pub opacity: f32,
}

impl Default for BodyLayer {
    fn default() -> Self {
        // A gradient picked for the body runs across each ball on its own.
        let fill = Paint { follow: Follow::EachBall, ..Paint::default() };
        Self { ball_shape: BallShape::Circle, roundness: 0.15, fill, opacity: 1.0 }
    }
}
