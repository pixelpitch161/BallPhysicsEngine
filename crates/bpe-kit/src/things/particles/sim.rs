//! How a Particles thing takes part in the world: it builds its state, makes its first births
//! when the world is built, and takes a particle step every second world step, last in the step
//! (after the balls have moved and the log has the step's hits). It never changes anything but its
//! own state (and the heat it puts into the air), so it is stepped beside the physics (`in_main`
//! false). A thing that follows or heats the air wakes it (particles part 2) and brings it along
//! before each of its particle steps.

use bpe_core::{ResolvedThing, Settings, SlotId, Thing, ThingId};
use bpe_runtime::{Log, ThingSim, World, state_mut};

use crate::air::{Air, AirSpec, air_of, put_air, take_air};

use super::forces::Forces;
use super::meets::MeetsSettings;
use super::state::ParticlesState;
use super::step;
use super::{KIND, ParticlesSettings};

pub struct ParticlesSim;

/// Reads the thing's Source and cards into `st`.
fn read_into(st: &mut ParticlesState, resolved: &ResolvedThing) {
    st.settings = ParticlesSettings::from_params(&resolved.params);
    st.forces = Forces::read(resolved.slots.get(&SlotId::Motion));
    st.meets = MeetsSettings::read(resolved.slots.get(&SlotId::Bounce).map(|s| &s.params));
}

/// Notes how much memory the thing's particles take, for playback budgets.
fn note_memory(world: &mut World, id: ThingId) {
    let bytes = world.states.get(&id).and_then(|s| (**s).as_any().downcast_ref::<ParticlesState>()).map(|s| s.memory());
    if let Some(bytes) = bytes {
        world.state_memory.insert(id, bytes);
    }
}

impl ThingSim for ParticlesSim {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let mut st = ParticlesState::new(thing.id, thing.parent, ParticlesSettings::default());
        read_into(&mut st, resolved);
        world.states.insert(thing.id, Box::new(st));
    }

    fn after_build(&self, id: ThingId, world: &mut World) {
        wake_air_if_wanted(world, id);
        let Some(mut boxed) = world.states.remove(&id) else { return };
        step::first(state_mut::<ParticlesState>(boxed.as_mut()), world);
        world.states.insert(id, boxed);
        note_memory(world, id);
    }

    fn after_step(&self, id: ThingId, world: &mut World, log: &mut Log) {
        if step::due(world.step, world.steps_per_second) <= state_of(world, id).map_or(u32::MAX, |s| s.step) {
            return;
        }
        let Some(mut boxed) = world.states.remove(&id) else { return };
        let started = std::time::Instant::now();
        let st = state_mut::<ParticlesState>(boxed.as_mut());
        let mut air = take_air(world);
        step::catch_up(st, world, log, step::threads(), air.as_mut().map(|a| a.get_mut()));
        st.last_ms = started.elapsed().as_secs_f32() * 1000.0;
        if let Some(air) = air {
            put_air(world, air);
        }
        world.states.insert(id, boxed);
        note_memory(world, id);
    }

    fn settings_changed(&self, id: ThingId, resolved: &ResolvedThing, world: &mut World) {
        if let Some(st) = world.states.get_mut(&id).and_then(|s| s.as_any_mut().downcast_mut::<ParticlesState>()) {
            read_into(st, resolved);
        }
        wake_air_if_wanted(world, id);
    }

    /// Every setting changes the particles; the fingerprints leave the thing out anyway (it is
    /// stepped beside the physics), so saying yes only lets keys reach `settings_changed`.
    fn affects_physics(&self, _key: &str) -> bool {
        true
    }

    /// Decoration: never pushes back on the balls (two-way is part 4).
    fn in_main(&self, _resolved: &ResolvedThing) -> bool {
        false
    }
}

/// Wakes the world's air when thing `id` follows or heats it, or the scene has an Air card (a
/// sleeping air is there): made with the card's settings, or the defaults without one.
fn wake_air_if_wanted(world: &mut World, id: ThingId) {
    let wants = state_of(world, id).is_some_and(|s| s.forces.follows_air() || s.forces.heats_air());
    let sleeping = air_of(world).is_some_and(|a| !a.awake());
    if !(wants || sleeping) {
        return;
    }
    let mut air = match take_air(world) {
        Some(air) => air,
        None => {
            world.states.insert(ThingId::ROOT, Box::new(Air::new(AirSpec::default())));
            take_air(world).expect("just put there")
        }
    };
    if !air.get().awake() {
        air.get_mut().wake(world);
        // It starts at the thing's step: nothing has stirred it before.
        let at = state_of(world, id).map_or(0, |s| s.step);
        air.get_mut().catch_up_to(at, world, 1);
    }
    put_air(world, air);
}

/// The thing's particles as the world has them now.
pub fn state_of(world: &World, id: ThingId) -> Option<&ParticlesState> {
    (**world.states.get(&id)?).as_any().downcast_ref::<ParticlesState>()
}
