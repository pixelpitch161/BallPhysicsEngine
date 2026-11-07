//! Tests of the Particles thing: births, forces, meeting things, replays and drawing.

use bpe_core::{Address, Resolved, Scene, Settings, SlotId, ThingId, Value, resolve};
use bpe_gfx::{Frame, build_frame};
use bpe_runtime::{Log, Playback, World, main_part};
use glam::Vec2;

use super::forces::{DRAG, GRAVITY};
use super::look::{Only, SPRITES, SUB_GLOW, SpritesLayer};
use super::looks::{Look, add_particles};
use super::meets::{AtBalls, AtEdges, AtShapes, MeetsSettings};
use super::sim::state_of;
use super::state::ParticlesState;
use super::step::{self, hash_of};
use super::{BornFrom, Emit, Heading, KIND, ParticlesSettings};
use crate::{kit, registry};

/// A blank project with one Particles thing of `look` under the root.
fn scene_with(look: Look) -> (Scene, ThingId) {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let id = add_particles(&mut scene, &reg, look, ThingId::ROOT).unwrap();
    (scene, id)
}

/// The test scene (a hollow circle and one ball) with an empty Particles thing under the root.
fn arena_with_particles() -> (Scene, ThingId) {
    let reg = registry();
    let mut scene = crate::new_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    (scene, id)
}

fn set_source(scene: &mut Scene, id: ThingId, s: &ParticlesSettings) {
    scene.things.get_mut(&id).unwrap().params = s.to_params();
}

fn set_meets(scene: &mut Scene, id: ThingId, m: &MeetsSettings) {
    scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Bounce).unwrap().params = m.to_params();
}

/// The world of `scene` run to `time` seconds, with its resolve.
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

#[test]
fn every_look_adds_a_thing_that_resolves_and_makes_particles() {
    let reg = registry();
    for look in Look::all() {
        let (scene, id) = scene_with(look);
        assert_eq!(scene.things[&id].kind, KIND);
        assert_eq!(scene.things[&id].name, look.name(), "named after its look");
        let (world, _, _) = run(&scene, 2.0);
        // Looks let off by hits need balls to hit something.
        let on_hits = ParticlesSettings::from_params(&scene.things[&id].params).emit == Emit::OnHits;
        assert!(on_hits || !particles(&world, id).specks.is_empty(), "{look:?} made no particles in 2 s");
        // Inside a ball group, the riding looks ride the balls.
        let mut scene = crate::new_scene(&reg);
        let ball = scene.order.iter().copied().find(|t| scene.things[t].kind == "balls").unwrap();
        let id = add_particles(&mut scene, &reg, look, ball).unwrap();
        let (world, _, _) = run(&scene, 2.0);
        assert!(!particles(&world, id).specks.is_empty(), "{look:?} inside a ball made no particles");
    }
    let mut scene = crate::new_scene(&reg);
    let ball = scene.order.iter().copied().find(|t| scene.things[t].kind == "balls").unwrap();
    let fire = add_particles(&mut scene, &reg, Look::FireOnABall, ball).unwrap();
    let s = ParticlesSettings::from_params(&scene.things[&fire].params);
    assert_eq!(s.from, BornFrom::RidingItsParent);
    let alone = add_particles(&mut scene, &reg, Look::FireOnABall, ThingId::ROOT).unwrap();
    assert_eq!(ParticlesSettings::from_params(&scene.things[&alone].params).from, BornFrom::APoint);
}

#[test]
fn a_particles_thing_gets_its_own_cards_and_no_ball_cards() {
    let reg = registry();
    let (scene, id) = scene_with(Look::Empty);
    let kinds: Vec<(SlotId, &str)> = scene.things[&id].slots.iter().map(|(s, c)| (*s, c.kind.as_str())).collect();
    assert_eq!(
        kinds,
        [
            (SlotId::Look, super::look::CARD),
            (SlotId::Motion, super::forces::CARD),
            (SlotId::Bounce, super::meets::CARD)
        ]
    );
    let kind = reg.thing(KIND).unwrap();
    for slot in [SlotId::Motion, SlotId::Bounce, SlotId::Look] {
        let fitting: Vec<&str> = reg.kinds_fitting(slot, kind).map(|k| k.id()).collect();
        assert_eq!(fitting.len(), 1, "{slot:?}: {fitting:?}");
    }
    let balls = reg.thing("balls").unwrap();
    assert!(reg.kinds_fitting(SlotId::Motion, balls).all(|k| k.id() != super::forces::CARD));
}

