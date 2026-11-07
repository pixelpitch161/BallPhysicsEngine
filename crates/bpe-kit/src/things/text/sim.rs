//! A Text in the world: only its pose (where it sits, turned by its Spin). It has no wall yet.

use bpe_core::{ResolvedThing, Settings, Thing, ThingId};
use bpe_runtime::{Pose, ThingSim, World, state_mut};

use super::{KIND, TextSettings};

pub struct TextSim;

/// How far a Text has turned by its Spin, and how fast it turns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextState {
    /// Radians, clockwise.
    pub angle: f32,
    /// Radians a second.
    pub spin: f32,
}

impl ThingSim for TextSim {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let spin = TextSettings::from_params(&resolved.params).spin.to_radians();
        let anchor = world.anchor(thing.id);
        world.poses.insert(thing.id, Pose { spin: anchor.spin + spin, ..anchor });
        world.states.insert(thing.id, Box::new(TextState { angle: 0.0, spin }));
    }

    fn settings_changed(&self, thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let spin = TextSettings::from_params(&resolved.params).spin.to_radians();
        if let Some(st) = world.states.get_mut(&thing) {
            state_mut::<TextState>(st.as_mut()).spin = spin;
        }
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        // Slow motion slows the spin with the balls.
        let dt = world.sim_dt();
        let anchor = world.anchor(thing);
        let Some(st) = world.states.get_mut(&thing) else { return };
        let st = state_mut::<TextState>(st.as_mut());
        st.angle += st.spin * dt;
        let pose = Pose { angle: anchor.angle + st.angle, spin: anchor.spin + st.spin, ..anchor };
        world.poses.insert(thing, pose);
    }

    /// Only the spin moves anything (what rides the Text); the words, font and path are drawn.
    fn affects_physics(&self, key: &str) -> bool {
        key == "spin"
    }
}
