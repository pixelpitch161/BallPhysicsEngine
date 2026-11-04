//! The Hit modulator: a number taken from the hit that played the note, held for the note.

use super::{BLOCK_S, ModCx, Modulator, NoteCx, bend};
use crate::voices::strength;
use bpe_core::ThingId;
use bpe_dsp::lfo::{random_at, smooth_coefficient};
use bpe_kit::sound::{HitPick, HitSettings};
use bpe_runtime::{Event, World};

/// Every number a hit gives, each 0..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitValues {
    /// How hard (speed over [`crate::voices::STRONG_HIT`]).
    pub strength: f32,
    /// Where on the edge, clockwise from the first corner.
    pub place: f32,
    /// Which side, first 0, last 1.
    pub side: f32,
    /// The ball's number in its group, first 0, last 1.
    pub number: f32,
    /// A random number for the hit.
    pub random: f32,
}

/// The numbers `hit` on shape `shape` gives (`seed` picks its random number).
pub fn hit_values(world: &World, shape: ThingId, hit: &Event, seed: u64) -> HitValues {
    let (place, side, sides) = bpe_kit::things::shape::place_on_edge(world, shape, hit.point).unwrap_or((0.0, 0, 1));
    let ball = hit.a;
    let number = bpe_kit::things::balls::sim::numbers(world, ball.thing)
        .filter(|n| n.len() > 1)
        .and_then(|n| n.get(ball.item as usize).map(|&k| k as f32 / (n.len() - 1) as f32))
        .unwrap_or(0.0);
    HitValues {
        strength: strength(hit.speed),
        place,
        side: if sides > 1 { side as f32 / (sides - 1) as f32 } else { 0.0 },
        number: number.clamp(0.0, 1.0),
        random: random_at(seed, 0),
    }
}

/// The number `pick` takes from `hv`, mapped from Low..High to 0..1 and bent by `curve` (-1..1).
pub fn hit_value(pick: HitPick, hv: &HitValues, low: f32, high: f32, curve: f32) -> f32 {
    let raw = match pick {
        HitPick::Strength => hv.strength,
        HitPick::Place => hv.place,
        HitPick::Side => hv.side,
        HitPick::BallNumber => hv.number,
        HitPick::Random => hv.random,
    };
    let x = if (high - low).abs() < 1e-6 { if raw >= low { 1.0 } else { 0.0 } } else { (raw - low) / (high - low) };
    bend(x, curve)
}

/// One note's Hit modulator.
#[derive(Clone, Debug)]
pub struct HitMod {
    pick: HitPick,
    low: f32,
    high: f32,
    curve: f32,
    smooth_s: f64,
    target: f32,
    value: f32,
    fresh: bool,
}

impl Default for HitMod {
    fn default() -> Self {
        HitMod {
            pick: HitPick::Strength,
            low: 0.0,
            high: 1.0,
            curve: 0.0,
            smooth_s: 0.0,
            target: 0.0,
            value: 0.0,
            fresh: true,
        }
    }
}

impl HitMod {
    pub fn set(&mut self, s: &HitSettings) {
        self.pick = s.pick;
        self.set_knobs(s.low, s.high, s.curve, s.smooth);
    }

    /// The knobs as they are now (keys and routes applied): Low, High, Curve (-100..100) and
    /// Smooth (ms).
    pub fn set_knobs(&mut self, low: f32, high: f32, curve: f32, smooth_ms: f32) {
        self.low = low;
        self.high = high;
        self.curve = curve / 100.0;
        self.smooth_s = smooth_ms.max(0.0) as f64 / 1000.0;
    }

    pub fn set_pick(&mut self, pick: HitPick) {
        self.pick = pick;
    }
}

impl Modulator for HitMod {
    fn value(&self) -> f32 {
        self.value
    }

    fn next_block(&mut self, _cx: &ModCx) {
        let c = smooth_coefficient(self.smooth_s, BLOCK_S);
        self.value += (self.target - self.value) * c;
    }

    fn note_on(&mut self, cx: &NoteCx) {
        self.target = hit_value(self.pick, cx.hit, self.low, self.high, self.curve);
        if std::mem::take(&mut self.fresh) {
            self.value = self.target;
        }
    }

    fn reset(&mut self) {
        self.fresh = true;
        self.value = 0.0;
        self.target = 0.0;
    }
}
