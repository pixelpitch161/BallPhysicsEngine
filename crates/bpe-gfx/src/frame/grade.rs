//! The picture's colour grade: the scene's Grade card as the frame carries it, every number
//! resolved at the frame's moment. `crate::grade` turns it into the steps the composite runs.

use bpe_core::CurvePoint;

/// The colour grade run over the linear HDR picture, after the effects, bloom and the Picture
/// card's Exposure and before the roll-off. Only the top frame's counts. With no masks, or every
/// panel at its defaults, it changes nothing and the composite skips it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradeStack {
    /// The Whole picture first, then the masks in the order they work.
    pub masks: Vec<GradeMask>,
    /// The editor's Before / After split: pixels left of this place across the picture (0..1) are
    /// left ungraded. Never set for an export.
    pub split: Option<f32>,
}

impl GradeStack {
    /// Whether the grade changes nothing: no panel of any mask does anything.
    pub fn is_identity(&self) -> bool {
        // A NaN amount counts as none.
        self.masks.iter().all(|m| m.amount.is_nan() || m.amount <= 0.0 || m.panels.iter().all(GradePanel::is_identity))
    }
}

/// One mask of the grade (part 1: only the Whole picture) and its panels in processing order.
#[derive(Clone, Debug, PartialEq)]
pub struct GradeMask {
    /// 0..1: how strongly its panels work.
    pub amount: f32,
    pub panels: Vec<GradePanel>,
}

impl Default for GradeMask {
    fn default() -> Self {
        Self { amount: 1.0, panels: Vec::new() }
    }
}

/// One panel of a mask, with its numbers in Lightroom's units.
#[derive(Clone, Debug, PartialEq)]
pub enum GradePanel {
    Basic(GradeBasic),
    ToneCurve(GradeToneCurve),
    Hsl(GradeHsl),
    PointColour(GradePointColour),
    ColourGrading(GradeColourGrading),
    Detail(GradeDetail),
    Effects(GradeEffects),
    Calibration(GradeCalibration),
}

impl GradePanel {
    /// Whether the panel changes nothing.
    pub fn is_identity(&self) -> bool {
        match self {
            GradePanel::Basic(b) => b.is_identity(),
            GradePanel::ToneCurve(t) => t.is_identity(),
            GradePanel::Hsl(h) => h.is_identity(),
            GradePanel::PointColour(p) => p.is_identity(),
            GradePanel::ColourGrading(c) => c.is_identity(),
            GradePanel::Detail(d) => d.is_identity(),
            GradePanel::Effects(e) => e.is_identity(),
            GradePanel::Calibration(c) => c.is_identity(),
        }
    }
}

/// The Basic panel: white balance, tone and presence. Every number is −100..100 except
/// Exposure, in stops (−5..5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradeBasic {
    pub temp: f32,
    pub tint: f32,
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    /// Medium-sized detail stronger or smoother (an area step).
    pub texture: f32,
    /// Local contrast in the midtones (an area step).
    pub clarity: f32,
    /// A veil of haze taken off or laid on (an area step).
    pub dehaze: f32,
    pub vibrance: f32,
    pub saturation: f32,
}

impl GradeBasic {
    /// Whether every number is 0 (a key passing through 0 may land on −0, which counts as 0).
    pub fn is_identity(&self) -> bool {
        self.linear_identity() && self.tone_identity() && self.area_identity() && self.colour_identity()
    }

    /// No Texture, Clarity or Dehaze.
    pub fn area_identity(&self) -> bool {
        self.texture == 0.0 && self.clarity == 0.0 && self.dehaze == 0.0
    }

    /// No white balance and no exposure.
    pub fn linear_identity(&self) -> bool {
        self.temp == 0.0 && self.tint == 0.0 && self.exposure == 0.0
    }

    /// No tone slider moved.
    pub fn tone_identity(&self) -> bool {
        [self.contrast, self.highlights, self.shadows, self.whites, self.blacks].iter().all(|v| *v == 0.0)
    }

    /// No Vibrance and no Saturation.
    pub fn colour_identity(&self) -> bool {
        self.vibrance == 0.0 && self.saturation == 0.0
    }
}

