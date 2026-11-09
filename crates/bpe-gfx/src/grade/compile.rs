//! The stack as the program the grade passes run: a list of steps, each in the space it works in,
//! cut into segments where an area step needs the pixels around it.
//!
//! Word 0 is the header (`[segments, split, 0, 0]`), then one word per segment (`[first word,
//! steps, needs the pyramid (0/1), 0]`). Each step starts with `[op, space, words, extra]`
//! (`words` counting the header) and its data follows:
//! - [`OP_MATRIX`] (linear): three rows of a 3×3 matrix.
//! - [`OP_CURVE`] (tone): the slope at 0 per channel, then [`CURVE_SAMPLES`] offsets from the
//!   straight line per channel, sample `i` at `u = i / CURVE_SAMPLES`, `p = u / (1 − u)`.
//! - [`OP_CHROMA`] (Oklab): `[vibrance, saturation, 0, 0]` (−1..1).
//! - [`OP_BEGIN`] / [`OP_END`] (linear): a mask weaker than full or over part of the picture;
//!   `extra` of the end is its amount. For a mask with an area the end's words are `[parts,
//!   invert, overlay, 0]` then each part `[kind, combine, invert, words]` and its words:
//!   [`PART_LINEAR`] `[x, y, angle, feather]` then copies; [`PART_RADIAL`] `[x, y, half width,
//!   half height]`, `[angle, roundness, feather, 0]` then copies; [`PART_COLOUR`] `[picks, inner,
//!   outer, 0]` then a pick a word (Oklab); [`PART_LUMINANCE`] `[low, high, fade, 0]`;
//!   [`PART_THINGS`] `[layer of the things field (−1 none), expand, feather, background]` (video
//!   pixels); [`PART_BRUSH`] `[x0, y0, 1 / texel, 0]`, `[width, height, grid word, 0]`, copies,
//!   then the grid (three bytes a lane, see `brush::BrushGrid::words`). Copies are `[count,
//!   reach, 0, 0]` then `[at x, at y, turn, scale]` each, sorted by `at y`.
//! - [`OP_HUE`] (Oklab): the HSL panel, [`HUE_SAMPLES`] samples over the hue of `[turn, saturation,
//!   luminance, 0]` (see `maths::hsl_row`).
//! - [`OP_POINT`] (Oklab): Point Colour; `extra` is the number of picks, three words each (see
//!   `maths::PickWords`).
//! - [`OP_WHEELS`] (Oklab): Colour Grading, five words (see `maths::WheelsWords`).
//! - [`OP_DARK_TINT`] (linear): Calibration's Shadows Tint, the gain on the darks.
//! - Area steps, each first in its segment or after other area steps:
//!   [`OP_SHARPEN`] (tone) `[gain, limit, σ, 0]`; [`OP_TEXTURE`] (tone) `[gain, limit, σ fine,
//!   σ coarse]`; [`OP_CLARITY`] (tone) `[gain, limit, σ, 0]`; [`OP_DEHAZE`] (linear) `[amount, σ, 0,
//!   0]`; σ in pixels of the video.
//! - [`OP_VIGNETTE`] (linear): `[amount, middle, width, roundness]`, `[highlights, σ, 0, 0]`; an
//!   area step when its Highlights work.
//! - [`OP_GRAIN`] (linear): `[amount, cell, roughness, 0]`.
//! - [`OP_GRADIENT_MAP`] (tone): `extra` is the amount, then [`MAP_SAMPLES`] colours (tone space).

use super::maths::{self, ToneCurves};
use super::*;
use crate::frame::{
    GradeArea, GradeBasic, GradeCalibration, GradeColourGrading, GradeCombine, GradeDetail, GradeEffects, GradeHsl,
    GradeMask, GradePanel, GradePlace, GradePointColour, GradeShape, GradeStack, GradeThings, GradeToneCurve,
};

/// The compiled grade.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub words: Vec<[f32; 4]>,
    pub steps: u32,
    /// What each pass runs, in order: one for a grade without area steps.
    pub segments: Vec<Segment>,
    /// The choices of things its Things parts read silhouettes of, in the things field's
    /// layers' order (none without Things parts: no silhouettes are drawn).
    pub selections: Vec<Selection>,
}

