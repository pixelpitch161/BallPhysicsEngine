//! The Colour Grading panel: Lightroom's colour wheels for the shadows, midtones and highlights
//! and a global one, each a tint (hue and strength) and a luminance, with how much the regions
//! overlap (Blending) and where the shadows give way to the highlights (Balance).

use bpe_core::Settings;

/// The wheels' key prefixes, in the order the 3-way view shows them, then Global.
pub const REGIONS: [&str; 4] = ["shadows", "midtones", "highlights", "global"];

/// The Colour Grading panel's settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ColourGradingPanel {
    /// The colour the dark parts are tinted towards (0 red, 120 green, 240 blue).
    #[setting(label = "Hue", range(0, 360), unit = "°", keyframe, pulse, group = "Shadows")]
    pub shadows_hue: f32,
    /// How strongly the dark parts are tinted.
    #[setting(label = "Saturation", range(0, 100), keyframe, pulse, group = "Shadows")]
    pub shadows_saturation: f32,
    /// The dark parts brighter or darker.
    #[setting(label = "Luminance", range(-100, 100), keyframe, pulse, group = "Shadows")]
    pub shadows_luminance: f32,
    /// The colour the middle tones are tinted towards (0 red, 120 green, 240 blue).
    #[setting(label = "Hue", range(0, 360), unit = "°", keyframe, pulse, group = "Midtones")]
    pub midtones_hue: f32,
    /// How strongly the middle tones are tinted.
    #[setting(label = "Saturation", range(0, 100), keyframe, pulse, group = "Midtones")]
    pub midtones_saturation: f32,
    /// The middle tones brighter or darker.
    #[setting(label = "Luminance", range(-100, 100), keyframe, pulse, group = "Midtones")]
    pub midtones_luminance: f32,
    /// The colour the bright parts are tinted towards (0 red, 120 green, 240 blue).
    #[setting(label = "Hue", range(0, 360), unit = "°", keyframe, pulse, group = "Highlights")]
    pub highlights_hue: f32,
    /// How strongly the bright parts are tinted.
    #[setting(label = "Saturation", range(0, 100), keyframe, pulse, group = "Highlights")]
    pub highlights_saturation: f32,
    /// The bright parts brighter or darker.
    #[setting(label = "Luminance", range(-100, 100), keyframe, pulse, group = "Highlights")]
    pub highlights_luminance: f32,
    /// The colour the whole picture are tinted towards (0 red, 120 green, 240 blue).
    #[setting(label = "Hue", range(0, 360), unit = "°", keyframe, pulse, group = "Global")]
    pub global_hue: f32,
    /// How strongly the whole picture are tinted.
    #[setting(label = "Saturation", range(0, 100), keyframe, pulse, group = "Global")]
    pub global_saturation: f32,
    /// The whole picture brighter or darker.
    #[setting(label = "Luminance", range(-100, 100), keyframe, pulse, group = "Global")]
    pub global_luminance: f32,
    /// How much the shadows, midtones and highlights overlap: low keeps them apart, high blends them.
    #[setting(label = "Blending", range(0, 100), keyframe, pulse, group = "Blend")]
    pub blending: f32,
    /// Where the shadows give way to the highlights: up gives the highlights more of the picture.
    #[setting(label = "Balance", range(-100, 100), keyframe, pulse, group = "Blend")]
    pub balance: f32,
}

impl Default for ColourGradingPanel {
    fn default() -> Self {
        Self {
            shadows_hue: 0.0,
            shadows_saturation: 0.0,
            shadows_luminance: 0.0,
            midtones_hue: 0.0,
            midtones_saturation: 0.0,
            midtones_luminance: 0.0,
            highlights_hue: 0.0,
            highlights_saturation: 0.0,
            highlights_luminance: 0.0,
            global_hue: 0.0,
            global_saturation: 0.0,
            global_luminance: 0.0,
            blending: 50.0,
            balance: 0.0,
        }
    }
}

impl ColourGradingPanel {
    /// The panel as the frame carries it.
    pub fn gpu(&self) -> bpe_gfx::GradeColourGrading {
        let n = |v: f32| if v.is_finite() { v } else { 0.0 };
        let wheel = |hue: f32, saturation: f32, luminance: f32| bpe_gfx::GradeWheel {
            hue: n(hue),
            saturation: n(saturation),
            luminance: n(luminance),
        };
        bpe_gfx::GradeColourGrading {
            shadows: wheel(self.shadows_hue, self.shadows_saturation, self.shadows_luminance),
            midtones: wheel(self.midtones_hue, self.midtones_saturation, self.midtones_luminance),
            highlights: wheel(self.highlights_hue, self.highlights_saturation, self.highlights_luminance),
            global: wheel(self.global_hue, self.global_saturation, self.global_luminance),
            blending: if self.blending.is_finite() { self.blending } else { 50.0 },
            balance: n(self.balance),
        }
    }
}
