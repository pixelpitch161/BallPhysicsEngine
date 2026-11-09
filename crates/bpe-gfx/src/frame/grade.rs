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
        self.masks.iter().all(|m| !m.does_something())
    }

    /// Places every gradient and brush that sits in the world (or follows something) where the
    /// moved Stage camera `view` shows it: back into the world through the flat camera it was
    /// placed through, then onto the screen through `view`. Its scale follows how big things show
    /// there, its turn the way its own x way shows; a place behind the camera is dropped. Places on
    /// the screen stay where they are.
    pub fn see_through(&mut self, view: &crate::StageView) {
        let flat = view.view;
        let zoom = if flat.zoom.is_finite() && flat.zoom > 0.0 { flat.zoom } else { 1.0 };
        let moved = |p: &GradePlace| -> Option<GradePlace> {
            if !p.world {
                return Some(*p);
            }
            let at = glam::Vec2::from(p.at);
            let world = flat.to_world(at);
            let seen = view.screen_of(world)?;
            let k = view.scale_at(world)?;
            let way = glam::Vec2::new(p.turn.cos(), p.turn.sin()) * 10.0;
            let ahead = view.screen_of(flat.to_world(at + way))? - seen;
            let turn = if ahead.length() > 1e-6 { ahead.y.atan2(ahead.x) } else { p.turn };
            let out = GradePlace { at: seen.into(), turn, scale: k * p.scale / zoom, world: true };
            crate::grade::maths::place_ok(&out).then_some(out)
        };
        for mask in &mut self.masks {
            let Some(area) = mask.area.as_mut() else { continue };
            for part in &mut area.parts {
                let places = match &mut part.shape {
                    GradeShape::Linear(l) => &mut l.places,
                    GradeShape::Radial(r) => &mut r.places,
                    GradeShape::Brush(b) => &mut b.places,
                    _ => continue,
                };
                *places = places.iter().filter_map(moved).collect();
            }
        }
    }
}

/// One mask of the grade (the Whole picture, or a mask over part of it) and its panels in
/// processing order.
#[derive(Clone, Debug, PartialEq)]
pub struct GradeMask {
    /// 0..1: how strongly its panels work.
    pub amount: f32,
    /// Where it works: `None` for the Whole picture (everywhere), else its parts.
    pub area: Option<GradeArea>,
    pub panels: Vec<GradePanel>,
    /// The editor's red overlay (O) shows where this mask covers. Never set for an export.
    pub overlay: bool,
    /// The mask's layer id on the Grade card, for the editor (overlay, handles).
    pub id: u32,
}

impl Default for GradeMask {
    fn default() -> Self {
        Self { amount: 1.0, area: None, panels: Vec::new(), overlay: false, id: 0 }
    }
}

impl GradeMask {
    /// Whether it changes the picture: its panels do something at an Amount over 0, or it shows
    /// its overlay. A NaN amount counts as none.
    pub fn does_something(&self) -> bool {
        let works = !self.amount.is_nan() && self.amount > 0.0 && !self.panels.iter().all(GradePanel::is_identity);
        works || (self.overlay && self.area.is_some())
    }
}

/// Where a mask works: its parts combined in order, then inverted when asked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradeArea {
    pub parts: Vec<GradePart>,
    pub invert: bool,
}

/// How a part joins the parts before it (Lightroom's mask components). The first part is always
/// taken as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GradeCombine {
    /// Where it covers, or the others do (the greater coverage).
    #[default]
    Add,
    /// Where the others cover and it doesn't.
    Subtract,
    /// Where both it and the others cover.
    Intersect,
}

/// One part of a mask.
#[derive(Clone, Debug, PartialEq)]
pub struct GradePart {
    pub combine: GradeCombine,
    /// Covers where it otherwise wouldn't.
    pub invert: bool,
    pub shape: GradeShape,
    /// The part's layer id on the Grade card, for the editor's handles.
    pub id: u32,
}

/// What a part covers.
#[derive(Clone, Debug, PartialEq)]
pub enum GradeShape {
    Linear(GradeLinear),
    Radial(GradeRadial),
    ColourRange(GradeColourRange),
    LuminanceRange(GradeLuminanceRange),
    Things(GradeThings),
    Brush(GradeBrush),
}

