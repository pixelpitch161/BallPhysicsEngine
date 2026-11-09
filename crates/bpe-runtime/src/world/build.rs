//! Building a world from a scene: every thing's state and cards, in tree order.

use super::*;

impl World {
    /// Builds the world: every resolved thing in tree order through its sim, then the Motion
    /// and Bounce modules it uses. Modules without a sim are left out; a thing without one
    /// builds nothing, but can still be named by a target.
    pub fn build(scene: &Scene, resolved: &Resolved, sims: &Sims) -> World {
        Self::build_with(scene, resolved, sims, true)
    }

    /// [`World::build`] for a world nobody draws (the sound engine's, the editor's particle and
    /// aiming threads'): it keeps none of the balls' past for trails and particle layers, which
    /// only drawing reads. What the physics itself reads of it (a Text's standings, which ask in
    /// their `after_build`) is still kept, and the balls move exactly as in a drawn world.
    pub fn build_unseen(scene: &Scene, resolved: &Resolved, sims: &Sims) -> World {
        Self::build_with(scene, resolved, sims, false)
    }

    pub(super) fn build_with(scene: &Scene, resolved: &Resolved, sims: &Sims, drawn: bool) -> World {
        let mut world = World {
            step: 0,
            steps_per_second: 240,
            gravity: Vec2::ZERO,
            bodies: BTreeMap::new(),
            walls: BTreeMap::new(),
            states: BTreeMap::new(),
            poses: BTreeMap::new(),
            names: BTreeMap::new(),
            tags: BTreeMap::new(),
            history: BTreeMap::new(),
            seed: 0,
            camera: CameraView::default(),
            video: Vec2::new(1080.0, 1920.0),
            beat_seconds: 0.5,
            beats_in_bar: 4,
            first_beat: 0.0,
            state_memory: BTreeMap::new(),
            gpu_memory: BTreeMap::new(),
            kinds: BTreeMap::new(),
            order: Vec::new(),
            pairs: Pairs::default(),
            walls_before: BTreeMap::new(),
            substep: (0, 1),
            attach: BTreeMap::new(),
            modules: BTreeMap::new(),
            animated: None,
            sound: None,
            light: None,
            pulls: BTreeMap::new(),
            air_felt: false,
            air_at: BTreeMap::new(),
            fingerprint: None,
            rules: None,
            pace: 1.0,
            run: RunId::fresh(),
            beside: BTreeSet::new(),
            decoration: BTreeSet::new(),
            parts: BTreeMap::new(),
            tree: None,
        };
        if resolved.things.values().any(|r| sims.thing(&r.kind).is_some_and(|s| s.reads_tree(r))) {
            world.tree = Some(std::sync::Arc::new(scene.clone()));
        }
        // Worked out first: a thing may cache work by it while it builds.
        let wants_fingerprint =
            resolved.things.values().any(|r| sims.thing(&r.kind).is_some_and(|s| s.needs_physics_print(r)));
        if wants_fingerprint {
            world.fingerprint = Some(sound_fingerprint(scene, resolved, sims));
        }
        // Keys and pulses on physics settings hold from the very start: the world is built from
        // the settings as they are at 0 s (the sound, which no step has made yet, left out), so
        // the first frame and the first step's judgement of sides see them.
        let animation = physics_animation(scene, resolved, sims);
        let at_start = animation.as_ref().map(|animation| {
            let mut at_start = resolved.clone();
            // Health is full at the start.
            let full = FullHealth(resolved);
            animation.apply(&mut at_start, 0.0, &Inputs { health: Some(&full), ..Inputs::NONE });
            at_start
        });
        let built_from = at_start.as_ref().unwrap_or(resolved);
        // What the world senses for its keys: a world with nothing stepped beside the physics
        // (the editor's main playback) leaves out the keys only those things read, so an Air
        // key following sound or light gives it no sound or light to work out.
        // (A rule may keep such a thing in the main playback: it is built, so it senses too.)
        // Balls that follow the air (and haze moving with it) read the Air card's keys in every
        // world.
        world.air_felt = crate::air_felt(scene, sims);
        let decorated = resolved.things.values().any(|r| sims.thing(&r.kind).is_some_and(|s| !s.in_main(r)));
        let sensed = if decorated || crate::air_read(scene, resolved, sims) {
            animation.clone()
        } else {
            animation.as_ref().and_then(main_animation)
        };
        // Things that need the walls (none of today's kinds) are built in a second pass, so
        // an arena after them in the tree is there all the same; they come last in `order`. A
        // thing without a sim is still known by kind and tags, so a target can name it.
        for needs_walls in [false, true] {
            for id in &scene.order {
                let Some(r) = built_from.things.get(id) else { continue };
                let Some(thing) = scene.things.get(id) else { continue };
                let sim = sims.thing(&r.kind);
                if sim.is_some_and(|s| s.needs_walls()) != needs_walls {
                    continue;
                }
                if let Some(parent) = thing.parent {
                    world.attach.insert(*id, (parent, thing.attach, thing.transform));
                }
                if let Some(sim) = sim {
                    sim.build(thing, r, &mut world);
                    world.order.push(*id);
                    if is_beside(scene, sims, *id, r) {
                        world.beside.insert(*id);
                    }
                    if !sim.in_main(r) {
                        world.decoration.insert(*id);
                    }
                    if let Some(want) = sim.history(r).filter(|_| drawn) {
                        world.keep_history(*id, want);
                    }
                }
                world.kinds.insert(*id, r.kind.clone());
                world.names.insert(*id, thing.name.clone());
                if !thing.tags.is_empty() {
                    world.tags.insert(*id, thing.tags.clone());
                }
                let mut prepared = Prepared::new();
                for slot in SIM_SLOTS {
                    let Some(s) = r.slots.get(&slot) else { continue };
                    if let Some(msim) = sims.module(&s.kind) {
                        prepared.insert(slot, Slotted::new(msim, &s.kind, &s.params));
                    }
                }
                if !prepared.is_empty() {
                    world.modules.insert(*id, prepared);
                }
            }
        }
        // The balls there from the start were there at step 0.
        for (id, history) in &mut world.history {
            if let Some(bodies) = world.bodies.get(id) {
                history.note(0, bodies.len());
            }
        }
        // Everything is there now: things that depend on the whole scene finish building.
        for id in world.order.clone() {
            if let Some(sim) = world.kinds.get(&id).and_then(|kind| sims.thing(kind)) {
                sim.after_build(id, &mut world);
            }
        }
        // Balls that follow the air find it awake from the start, in every world.
        if world.air_felt
            && let Some(air) = sims.air.clone()
        {
            air.wake(&mut world);
        }
        // Light sensing, only when something physical follows light: the probe is there before
        // the balls' places in light spreads are first worked out.
        if let Some(maker) = &sims.light
            && needs_light(scene, built_from, sims, sensed.as_ref())
        {
            world.light = Some(maker.make(scene, resolved));
        }
        world.refresh_rows(sims, &Log::default());
        world.rules = crate::rules::RulesState::build(scene, resolved, sims, &world);
        let pristine =
            world.rules.as_ref().filter(|r| r.changes_settings()).map(|_| std::sync::Arc::new(built_from.clone()));
        world.animated = match (animation.zip(at_start), pristine) {
            (Some((animation, resolved)), pristine) => Some(Animated { animation, resolved, pristine }),
            (None, Some(pristine)) => Some(Animated {
                animation: Animation::default(),
                resolved: (*pristine).clone(),
                pristine: Some(pristine),
            }),
            (None, None) => None,
        };
        if let (Some(maker), Some(_), Some(sensed)) = (&sims.sound, &world.animated, &sensed)
            && follows_sound(scene, sensed)
        {
            world.sound = Some(maker.make(scene, resolved));
        }
        world
    }
}
