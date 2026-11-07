//! Tests of particles part 7: moments, the sound's Moment events, the rules' When Particles
//! block, lighting on a Particles thing's layers and hits pulses on its settings.

use bpe_core::{Address, BlockSection, PulseSource, Scene, Settings, SlotId, ThingId, Value, Who, resolve};
use bpe_gfx::{Frame, Particle, build_frame};
use bpe_runtime::world::sound_fingerprint;
use bpe_runtime::{EventKind, Log, World, fingerprint, heard_part, is_beside, main_part};

use super::look::SPRITES;
use super::looks::{Look, add_particles};
use super::meets::{AtShapes, MeetsSettings};
use super::moments::{MOST_MOMENTS, MomentKind};
use super::sim::state_of;
use super::{BornFrom, Emit, Heading, KIND, ParticlesSettings};
use crate::sound::{VoiceSettings, starter_voice};
use crate::{Kit, kit};

/// The test scene (a hollow circle and one ball) with an empty Particles thing under the root
/// bursting `count` particles from the middle at the start, bouncing off shapes. The ball is
/// switched off when `ball` is false.
fn burst_in_circle(count: u32, at_shapes: AtShapes, ball: bool) -> (Scene, ThingId, Kit) {
    let kit = kit();
    let mut scene = crate::new_scene(&kit.registry);
    let id = add_particles(&mut scene, &kit.registry, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count,
        speed: 900.0,
        heading: Heading::AllRound,
        life: 20.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = source.to_params();
    let meets = MeetsSettings { at_shapes, bounciness: 1.0, friction: 0.0, ..MeetsSettings::default() };
    scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Bounce).unwrap().params = meets.to_params();
    if !ball {
        let b = scene.order.iter().copied().find(|t| scene.things[t].kind == "balls").unwrap();
        scene.things.get_mut(&b).unwrap().enabled = false;
    }
    (scene, id, kit)
}

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|t| scene.things[t].kind == "shape").unwrap()
}

/// One world step's moments (kind, with, seed) and counts.
type StepMoments = (u64, Vec<(MomentKind, Option<ThingId>, u32)>, [u32; 5]);

/// Each world step's moments of `id`, run to `seconds`.
fn moments_by_step(scene: &Scene, kit: &Kit, id: ThingId, seconds: f64) -> Vec<StepMoments> {
    let resolved = resolve(scene, &kit.registry);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    let mut out = Vec::new();
    let steps = (seconds * world.steps_per_second as f64) as u64;
    for k in 1..=steps {
        world.advance_to(k as f64 / world.steps_per_second as f64, &kit.sims, &mut log);
        let st = state_of(&world, id).unwrap();
        let (moments, counts) = st.moments_at(world.step);
        if counts.iter().any(|c| *c > 0) {
            out.push((world.step, moments.iter().map(|m| (m.kind, m.with, m.seed)).collect(), counts));
        }
    }
    out
}

#[test]
fn bouncing_off_a_shape_is_a_moment_with_the_shape_and_a_burst_counts_its_births() {
    let (scene, id, kit) = burst_in_circle(50, AtShapes::Bounce, false);
    let shape = shape_of(&scene);
    let by_step = moments_by_step(&scene, &kit, id, 2.0);
    let (first, _, counts) = &by_step[0];
    assert_eq!(*first, 1, "the start's burst is the first step's");
    assert_eq!(counts[MomentKind::Born as usize], 50);
    assert_eq!(counts[MomentKind::Burst as usize], 1);
    let hits: Vec<_> = by_step.iter().flat_map(|(_, m, _)| m).filter(|m| m.0 == MomentKind::HitShape).collect();
    assert!(hits.len() >= 50, "each one hits the circle: {}", hits.len());
    assert!(hits.iter().all(|m| m.1 == Some(shape)), "with the shape");
    // The counts are the kept moments while there is room.
    for (_, m, counts) in &by_step {
        assert_eq!(m.len() as u32, counts.iter().sum::<u32>().min(MOST_MOMENTS as u32));
    }
}

#[test]
fn a_particle_lands_once() {
    let (scene, id, kit) = burst_in_circle(30, AtShapes::Stop, false);
    let by_step = moments_by_step(&scene, &kit, id, 3.0);
    let lands: u32 = by_step.iter().map(|(_, _, c)| c[MomentKind::Land as usize]).sum();
    assert_eq!(lands, 30, "each one lands once and stays");
}

