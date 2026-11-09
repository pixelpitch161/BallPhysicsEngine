//! The stack as the program the composite runs: a list of steps, each in the space it works in.
//!
//! Word 0 is the header (`[steps, split, 0, 0]`). Each step starts with `[op, space, words, extra]`
//! (`words` counting the header) and its data follows:
//! - [`OP_MATRIX`] (linear): three rows of a 3×3 matrix.
//! - [`OP_CURVE`] (tone): the slope at 0 per channel, then [`CURVE_SAMPLES`] offsets from the
//!   straight line per channel, sample `i` at `u = i / CURVE_SAMPLES`, `p = u / (1 − u)`.
//! - [`OP_CHROMA`] (Oklab): `[vibrance, saturation, 0, 0]` (−1..1).
//! - [`OP_BEGIN`] / [`OP_END`] (linear): a mask weaker than full; `extra` of the end is its amount.
//! - [`OP_HUE`] (Oklab): the HSL panel, [`HUE_SAMPLES`] samples over the hue of `[turn, saturation,
//!   luminance, 0]` (see `maths::hsl_row`).
//! - [`OP_POINT`] (Oklab): Point Colour; `extra` is the number of picks, three words each (see
//!   `maths::PickWords`).
//! - [`OP_WHEELS`] (Oklab): Colour Grading, five words (see `maths::WheelsWords`).
//! - [`OP_DARK_TINT`] (linear): Calibration's Shadows Tint, the gain on the darks.

use super::maths::{self, ToneCurves};
use super::*;
use crate::frame::{
    GradeBasic, GradeCalibration, GradeColourGrading, GradeHsl, GradeMask, GradePanel, GradePointColour, GradeStack,
    GradeToneCurve,
};

/// The compiled grade.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub words: Vec<[f32; 4]>,
    pub steps: u32,
}

impl Program {
    /// Whether the program does nothing (the composite then skips the grade).
    pub fn is_empty(&self) -> bool {
        self.steps == 0
    }

    fn step(&mut self, op: u32, space: u32, extra: f32, data: impl IntoIterator<Item = [f32; 4]>) {
        let at = self.words.len();
        self.words.push([op as f32, space as f32, 0.0, extra]);
        self.words.extend(data);
        self.words[at][2] = (self.words.len() - at) as f32;
        self.steps += 1;
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

    fn mask(&mut self, m: &GradeMask) {
        let amount = if m.amount.is_finite() { m.amount.clamp(0.0, 1.0) } else { 0.0 };
        if amount <= 0.0 || m.panels.iter().all(GradePanel::is_identity) {
            return;
        }
        let weak = amount < 1.0;
        if weak {
            self.step(OP_BEGIN, SPACE_LINEAR, 0.0, []);
        }
        for panel in &m.panels {
            match panel {
                GradePanel::Basic(b) => self.basic(b),
                GradePanel::ToneCurve(t) => self.tone_curve(t),
                GradePanel::Hsl(h) => self.hsl(h),
                GradePanel::PointColour(p) => self.point_colour(p),
                GradePanel::ColourGrading(g) => self.colour_grading(g),
                GradePanel::Calibration(c) => self.calibration(c),
            }
        }
        if weak {
            self.step(OP_END, SPACE_LINEAR, amount, []);
        }
    }
}

/// The step of the slope at 0 worked out from.
const SLOPE_STEP: f64 = 1e-4;

/// The program for `stack`: its masks in order, panels at their defaults left out.
pub fn compile(stack: &GradeStack) -> Program {
    let mut program = Program { words: vec![[0.0; 4]], steps: 0 };
    for m in &stack.masks {
        program.mask(m);
    }
    let split = stack.split.filter(|s| s.is_finite()).map_or(0.0, |s| s.clamp(0.0, 1.0));
    program.words[0] = [program.steps as f32, split, 0.0, 0.0];
    program
}
