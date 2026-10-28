//! Beat sync (the controller stage at the start of each step). Owned by the music workstream,
//! which fills this in.

use crate::{SimInputs, Simulation};
use bpe_scene::Scene;

/// Steers balls so their bounces land on the song's beats.
#[derive(Clone, Debug, Default)]
pub(crate) struct Controller {}

impl Controller {
    pub fn new(_scene: &Scene, _inputs: &SimInputs) -> Self {
        Self {}
    }

    /// Runs before each step's physics. Take `sim.controller` out with `std::mem::take` to look
    /// ahead with clones of the simulation.
    pub fn before_step(_sim: &mut Simulation) {}
}
