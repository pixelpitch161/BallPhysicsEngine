//! What a running graph may ask of the world around it.

use bpe_core::{Target, ThingId};
use glam::Vec2;

use crate::{Ball, BallInfo};

/// The world as a graph sees it. Every method has a do-nothing default, so a host only answers
/// what it knows.
pub trait Host {
    /// The scene's seed, so random blocks repeat from run to run.
    fn seed(&self) -> u64 {
        0
    }
    fn gravity(&self) -> Vec2 {
        Vec2::ZERO
    }
    /// A live ball's state, or `None` when it is gone.
    fn ball(&self, _ball: Ball) -> Option<BallInfo> {
        None
    }
    /// Whether a ball belongs to a target, seen from the thing that owns the graph.
    fn matches(&self, _ball: Ball, target: &Target) -> bool {
        matches!(target, Target::Anything)
    }
    /// Every live ball of a target, in thing then row order.
    fn balls(&self, _target: &Target) -> Vec<Ball> {
        Vec::new()
    }
    /// Every thing a target names, in id order.
    fn things_of(&self, _target: &Target) -> Vec<ThingId> {
        Vec::new()
    }
    /// A note (MIDI key) of the chord playing at `time`, picked by `index`.
    fn chord_note(&self, _time: f32, _index: u64) -> Option<f32> {
        None
    }
}

/// A host that knows nothing: for graphs that only need their inputs.
pub struct NoHost;

impl Host for NoHost {}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Target;

    #[test]
    fn no_host_answers_nothing() {
        assert_eq!(NoHost.seed(), 0);
        assert_eq!(NoHost.ball(Ball { thing: bpe_core::ThingId(1), item: 0 }), None);
        assert!(NoHost.matches(Ball { thing: bpe_core::ThingId(1), item: 0 }, &Target::Anything));
        assert!(!NoHost.matches(Ball { thing: bpe_core::ThingId(1), item: 0 }, &Target::Me));
        assert!(NoHost.balls(&Target::Anything).is_empty());
        assert_eq!(NoHost.chord_note(0.0, 0), None);
    }
}