/// The Tone Curve panel: the parametric curve (four regions, −100..100, between three splits,
/// 0..100) then the point curves (all channels, then red, green and blue), both applied.
#[derive(Clone, Debug, PartialEq)]
pub struct GradeToneCurve {
    pub highlights: f32,
    pub lights: f32,
    pub darks: f32,
    pub shadows: f32,
    /// Shadows / Darks, Darks / Lights, Lights / Highlights, 0..100.
    pub splits: [f32; 3],
    pub rgb: Vec<CurvePoint>,
    pub red: Vec<CurvePoint>,
    pub green: Vec<CurvePoint>,
    pub blue: Vec<CurvePoint>,
}

/// The straight line a point curve starts as.
pub fn straight_curve() -> Vec<CurvePoint> {
    vec![CurvePoint::new(0.0, 0.0, 0.0), CurvePoint::new(1.0, 1.0, 0.0)]
}

impl Default for GradeToneCurve {
    fn default() -> Self {
        Self {
            highlights: 0.0,
            lights: 0.0,
            darks: 0.0,
            shadows: 0.0,
            splits: [25.0, 50.0, 75.0],
            rgb: straight_curve(),
            red: straight_curve(),
            green: straight_curve(),
            blue: straight_curve(),
        }
    }
}

impl GradeToneCurve {
    /// Whether the parametric regions are all 0 and every point curve is the straight line (its
    /// points all on it: a point added on the line changes nothing).
    pub fn is_identity(&self) -> bool {
        let on_line = |c: &[CurvePoint]| c.iter().all(|p| p.x == p.y);
        [self.highlights, self.lights, self.darks, self.shadows].iter().all(|v| *v == 0.0)
            && [&self.rgb, &self.red, &self.green, &self.blue].iter().all(|c| on_line(c))
    }
}

/// The HSL / Colour panel: hue, saturation and luminance for Lightroom's eight bands (red,
/// orange, yellow, green, aqua, blue, purple, magenta), each −100..100.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradeHsl {
    pub hue: [f32; 8],
    pub saturation: [f32; 8],
    pub luminance: [f32; 8],
}

impl GradeHsl {
    /// Whether every slider is 0.
    pub fn is_identity(&self) -> bool {
        [self.hue, self.saturation, self.luminance].iter().flatten().all(|v| *v == 0.0)
    }
}

/// The Point Colour panel: the used picks, in slot order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradePointColour {
    pub picks: Vec<GradePick>,
}

impl GradePointColour {
    /// Whether no pick moves anything.
    pub fn is_identity(&self) -> bool {
        self.picks.iter().all(GradePick::is_identity)
    }
}

/// One picked colour of Point Colour and how the colours near it change.
#[derive(Clone, Debug, PartialEq)]
pub struct GradePick {
    /// The picked colour, in linear light.
    pub colour: [f32; 3],
    /// Hue, Saturation and Luminance shifts, −100..100.
    pub hue: f32,
    pub saturation: f32,
    pub luminance: f32,
    /// How far from the pick a colour may be and still be moved, 0..100.
    pub range: f32,
}

impl Default for GradePick {
    fn default() -> Self {
        Self { colour: [1.0, 0.0, 0.0], hue: 0.0, saturation: 0.0, luminance: 0.0, range: 50.0 }
    }
}

impl GradePick {
    /// Whether it shifts nothing.
    pub fn is_identity(&self) -> bool {
        self.hue == 0.0 && self.saturation == 0.0 && self.luminance == 0.0
    }
}

/// One wheel of Colour Grading: the tint's hue (0..360°, Lightroom's: 0 red, 120 green, 240
/// blue), its strength (0..100) and the region's luminance (−100..100).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GradeWheel {
    pub hue: f32,
    pub saturation: f32,
    pub luminance: f32,
}

impl GradeWheel {
    /// No tint and no luminance (the hue alone does nothing).
    pub fn is_identity(&self) -> bool {
        self.saturation == 0.0 && self.luminance == 0.0
    }
}