/// A Things part: exactly what chosen things draw (their bodies as seen, with soft edges),
/// grown by Expand and faded out over Feather, or the background where no thing draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradeThings {
    /// The chosen things' codes ([`crate::thing_code`] as whole numbers), sorted; `None` for every
    /// thing. An empty list covers nothing.
    pub things: Option<Vec<u32>>,
    /// Covers where no chosen thing does instead (with `things` `None`: the background).
    pub background: bool,
    /// How far out from the things it reaches at full strength, in pixels of the video.
    pub expand: f32,
    /// How far past that it fades out, in pixels of the video.
    pub feather: f32,
}

impl GradeThings {
    /// How far past a thing's edge it reaches, in pixels of the video.
    pub fn reach(&self) -> f32 {
        let ok = |v: f32| if v.is_finite() { v.clamp(0.0, MOST_REACH) } else { 0.0 };
        ok(self.expand) + ok(self.feather)
    }
}

/// The furthest a Things part's Expand or Feather reaches, in pixels of the video.
pub const MOST_REACH: f32 = 2000.0;

/// A Brush part: painted strokes, in its place's units (pixels of the video from its place).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradeBrush {
    pub strokes: bpe_core::Strokes,
    /// One copy at each place, as a gradient's.
    pub places: Vec<GradePlace>,
}

/// Where a gradient sits, in screen units (pixels of the video from the picture's middle, y down,
/// as `CameraView::to_screen`): its middle is `at + rotate((x, y) · scale, turn)`, its sizes are
/// times `scale` and its angle plus `turn`. On the screen it is [`GradePlace::SCREEN`]; in the
/// world, or on a thing or ball, the scene's draw works it out through the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradePlace {
    pub at: [f32; 2],
    /// Radians, clockwise on screen.
    pub turn: f32,
    pub scale: f32,
    /// Placed through the camera (in the world, or following something), so a moved Stage camera
    /// moves it too ([`GradeStack::see_through`]). The GPU never reads it.
    pub world: bool,
}

impl GradePlace {
    pub const SCREEN: GradePlace = GradePlace { at: [0.0, 0.0], turn: 0.0, scale: 1.0, world: false };

    /// Where the point `(x, y)` of a gradient placed here is on the screen.
    pub fn point(&self, x: f32, y: f32) -> [f32; 2] {
        let (s, c) = self.turn.sin_cos();
        let (x, y) = (x * self.scale, y * self.scale);
        [self.at[0] + x * c - y * s, self.at[1] + x * s + y * c]
    }
}

/// A Linear Gradient: full effect on one side of a line, none on the other, fading over Feather.
#[derive(Clone, Debug, PartialEq)]
pub struct GradeLinear {
    /// Its middle line's middle, in its place's units.
    pub x: f32,
    pub y: f32,
    /// Degrees clockwise from the right: the way the effect fades out.
    pub angle: f32,
    /// The distance between the full-effect line and the no-effect line.
    pub feather: f32,
    /// One copy at each place (one for the screen, the world, a thing or a ball; one a ball for
    /// every ball). None covers nothing.
    pub places: Vec<GradePlace>,
}

/// A Radial Gradient: an ellipse covered inside, fading out towards its edge.
#[derive(Clone, Debug, PartialEq)]
pub struct GradeRadial {
    pub x: f32,
    pub y: f32,
    /// The whole ellipse's width and height.
    pub width: f32,
    pub height: f32,
    /// Degrees clockwise.
    pub angle: f32,
    /// −100 (a rounder rectangle) .. 100 (a circle on the short side).
    pub roundness: f32,
    /// 0..100: how far in from the edge the fade starts.
    pub feather: f32,
    pub places: Vec<GradePlace>,
}

/// A Colour Range: the colours near up to five picks.
#[derive(Clone, Debug, PartialEq)]
pub struct GradeColourRange {
    /// The picked colours, in linear light.
    pub picks: Vec<[f32; 3]>,
    /// 0..100: how far from a pick a colour may be and still be covered.
    pub refine: f32,
}

/// A Luminance Range: the brightnesses between Low and High (0..100), fading out over Smoothness.
#[derive(Clone, Debug, PartialEq)]
pub struct GradeLuminanceRange {
    pub low: f32,
    pub high: f32,
    pub smoothness: f32,
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
