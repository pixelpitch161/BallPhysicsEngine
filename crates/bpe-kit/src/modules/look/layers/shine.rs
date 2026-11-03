//! Shine: light on the ball, so it looks round and shiny. At full strength with its spot where it
//! starts, it lights the ball exactly as the old Shiny style did.

use bpe_core::{Blend, Settings};
use glam::Vec2;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ShineLayer {
    /// 0 leaves the layers under it as they are, 1 lights them fully.
    #[setting(label = "Strength", range(0, 1), simple, keyframe, pulse)]
    pub strength: f32,
    /// Where the light comes from across the ball: -1 to 1 left to right and top to bottom.
    #[setting(label = "Spot", span(-1, 1), simple, keyframe)]
    pub spot: Vec2,
    /// How big the bright spot is.
    #[setting(label = "Size", range(0.2, 3), simple, keyframe)]
    pub size: f32,
    /// 0 is invisible, 1 is solid.
    #[setting(label = "Opacity", range(0, 1), group = "Mixing", keyframe, pulse)]
    pub opacity: f32,
    /// How it mixes with the layers under it in the same ball.
    #[setting(label = "Blend", group = "Mixing")]
    pub blend: Blend,
}

/// Where the light comes from on a new Shine: the upper left, as the classic shiny ball.
pub const DEFAULT_SPOT: Vec2 = Vec2::new(-0.45, -0.55);

impl Default for ShineLayer {
    fn default() -> Self {
        Self { strength: 1.0, spot: DEFAULT_SPOT, size: 1.0, opacity: 1.0, blend: Blend::Normal }
    }
}
