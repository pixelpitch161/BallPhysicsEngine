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

pub use compile::{Program, Segment, compile};
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

/// Area steps (part 3): they read the pixels around a pixel, so each group of them starts a new
/// pass (see `compile.rs`).
pub const OP_SHARPEN: u32 = 10;
pub const OP_TEXTURE: u32 = 11;
pub const OP_CLARITY: u32 = 12;
pub const OP_DEHAZE: u32 = 13;
/// An area step only when its Highlights keep bright parts bright under a dark vignette.
pub const OP_VIGNETTE: u32 = 14;
/// Point steps (part 3).
pub const OP_GRAIN: u32 = 15;
pub const OP_GRADIENT_MAP: u32 = 16;

/// Samples in the gradient map's row (`shaders/grade.wgsl` reads the same number).
pub const MAP_SAMPLES: usize = 256;

/// Mips of the blur pyramid an area pass reads (half size first); fewer on a small picture.
pub const PYRAMID_LEVELS: u32 = 8;

/// The blurs area steps read, as a Gaussian's σ in pixels of the video (the scene's own size):
/// Texture's fine and coarse ends, Clarity's surround, Dehaze's surround and the vignette's
/// Highlights' surround.
pub const TEXTURE_FINE: f32 = 2.0;
pub const TEXTURE_COARSE: f32 = 8.0;
pub const CLARITY_BLUR: f32 = 40.0;
pub const DEHAZE_BLUR: f32 = 80.0;
pub const VIGNETTE_BLUR: f32 = 6.0;

/// A mask's part kinds and how parts join, as `OP_END`'s words name them (part 4; the same
/// numbers in `shaders/grade.wgsl`).
pub const PART_LINEAR: u32 = 1;
pub const PART_RADIAL: u32 = 2;
pub const PART_COLOUR: u32 = 3;
pub const PART_LUMINANCE: u32 = 4;
pub const COMBINE_ADD: u32 = 0;
pub const COMBINE_SUBTRACT: u32 = 1;
pub const COMBINE_INTERSECT: u32 = 2;

/// At most this many copies of one gradient (a copy a ball for every ball); the rest are left out.
pub const MAX_BALLS: usize = 4096;

/// The editor's mask overlay: this red (linear light) mixed in by half the coverage.
pub const OVERLAY_RED: [f32; 3] = [1.0, 0.02, 0.02];
pub const OVERLAY_MIX: f32 = 0.5;