/// One choice of things the renderer draws a silhouette of (a layer of the things field).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    /// Their codes, sorted (`None`: every thing).
    pub things: Option<Vec<u32>>,
    /// The furthest past their edges any part on it reaches, in pixels of the video: how far its
    /// distances are needed.
    pub reach: f32,
}

/// One pass's share of the program.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Segment {
    /// The word its first step starts at.
    pub first: u32,
    pub steps: u32,
    /// Its area steps read the blur pyramid of the pass's input.
    pub pyramid: bool,
}

/// What kind of step: area steps start a segment (sharing it with area steps straight before).
#[derive(Clone, Copy, PartialEq)]
enum Reach {
    Point,
    /// An area step; `true` when it reads the pyramid.
    Area(bool),
}

/// The program as it is built: steps in `body`, segments starting at body words.
struct Builder {
    body: Vec<[f32; 4]>,
    steps: u32,
    segments: Vec<Segment>,
    after_area: bool,
    selections: Vec<Selection>,
}

impl Builder {
    fn step(&mut self, op: u32, space: u32, extra: f32, data: impl IntoIterator<Item = [f32; 4]>) {
        self.step_reaching(Reach::Point, op, space, extra, data);
    }

    fn step_reaching(
        &mut self,
        reach: Reach,
        op: u32,
        space: u32,
        extra: f32,
        data: impl IntoIterator<Item = [f32; 4]>,
    ) {
        if let Reach::Area(pyramid) = reach {
            if !self.after_area {
                self.segments.push(Segment { first: self.body.len() as u32, steps: 0, pyramid: false });
            }
            self.segments.last_mut().expect("one at least").pyramid |= pyramid;
        }
        self.after_area = reach != Reach::Point;
        let at = self.body.len();
        self.body.push([op as f32, space as f32, 0.0, extra]);
        self.body.extend(data);
        self.body[at][2] = (self.body.len() - at) as f32;
        self.steps += 1;
        self.segments.last_mut().expect("one at least").steps += 1;
    }

    /// The tone curve `f` (tone value to tone value, per channel) as a step.
    fn curve(&mut self, f: impl Fn(f64) -> [f64; 3]) {
        let at0 = f(0.0);
        let near = f(SLOPE_STEP);
        let slope = [0, 1, 2].map(|c| ((near[c] - at0[c]) / SLOPE_STEP) as f32);
        let samples = (0..CURVE_SAMPLES).map(|i| {
            let u = i as f64 / CURVE_SAMPLES as f64;
            let p = u / (1.0 - u);
            let y = f(p);
            [(y[0] - p) as f32, (y[1] - p) as f32, (y[2] - p) as f32, 0.0]
        });
        let data: Vec<[f32; 4]> = std::iter::once([slope[0], slope[1], slope[2], 0.0]).chain(samples).collect();
        self.step(OP_CURVE, SPACE_TONE, 0.0, data);
    }

