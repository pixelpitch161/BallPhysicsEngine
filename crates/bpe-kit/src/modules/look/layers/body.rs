//! Body: the ball itself, always the bottom layer: its shape and its colours. Its size is the
//! Balls thing's own Size, shown with it in the editor.

use bpe_core::Settings;

use crate::modules::look::BallShape;
use crate::paint::Paint;

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
        follows = "place|start|order|speed|hits|step|time|path",
        simple
    )]
    pub fill: Paint,
}

impl Default for BodyLayer {
    fn default() -> Self {
        Self { ball_shape: BallShape::Circle, roundness: 0.15, fill: Paint::default() }
    }
}
