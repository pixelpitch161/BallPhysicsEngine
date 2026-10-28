//! Jelly balls, ropes, cloth and small fluids (the last stage of each substep). Owned by the
//! physics workstream, which fills this in.

use crate::{Event, Simulation};
use bpe_scene::Scene;

#[derive(Clone, Debug, Default)]
pub(crate) struct BodyState {}

impl BodyState {
    pub fn new(_scene: &Scene) -> Self {
        Self {}
    }
}

/// Moves soft bodies forward by `dt` and collides them with walls and balls.
pub(crate) fn step(_sim: &mut Simulation, _dt: f32, _time: f64, _events: &mut Vec<Event>) {}
