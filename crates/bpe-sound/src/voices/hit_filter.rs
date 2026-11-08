//! Which hits on a shape play a note: hard enough, not too soon after the same ball's last one,
//! and from the balls the shape listens to.

use crate::BlockHit;
use bpe_core::{Target, ThingId};
use bpe_runtime::World;

/// A hit at this speed (px/s) or more is full strength, unless a sound's Full strength at says
/// otherwise.
pub const STRONG_HIT: f32 = 1500.0;
/// Balls the retrigger guard remembers at once.
const GUARD_SLOTS: usize = 2048;
/// How far a ball's slot may be from where it hashes to.
const PROBE: usize = 8;

/// How hard a hit at `speed` px/s is, 0..1, on the straight line to [`STRONG_HIT`].
pub fn strength(speed: f32) -> f32 {
    StrengthCurve::default().of(speed)
}

/// How a sound turns a hit's speed into its strength: full at `full_at` px/s, bent by `curve`
/// (-100..100, 0 straight; above 0 soft hits count for more).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrengthCurve {
    pub full_at: f32,
    pub curve: f32,
}

impl Default for StrengthCurve {
    fn default() -> Self {
        StrengthCurve { full_at: STRONG_HIT, curve: 0.0 }
    }
}

impl StrengthCurve {
    /// From the voice's Full strength at and Strength curve knobs.
    pub fn of_knobs(full_at: f32, curve: f32) -> Self {
        StrengthCurve { full_at, curve }
    }

    /// How hard a hit at `speed` px/s is, 0..1.
    pub fn of(&self, speed: f32) -> f32 {
        if !speed.is_finite() {
            return 0.0;
        }
        let full = if self.full_at.is_finite() { self.full_at.max(1.0) } else { STRONG_HIT };
        let x = (speed.max(0.0) / full).min(1.0);
        let curve = if self.curve.is_finite() { self.curve / 100.0 } else { 0.0 };
        crate::mods::bend(x, curve)
    }
}

/// A shape's hit filter.
#[derive(Clone, Debug)]
pub struct HitFilter {
    pub shape: ThingId,
    /// 0..1: hits softer than this play nothing.
    pub min_strength: f32,
    /// How a hit's speed becomes its strength.
    pub curve: StrengthCurve,
    /// Samples a ball must wait before it plays again.
    pub retrigger: u64,
    pub heard_from: Target,
    /// Each ball's key (`thing << 32 | item`, plus one; 0 for empty) and its last played sample.
    guard: Vec<(u64, u64)>,
}

impl HitFilter {
    /// Allocates its table: build it off the audio path.
    pub fn new(shape: ThingId, min_strength: f32, retrigger_s: f32, heard_from: Target) -> Self {
        let mut f = HitFilter {
            shape,
            min_strength: 0.0,
            curve: StrengthCurve::default(),
            retrigger: 0,
            heard_from,
            guard: vec![(0, 0); GUARD_SLOTS],
        };
        f.set(min_strength, retrigger_s, StrengthCurve::default());
        f
    }

    /// New thresholds and strength curve (keyframed knobs move them every block).
    pub fn set(&mut self, min_strength: f32, retrigger_s: f32, curve: StrengthCurve) {
        self.min_strength = min_strength.clamp(0.0, 1.0);
        self.retrigger = (retrigger_s.max(0.0) * crate::SAMPLE_RATE) as u64;
        self.curve = curve;
    }

    /// Forgets every ball (on a seek).
    pub fn reset(&mut self) {
        self.guard.iter_mut().for_each(|g| *g = (0, 0));
    }

    /// Takes over another filter's memory of the balls.
    pub fn take_memory(&mut self, other: &HitFilter) {
        if other.guard.len() == self.guard.len() {
            self.guard.copy_from_slice(&other.guard);
        }
    }

    /// The hit's strength if it plays a note on this shape at sound sample `sample`.
    pub fn accept(&mut self, hit: &BlockHit, sample: u64, world: &World) -> Option<f32> {
        let e = &hit.event;
        let b = e.b?;
        if b.thing != self.shape {
            return None;
        }
        let ball = e.a;
        // A Particles thing's moments are its own particles': no Heard from (particles part 7).
        let own = e.kind == bpe_runtime::EventKind::Moment && ball.thing == self.shape;
        if !own && !world.names(&self.heard_from, self.shape, ball.thing) {
            return None;
        }
        let s = self.curve.of(e.speed);
        if s < self.min_strength {
            return None;
        }
        let key = ((ball.thing.0 as u64) << 32 | ball.item as u64).wrapping_add(1);
        let home = (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 53) as usize % GUARD_SLOTS;
        let mut free = None;
        for k in 0..PROBE {
            let i = (home + k) % GUARD_SLOTS;
            let (held, last) = self.guard[i];
            if held == key {
                if self.retrigger > 0 && sample < last.saturating_add(self.retrigger) && sample >= last {
                    return None;
                }
                self.guard[i].1 = sample;
                return Some(s);
            }
            if held == 0 && free.is_none() {
                free = Some(i);
            }
        }
        // A new ball: its own slot, or (a crowded table) the home slot.
        self.guard[free.unwrap_or(home)] = (key, sample);
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_curve_is_the_straight_line() {
        for speed in [0.0, 1.0, 300.0, 749.9, 1500.0, 1e6, -5.0, f32::NAN, f32::INFINITY] {
            let straight = if speed.is_finite() { (speed.max(0.0) / STRONG_HIT).min(1.0) } else { 0.0 };
            assert_eq!(StrengthCurve::default().of(speed).to_bits(), straight.to_bits(), "{speed}");
        }
    }

    #[test]
    fn full_at_and_curve_reshape_the_strength() {
        let soon = StrengthCurve::of_knobs(500.0, 0.0);
        assert_eq!(soon.of(250.0), 0.5);
        assert_eq!(soon.of(600.0), 1.0, "full sooner");
        let soft = StrengthCurve::of_knobs(1500.0, 60.0);
        let hard = StrengthCurve::of_knobs(1500.0, -60.0);
        let mid = StrengthCurve::default().of(450.0);
        assert!(soft.of(450.0) > mid && hard.of(450.0) < mid, "a curve bends the middle");
        assert_eq!((soft.of(0.0), soft.of(1500.0)), (0.0, 1.0), "the ends stay put");
    }
}