/// The Colour Grading panel: three tone wheels and a global one, how much the regions overlap
/// (Blending 0..100) and where shadows give way to highlights (Balance −100..100).
#[derive(Clone, Debug, PartialEq)]
pub struct GradeColourGrading {
    pub shadows: GradeWheel,
    pub midtones: GradeWheel,
    pub highlights: GradeWheel,
    pub global: GradeWheel,
    pub blending: f32,
    pub balance: f32,
}

impl Default for GradeColourGrading {
    fn default() -> Self {
        Self {
            shadows: GradeWheel::default(),
            midtones: GradeWheel::default(),
            highlights: GradeWheel::default(),
            global: GradeWheel::default(),
            blending: 50.0,
            balance: 0.0,
        }
    }
}

impl GradeColourGrading {
    /// Whether no wheel tints or moves luminance (Blending and Balance alone do nothing).
    pub fn is_identity(&self) -> bool {
        [self.shadows, self.midtones, self.highlights, self.global].iter().all(GradeWheel::is_identity)
    }
}

/// The Calibration panel: Shadows Tint and the red, green and blue primaries' hue and
/// saturation, each −100..100.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradeCalibration {
    pub shadows_tint: f32,
    pub red_hue: f32,
    pub red_saturation: f32,
    pub green_hue: f32,
    pub green_saturation: f32,
    pub blue_hue: f32,
    pub blue_saturation: f32,
}

impl GradeCalibration {
    /// Whether no primary moves.
    pub fn primaries_identity(&self) -> bool {
        [self.red_hue, self.red_saturation, self.green_hue, self.green_saturation, self.blue_hue, self.blue_saturation]
            .iter()
            .all(|v| *v == 0.0)
    }

    /// Whether every slider is 0.
    pub fn is_identity(&self) -> bool {
        self.shadows_tint == 0.0 && self.primaries_identity()
    }
}

/// The Detail panel: Sharpening's Amount (0..150), Radius (0.5..3 pixels of the video) and
/// Detail (0..100, how much of the finest detail and its halos are let through).
#[derive(Clone, Debug, PartialEq)]
pub struct GradeDetail {
    pub amount: f32,
    pub radius: f32,
    pub detail: f32,
}

impl Default for GradeDetail {
    fn default() -> Self {
        Self { amount: 0.0, radius: 1.0, detail: 25.0 }
    }
}

impl GradeDetail {
    /// No sharpening.
    pub fn is_identity(&self) -> bool {
        self.amount == 0.0
    }
}

/// The Effects panel: the post-crop vignette, grain that moves from frame to frame, and the
/// gradient map (the picture's brightness recoloured).
#[derive(Clone, Debug, PartialEq)]
pub struct GradeEffects {
    /// −100 (dark corners) .. 100 (light corners).
    pub vignette_amount: f32,
    /// 0..100: how far out it starts.
    pub vignette_midpoint: f32,
    /// −100 (a rounder rectangle) .. 100 (a circle).
    pub vignette_roundness: f32,
    /// 0..100: how soft its edge is.
    pub vignette_feather: f32,
    /// 0..100: how much bright things keep their brightness under a dark vignette.
    pub vignette_highlights: f32,
    /// 0..100.
    pub grain_amount: f32,
    /// 0..100: how big a grain is.
    pub grain_size: f32,
    /// 0..100: how uneven the grain is.
    pub grain_roughness: f32,
    /// The colours the picture's brightness is mapped to, dark to light.
    pub map: bpe_core::Gradient,
    /// 0..100: how much of the mapped colour replaces the picture's own.
    pub map_amount: f32,
}

impl Default for GradeEffects {
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
            map: bpe_core::Gradient::even(&[bpe_core::Color::rgb(0, 0, 0), bpe_core::Color::WHITE]),
            map_amount: 0.0,
        }
    }
}

impl GradeEffects {
    /// No vignette, grain or map.
    pub fn is_identity(&self) -> bool {
        self.vignette_amount == 0.0 && self.grain_amount == 0.0 && self.map_amount == 0.0
    }
}