    fn basic(&mut self, b: &GradeBasic) {
        if !b.linear_identity() {
            let m = maths::basic_matrix(b);
            let rows = m.map(|r| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0]);
            self.step(OP_MATRIX, SPACE_LINEAR, 0.0, rows);
        }
        if !b.tone_identity() {
            self.curve(|p| [maths::basic_tone(p, b); 3]);
        }
        // Presence's area sliders, together so they share one pass and one pyramid.
        let unit = |v: f32| if v.is_finite() { (v / 100.0).clamp(-1.0, 1.0) } else { 0.0 };
        let (texture, clarity, dehaze) = (unit(b.texture), unit(b.clarity), unit(b.dehaze));
        if texture != 0.0 {
            let data = [[1.2 * texture, 0.1, TEXTURE_FINE, TEXTURE_COARSE]];
            self.step_reaching(Reach::Area(true), OP_TEXTURE, SPACE_TONE, 0.0, data);
        }
        if clarity != 0.0 {
            let data = [[0.9 * clarity, 0.15, CLARITY_BLUR, 0.0]];
            self.step_reaching(Reach::Area(true), OP_CLARITY, SPACE_TONE, 0.0, data);
        }
        if dehaze != 0.0 {
            self.step_reaching(Reach::Area(true), OP_DEHAZE, SPACE_LINEAR, 0.0, [[dehaze, DEHAZE_BLUR, 0.0, 0.0]]);
        }
        if !b.colour_identity() {
            let (v, s) = ((b.vibrance / 100.0).clamp(-1.0, 1.0), (b.saturation / 100.0).clamp(-1.0, 1.0));
            self.step(OP_CHROMA, SPACE_OKLAB, 0.0, [[v, s, 0.0, 0.0]]);
        }
    }

    fn tone_curve(&mut self, t: &GradeToneCurve) {
        if t.is_identity() {
            return;
        }
        let curves = ToneCurves::new(t);
        self.curve(|p| curves.at(p));
    }

    fn hsl(&mut self, h: &GradeHsl) {
        if h.is_identity() {
            return;
        }
        let row = maths::hsl_row(h).into_iter().map(|w| w.map(|v| v as f32));
        self.step(OP_HUE, SPACE_OKLAB, 0.0, row);
    }

    fn point_colour(&mut self, p: &GradePointColour) {
        let picks: Vec<maths::PickWords> = p.picks.iter().filter_map(maths::pick_words).collect();
        if picks.is_empty() {
            return;
        }
        let f = |v: [f64; 3]| [v[0] as f32, v[1] as f32, v[2] as f32, 0.0];
        let data: Vec<[f32; 4]> = picks.iter().flat_map(|p| [f(p.lab), f(p.shift), f(p.inv)]).collect();
        self.step(OP_POINT, SPACE_OKLAB, picks.len() as f32, data);
    }

    fn colour_grading(&mut self, g: &GradeColourGrading) {
        if g.is_identity() {
            return;
        }
        let w = maths::wheels_words(g);
        let f = |v: [f64; 4]| v.map(|x| x as f32);
        let data: Vec<[f32; 4]> = std::iter::once(f(w.head)).chain(w.regions.map(f)).collect();
        self.step(OP_WHEELS, SPACE_OKLAB, 0.0, data);
    }

    fn calibration(&mut self, c: &GradeCalibration) {
        if !c.primaries_identity() {
            let m = maths::calibration_matrix(c);
            let rows = m.map(|r| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0]);
            self.step(OP_MATRIX, SPACE_LINEAR, 0.0, rows);
        }
        if c.shadows_tint != 0.0 && c.shadows_tint.is_finite() {
            let g = maths::dark_tint_gain(c.shadows_tint);
            self.step(OP_DARK_TINT, SPACE_LINEAR, 0.0, [[g[0] as f32, g[1] as f32, g[2] as f32, 0.0]]);
        }
    }

    fn detail(&mut self, d: &GradeDetail) {
        let ok = |v: f32, lo: f32, hi: f32, or: f32| if v.is_finite() { v.clamp(lo, hi) } else { or };
        let amount = ok(d.amount, 0.0, 150.0, 0.0) / 100.0;
        if amount == 0.0 {
            return;
        }
        let limit = maths::sharpen_limit(ok(d.detail, 0.0, 100.0, 25.0) as f64) as f32;
        let data = [[amount, limit, ok(d.radius, 0.5, 3.0, 1.0), 0.0]];
        self.step_reaching(Reach::Area(false), OP_SHARPEN, SPACE_TONE, 0.0, data);
    }

    fn effects(&mut self, e: &GradeEffects) {
        let unit = |v: f32| if v.is_finite() { (v / 100.0).clamp(0.0, 1.0) } else { 0.0 };
        let map = unit(e.map_amount);
        if map > 0.0 {
            let row = maths::gradient_row(e).into_iter().map(|c| [c[0] as f32, c[1] as f32, c[2] as f32, 0.0]);
            self.step(OP_GRADIENT_MAP, SPACE_TONE, map, row);
        }
        let w = maths::vignette_words(e).map(|v| v as f32);
        if w[0] != 0.0 {
            let highlights = unit(e.vignette_highlights);
            // Highlights only work under a darkening vignette, and need the brightness round a pixel.
            let reach = if highlights > 0.0 && w[0] < 0.0 { Reach::Area(true) } else { Reach::Point };
            self.step_reaching(reach, OP_VIGNETTE, SPACE_LINEAR, 0.0, [w, [highlights, VIGNETTE_BLUR, 0.0, 0.0]]);
        }
        let grain = unit(e.grain_amount);
        if grain > 0.0 {
            let cell = 1.0 + 3.0 * unit(e.grain_size);
            self.step(OP_GRAIN, SPACE_LINEAR, 0.0, [[grain, cell, unit(e.grain_roughness), 0.0]]);
        }
    }

    fn mask(&mut self, m: &GradeMask) {
        let amount = if m.amount.is_finite() { m.amount.clamp(0.0, 1.0) } else { 0.0 };
        let works = amount > 0.0 && !m.panels.iter().all(GradePanel::is_identity);
        let overlay = m.overlay && m.area.is_some();
        if !works && !overlay {
            return;
        }
        // A mask over part of the picture always mixes back by its coverage; the Whole picture
        // only when it is weaker than full.
        let weak = amount < 1.0 || m.area.is_some();
        if weak {
            self.step(OP_BEGIN, SPACE_LINEAR, 0.0, []);
        }
        let panels: &[GradePanel] = if works { &m.panels } else { &[] };
        for panel in panels {
            match panel {
                GradePanel::Basic(b) => self.basic(b),
                GradePanel::ToneCurve(t) => self.tone_curve(t),
                GradePanel::Hsl(h) => self.hsl(h),
                GradePanel::PointColour(p) => self.point_colour(p),
                GradePanel::ColourGrading(g) => self.colour_grading(g),
                GradePanel::Detail(d) => self.detail(d),
                GradePanel::Effects(e) => self.effects(e),
                GradePanel::Calibration(c) => self.calibration(c),
            }
        }
        if weak {
            let words = m.area.as_ref().map(|a| self.cover_words(a, overlay)).unwrap_or_default();
            self.step(OP_END, SPACE_LINEAR, amount, words);
        }
    }

    /// The things field's layer for a Things part's choice (made the first time it is asked
    /// for), `None` past [`MAX_SELECTIONS`] or for an empty choice.
    fn selection(&mut self, t: &GradeThings) -> Option<usize> {
        let mut things = t.things.clone();
        if let Some(list) = things.as_mut() {
            list.sort_unstable();
            list.dedup();
            if list.is_empty() {
                return None;
            }
        }
        let reach = t.reach();
        if let Some(i) = self.selections.iter().position(|s| s.things == things) {
            self.selections[i].reach = self.selections[i].reach.max(reach);
            return Some(i);
        }
        if self.selections.len() >= MAX_SELECTIONS {
            return None;
        }
        self.selections.push(Selection { things, reach });
        Some(self.selections.len() - 1)
    }

    /// `OP_END`'s words for a mask's area: `[parts, invert, overlay, 0]`, then each part `[kind,
    /// combine, invert, words]` and its own words (see the module's notes).
    fn cover_words(&mut self, area: &GradeArea, overlay: bool) -> Vec<[f32; 4]> {
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        let mut out = vec![[area.parts.len() as f32, flag(area.invert), flag(overlay), 0.0]];
        for part in &area.parts {
            let (kind, data) = match &part.shape {
                GradeShape::Things(t) => {
                    let layer = self.selection(t).map_or(-1.0, |i| i as f32);
                    let ok = |v: f32| if v.is_finite() { v.clamp(0.0, crate::MOST_REACH) } else { 0.0 };
                    (PART_THINGS, vec![[layer, ok(t.expand), ok(t.feather), flag(t.background)]])
                }
                shape => part_words(shape),
            };
            let combine = match part.combine {
                GradeCombine::Add => COMBINE_ADD,
                GradeCombine::Subtract => COMBINE_SUBTRACT,
                GradeCombine::Intersect => COMBINE_INTERSECT,
            };
            out.push([kind as f32, combine as f32, flag(part.invert), (data.len() + 1) as f32]);
            out.extend(data);
        }
        out
    }
}