#[test]
fn moments_are_the_same_on_any_number_of_threads() {
    let (scene, id, kit) = burst_in_circle(14_000, AtShapes::Bounce, true);
    let resolved = resolve(&scene, &kit.registry);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(0.45, &kit.sims, &mut log);
    let st = state_of(&world, id).unwrap().clone();
    assert!(st.specks.len() > 3 * super::step::CHUNK);
    let mut seen = Vec::new();
    for threads in [1, 3, 8] {
        let mut copy = st.clone();
        for _ in 0..30 {
            super::step::step(&mut copy, &world, &log, threads);
        }
        let (moments, counts) = copy.moments_at(copy.moments_step);
        let moments: Vec<_> = moments.iter().map(|m| (m.kind, m.with, m.seed, m.place.x.to_bits())).collect();
        seen.push((moments, counts));
    }
    assert!(seen[0].1[MomentKind::HitShape as usize] > 100, "{:?}", seen[0].1);
    assert!(seen.windows(2).all(|w| w[0] == w[1]));
}

/// The scene's Particles thing given the starter sound.
fn with_sound(scene: &mut Scene, kit: &Kit, id: ThingId, set: impl FnOnce(&mut VoiceSettings)) {
    let mut card = starter_voice(&kit.registry);
    let mut v = VoiceSettings::from_params(&card.params);
    set(&mut v);
    card.params = v.to_params();
    scene.things.get_mut(&id).unwrap().slots.insert(SlotId::Sound, card);
}

fn moment_events(scene: &Scene, kit: &Kit, seconds: f64) -> Vec<bpe_runtime::Event> {
    let resolved = resolve(scene, &kit.registry);
    let heard = heard_part(scene, &resolved, &kit.sims);
    let mut world = World::build(scene, &heard, &kit.sims);
    let mut log = Log::default();
    world.advance_to(seconds, &kit.sims, &mut log);
    log.events().iter().filter(|e| e.kind == EventKind::Moment).cloned().collect()
}

#[test]
fn a_sound_card_logs_the_strongest_moments_up_to_most_a_second() {
    let (mut scene, id, kit) = burst_in_circle(400, AtShapes::Bounce, false);
    assert!(moment_events(&scene, &kit, 2.0).is_empty(), "no sound, no notes");
    with_sound(&mut scene, &kit, id, |v| v.moments_most = 20.0);
    let events = moment_events(&scene, &kit, 2.0);
    // The budget starts at one note and grows by 20 a second, holding at most 2.
    assert!(!events.is_empty() && events.len() <= 1 + 40, "{}", events.len());
    assert!(events.iter().all(|e| e.a.thing == id && e.b.is_some_and(|b| b.thing == id)));
    let codes: Vec<u32> = events.iter().map(|e| e.b.unwrap().item).collect();
    assert!(codes.iter().all(|c| *c == MomentKind::HitShape.code()), "only hits play by default: {codes:?}");
    // Born plays too when switched on: the start's burst is 400 births, but only the budget's
    // one note plays on the first step.
    with_sound(&mut scene, &kit, id, |v| {
        v.moments_most = 20.0;
        v.moments_born = true;
    });
    let events = moment_events(&scene, &kit, 0.01);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].b.unwrap().item, MomentKind::Born.code());
}

#[test]
fn heard_particles_join_the_sound_but_never_the_main_physics() {
    let (mut scene, id, kit) = burst_in_circle(40, AtShapes::Bounce, true);
    let resolved = resolve(&scene, &kit.registry);
    let plain = sound_fingerprint(&scene, &resolved, &kit.sims);
    assert!(!heard_part(&scene, &resolved, &kit.sims).things.contains_key(&id));
    with_sound(&mut scene, &kit, id, |_| {});
    let resolved = resolve(&scene, &kit.registry);
    assert!(heard_part(&scene, &resolved, &kit.sims).things.contains_key(&id));
    assert!(!main_part(&scene, &resolved, &kit.sims).things.contains_key(&id));
    assert_ne!(sound_fingerprint(&scene, &resolved, &kit.sims), plain, "the sound rebuilds");
}

fn lane(scene: &mut Scene, kit: &Kit, blocks: &[(&str, Vec<(&str, Value)>)]) -> bpe_core::RuleId {
    let rule = scene.add_rule("Test");
    let mut last = None;
    for (kind, params) in blocks {
        let b = scene.add_block(&kit.registry, rule, kind, last).unwrap();
        for (k, v) in params {
            scene.set_block_param(&kit.registry, rule, b, k, v.clone()).unwrap_or_else(|e| panic!("{kind} {k}: {e}"));
        }
        last = Some(b);
    }
    rule
}

#[test]
fn the_when_particles_block_is_registered_as_a_when() {
    let kit = kit();
    let b = kit.registry.block("when.particles").unwrap();
    assert_eq!(b.section(), BlockSection::When);
    assert!(kit.sims.block("when.particles").is_some());
}

