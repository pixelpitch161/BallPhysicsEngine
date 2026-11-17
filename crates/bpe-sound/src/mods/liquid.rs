//! The Liquid modulator (liquid part 5): how a liquid sloshes, how mixed it is, how high it stands
//! and how much of it there is, read once a block from the sound's world
//! (`bpe_kit::things::liquid::stats_of`, from the last feel) and smoothed. Shared by the whole
//! voice; its settings are read when the patch is built.

use super::{BLOCK_S, ModCx, Modulator, NoteCx};
use bpe_core::{Target, ThingId};
use bpe_dsp::lfo::smooth_coefficient;
use bpe_kit::sound::{LiquidModSettings, LiquidReads};
use bpe_kit::things::liquid::stats_of;
use bpe_runtime::World;
use std::sync::Arc;

/// A Liquid modulator's settings, ready for the audio path (its Liquid shared, so a copy
/// allocates nothing).
#[derive(Clone, Debug)]
pub struct LiquidParams {
    pub reads: LiquidReads,
    pub liquid: Arc<Target>,
    pub full_at: f32,
    pub smooth_ms: f32,
    pub invert: bool,
}

impl LiquidParams {
    pub fn of(s: &LiquidModSettings) -> Self {
        let num = |x: f32, or: f32| if x.is_finite() { x } else { or };
        LiquidParams {
            reads: s.reads,
            liquid: Arc::new(s.liquid.clone()),
            full_at: num(s.full_at, 200.0).max(1.0),
            smooth_ms: num(s.smooth, 50.0).max(0.0),
            invert: s.invert,
        }
    }
}

/// What the modulator reads now, 0..1 before smoothing: 0 with no liquid to read.
pub fn liquid_value(p: &LiquidParams, world: &World, me: ThingId) -> f32 {
    let Some(s) = stats_of(world, |t| world.names(&p.liquid, me, t)) else { return if p.invert { 1.0 } else { 0.0 } };
    let raw = match p.reads {
        LiquidReads::Slosh => s.speed / p.full_at,
        LiquidReads::Mixed => s.mixed,
        LiquidReads::Height => s.height,
        LiquidReads::Amount => s.amount,
    };
    let raw = if raw.is_finite() { raw.clamp(0.0, 1.0) } else { 0.0 };
    if p.invert { 1.0 - raw } else { raw }
}

/// One Liquid modulator.
#[derive(Clone, Debug)]
pub struct LiquidMod {
    p: LiquidParams,
    value: f32,
    fresh: bool,
}

impl LiquidMod {
    pub fn new(p: LiquidParams) -> Self {
        LiquidMod { p, value: 0.0, fresh: true }
    }

    /// Takes over another's value (a patch swap), so it carries on smoothly.
    pub fn take_over(&mut self, old: &LiquidMod) {
        self.value = old.value;
        self.fresh = old.fresh;
    }
}

impl Modulator for LiquidMod {
    fn value(&self) -> f32 {
        self.value
    }

    fn next_block(&mut self, cx: &ModCx) {
        let raw = liquid_value(&self.p, cx.world, cx.shape);
        if std::mem::take(&mut self.fresh) {
            self.value = raw;
        } else {
            let c = smooth_coefficient(self.p.smooth_ms as f64 / 1000.0, BLOCK_S);
            self.value += (raw - self.value) * c;
        }
    }

    fn note_on(&mut self, _cx: &NoteCx) {}

    fn reset(&mut self) {
        self.fresh = true;
        self.value = 0.0;
    }
}
