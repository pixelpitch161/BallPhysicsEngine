//! The Effects panel: Lightroom's post-crop vignette and grain, and the gradient map (the picture's
//! brightness recoloured). The grain moves: a new pattern each frame, the same one every render.

use bpe_core::{Color, Gradient, Settings};

/// The Effects panel's settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct EffectsPanel {
    /// Darker (down) or lighter (up) towards the edges and corners.
    #[setting(label = "Amount", range(-100, 100), keyframe, pulse, group = "Post-crop vignette")]
    pub vignette_amount: f32,
    /// How far out from the middle it starts.
    #[setting(label = "Midpoint", range(0, 100), keyframe, pulse, group = "Post-crop vignette")]
    pub vignette_midpoint: f32,
    /// Down a rounder rectangle, up a circle; 0 follows the picture's shape.
    #[setting(label = "Roundness", range(-100, 100), keyframe, pulse, group = "Post-crop vignette")]
    pub vignette_roundness: f32,
    /// How soft its edge is.
    #[setting(label = "Feather", range(0, 100), keyframe, pulse, group = "Post-crop vignette")]
    pub vignette_feather: f32,
    /// Under a dark vignette, how much bright things (lamps, glows) keep their brightness.
    #[setting(label = "Highlights", range(0, 100), keyframe, pulse, group = "Post-crop vignette")]
    pub vignette_highlights: f32,
    /// How much film grain, a new pattern each frame.
    #[setting(label = "Amount", range(0, 100), keyframe, pulse, group = "Grain")]
    pub grain_amount: f32,
    /// How big a grain is.
    #[setting(label = "Size", range(0, 100), keyframe, pulse, group = "Grain")]
    pub grain_size: f32,
    /// Even (down) or clumpy and uneven (up).
    #[setting(label = "Roughness", range(0, 100), keyframe, pulse, group = "Grain")]
    pub grain_roughness: f32,
    /// The colours the picture's brightness is mapped to: dark parts take the first colours,
    /// light parts the last.
    #[setting(label = "Gradient", keyframe, group = "Gradient map")]
    pub map: Gradient,
    /// How much of the mapped colour replaces the picture's own.
    #[setting(label = "Amount", range(0, 100), keyframe, pulse, group = "Gradient map")]
    pub map_amount: f32,
}

impl Default for EffectsPanel {
    fn default() -> Self {
        Self {
            vignette_amount: 0.0,
            vignette_midpoint: 50.0,
            vignette_roundness: 0.0,
            vignette_feather: 50.0,
            vignette_highlights: 0.0,
            grain_amount: 0.0,
            grain_size: 25.0,
            grain_roughness: 50.0,
            map: Gradient::even(&[Color::rgb(0, 0, 0), Color::WHITE]),
            map_amount: 0.0,
        }
    }
}

impl EffectsPanel {
    /// The panel as the frame carries it.
    pub fn gpu(&self) -> bpe_gfx::GradeEffects {
        let d = bpe_gfx::GradeEffects::default();
        let n = |v: f32, or: f32| if v.is_finite() { v } else { or };
        bpe_gfx::GradeEffects {
            vignette_amount: n(self.vignette_amount, d.vignette_amount),
            vignette_midpoint: n(self.vignette_midpoint, d.vignette_midpoint),
            vignette_roundness: n(self.vignette_roundness, d.vignette_roundness),
            vignette_feather: n(self.vignette_feather, d.vignette_feather),
            vignette_highlights: n(self.vignette_highlights, d.vignette_highlights),
            grain_amount: n(self.grain_amount, d.grain_amount),
            grain_size: n(self.grain_size, d.grain_size),
            grain_roughness: n(self.grain_roughness, d.grain_roughness),
            map: self.map.clone(),
            map_amount: n(self.map_amount, d.map_amount),
        }
    }
}