#[test]
fn particles_hitting_a_shape_can_break_it() {
    let (mut scene, id, kit) = burst_in_circle(40, AtShapes::Bounce, false);
    let shape = shape_of(&scene);
    let resolved = resolve(&scene, &kit.registry);
    assert!(is_beside(&scene, &kit.sims, id, &resolved.things[&id]));
    lane(
        &mut scene,
        &kit,
        &[
            ("when.particles", vec![("moment", Value::Choice("hit_a_shape".into())), ("at_least", Value::Int(3))]),
            ("do.break", vec![("who", Value::Who(Who::Other))]),
        ],
    );
    let resolved = resolve(&scene, &kit.registry);
    assert!(!is_beside(&scene, &kit.sims, id, &resolved.things[&id]), "a rule reads it, so it steps with the physics");
    assert!(main_part(&scene, &resolved, &kit.sims).things.contains_key(&id));
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(0.25, &kit.sims, &mut log);
    assert!(!world.broken(shape), "not hit yet");
    world.advance_to(2.0, &kit.sims, &mut log);
    assert!(world.broken(shape), "the particles broke what they hit");
    if let Some(rules) = world.rules() {
        for r in &scene.rules.list {
            assert_eq!(rules.problem(r.id), None);
        }
    }
}

#[test]
fn a_rule_naming_another_particles_thing_leaves_this_one_beside() {
    let (mut scene, id, kit) = burst_in_circle(10, AtShapes::Bounce, false);
    let other = add_particles(&mut scene, &kit.registry, Look::Empty, ThingId::ROOT).unwrap();
    lane(
        &mut scene,
        &kit,
        &[("when.particles", vec![("who", Value::Who(Who::Thing { thing: other, ball: None }))]), ("do.break", vec![])],
    );
    let resolved = resolve(&scene, &kit.registry);
    assert!(is_beside(&scene, &kit.sims, id, &resolved.things[&id]));
    assert!(!is_beside(&scene, &kit.sims, other, &resolved.things[&other]));
}

#[test]
fn a_particles_thing_in_the_physics_counts_in_its_fingerprint() {
    let (mut scene, id, kit) = burst_in_circle(10, AtShapes::Bounce, false);
    lane(&mut scene, &kit, &[("when.particles", vec![]), ("do.break", vec![("who", Value::Who(Who::Other))])]);
    let resolved = resolve(&scene, &kit.registry);
    let before = fingerprint(&scene, &resolved, &kit.sims);
    let meets = MeetsSettings { at_shapes: AtShapes::Vanish, ..MeetsSettings::default() };
    scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Bounce).unwrap().params = meets.to_params();
    let resolved = resolve(&scene, &kit.registry);
    assert_ne!(fingerprint(&scene, &resolved, &kit.sims), before);
}

