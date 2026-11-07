//! A Text in the world: only its pose (where it sits, turned by its Spin). It has no wall yet. A
//! Text whose words hold standings keeps the balls' past (a little), so a line moving to its new
//! place starts from where the ranking stood.

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
    /// Whether its words hold standings (which read the balls' past).
    pub standings: bool,
}

/// How much of the balls' past a Text with standings keeps.
fn standings_history() -> bpe_runtime::HistoryWant {
    bpe_runtime::HistoryWant { seconds: 2.0, forever: false }
}

/// Whether a Text with these settings shows standings.
fn has_standings(resolved: &ResolvedThing) -> bool {
    !crate::text::live::standings_in(&TextSettings::from_params(&resolved.params).words).is_empty()
}

impl ThingSim for TextSim {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let spin = TextSettings::from_params(&resolved.params).spin.to_radians();
        let anchor = world.anchor(thing.id);
        world.poses.insert(thing.id, Pose { spin: anchor.spin + spin, ..anchor });
        let standings = has_standings(resolved);
        world.states.insert(thing.id, Box::new(TextState { angle: 0.0, spin, standings }));
    }

    /// With standings, every thing with balls keeps a little of its past.
    fn after_build(&self, thing: ThingId, world: &mut World) {
        let standings = world.states.get(&thing).is_some_and(|s| bpe_runtime::state::<TextState>(s.as_ref()).standings);
        if !standings {
            return;
        }
        let with_balls: Vec<(ThingId, usize)> = world.bodies.iter().map(|(id, b)| (*id, b.len())).collect();
        for (id, rows) in with_balls {
            world.keep_history(id, standings_history());
            if let Some(h) = world.history.get_mut(&id) {
                h.note(world.step, rows);
            }
        }
    }

    /// Standings ask for the balls' past; asking makes a change to them rebuild the world.
    fn history(&self, resolved: &ResolvedThing) -> Option<bpe_runtime::HistoryWant> {
        has_standings(resolved).then(standings_history)
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
