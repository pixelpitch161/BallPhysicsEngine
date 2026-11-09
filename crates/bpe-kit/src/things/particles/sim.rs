//! How a Particles thing takes part in the world: it builds its state, makes its first births
//! when the world is built, and takes a particle step every second world step, last in the step
//! (after the balls have moved and the log has the step's hits). Unless it is two-way it changes
//! nothing but its own state (and the heat it puts into the air), so it is stepped beside the
//! physics (`in_main` false); two-way, it pushes the balls in `forces` and holds them up on its
//! piles in `settle`. A thing that follows or heats the air wakes it (particles part 2) and brings
//! it along before each of its particle steps; a thing fed by its parent's particles steps
//! straight after its parent, reading the moments of that very step.

use bpe_core::{ResolvedThing, Settings, SlotId, Thing, ThingId};
use bpe_runtime::{Log, ThingSim, ThingState, World, state_mut};

use crate::air::{Air, AirSpec, air_of, put_air, take_air};

use super::forces::Forces;
use super::look::{Engine, engine_of};
use super::meets::{AtShapes, MeetsSettings};
use super::moments::{MomentSound, log_notes};
use super::state::ParticlesState;
use super::step;
use super::swarm::{self, Swarm};
use super::wants::Wants;
use super::{BornFrom, KIND, ParticlesSettings};

pub struct ParticlesSim;

/// Reads the thing's Source and cards into `st` (and its places again when they changed; `video`
/// is the video's size, which a grid covering the screen reads); with `look`, also what its Look
/// asks the stepping to keep ([`Wants`]: nothing it reads can be keyed, so a key moving needn't
/// read the Look again).
fn read_into(
    st: &mut ParticlesState,
    resolved: &ResolvedThing,
    video: glam::Vec2,
    tree: Option<&bpe_core::Scene>,
    look: bool,
) {
    st.settings = ParticlesSettings::from_params(&resolved.params);
    st.forces = Forces::read(resolved.slots.get(&SlotId::Motion));
    st.meets = MeetsSettings::read(resolved.slots.get(&SlotId::Bounce).map(|s| &s.params));
    st.refresh_spots(video, tree);
    st.sound = MomentSound::of(resolved);
    if look {
        st.look_wants = Wants::read(resolved.slots.get(&SlotId::Look));
    }
    // At most MOST_RING_PLACES over as many particles as there can be.
    let room = super::trail::MOST_RING_PLACES / (st.settings.most.clamp(1, super::MOST_EVER) as usize);
    let points = if st.look_wants.trail_points == 0 { 0 } else { st.look_wants.trail_points.min(room.max(2)) };
    st.trail.set_points(points, &st.specks);
    super::marks::prune(st);
}

/// Whether `s` is born from a Text thing's letters.
fn made_of_text(s: &ParticlesSettings) -> bool {
    s.from == BornFrom::AWordOrPicture && s.word_from == super::WordFrom::ATextThing
}

/// Whether its source is a picture (kept in the scene, or an older scene's file).
fn made_of_picture(s: &ParticlesSettings) -> bool {
    s.from == BornFrom::AWordOrPicture && s.word_from == super::WordFrom::Picture && !s.picture.is_empty()
}

/// The Particles state in a box, if it holds one.
fn particles_in(b: &dyn ThingState) -> Option<&ParticlesState> {
    b.as_any().downcast_ref::<ParticlesState>()
}

/// Notes how much memory the thing's particles take (a swarm's in video memory), for playback
/// budgets.
fn note_memory(world: &mut World, id: ThingId) {
    let Some(st) = state_of(world, id) else { return };
    let (bytes, gpu) = (st.memory(), st.swarm.as_ref().map(|s| s.gpu_memory()));
    world.state_memory.insert(id, bytes);
    if let Some(gpu) = gpu {
        world.gpu_memory.insert(id, gpu);
    }
}

