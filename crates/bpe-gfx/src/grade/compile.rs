//! The stack as the program the composite runs: a list of steps, each in the space it works in.
//!
//! Word 0 is the header (`[steps, split, 0, 0]`). Each step starts with `[op, space, words, extra]`
//! (`words` counting the header) and its data follows:
//! - [`OP_MATRIX`] (linear): three rows of a 3×3 matrix.
//! - [`OP_CURVE`] (tone): the slope at 0 per channel, then [`CURVE_SAMPLES`] offsets from the
//!   straight line per channel, sample `i` at `u = i / CURVE_SAMPLES`, `p = u / (1 − u)`.
//! - [`OP_CHROMA`] (Oklab): `[vibrance, saturation, 0, 0]` (−1..1).
//! - [`OP_BEGIN`] / [`OP_END`] (linear): a mask weaker than full; `extra` of the end is its amount.

use super::maths::{self, ToneCurves};
use super::*;
use crate::frame::{GradeBasic, GradeMask, GradePanel, GradeStack, GradeToneCurve};

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
