//! Tests of Particles things in the air (particles part 2): following it, heating it, sharing it,
//! keys on the Air card, and the fingerprints that keep air edits off the physics.

use bpe_core::{Address, Resolved, Scene, Settings, SlotId, ThingId, Value, layered_key, resolve};
use bpe_runtime::world::sound_fingerprint;
use bpe_runtime::{Log, World, beside_fingerprint, fingerprint, main_part};
use glam::Vec2;

use super::forces::{AIR, RISE};
use super::looks::{Look, add_particles};
use super::meets::MeetsSettings;
use super::sim::state_of;
use super::state::ParticlesState;
use super::{AreaShape, BornFrom, Emit, Heading, ParticlesSettings};
use crate::air::air_of;
use crate::modules::air::AirSettings;
use crate::{kit, registry};

fn run(scene: &Scene, time: f64) -> (World, Log, Resolved) {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    (world, log, resolved)
}

fn particles(world: &World, id: ThingId) -> &ParticlesState {
    state_of(world, id).expect("the thing has particles")
}

/// A crowd of still particles round the middle, with no forces and nothing pushing them.
fn still_crowd(scene: &mut Scene, parent: ThingId) -> ThingId {
    let reg = registry();
    let id = add_particles(scene, &reg, Look::Empty, parent).unwrap();
    let s = ParticlesSettings {
        from: BornFrom::AnArea,
        area: AreaShape::Circle,
        radius: 300.0,
        emit: Emit::OnceAtTheStart,
        count: 1500,
        life: 100.0,
        speed: 0.0,
        heading: Heading::AllRound,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = s.to_params();
    let card = scene.card_mut(id, SlotId::Bounce).unwrap();
    card.params = MeetsSettings { push: 0.0, ..MeetsSettings::default() }.to_params();
    id
}

/// Adds a force of `kind` to `id`'s Motion card with `settings`.
fn add_force(scene: &mut Scene, id: ThingId, kind: &str, settings: &[(&str, Value)]) {
    let reg = registry();
    let layer = scene.add_layer(&reg, id, SlotId::Motion, kind).unwrap();
    let card = scene.card_mut(id, SlotId::Motion).unwrap();
    for (key, v) in settings {
        card.params.set(&layered_key(layer, key), v.clone());
    }
}

fn set_air(scene: &mut Scene, f: impl FnOnce(&mut AirSettings)) {
    let reg = registry();
    super::air::ensure_air_card(scene, &reg).unwrap();
    let card = scene.card_mut(ThingId::ROOT, SlotId::Air).unwrap();
    let mut a = AirSettings::from_params(&card.params);
    f(&mut a);
    card.params = a.to_params();
}

fn mean_velocity(st: &ParticlesState) -> Vec2 {
    st.specks.iter().map(|s| s.velocity).sum::<Vec2>() / st.specks.len().max(1) as f32
}

#[test]
fn following_the_air_carries_particles_on_the_wind() {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let id = still_crowd(&mut scene, ThingId::ROOT);
    set_air(&mut scene, |a| {
        a.wind_speed = 300.0;
        a.wind_direction = 0.0;
    });
    let (world, _, _) = run(&scene, 1.0);
    assert!(mean_velocity(particles(&world, id)).length() < 1e-3, "without Follow the air they stay still");
    add_force(&mut scene, id, AIR, &[("grip", Value::Number(20.0))]);
    let (world, _, _) = run(&scene, 1.0);
    let v = mean_velocity(particles(&world, id));
    assert!((v - Vec2::new(300.0, 0.0)).length() < 10.0, "carried on the wind: {v}");
    assert!(air_of(&world).is_some_and(|a| a.awake()));
}

#[test]
fn a_ball_moves_particles_through_the_air_it_stirs() {
    let reg = registry();
    // The test arena: a ball bouncing in a ring.
    let mut scene = crate::new_scene(&reg);
    let id = still_crowd(&mut scene, ThingId::ROOT);
    add_force(&mut scene, id, AIR, &[]);
    let (world, _, _) = run(&scene, 2.0);
    let st = particles(&world, id);
    let fastest = st.specks.iter().map(|s| s.velocity.length()).fold(0.0, f32::max);
    assert!(fastest > 20.0, "the ball's wake carries some of them (Balls push is 0): {fastest}");
    // With the air not stirred by balls, nothing moves them.
    set_air(&mut scene, |a| a.stir = 0.0);
    let (world, _, _) = run(&scene, 2.0);
    let st = particles(&world, id);
    assert!(st.specks.iter().all(|s| s.velocity.length() < 1e-3));
}

#[test]
fn particles_that_heat_the_air_lift_the_ones_that_follow_it() {
    let reg = registry();
    let lift = |heats: f64| {
        let mut scene = crate::new_studio_scene(&reg);
        // One thing follows the air; another, after it, warms it (with a Rise that pushes nothing).
        let follower = still_crowd(&mut scene, ThingId::ROOT);
        add_force(&mut scene, follower, AIR, &[]);
        let heater = still_crowd(&mut scene, ThingId::ROOT);
        add_force(&mut scene, heater, RISE, &[("strength", Value::Number(0.0)), ("heats", Value::Number(heats))]);
        let (world, _, _) = run(&scene, 1.5);
        assert!(
            particles(&world, heater).specks.iter().all(|s| s.velocity.length() < 1e-3),
            "the Rise itself pushes 0"
        );
        mean_velocity(particles(&world, follower)).y
    };
    assert!(lift(0.0).abs() < 1e-3);
    let up = lift(1.0);
    assert!(up < -5.0, "the warmed air rises and carries them up: {up}");
}

#[test]
fn every_thing_reads_the_one_air_in_step() {
    let reg = registry();
    let mut scene = crate::new_scene(&reg);
    let a = still_crowd(&mut scene, ThingId::ROOT);
    add_force(&mut scene, a, AIR, &[]);
    let b = still_crowd(&mut scene, ThingId::ROOT);
    add_force(&mut scene, b, AIR, &[]);
    let (world, _, _) = run(&scene, 1.0);
    let air = air_of(&world).expect("one air");
    assert_eq!(air.step, particles(&world, a).step);
    // The two crowds were born alike, so they move alike in the same air.
    let (pa, pb) = (particles(&world, a), particles(&world, b));
    let same = pa.specks.iter().zip(&pb.specks).filter(|(x, y)| x.place == y.place).count();
    assert_eq!(same, 0, "other seeds, other places");
    // At any Physics rate the air keeps step with the particles.
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("steps_per_second", Value::Int(60));
    let (world, _, _) = run(&scene, 1.0);
    assert_eq!(air_of(&world).unwrap().step, 120);
}

#[test]
fn the_air_card_moves_with_its_keys() {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let id = still_crowd(&mut scene, ThingId::ROOT);
    add_force(&mut scene, id, AIR, &[("grip", Value::Number(20.0))]);
    // It settles to the wind quickly, so the particles show the wind as keyed.
    set_air(&mut scene, |a| a.calm = 30.0);
    let wind = Address::on(ThingId::ROOT, SlotId::Air, "wind_speed");
    scene.set_key(&reg, &wind, 0.0, Value::Number(0.0)).unwrap();
    scene.set_key(&reg, &wind, 1.0, Value::Number(400.0)).unwrap();
    let (world, _, _) = run(&scene, 2.0);
    assert_eq!(air_of(&world).unwrap().spec.wind_speed, 400.0);
    assert!(mean_velocity(particles(&world, id)).x > 350.0);
}

#[test]
fn air_edits_restart_only_the_particles() {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = crate::new_scene(reg);
    let id = still_crowd(&mut scene, ThingId::ROOT);
    add_force(&mut scene, id, AIR, &[]);
    let prints = |scene: &Scene| {
        let r = resolve(scene, reg);
        (
            fingerprint(scene, &r, &kit.sims),
            sound_fingerprint(scene, &r, &kit.sims),
            beside_fingerprint(scene, &r, &kit.sims),
        )
    };
    let before = prints(&scene);
    set_air(&mut scene, |a| a.wind_speed = 250.0);
    let card = prints(&scene);
    assert_eq!((card.0, card.1), (before.0, before.1), "the balls and the sound play on");
    assert_ne!(card.2, before.2, "the particles start again");
    let wind = Address::on(ThingId::ROOT, SlotId::Air, "wind_speed");
    scene.set_key(reg, &wind, 1.0, Value::Number(10.0)).unwrap();
    let keyed = prints(&scene);
    assert_eq!((keyed.0, keyed.1), (before.0, before.1));
    assert_ne!(keyed.2, card.2);
    // The world the editor plays the balls in has no grid: the air costs nothing there.
    let resolved = resolve(&scene, reg);
    let main = World::build(&scene, &main_part(&resolved, &kit.sims), &kit.sims);
    assert!(air_of(&main).is_some_and(|a| !a.awake()));
    let whole = World::build(&scene, &resolved, &kit.sims);
    assert!(air_of(&whole).is_some_and(|a| a.awake()));
}

#[test]
fn looks_that_follow_the_air_add_the_air_card_once() {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    assert!(!scene.things[&ThingId::ROOT].slots.contains_key(&SlotId::Air));
    add_particles(&mut scene, &reg, Look::DustClouds, ThingId::ROOT).unwrap();
    assert_eq!(scene.things[&ThingId::ROOT].slots[&SlotId::Air].kind, crate::modules::air::AIR);
    add_particles(&mut scene, &reg, Look::ThickFog, ThingId::ROOT).unwrap();
    add_particles(&mut scene, &reg, Look::Snow, ThingId::ROOT).unwrap();
    let resolved = resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    // A look that doesn't follow the air leaves the scene as it is.
    let mut plain = crate::new_studio_scene(&reg);
    add_particles(&mut plain, &reg, Look::Snow, ThingId::ROOT).unwrap();
    assert!(!plain.things[&ThingId::ROOT].slots.contains_key(&SlotId::Air));
}
