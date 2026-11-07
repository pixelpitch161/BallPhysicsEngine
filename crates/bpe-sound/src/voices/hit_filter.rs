//! Which hits on a shape play a note: hard enough, not too soon after the same ball's last one,
//! and from the balls the shape listens to.

use crate::BlockHit;
use bpe_core::{Target, ThingId};
use bpe_runtime::World;

/// A hit at this speed (px/s) or more is full strength.
pub const STRONG_HIT: f32 = 1500.0;
/// Balls the retrigger guard remembers at once.
const GUARD_SLOTS: usize = 2048;
/// How far a ball's slot may be from where it hashes to.
const PROBE: usize = 8;

/// How hard a hit at `speed` px/s is, 0..1.
pub fn strength(speed: f32) -> f32 {
    if speed.is_finite() { (speed.max(0.0) / STRONG_HIT).min(1.0) } else { 0.0 }
}

/// A shape's hit filter.
#[derive(Clone, Debug)]
pub struct HitFilter {
    pub shape: ThingId,
    /// 0..1: hits softer than this play nothing.
    pub min_strength: f32,
    /// Samples a ball must wait before it plays again.
    pub retrigger: u64,
    pub heard_from: Target,
    /// Each ball's key (`thing << 32 | item`, plus one; 0 for empty) and its last played sample.
    guard: Vec<(u64, u64)>,
}

impl HitFilter {
    /// Allocates its table: build it off the audio path.
    pub fn new(shape: ThingId, min_strength: f32, retrigger_s: f32, heard_from: Target) -> Self {
        let mut f = HitFilter { shape, min_strength: 0.0, retrigger: 0, heard_from, guard: vec![(0, 0); GUARD_SLOTS] };
        f.set(min_strength, retrigger_s);
        f
    }

    /// New thresholds (keyframed knobs move them every block).
    pub fn set(&mut self, min_strength: f32, retrigger_s: f32) {
        self.min_strength = min_strength.clamp(0.0, 1.0);
        self.retrigger = (retrigger_s.max(0.0) * crate::SAMPLE_RATE) as u64;
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
        let s = strength(e.speed);
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
