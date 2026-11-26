//! Water mode's own pictures of the water (`docs/notes/water.md`, part C3).
//!
//! [`plain`]: the plain drawing on the card, straight from a body's drops ([`crate::DropBuffers`]),
//! never read back: a flat back, the rock from the solid field, and the liquid as a smooth body with
//! a crisp outline, darker with depth over its Clarity, with a thin lighter rim along its surface.
//! [`stored`]: drops that are not a body's (a list of [`crate::Drop`]s) put on the card in a body's
//! layout, so the drawing draws them too. [`sets`] (feature `test-scenes`): drops laid out by hand
//! for the drawing's tests, its pinned picture and the look check.

pub mod plain;
#[cfg(feature = "test-scenes")]
pub mod sets;
pub mod stored;

pub use plain::{Plain, PlainGpu, PlainLiquid, PlainLook, View};
pub use stored::StoredDrops;