#[test]
fn particles_never_move_the_balls() {
    let (mut scene, id) = arena_with_particles();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        rate: 2000.0,
        speed: 900.0,
        heading: Heading::AllRound,
        life: 3.0,
        ..ParticlesSettings::default()
    };
    set_source(&mut scene, id, &source);
    set_meets(
        &mut scene,
        id,
        &MeetsSettings { at_balls: AtBalls::BounceOff, at_shapes: AtShapes::Bounce, ..MeetsSettings::default() },
    );
    let kit = kit();
    let resolved = resolve(&scene, &kit.registry);
    let mut full = World::build(&scene, &resolved, &kit.sims);
    let mut main = World::build(&scene, &main_part(&resolved, &kit.sims), &kit.sims);
    assert!(full.beside.contains(&id) && state_of(&main, id).is_none());
    let (mut a, mut b) = (Log::default(), Log::default());
    full.advance_to(3.0, &kit.sims, &mut a);
    main.advance_to(3.0, &kit.sims, &mut b);
    assert!(!particles(&full, id).specks.is_empty());
    assert_eq!(a.hash(), b.hash(), "the same hits");
    for (thing, bodies) in &main.bodies {
        assert_eq!(full.bodies[thing].position, bodies.position, "the balls are where they'd be without particles");
    }
    // And the fingerprint that restarts the physics doesn't hear the particles.
    let before = bpe_runtime::fingerprint(&scene, &resolved, &kit.sims);
    set_source(&mut scene, id, &ParticlesSettings { rate: 5.0, ..source });
    let after = bpe_runtime::fingerprint(&scene, &resolve(&scene, &kit.registry), &kit.sims);
    assert_eq!(before, after);
}

#[test]
fn any_number_of_threads_makes_the_same_particles() {
    let (mut scene, id) = scene_with(Look::Rain);
    let mut s = ParticlesSettings::from_params(&scene.things[&id].params);
    s.rate = 60_000.0;
    set_source(&mut scene, id, &s);
    let (world, log, _) = run(&scene, 1.0);
    let st = particles(&world, id).clone();
    assert!(st.specks.len() > 3 * step::CHUNK, "enough for several chunks: {}", st.specks.len());
    let mut hashes = Vec::new();
    for threads in [1, 2, 3, 8] {
        let mut copy = st.clone();
        for _ in 0..10 {
            step::step(&mut copy, &world, &log, threads);
        }
        hashes.push(hash_of(&copy));
    }
    assert!(hashes.windows(2).all(|w| w[0] == w[1]), "{hashes:?}");
}

#[test]
fn a_seek_replays_the_same_particles() {
    let (scene, id) = scene_with(Look::Snow);
    let kit = kit();
    let resolved = resolve(&scene, &kit.registry);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(3.0, &kit.sims, &mut log);
    let straight = hash_of(particles(&world, id));
    let mut p = Playback::new(&scene, &resolved, kit.sims.clone());
    p.seek(3.0);
    assert_eq!(hash_of(particles(p.world(), id)), straight);
    p.seek(1.0);
    p.seek(3.0);
    assert_eq!(hash_of(particles(p.world(), id)), straight, "back and forth");
    let (again, _, _) = run(&scene, 3.0);
    assert_eq!(hash_of(particles(&again, id)), straight, "a fresh run");
}

#[test]
fn steady_births_come_at_the_rate_and_stop_at_most_particles() {
    let (mut scene, id) = scene_with(Look::Empty);
    let source = ParticlesSettings { rate: 120.0, life: 100.0, speed: 0.0, ..ParticlesSettings::default() };
    set_source(&mut scene, id, &source);
    let (world, _, _) = run(&scene, 2.0);
    let n = particles(&world, id).specks.len() as i64;
    assert!((n - 240).abs() <= 2, "{n}");
    set_source(&mut scene, id, &ParticlesSettings { rate: 10_000.0, most: 500, ..source.clone() });
    let (world, _, _) = run(&scene, 1.0);
    let st = particles(&world, id);
    assert_eq!(st.specks.len(), 500);
    assert!(st.births > 9_000, "births past the cap are still counted: {}", st.births);
    // Once at the start makes Count and no more; a crowd stays about Count.
    set_source(&mut scene, id, &ParticlesSettings { emit: Emit::OnceAtTheStart, count: 77, ..source.clone() });
    let (world, _, _) = run(&scene, 1.0);
    assert_eq!(particles(&world, id).specks.len(), 77);
    set_source(&mut scene, id, &ParticlesSettings { emit: Emit::KeepACrowd, count: 40, life: 0.3, ..source });
    let (world, _, _) = run(&scene, 2.0);
    let n = particles(&world, id).specks.len();
    assert!((36..=40).contains(&n), "{n}");
}