#[test]
fn a_hits_pulse_on_the_rate_makes_particles_when_the_ball_bounces() {
    let kit = kit();
    let mut scene = crate::new_scene(&kit.registry);
    let id = add_particles(&mut scene, &kit.registry, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings { rate: 0.0, life: 100.0, speed: 0.0, ..ParticlesSettings::default() };
    scene.things.get_mut(&id).unwrap().params = source.to_params();
    let count = |scene: &Scene| {
        let resolved = resolve(scene, &kit.registry);
        let mut world = World::build(scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(4.0, &kit.sims, &mut log);
        assert!(log.events().iter().any(|e| e.kind == EventKind::Hit), "the ball bounces");
        state_of(&world, id).unwrap().specks.len()
    };
    assert_eq!(count(&scene), 0);
    let at = Address { thing: id, slot: None, key: "rate".into() };
    scene.set_pulse(&kit.registry, &at, PulseSource::Hits, Value::Number(2000.0), 0.3).unwrap();
    assert!(count(&scene) > 10, "a bounce makes some");
}

/// Every sprite of the frame of `scene` at `time`.
fn sprites_at(scene: &Scene, kit: &Kit, time: f64) -> (Frame, Vec<Particle>) {
    let resolved = resolve(scene, &kit.registry);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, &world, &log, time, &kit.draws, &mut frame);
    let all = frame.particles.iter().chain(&frame.particles_front).copied().collect();
    (frame, all)
}

fn fireflies() -> (Scene, ThingId, Kit) {
    let kit = kit();
    let mut scene = crate::new_studio_scene(&kit.registry);
    let id = add_particles(&mut scene, &kit.registry, Look::Fireflies, ThingId::ROOT).unwrap();
    (scene, id, kit)
}

#[test]
fn a_layer_that_gives_off_light_stamps_it_on_its_own_sprites_only_and_is_unlit() {
    let (scene, _, kit) = fireflies();
    let (_, sprites) = sprites_at(&scene, &kit, 1.0);
    let lit: Vec<_> = sprites.iter().filter(|s| s.emit[..3].iter().any(|c| *c > 0.0)).collect();
    assert!(!lit.is_empty(), "the fireflies give off light");
    assert!(lit.len() < sprites.len(), "their glow gives none");
    assert!(sprites.iter().all(|s| s.light[0] == 0.0), "a lamp layer takes no light");
}

#[test]
fn a_plain_layer_is_lit_and_carries_its_link_only_in_a_lit_scene() {
    let kit = kit();
    let mut scene = crate::new_studio_scene(&kit.registry);
    let id = add_particles(&mut scene, &kit.registry, Look::Empty, ThingId::ROOT).unwrap();
    let source =
        ParticlesSettings { emit: Emit::OnceAtTheStart, count: 20, life: 10.0, ..ParticlesSettings::default() };
    scene.things.get_mut(&id).unwrap().params = source.to_params();
    let (frame, sprites) = sprites_at(&scene, &kit, 0.5);
    assert!(frame.light.is_none());
    assert_eq!(sprites.len(), 20);
    assert!(sprites.iter().all(|s| s.light[0] == 1.0 && s.emit == [0.0; 4] && s.extra == [0.0; 4]));
    crate::things::light::add_light(&mut scene, &kit.registry, crate::things::light::LightType::Point, ThingId::ROOT)
        .unwrap();
    let (frame, sprites) = sprites_at(&scene, &kit, 0.5);
    assert!(frame.light.is_some());
    let link = crate::things::light::links::number_of(&resolve(&scene, &kit.registry), id);
    assert!(link > 0.0);
    assert!(sprites.iter().all(|s| s.extra[3] == link), "{:?}", sprites[0].extra);
}

#[test]
fn every_drawn_particle_layer_has_a_lighting_fold() {
    let kit = kit();
    for id in [SPRITES, super::look::STREAKS] {
        let defs = kit.registry.layer(id).unwrap().light_defs();
        assert!(defs.iter().any(|d| d.key == "lit"), "{id}");
        assert!(
            defs.iter().any(|d| d.key == "catches_beams"),
            "{id}: {:?}",
            defs.iter().map(|d| d.key).collect::<Vec<_>>()
        );
    }
    assert!(kit.registry.layer(super::look::SUB_LIGHT).is_some());
    let _ = KIND;
}

#[test]
fn a_sound_edit_on_particles_a_rule_keeps_runs_the_sound_again() {
    let (mut scene, id, kit) = burst_in_circle(10, AtShapes::Bounce, false);
    lane(&mut scene, &kit, &[("when.particles", vec![]), ("do.break", vec![("who", Value::Who(Who::Other))])]);
    with_sound(&mut scene, &kit, id, |_| {});
    let resolved = resolve(&scene, &kit.registry);
    assert!(!is_beside(&scene, &kit.sims, id, &resolved.things[&id]));
    let before = sound_fingerprint(&scene, &resolved, &kit.sims);
    with_sound(&mut scene, &kit, id, |v| v.moments_born = true);
    let resolved = resolve(&scene, &kit.registry);
    assert_ne!(sound_fingerprint(&scene, &resolved, &kit.sims), before);
}

#[test]
fn a_high_at_least_never_fires_and_test_flights_leave_the_particles_out() {
    let (mut scene, id, kit) = burst_in_circle(40, AtShapes::Bounce, false);
    let shape = shape_of(&scene);
    lane(
        &mut scene,
        &kit,
        &[
            ("when.particles", vec![("at_least", Value::Int(1000))]),
            ("do.break", vec![("who", Value::Who(Who::Other))]),
        ],
    );
    let resolved = resolve(&scene, &kit.registry);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(2.0, &kit.sims, &mut log);
    assert!(!world.broken(shape));
    assert!(state_of(&world, id).is_some(), "a rule keeps it in the physics");
    assert!(state_of(&world.lone(&|_| true), id).is_none(), "but never in a test flight");
}

#[test]
fn a_flame_field_carries_its_light_in_a_lit_scene() {
    let kit = kit();
    let mut scene = crate::new_studio_scene(&kit.registry);
    add_particles(&mut scene, &kit.registry, Look::FireOnABall, ThingId::ROOT).unwrap();
    crate::things::light::add_light(&mut scene, &kit.registry, crate::things::light::LightType::Point, ThingId::ROOT)
        .unwrap();
    let (frame, _) = sprites_at(&scene, &kit, 1.0);
    let fields: Vec<_> = frame.fields.iter().chain(&frame.fields_front).collect();
    assert!(!fields.is_empty(), "the flame is a field");
    for f in fields {
        assert!(f.look.emit[..3].iter().any(|c| *c > 0.0), "it gives off light");
        assert_eq!(f.look.light[0], 0.0, "a lamp takes no light");
        assert!(f.look.extra[3] > 0.0, "its link number");
    }
}
