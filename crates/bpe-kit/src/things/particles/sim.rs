//! How a Particles thing takes part in the world: it builds its state, makes its first births
//! when the world is built, and takes a particle step every second world step, last in the step
//! (after the balls have moved and the log has the step's hits). It never changes anything but its
//! own state (and the heat it puts into the air), so it is stepped beside the physics (`in_main`
//! false). A thing that follows or heats the air wakes it (particles part 2) and brings it along
//! before each of its particle steps.

use bpe_core::{ResolvedThing, Settings, SlotId, Thing, ThingId};
use bpe_runtime::{Log, ThingSim, ThingState, World, state_mut};

use crate::air::{Air, AirSpec, air_of, put_air, take_air};

use super::forces::Forces;
use super::meets::MeetsSettings;
use super::state::ParticlesState;
use super::step;
use super::wants::Wants;
use super::{KIND, ParticlesSettings};

pub struct ParticlesSim;

/// Reads the thing's Source and cards into `st`; with `look`, also what its Look asks the stepping
/// to keep ([`Wants`]: nothing it reads can be keyed, so a key moving needn't read the Look again).
fn read_into(st: &mut ParticlesState, resolved: &ResolvedThing, look: bool) {
    st.settings = ParticlesSettings::from_params(&resolved.params);
    st.forces = Forces::read(resolved.slots.get(&SlotId::Motion));
    st.meets = MeetsSettings::read(resolved.slots.get(&SlotId::Bounce).map(|s| &s.params));
    if look {
        st.wants = Wants::read(resolved.slots.get(&SlotId::Look));
    }
    // At most MOST_RING_PLACES over as many particles as there can be.
    let room = super::trail::MOST_RING_PLACES / (st.settings.most.clamp(1, super::MOST_EVER) as usize);
    let points = if st.wants.trail_points == 0 { 0 } else { st.wants.trail_points.min(room.max(2)) };
    st.trail.set_points(points, &st.specks);
    super::marks::prune(st);
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
        read_into(&mut st, resolved, true);
        world.states.insert(thing.id, Box::new(st));
    }

    fn after_build(&self, id: ThingId, world: &mut World) {
        wake_air_if_wanted(world, id);
        let Some(mut boxed) = world.states.remove(&id) else { return };
        step::first(state_mut::<ParticlesState>(boxed.as_mut()), world);
        world.states.insert(id, boxed);
        note_memory(world, id);
    }

    /// The first Particles thing due a step takes every Particles thing's due steps, all together
    /// one particle step at a time in tree order, so at any Physics rate each reads the one air as
    /// the others left it at that step; the rest then find themselves up to date.
    fn after_step(&self, id: ThingId, world: &mut World, log: &mut Log) {
        let due = step::due(world.step, world.steps_per_second);
        if due <= state_of(world, id).map_or(u32::MAX, |s| s.step) {
            return;
        }
        let ids: Vec<ThingId> =
            world.order().iter().copied().filter(|t| state_of(world, *t).is_some_and(|s| s.step < due)).collect();
        let mut taken: Vec<(ThingId, Box<dyn ThingState>, f32)> =
            ids.iter().filter_map(|t| Some((*t, world.states.remove(t)?, 0.0))).collect();
        let mut air = take_air(world);
        let threads = step::threads();
        let from = taken.iter_mut().map(|(_, b, _)| state_mut::<ParticlesState>(b.as_mut()).step).min().unwrap_or(due);
        for k in from..due {
            if let Some(air) = air.as_mut() {
                air.get_mut().catch_up_to(k + 1, world, threads);
            }
            for (_, boxed, ms) in &mut taken {
                let st = state_mut::<ParticlesState>(boxed.as_mut());
                if st.step != k {
                    continue;
                }
                let started = std::time::Instant::now();
                step::step_in(st, world, log, threads, air.as_ref().map(|a| a.get()).filter(|a| a.awake()));
                if let Some(air) = air.as_mut().filter(|a| a.get().awake()) {
                    step::put_heat(st, air.get_mut());
                }
                *ms += started.elapsed().as_secs_f32() * 1000.0;
            }
        }
        if let Some(air) = air {
            put_air(world, air);
        }
        for (t, mut boxed, ms) in taken {
            state_mut::<ParticlesState>(boxed.as_mut()).last_ms = ms;
            world.states.insert(t, boxed);
            note_memory(world, t);
        }
    }

    fn settings_changed(&self, id: ThingId, resolved: &ResolvedThing, world: &mut World) {
        // `(**s)`: the boxed state itself, not the box (which is a `ThingState` too).
        if let Some(st) = world.states.get_mut(&id).and_then(|s| (**s).as_any_mut().downcast_mut::<ParticlesState>()) {
            read_into(st, resolved, false);
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

    /// What its Look asks the stepping to keep: trails and marks (part 6).
    fn more_fingerprint(&self, resolved: &ResolvedThing) -> u64 {
        Wants::read(resolved.slots.get(&SlotId::Look)).hash()
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
        // It starts still at the thing's step: nothing has stirred it before, so there is
        // nothing to run up to there.
        let at = state_of(world, id).map_or(0, |s| s.step);
        let air = air.get_mut();
        air.wake(world);
        air.step = air.step.max(at);
    }
    put_air(world, air);
}

/// The thing's particles as the world has them now.
pub fn state_of(world: &World, id: ThingId) -> Option<&ParticlesState> {
    (**world.states.get(&id)?).as_any().downcast_ref::<ParticlesState>()
}
