//! The Detail panel: Lightroom's Sharpening (Amount, Radius, Detail). Noise Reduction is left out:
//! a rendered video has no sensor noise.

use bpe_core::Settings;

/// The Detail panel's settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct DetailPanel {
    /// How strongly edges and fine detail are sharpened.
    #[setting(label = "Amount", range(0, 150), keyframe, pulse, group = "Sharpening")]
    pub amount: f32,
    /// How wide the sharpened edges are, in pixels of the video.
    #[setting(label = "Radius", range(0.5, 3), unit = "px", keyframe, pulse, group = "Sharpening")]
    pub radius: f32,
    /// Low keeps sharpening to strong edges and holds halos back; high brings out the finest
    /// detail too.
    #[setting(label = "Detail", range(0, 100), keyframe, pulse, group = "Sharpening")]
    pub detail: f32,
}

impl Default for DetailPanel {
    fn default() -> Self {
        Self { amount: 0.0, radius: 1.0, detail: 25.0 }
    }
}

impl DetailPanel {
    /// The panel as the frame carries it.
    pub fn gpu(&self) -> bpe_gfx::GradeDetail {
        let d = bpe_gfx::GradeDetail::default();
        let n = |v: f32, or: f32| if v.is_finite() { v } else { or };
        bpe_gfx::GradeDetail {
            amount: n(self.amount, d.amount),
            radius: n(self.radius, d.radius),
            detail: n(self.detail, d.detail),
        }
    }
}
