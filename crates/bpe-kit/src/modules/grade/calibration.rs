//! The Calibration panel: Lightroom's Shadows Tint and the red, green and blue primaries' hue and
//! saturation. Moving a primary moves every colour made with it; greys never move.

use bpe_core::Settings;

/// The Calibration panel's settings.
#[derive(Clone, Debug, Default, PartialEq, Settings)]
pub struct CalibrationPanel {
    /// The darks towards green (down) or magenta (up), at the same brightness.
    #[setting(label = "Tint", range(-100, 100), keyframe, pulse, group = "Shadows")]
    pub shadows_tint: f32,
    /// The red primary towards magenta (down) or orange (up).
    #[setting(label = "Hue", range(-100, 100), keyframe, pulse, group = "Red primary")]
    pub red_hue: f32,
    /// Every colour with red in it less or more colourful.
    #[setting(label = "Saturation", range(-100, 100), keyframe, pulse, group = "Red primary")]
    pub red_saturation: f32,
    /// The green primary towards yellow (down) or cyan (up).
    #[setting(label = "Hue", range(-100, 100), keyframe, pulse, group = "Green primary")]
    pub green_hue: f32,
    /// Every colour with green in it less or more colourful.
    #[setting(label = "Saturation", range(-100, 100), keyframe, pulse, group = "Green primary")]
    pub green_saturation: f32,
    /// The blue primary towards cyan (down) or purple (up).
    #[setting(label = "Hue", range(-100, 100), keyframe, pulse, group = "Blue primary")]
    pub blue_hue: f32,
    /// Every colour with blue in it less or more colourful.
    #[setting(label = "Saturation", range(-100, 100), keyframe, pulse, group = "Blue primary")]
    pub blue_saturation: f32,
}

impl CalibrationPanel {
    /// The panel as the frame carries it.
    pub fn gpu(&self) -> bpe_gfx::GradeCalibration {
        let n = |v: f32| if v.is_finite() { v } else { 0.0 };
        bpe_gfx::GradeCalibration {
            shadows_tint: n(self.shadows_tint),
            red_hue: n(self.red_hue),
            red_saturation: n(self.red_saturation),
            green_hue: n(self.green_hue),
            green_saturation: n(self.green_saturation),
            blue_hue: n(self.blue_hue),
            blue_saturation: n(self.blue_saturation),
        }
    }
}
