//! The swarm's place in the processor's world: only its settings. The points live on the
//! graphics card, so nothing here moves and nothing here changes the physics.

use bpe_core::{ResolvedThing, Settings, Thing};
use bpe_runtime::{ThingSim, World};
use glam::Vec2;

use super::SwarmSettings;

pub struct SwarmSim;

/// What the swarm keeps between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct SwarmState {
    pub settings: SwarmSettings,
    pub center: Vec2,
}

impl ThingSim for SwarmSim {
    fn kind(&self) -> &'static str {
        "swarm"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let st =
            SwarmState { settings: SwarmSettings::from_params(&resolved.params), center: thing.transform.position };
        world.states.insert(thing.id, Box::new(st));
    }

    fn affects_physics(&self, _key: &str) -> bool {
        false
    }
}