fn f4(v: [f64; 4]) -> [f32; 4] {
    v.map(|x| x as f32)
}

/// A part's kind and words.
fn part_words(shape: &GradeShape) -> (u32, Vec<[f32; 4]>) {
    match shape {
        GradeShape::Linear(l) => {
            let n = maths::linear_numbers(l);
            let mut data = vec![f4(n)];
            data.extend(copies(&l.places, -1.0));
            (PART_LINEAR, data)
        }
        GradeShape::Radial(r) => {
            let n = maths::radial_numbers(r);
            let [[x, y, hw, hh], _] = n;
            let reach = (x.hypot(y) + hw.hypot(hh)) as f32;
            let mut data = vec![f4(n[0]), f4(n[1])];
            data.extend(copies(&r.places, reach));
            (PART_RADIAL, data)
        }
        GradeShape::ColourRange(r) => {
            let picks = maths::range_picks(r);
            let [inner, outer] = maths::range_widths(r.refine);
            let mut data = vec![[picks.len() as f32, inner as f32, outer as f32, 0.0]];
            data.extend(picks.iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32, 0.0]));
            (PART_COLOUR, data)
        }
        GradeShape::LuminanceRange(l) => {
            let [low, high, fade] = maths::luminance_numbers(l);
            (PART_LUMINANCE, vec![[low as f32, high as f32, fade as f32, 0.0]])
        }
        GradeShape::Brush(b) => match brush::grid(&b.strokes) {
            Some(g) => {
                let copies = copies(&b.places, g.reach());
                let grid_at = 2 + copies.len();
                let mut data = vec![[g.origin[0], g.origin[1], 1.0 / g.texel, 0.0]];
                data.push([g.size[0] as f32, g.size[1] as f32, grid_at as f32, 0.0]);
                data.extend(copies);
                data.extend(g.words());
                (PART_BRUSH, data)
            }
            // No paint: no copies, so it covers nothing.
            None => (PART_BRUSH, vec![[0.0; 4], [0.0; 4], [0.0; 4]]),
        },
        GradeShape::Things(_) => unreachable!("worked out by the builder"),
    }
}