#[test]
fn particles_die_at_the_end_of_their_life() {
    let (mut scene, id) = scene_with(Look::Empty);
    let source = ParticlesSettings {
        emit: Emit::OnceAtTheStart,
        count: 50,
        life: 1.0,
        life_spread: 0.0,
        ..ParticlesSettings::default()
    };
    set_source(&mut scene, id, &source);
    let (world, _, _) = run(&scene, 0.9);
    assert_eq!(particles(&world, id).specks.len(), 50);
    let (world, _, _) = run(&scene, 1.1);
    assert!(particles(&world, id).specks.is_empty());
}

#[test]
fn gravity_and_drag_move_them_as_forces_in_turn() {
    let reg = registry();
    let (mut scene, id) = scene_with(Look::Empty);
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count: 10,
        speed: 0.0,
        speed_spread: 0.0,
        life: 10.0,
        ..ParticlesSettings::default()
    };
    set_source(&mut scene, id, &source);
    scene.add_layer(&reg, id, SlotId::Motion, GRAVITY).unwrap();
    let (world, _, _) = run(&scene, 1.0);
    let fall = step::middle(particles(&world, id)).unwrap().y;
    // ½·400·1² = 200 px, near enough in 1/120 s steps.
    assert!((fall - 200.0).abs() < 5.0, "{fall}");
    scene.add_layer(&reg, id, SlotId::Motion, DRAG).unwrap();
    let (world, _, _) = run(&scene, 1.0);
    let slowed = step::middle(particles(&world, id)).unwrap().y;
    assert!(slowed < fall - 20.0, "drag slows the fall: {slowed} vs {fall}");
}

#[test]
fn a_keyed_rate_changes_the_births_as_it_plays() {
    let reg = registry();
    let (mut scene, id) = scene_with(Look::Empty);
    let source = ParticlesSettings { rate: 0.0, life: 100.0, speed: 0.0, ..ParticlesSettings::default() };
    set_source(&mut scene, id, &source);
    let at = Address { thing: id, slot: None, key: "rate".into() };
    scene.set_key(&reg, &at, 1.0, Value::Number(0.0)).unwrap();
    scene.set_key(&reg, &at, 1.05, Value::Number(600.0)).unwrap();
    let (world, _, _) = run(&scene, 0.9);
    assert!(particles(&world, id).specks.is_empty());
    let (world, _, _) = run(&scene, 2.0);
    let n = particles(&world, id).specks.len();
    assert!((540..=600).contains(&n), "{n}");
}

#[test]
fn vanishing_at_the_edges_keeps_rain_from_piling_up() {
    let (mut scene, id) = scene_with(Look::Rain);
    let mut s = ParticlesSettings::from_params(&scene.things[&id].params);
    s.life = 1000.0;
    set_source(&mut scene, id, &s);
    let (world, _, _) = run(&scene, 6.0);
    let n = particles(&world, id).specks.len() as f32;
    assert!(n < s.rate * 4.0, "rain leaves at the bottom: {n}");
    set_meets(&mut scene, id, &MeetsSettings { at_edges: AtEdges::Pass, ..MeetsSettings::default() });
    let (world, _, _) = run(&scene, 6.0);
    assert!(particles(&world, id).specks.len() as f32 > n * 1.2, "passing the edges, they stay");
}

#[test]
fn bouncing_particles_stay_inside_a_circle() {
    let (mut scene, id) = arena_with_particles();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count: 400,
        speed: 900.0,
        heading: Heading::AllRound,
        life: 20.0,
        ..ParticlesSettings::default()
    };
    set_source(&mut scene, id, &source);
    set_meets(
        &mut scene,
        id,
        &MeetsSettings { at_shapes: AtShapes::Bounce, bounciness: 1.0, friction: 0.0, ..MeetsSettings::default() },
    );
    let (world, _, _) = run(&scene, 4.0);
    let st = particles(&world, id);
    assert_eq!(st.specks.len(), 400);
    let wall = world.walls.values().next().unwrap();
    let centre = match &wall.outline {
        bpe_runtime::Outline::Ring { center, .. } => *center,
        _ => Vec2::ZERO,
    };
    let outside = st.specks.iter().filter(|s| s.place.distance(centre) > 508.0 + 4.0).count();
    assert_eq!(outside, 0, "every one bounced back in");
    // With Vanish they go at the wall.
    set_meets(&mut scene, id, &MeetsSettings { at_shapes: AtShapes::Vanish, ..MeetsSettings::default() });
    let (world, _, _) = run(&scene, 2.0);
    assert!(particles(&world, id).specks.is_empty());
}