impl ThingSim for ParticlesSim {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let mut st = ParticlesState::new(thing.id, thing.parent, ParticlesSettings::default());
        read_into(&mut st, resolved, world.video, world.tree.as_deref(), true);
        if engine_of(resolved.slots.get(&SlotId::Look)) == Engine::HugeSwarm {
            st.swarm = Some(Swarm::new());
        }
        world.states.insert(thing.id, Box::new(st));
    }

    fn after_build(&self, id: ThingId, world: &mut World) {
        wake_air_if_wanted(world, id);
        let Some(mut boxed) = world.states.remove(&id) else { return };
        let st = state_mut::<ParticlesState>(boxed.as_mut());
        if st.swarm.is_some() {
            swarm::first(st, world);
        } else {
            step::first(st, world);
        }
        world.states.insert(id, boxed);
        note_memory(world, id);
    }

    /// The first Particles thing due a step takes every Particles thing's due steps, all together
    /// one particle step at a time in tree order, so at any Physics rate each reads the one air as
    /// the others left it at that step; the rest then find themselves up to date.
    fn after_step(&self, id: ThingId, world: &mut World, log: &mut Log) {
        let due = step::due(world.step, world.steps_per_second);
        if due > state_of(world, id).map_or(u32::MAX, |s| s.step) {
            catch_up_all(world, log, due);
        }
        // A thing whose sound plays its moments logs some every world step its budget allows.
        if state_of(world, id).is_some_and(|s| s.sound.is_some())
            && let Some(mut boxed) = world.states.remove(&id)
        {
            let st = state_mut::<ParticlesState>(boxed.as_mut());
            if let Some(sound) = st.sound.clone() {
                log_notes(st, &sound, world.step, 1.0 / world.steps_per_second.max(1) as f32, log);
            }
            world.states.insert(id, boxed);
        }
    }

    /// Two-way: the particles' pushes reach the balls, and stuck particles weigh them down.
    fn forces(&self, id: ThingId, world: &mut World, dt: f32) {
        if !state_of(world, id)
            .is_some_and(|st| step::two_way(st) && !st.two_way.pushes.is_empty() | !st.two_way.weigh.is_empty())
        {
            return;
        }
        let Some(mut boxed) = world.states.remove(&id) else { return };
        super::two_way::apply(state_mut::<ParticlesState>(boxed.as_mut()), world, dt);
        world.states.insert(id, boxed);
    }

    /// Two-way: balls rest on the piles.
    fn settle(&self, id: ThingId, world: &mut World, dt: f32) {
        let holds = state_of(world, id).is_some_and(|st| {
            step::two_way(st)
                && st.meets.holds_up
                && st.meets.at_shapes == AtShapes::PileUp
                && !st.piles.list.is_empty()
        });
        if !holds {
            return;
        }
        let Some(mut boxed) = world.states.remove(&id) else { return };
        super::two_way::hold_up(state_mut::<ParticlesState>(boxed.as_mut()), world, dt);
        world.states.insert(id, boxed);
    }

    fn settings_changed(&self, id: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let video = world.video;
        let tree = world.tree.clone();
        // `(**s)`: the boxed state itself, not the box (which is a `ThingState` too).
        if let Some(st) = world.states.get_mut(&id).and_then(|s| (**s).as_any_mut().downcast_mut::<ParticlesState>()) {
            read_into(st, resolved, video, tree.as_deref(), false);
        }
        wake_air_if_wanted(world, id);
    }

    /// Every setting changes the particles; the fingerprints leave the thing out anyway (it is
    /// stepped beside the physics), so saying yes only lets keys reach `settings_changed`.
    fn key_restarts_physics(&self, _key: &str) -> bool {
        true
    }

    /// Decoration, unless two-way is on (and it isn't fed by another Particles thing, nor a Huge
    /// swarm, which never pushes back).
    fn in_main(&self, resolved: &ResolvedThing) -> bool {
        let meets = MeetsSettings::read(resolved.slots.get(&SlotId::Bounce).map(|s| &s.params));
        meets.two_way
            && ParticlesSettings::from_params(&resolved.params).from != BornFrom::ItsParentsParticles
            && engine_of(resolved.slots.get(&SlotId::Look)) == Engine::Exact
    }

    /// Its Rise forces heat the air (a Huge swarm never does): while balls follow the air, the
    /// heat moves them, so the thing steps with the physics. A thing fed by its parent's particles
    /// doesn't count: while balls follow the air it puts in no heat (its parent may be beside).
    fn heats_air(&self, resolved: &ResolvedThing) -> bool {
        engine_of(resolved.slots.get(&SlotId::Look)) == Engine::Exact
            && ParticlesSettings::from_params(&resolved.params).from != BornFrom::ItsParentsParticles
            && Forces::read(resolved.slots.get(&SlotId::Motion)).heats_air()
    }

    /// The Look card's Engine picks how the particles step (Exact or Huge swarm).
    fn card_key_restarts_beside(&self, slot: SlotId, key: &str) -> bool {
        slot == SlotId::Look && key == super::look::ENGINE_KEY
    }

    /// Its sound hears its moments when its Sound card plays some (particles part 7).
    fn heard(&self, resolved: &ResolvedThing) -> Option<u64> {
        MomentSound::of(resolved).map(|s| s.hash())
    }

    /// Its Motion and Meets things cards have no module sims: it reads them itself.
    fn steps_own_cards(&self) -> bool {
        true
    }

    /// Its Rate, Count, forces and pushes may pulse on hits: nothing reads the particles back.
    fn pulses_on_hits(&self) -> bool {
        true
    }

    /// A source made of a Text thing reads that Text from the scene (Text part 4), and one made
    /// of a picture reads the picture the scene keeps (W5).
    fn reads_tree(&self, resolved: &ResolvedThing) -> bool {
        let s = ParticlesSettings::from_params(&resolved.params);
        made_of_text(&s) || made_of_picture(&s)
    }

    /// The Text its source is made of: its saved settings and font place the particles.
    fn physics_print_extra(&self, scene: &bpe_core::Scene, thing: &Thing, resolved: &ResolvedThing) -> u64 {
        let s = ParticlesSettings::from_params(&resolved.params);
        if !made_of_text(&s) {
            return 0;
        }
        crate::things::text::shape::text_named(scene, &s.text_thing, thing.id)
            .map_or(1, |text| crate::things::text::shape::letters_hash(scene, text))
    }

    /// What its Look asks the stepping to keep: trails and marks (part 6).
    fn beside_print_extra(&self, resolved: &ResolvedThing) -> u64 {
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

/// Every Particles thing's due particle steps, all together one particle step at a time in tree
/// order (see [`ParticlesSim::after_step`]).
fn catch_up_all(world: &mut World, log: &mut Log, due: u32) {
    let ids: Vec<ThingId> =
        world.order().iter().copied().filter(|t| state_of(world, *t).is_some_and(|s| s.step < due)).collect();
    let mut taken: Vec<(ThingId, Box<dyn ThingState>, f32)> =
        ids.iter().filter_map(|t| Some((*t, world.states.remove(t)?, 0.0))).collect();
    let mut air = take_air(world);
    let threads = step::threads();
    let from = taken.iter_mut().map(|(_, b, _)| state_mut::<ParticlesState>(b.as_mut()).step).min().unwrap_or(due);
    // Each thing keeps the feeds the things it feeds ask for (they step right after it, in tree
    // order).
    let wants: Vec<u32> = {
        let states: Vec<&ParticlesState> = taken.iter().filter_map(|(_, b, _)| particles_in(b.as_ref())).collect();
        taken.iter().map(|(t, _, _)| step::wants_of(states.iter().copied(), *t)).collect()
    };
    for k in from..due {
        if let Some(air) = air.as_mut() {
            air.get_mut().catch_up_to(k + 1, world, threads);
        }
        #[allow(clippy::needless_range_loop)] // `split_at_mut(i)` needs the index.
        for i in 0..taken.len() {
            let (before, rest) = taken.split_at_mut(i);
            let (_, boxed, ms) = &mut rest[0];
            let st = state_mut::<ParticlesState>(boxed.as_mut());
            if st.step != k {
                continue;
            }
            // Fed: the parent's state just after the same particle step.
            let parent = (st.settings.from == BornFrom::ItsParentsParticles)
                .then_some(st.parent)
                .flatten()
                .and_then(|p| before.iter().find(|(t, _, _)| *t == p))
                .and_then(|(_, b, _)| particles_in(b.as_ref()))
                .filter(|p| p.step == k + 1);
            st.wants = wants[i];
            let started = std::time::Instant::now();
            let air_now = air.as_ref().map(|a| a.get()).filter(|a| a.awake());
            if st.swarm.is_some() {
                // A swarm never heats the air, feeds or is fed: its particles stay on the card.
                swarm::step_in(st, world, log, air_now);
                *ms += started.elapsed().as_secs_f32() * 1000.0;
                continue;
            }
            step::step_in(st, world, log, threads, air_now, parent);
            // While balls follow the air, a thing fed by its parent's particles puts in no heat:
            // its parent may step beside the physics, and every playback must move the balls
            // alike ([`ParticlesSim::heats_air`]).
            let heats = !(world.air_felt && st.settings.from == BornFrom::ItsParentsParticles);
            if let Some(air) = air.as_mut().filter(|a| heats && a.get().awake()) {
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
