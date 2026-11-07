//! Moments on the beat of the scene's tempo (particles part 3): when On the beat births happen
//! and when Pulse on the beat kicks. Everything is worked out from the particle clock, so a
//! moment falls on the same particle step however the world was reached.

use bpe_runtime::World;

use super::BeatEvery;
use crate::things::scene::sim::beat_of;

/// Moments `first + k × every` (k = 0, 1, 2…), in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moments {
    pub first: f64,
    pub every: f64,
}

/// Lets a moment that falls right on a step's end count in that step.
const EPS: f64 = 1e-9;

impl Moments {
    /// Every `every` of the tempo in `world` (`bars` for Bars), starting `offset` beats in.
    pub fn of(world: &World, every: BeatEvery, bars: u32, offset: f32) -> Moments {
        let b = beat_of(world);
        let beat = b.beat.max(1e-3);
        let gap = match every {
            BeatEvery::Beat => beat,
            BeatEvery::Bar => beat * b.per_bar.max(1) as f64,
            BeatEvery::Bars => beat * b.per_bar.max(1) as f64 * bars.max(1) as f64,
        };
        let offset = if offset.is_finite() { offset.max(0.0) as f64 } else { 0.0 };
        Moments { first: offset * beat, every: gap }
    }

    /// How many moments at or before `t`.
    fn up_to(&self, t: f64) -> i64 {
        if t + EPS < self.first {
            return 0;
        }
        ((t - self.first + EPS) / self.every).floor() as i64 + 1
    }

    /// How many moments fall in `(t0, t1]`.
    pub fn count_in(&self, t0: f64, t1: f64) -> u32 {
        (self.up_to(t1) - self.up_to(t0)).clamp(0, u32::MAX as i64) as u32
    }

    /// The moments at or before `t`: seconds since the latest, their gap and how many (`None`
    /// before the first).
    pub fn beats_at(&self, t: f64) -> Option<Beats> {
        let n = self.up_to(t);
        (n >= 1).then(|| Beats { since: t - (self.first + (n - 1) as f64 * self.every), every: self.every, count: n })
    }
}

/// The moments up to a time: `since` seconds since the latest, `every` seconds apart, `count`
/// of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beats {
    pub since: f64,
    pub every: f64,
    pub count: i64,
}