/// A gradient's copies: `[count, reach, 0, 0]` then `[at x, at y, turn, scale]` each, sorted down
/// the screen so the shader finds those near a pixel's row. `reach` (−1: everywhere) is how far
/// a copy's shape gets from its place at scale 1, and the word holds it at the largest scale.
fn copies(places: &[GradePlace], reach: f32) -> Vec<[f32; 4]> {
    let mut ok: Vec<&GradePlace> = places.iter().filter(|p| maths::place_ok(p)).take(MAX_BALLS).collect();
    ok.sort_by(|a, b| a.at[1].total_cmp(&b.at[1]));
    let biggest = ok.iter().map(|p| p.scale).fold(0.0f32, f32::max);
    let reach = if reach < 0.0 { -1.0 } else { reach * biggest + 2.0 };
    std::iter::once([ok.len() as f32, reach, 0.0, 0.0])
        .chain(ok.iter().map(|p| [p.at[0], p.at[1], p.turn, p.scale]))
        .collect()
}

/// The step of the slope at 0 worked out from.
const SLOPE_STEP: f64 = 1e-4;

impl Program {
    /// Whether the program does nothing (the composite then skips the grade).
    pub fn is_empty(&self) -> bool {
        self.steps == 0
    }
}

/// The program for `stack`: its masks in order, panels at their defaults left out.
pub fn compile(stack: &GradeStack) -> Program {
    let mut b = Builder {
        body: Vec::new(),
        steps: 0,
        segments: vec![Segment::default()],
        after_area: false,
        selections: Vec::new(),
    };
    for m in &stack.masks {
        b.mask(m);
    }
    let split = stack.split.filter(|s| s.is_finite()).map_or(0.0, |s| s.clamp(0.0, 1.0));
    let start = 1 + b.segments.len() as u32;
    let segments: Vec<Segment> = b.segments.iter().map(|s| Segment { first: s.first + start, ..*s }).collect();
    let mut words = Vec::with_capacity(start as usize + b.body.len());
    words.push([segments.len() as f32, split, 0.0, 0.0]);
    words.extend(segments.iter().map(|s| [s.first as f32, s.steps as f32, if s.pyramid { 1.0 } else { 0.0 }, 0.0]));
    words.extend(b.body);
    Program { words, steps: b.steps, segments, selections: b.selections }
}
