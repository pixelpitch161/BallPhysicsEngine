//! The modulators at audio rate: each one gives a value 0..1 once a block, which the voice
//! spreads across the block and feeds to its routes. ADSR envelopes run sample by sample in
//! the voice itself; these are the others: [`LfoMod`], [`HitMod`], [`DistanceMod`] and [`LightMod`].
//!
//! Nothing here allocates once built.

pub mod distance;
pub mod hit;
pub mod lfo;
pub mod light;
pub mod order;

pub use distance::{DistanceMod, DistanceParams};
pub use hit::{HitMod, HitValues, hit_values};
pub use lfo::{LfoMod, LfoParams, lfo_rate_hz};
pub use light::{LightMod, LightParams, LightTable, light_value};
pub use order::mod_order;

use crate::TempoMap;
use bpe_core::ThingId;
use bpe_runtime::{LightAt, Log, World};

/// What a modulator reads each block.
pub struct ModCx<'a> {
    /// The block's first sample, from the start.
    pub start_sample: u64,
    pub tempo: &'a TempoMap,
    /// The physics at or a little past the block's end.
    pub world: &'a World,
    /// The shape whose sound it belongs to.
    pub shape: ThingId,
    /// The events up to the world's step.
    pub log: &'a Log,
    /// The scene's light probe, when the sound has a Light modulator and the scene a Lighting card.
    pub light: Option<&'a dyn LightAt>,
    /// The light on the balls, worked out ahead for the world's step (read before the probe).
    pub light_table: Option<&'a LightTable>,
}

/// A note starting (or, for a modulator shared by the whole shape, a hit on the shape).
pub struct NoteCx<'a> {
    /// The sample it starts on, from the start.
    pub sample: u64,
    /// The numbers its hit gives.
    pub hit: &'a HitValues,
    /// The ball whose hit it is (none for a key clicked in the editor).
    pub ball: Option<bpe_runtime::Party>,
}

/// A modulator that moves once a block.
pub trait Modulator {
    /// Its value at the end of the last block, 0..1.
    fn value(&self) -> f32;
    /// Moves on over the block starting at `cx.start_sample`.
    fn next_block(&mut self, cx: &ModCx);
    /// A note (or a hit on the shape) starts.
    fn note_on(&mut self, cx: &NoteCx);
    /// Starts over, as new (a seek).
    fn reset(&mut self);
}

/// `x` (0..1) bent by `curve` (-1..1): 0 is straight, above 0 bows up (rises early), below 0
/// bows down. The ends stay where they are.
#[inline]
pub fn bend(x: f32, curve: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let c = curve.clamp(-1.0, 1.0);
    if c.abs() < 1e-6 { x } else { x.powf(4f32.powf(-c)) }
}

/// Seconds in one block.
pub(crate) const BLOCK_S: f64 = crate::BLOCK as f64 / crate::SAMPLE_RATE as f64;
