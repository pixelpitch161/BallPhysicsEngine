//! Rules: "when this happens, do that" graphs, run at the end of every step from that step's
//! events.

use crate::{Event, Simulation};
use bpe_scene::Scene;

/// The scene's rules, compiled once, and what they have done so far.
#[derive(Clone, Debug, Default)]
pub struct RuleState {}

impl RuleState {
    pub fn new(_scene: &Scene) -> Self {
        Self {}
    }

    /// For an "add to counter" action: which counter it changes and by how much.
    pub fn counter_change(&self, _rule: u16, _action: u16) -> Option<(usize, f64)> {
        None
    }
}

/// Runs the rules on the events from `first` onwards, which happened during this step.
pub(crate) fn run(_sim: &mut Simulation, _first: usize, _events: &mut Vec<Event>) {}
