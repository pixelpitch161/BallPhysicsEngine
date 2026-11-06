//! Pattern: a pattern painted over the layers under it, in a shade of the body's colour or in
//! its own colours.

use bpe_core::{Blend, Settings};

use crate::paint::{InsidePaint, Pattern};

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct PatternLayer {
    /// How the colour is laid out inside the ball.
    #[setting(paint, label = "Pattern", prefix = "in_")]
    pub inside: InsidePaint,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), group = "Mixing", keyframe, pulse)]
    pub opacity: f32,
    /// How it mixes with the layers under it in the same ball.
    #[setting(label = "Blend", group = "Mixing")]
    pub blend: Blend,
}

impl Default for PatternLayer {
    fn default() -> Self {
        // A new pattern shows straight away: stripes in a darker shade of the body.
        Self {
            inside: InsidePaint { pattern: Pattern::Gradient, ..InsidePaint::default() },
            opacity: 1.0,
            blend: Blend::Normal,
        }
    }
}
