//! An oscillator's layer chain (Modulation, Filter, Distortion), run between its Gain and its
//! Level. Part 2 has no layers yet: the chain passes the sound through.

use crate::BLOCK;

/// One oscillator's chain in one note.
#[derive(Clone, Debug, Default)]
pub struct Chain {}

impl Chain {
    /// A chain with no layers.
    pub fn empty() -> Self {
        Chain {}
    }

    /// Whether it has no layers (and so changes nothing).
    pub fn is_empty(&self) -> bool {
        true
    }

    /// Runs one block in place.
    #[inline]
    pub fn process(&mut self, _l: &mut [f32; BLOCK], _r: &mut [f32; BLOCK]) {}

    /// Silences its layers' memories (a new note or a seek).
    pub fn reset(&mut self) {}
}
