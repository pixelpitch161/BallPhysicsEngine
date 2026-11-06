//! What is left of a broken ball or thing while its Break look plays out (rules part 3, plan
//! `docs/superpowers/plans/rules/part3.md`). The physics never reads it: a broken thing stops
//! simulating at once and only its picture goes on, worked out by the kit's drawing from these
//! records at the moment drawn.

use std::sync::Arc;

use bpe_core::Params;
use glam::Vec2;

use crate::Wall;

/// How a break's look plays out: the look's settings (read by the kit's drawing, never by the
/// runtime) and how long it shows after the break, in seconds. 0 is gone at once (Vanish), and
/// nothing is kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BreakShow {
    pub look: Params,
    pub lasts: f64,
}

impl BreakShow {
    /// Whether the look shows at all after the break.
    pub fn shows(&self) -> bool {
        self.lasts.is_finite() && self.lasts > 0.0
    }
}

/// A ball as it was when it broke.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BallAt {
    pub position: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
}

/// What is left of one broken ball, or of a whole broken thing.
#[derive(Clone, Debug, PartialEq)]
pub struct Remains {
    /// The step it broke on, and that step's time.
    pub step: u64,
    pub time: f64,
    /// The hit, as the `Break` event has it.
    pub point: Vec2,
    pub normal: Vec2,
    pub strength: f32,
    /// Its look, shared by every ball of one break.
    pub show: Arc<BreakShow>,
    /// The world's gravity when it broke (its pieces fall by it).
    pub gravity: Vec2,
    /// A shape's wall as it stood.
    pub wall: Option<Wall>,
    /// A ball as it was.
    pub ball: Option<BallAt>,
}

impl Remains {
    /// Whether its look still shows at `time` (seconds into the clip). A world only has the record
    /// from the step it broke on, so a moment drawn a little before that step's time (frame times
    /// fall between steps) counts as the break itself.
    pub fn shows_at(&self, time: f64) -> bool {
        time < self.time + self.show.lasts
    }

    /// Seconds since it broke at `time`, never below 0.
    pub fn age(&self, time: f64) -> f64 {
        (time - self.time).max(0.0)
    }
}
