//! The Tone Curve panel: Lightroom's parametric curve (four regions between three splits) and
//! point curves (all channels, red, green, blue). Both apply, parametric first.

use bpe_core::{Choice, CurvePoint, Settings};

/// Which curve the panel shows to edit. Both always apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum CurveMode {
    /// Four sliders for the regions of brightness, with their splits.
    #[default]
    Parametric,
    /// Points dragged on the curve, for all channels or one.
    Point,
}

/// The Tone Curve panel's settings.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ToneCurvePanel {
    /// Which curve the panel shows (both apply).
    #[setting(label = "Curve")]
    pub mode: CurveMode,
    /// The brightest region up or down.
    #[setting(label = "Highlights", range(-100, 100), keyframe, pulse, when = "mode=parametric", group = "Region")]
    pub highlights: f32,
    /// The light region up or down.
    #[setting(label = "Lights", range(-100, 100), keyframe, pulse, when = "mode=parametric", group = "Region")]
    pub lights: f32,
    /// The dark region up or down.
    #[setting(label = "Darks", range(-100, 100), keyframe, pulse, when = "mode=parametric", group = "Region")]
    pub darks: f32,
    /// The darkest region up or down.
    #[setting(label = "Shadows", range(-100, 100), keyframe, pulse, when = "mode=parametric", group = "Region")]
    pub shadows: f32,
    /// Where the Shadows region ends and Darks begins.
    #[setting(label = "Shadows split", range(0, 100), keyframe, pulse, when = "mode=parametric", group = "Splits")]
    pub split_shadows: f32,
    /// Where Darks ends and Lights begins.
    #[setting(label = "Middle split", range(0, 100), keyframe, pulse, when = "mode=parametric", group = "Splits")]
    pub split_middle: f32,
    /// Where Lights ends and Highlights begins.
    #[setting(label = "Highlights split", range(0, 100), keyframe, pulse, when = "mode=parametric", group = "Splits")]
    pub split_highlights: f32,
    /// The curve for all channels.
    #[setting(label = "Point curve")]
    pub rgb: Vec<CurvePoint>,
    /// The red channel's own curve.
    #[setting(label = "Red curve")]
    pub red: Vec<CurvePoint>,
    /// The green channel's own curve.
    #[setting(label = "Green curve")]
    pub green: Vec<CurvePoint>,
    /// The blue channel's own curve.
    #[setting(label = "Blue curve")]
    pub blue: Vec<CurvePoint>,
}

impl Default for ToneCurvePanel {
    fn default() -> Self {
        Self {
            mode: CurveMode::Parametric,
            highlights: 0.0,
            lights: 0.0,
            darks: 0.0,
            shadows: 0.0,
            split_shadows: 25.0,
            split_middle: 50.0,
            split_highlights: 75.0,
            rgb: bpe_gfx::straight_curve(),
            red: bpe_gfx::straight_curve(),
            green: bpe_gfx::straight_curve(),
            blue: bpe_gfx::straight_curve(),
        }
    }
}

/// The point curves' keys, in the order the channel chips show them.
pub const CURVE_KEYS: [&str; 4] = ["rgb", "red", "green", "blue"];

impl ToneCurvePanel {
    /// The panel as the frame carries it.
    pub fn gpu(&self) -> bpe_gfx::GradeToneCurve {
        let n = |v: f32| if v.is_finite() { v } else { 0.0 };
        bpe_gfx::GradeToneCurve {
            highlights: n(self.highlights),
            lights: n(self.lights),
            darks: n(self.darks),
            shadows: n(self.shadows),
            splits: [n(self.split_shadows), n(self.split_middle), n(self.split_highlights)],
            rgb: self.rgb.clone(),
            red: self.red.clone(),
            green: self.green.clone(),
            blue: self.blue.clone(),
        }
    }
}