#[test]
fn riding_each_ball_births_on_the_balls() {
    let reg = registry();
    let mut scene = crate::new_scene(&reg);
    let ball = scene.order.iter().copied().find(|t| scene.things[t].kind == "balls").unwrap();
    let id = add_particles(&mut scene, &reg, Look::Empty, ball).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::RidingItsParent,
        rate: 60.0,
        speed: 0.0,
        life: 0.05,
        ..ParticlesSettings::default()
    };
    set_source(&mut scene, id, &source);
    let (world, _, _) = run(&scene, 1.0);
    let st = particles(&world, id);
    assert!(!st.specks.is_empty());
    let bodies = &world.bodies[&ball];
    for s in &st.specks {
        assert!(
            s.place.distance(bodies.position[0]) < bodies.radius[0] + 30.0,
            "{:?} vs {:?}",
            s.place,
            bodies.position[0]
        );
        assert_eq!(s.ball, 1, "it notes its ball");
    }
}

/// The frame of `scene` at `time`.
fn frame_of(scene: &Scene, time: f64) -> (Frame, usize, ThingId) {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    let id = scene.order.iter().copied().find(|t| scene.things[t].kind == KIND).unwrap();
    let count = particles(&world, id).specks.len();
    let mut frame = Frame::default();
    build_frame(scene, &resolved, &world, &log, time, &kit.draws, &mut frame);
    (frame, count, id)
}

#[test]
fn each_layer_draws_every_particle_with_its_sub_layers() {
    let reg = registry();
    let (mut scene, id) = scene_with(Look::Empty);
    let source =
        ParticlesSettings { emit: Emit::OnceAtTheStart, count: 64, life: 10.0, ..ParticlesSettings::default() };
    set_source(&mut scene, id, &source);
    let (frame, n, _) = frame_of(&scene, 0.5);
    assert_eq!(n, 64);
    assert_eq!(frame.particles.len(), 64, "one sprite each, behind the balls");
    assert!(frame.particles.iter().all(|p| p.color[3] > 0.0), "Normal covers");
    // A Glow adds one more each, under it.
    let look = scene.things[&id].slots[&SlotId::Look].layers[0].id;
    scene.add_sub_layer(&reg, id, SlotId::Look, look, SUB_GLOW).unwrap();
    let (frame, _, _) = frame_of(&scene, 0.5);
    assert_eq!(frame.particles.len(), 128);
    assert!(frame.particles[..64].iter().all(|p| p.color[3] < 0.0), "the glow adds light");
}

#[test]
fn a_share_and_the_rest_split_the_particles_between_two_layers() {
    let reg = registry();
    let (mut scene, id) = scene_with(Look::Empty);
    let source =
        ParticlesSettings { emit: Emit::OnceAtTheStart, count: 400, life: 10.0, ..ParticlesSettings::default() };
    set_source(&mut scene, id, &source);
    let first = scene.things[&id].slots[&SlotId::Look].layers[0].id;
    let second = scene.add_layer(&reg, id, SlotId::Look, SPRITES).unwrap();
    let card = scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    for (layer, only) in [(first, Only::AShare), (second, Only::TheRest)] {
        let p = SpritesLayer { only, share: 0.25, blend: bpe_core::Blend::Add, ..SpritesLayer::default() }.to_params();
        for (k, v) in p.0 {
            card.params.0.insert(bpe_core::layered_key(layer, &k), v);
        }
    }
    let (frame, n, _) = frame_of(&scene, 0.5);
    assert_eq!(frame.particles.len(), n, "each particle drawn once");
    assert!(frame.particles.iter().all(|p| p.color[3] < 0.0), "Add adds light");
    // About a quarter in the first layer (they are listed layer by layer).
    let first_count = {
        let resolved = resolve(&scene, &reg);
        let layers = super::look::layers_of(&resolved.things[&id].slots[&SlotId::Look]);
        let (world, _, _) = run(&scene, 0.5);
        let st = particles(&world, id);
        st.specks.iter().filter(|s| super::draw::draws(&layers[0].common, s, st.step)).count()
    };
    assert!((60..=140).contains(&first_count), "{first_count}");
}

#[test]
fn particles_keep_time_at_any_physics_rate() {
    let mut counts = Vec::new();
    for rate in [60u32, 240, 480, 1000] {
        let (mut scene, id) = scene_with(Look::Snow);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("steps_per_second", bpe_core::Value::Int(rate as i64));
        let (world, _, _) = run(&scene, 1.0);
        assert_eq!(world.steps_per_second, rate);
        let st = particles(&world, id);
        assert_eq!(st.step, 120, "a second is 120 particle steps at {rate} per s");
        counts.push(st.births);
    }
    assert!(counts.windows(2).all(|w| w[0] == w[1]), "as many born at every rate: {counts:?}");
}
