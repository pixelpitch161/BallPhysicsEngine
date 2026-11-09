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
        self.masks.iter().all(|m| !(m.amount > 0.0) || m.panels.iter().all(GradePanel::is_identity))
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
}

impl GradePanel {
    /// Whether the panel changes nothing.
    pub fn is_identity(&self) -> bool {
        match self {
            GradePanel::Basic(b) => b.is_identity(),
            GradePanel::ToneCurve(t) => t.is_identity(),
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
    pub vibrance: f32,
    pub saturation: f32,
}

impl GradeBasic {
    /// Whether every number is 0 (a key passing through 0 may land on −0, which counts as 0).
    pub fn is_identity(&self) -> bool {
        self.linear_identity() && self.tone_identity() && self.colour_identity()
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
