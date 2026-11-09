//! The colour grade (colour grading part 1 onwards, `docs/notes/grading.md`): the frame's
//! [`GradeStack`](crate::GradeStack) turned into steps the composite runs, and the scopes.
//!
//! - `maths.rs`: the working spaces, white balance, the tone sliders and curves, on the CPU;
//! - `compile.rs`: the stack as a [`Program`] (the words the shader reads);
//! - `scopes.rs`: the [`Histogram`] of the finished picture, for the editor;
//! - the shader is `shaders/grade.wgsl`, run inside the composite (`shaders/post.wgsl`).

mod compile;
pub mod maths;
#[cfg(test)]
mod pictures_tests;
mod scopes;
#[cfg(test)]
mod tests;

pub use compile::{Program, compile};
pub use scopes::{Histogram, HistogramBins};

/// Samples in a curve's lookup row (`shaders/grade.wgsl` reads the same number).
pub const CURVE_SAMPLES: usize = 1024;

/// The working spaces a step names (as in `shaders/grade.wgsl`).
pub const SPACE_LINEAR: u32 = 0;
pub const SPACE_TONE: u32 = 1;
pub const SPACE_OKLAB: u32 = 2;

/// The kinds of step (as in `shaders/grade.wgsl`).
pub const OP_MATRIX: u32 = 1;
pub const OP_CURVE: u32 = 2;
pub const OP_CHROMA: u32 = 3;
pub const OP_BEGIN: u32 = 4;
pub const OP_END: u32 = 5;
pub const OP_HUE: u32 = 6;
pub const OP_POINT: u32 = 7;
pub const OP_WHEELS: u32 = 8;
pub const OP_DARK_TINT: u32 = 9;

/// Samples in the HSL panel's row over the Oklab hue, one a degree (`shaders/grade.wgsl` reads the same number).
pub const HUE_SAMPLES: usize = 360;
