//! Blend: how the whole finished ball mixes with what is behind it. Without this layer the ball
//! paints over what is behind it.

use bpe_core::{Blend, Settings};

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BlendLayer {
    /// How it mixes with what is under it: Normal paints over it, the others mix with it.
    /// Everything with the same Blend and amount is mixed in once, as one picture, so where its
    /// parts overlap they do not stack, unless Blend with each other is on.
    #[setting(label = "Blend", simple)]
    pub blend: Blend,
    /// How much of the blend shows: 0 is as if it weren't there, 1 is the whole blend.
    #[setting(label = "Blend amount", range(0, 1), simple, keyframe)]
    pub blend_amount: f32,
    /// Where balls overlap, they blend with each other too, as they do with what is under them;
    /// off, they cover each other and are mixed in as one picture.
    #[setting(label = "Blend with each other", when = "blend=add|screen|multiply|lighten|darken|subtract", simple)]
    pub blend_each: bool,
}

impl Default for BlendLayer {
    fn default() -> Self {
        // A new Blend layer shows straight away: light added to what is behind.
        Self { blend: Blend::Add, blend_amount: 1.0, blend_each: false }
    }
}
