//! Tests of the air: calm wind, balls stirring it, shapes blocking it, heat, gust waves, the grid
//! following the camera, threads.

use super::*;
use bpe_core::{Resolved, Scene};
use bpe_runtime::{Bodies, Wall};

/// An empty world (a blank project: the scene settings and a camera).
fn world() -> World {
    let kit = crate::kit();
    let scene: Scene = crate::new_studio_scene(&kit.registry);
    let resolved: Resolved = crate::resolve_scene(&scene, &kit.registry);
    World::build(&scene, &resolved, &kit.sims)
}

fn awake(spec: AirSpec, world: &World) -> Air {
    let mut air = Air::new(spec);
    air.wake(world);
    air
}

fn with_ball(world: &mut World, place: Vec2, velocity: Vec2, radius: f32) {
    let mut b = Bodies::default();
    b.push(place, velocity, radius);
    world.bodies.insert(ThingId(900), b);
}

#[test]
fn calm_air_is_the_wind_on_and_off_the_grid() {
    let w = world();
    let spec = AirSpec { wind_speed: 120.0, wind_direction: 90.0, swirls: 0.0, ..AirSpec::default() };
    let mut air = awake(spec, &w);
    assert!(air.size().0 > 10 && air.size().1 > 10, "a grid over the shot: {:?}", air.size());
    air.catch_up_to(60, &w, 1);
    for p in [Vec2::ZERO, Vec2::new(200.0, -300.0), Vec2::new(1e5, 0.0)] {
        let v = air.at(p);
        assert!((v - Vec2::new(0.0, 120.0)).length() < 0.5, "{p}: {v}");
    }
    assert_eq!(air.heat_at(Vec2::ZERO), 0.0);
}

#[test]
fn a_sleeping_air_keeps_count_and_reads_as_the_wind() {
    let w = world();
    let mut air = Air::new(AirSpec { wind_speed: 50.0, ..AirSpec::default() });
    air.catch_up_to(10, &w, 1);
    assert_eq!(air.step, 10);
    assert!(!air.awake());
    assert_eq!(air.size(), (0, 0));
    assert!((air.at(Vec2::ZERO) - Vec2::new(50.0, 0.0)).length() < 1e-3);
}

#[test]
fn a_moving_ball_drags_the_air_its_way() {
    let mut w = world();
    with_ball(&mut w, Vec2::ZERO, Vec2::new(400.0, 0.0), 40.0);
    let mut air = awake(AirSpec::default(), &w);
    air.catch_up_to(30, &w, 1);
    let middle = air.at(Vec2::ZERO);
    assert!(middle.x > 100.0, "the air in the ball goes its way: {middle}");
    // Ahead of the ball the air is pushed on too, and far off it is still.
    assert!(air.at(Vec2::new(80.0, 0.0)).x > 1.0);
    assert!(air.at(Vec2::new(0.0, 600.0)).length() < 20.0, "far off: {}", air.at(Vec2::new(0.0, 600.0)));
    // A still ball stirs nothing.
    let mut still = world();
    with_ball(&mut still, Vec2::ZERO, Vec2::ZERO, 40.0);
    let mut air = awake(AirSpec::default(), &still);
    air.catch_up_to(30, &still, 1);
    assert!(air.at(Vec2::ZERO).length() < 1e-3);
}

#[test]
fn balls_stir_less_when_told_and_not_at_all_at_zero() {
    let mut w = world();
    with_ball(&mut w, Vec2::ZERO, Vec2::new(400.0, 0.0), 40.0);
    let at = |stir: f32| {
        let mut air = awake(AirSpec { stir, ..AirSpec::default() }, &w);
        air.catch_up_to(20, &w, 1);
        air.at(Vec2::ZERO).x
    };
    assert!(at(1.0) > at(0.3));
    assert!(at(0.0).abs() < 1e-3);
}

#[test]
fn a_shape_blocks_the_wind_and_moves_the_air_in_it() {
    let mut w = world();
    let outline = Outline::Ring { center: Vec2::ZERO, radius: 150.0, sides: 0, angle: 0.0, thickness: 4.0 };
    let wall = Wall { outline, solid: true, spin: 0.0, growth: 0.0, velocity: Vec2::ZERO, passes: false };
    w.walls.insert(ThingId(901), wall.clone());
    let spec = AirSpec { wind_speed: 200.0, swirls: 0.0, ..AirSpec::default() };
    let mut air = awake(spec.clone(), &w);
    air.catch_up_to(120, &w, 1);
    assert!(air.at(Vec2::ZERO).length() < 1.0, "still inside a solid shape: {}", air.at(Vec2::ZERO));
    // Just upwind the air is slowed, and it flows on round the sides.
    let before = air.at(Vec2::new(-170.0, 0.0)).x;
    assert!(before < 150.0, "slowed before the shape: {before}");
    let side = air.at(Vec2::new(0.0, -200.0)).x;
    assert!(side > 150.0, "faster round its side: {side}");
    // Unblocked, the shape is nothing to the air.
    let mut open = awake(AirSpec { shapes_block: false, ..spec }, &w);
    open.catch_up_to(120, &w, 1);
    assert!((open.at(Vec2::ZERO).x - 200.0).abs() < 1.0);
    // A spinning shape turns the air in it.
    let mut spun = world();
    spun.walls.insert(ThingId(901), Wall { spin: 2.0, ..wall });
    let mut air = awake(AirSpec { swirls: 0.0, ..AirSpec::default() }, &spun);
    air.catch_up_to(30, &spun, 1);
    let v = air.at(Vec2::new(100.0, 0.0));
    assert!(v.y > 100.0, "the air at the right goes round with it: {v}");
}

#[test]
fn heat_lifts_the_air() {
    let w = world();
    let mut air = awake(AirSpec { swirls: 0.0, ..AirSpec::default() }, &w);
    for k in 1..=60 {
        air.add_heat(Vec2::ZERO, 5.0);
        air.catch_up_to(k, &w, 1);
    }
    assert!(air.heat_at(Vec2::ZERO) > 1.0);
    let v = air.at(Vec2::ZERO);
    assert!(v.y < -10.0, "warm air rises: {v}");
    // No heat in, none left after a while.
    air.catch_up_to(60 + 120 * 20, &w, 1);
    assert!(air.heat_at(Vec2::ZERO) < 1e-3);
}

#[test]
fn gust_waves_travel_along_the_wind() {
    let way = Vec2::X;
    let (a, b) = (Vec2::ZERO, Vec2::new(300.0, 0.0));
    // All at once: the same gust everywhere.
    for t in [0.0, 0.7, 1.9] {
        assert_eq!(gust_factor(0.8, 2.0, 0.0, t, a, way), gust_factor(0.8, 2.0, 0.0, t, b, way));
    }
    // As waves 600 px apart, every 2 s: 300 px on is 1 s later.
    let differs = [0.0f32, 0.5, 1.3]
        .iter()
        .any(|&t| gust_factor(0.8, 2.0, 600.0, t, a, way) != gust_factor(0.8, 2.0, 600.0, t, b, way));
    assert!(differs);
    for t in [1.0, 1.5, 2.6] {
        let here = gust_factor(0.8, 2.0, 600.0, t, b, way);
        let earlier = gust_factor(0.8, 2.0, 600.0, t - 1.0, a, way);
        assert!((here - earlier).abs() < 1e-5, "{t}: {here} {earlier}");
    }
    assert_eq!(gust_factor(0.0, 2.0, 600.0, 1.0, a, way), 1.0);
}

#[test]
fn the_grid_follows_the_camera_keeping_what_it_shares() {
    let mut w = world();
    with_ball(&mut w, Vec2::ZERO, Vec2::new(400.0, 0.0), 40.0);
    let mut air = awake(AirSpec { calm: 0.0, swirls: 0.0, ..AirSpec::default() }, &w);
    air.catch_up_to(20, &w, 1);
    let origin = air.origin();
    // The camera moves 10 cells right: the cells round the ball are still there, as they were.
    w.camera.center.x += 160.0;
    w.bodies.clear();
    let mut moved = air.clone();
    moved.place_grid(&w);
    assert_eq!(moved.origin(), origin + IVec2::new(10, 0));
    let (old, new) = (by_place(&air), by_place(&moved));
    let shared: Vec<_> = new.keys().filter(|k| old.contains_key(k)).collect();
    assert!(!shared.is_empty());
    for k in shared {
        assert_eq!(old[k], new[k]);
    }
}

#[test]
fn one_thread_and_many_give_the_same_air() {
    let mut w = world();
    with_ball(&mut w, Vec2::new(-100.0, 50.0), Vec2::new(500.0, -200.0), 50.0);
    let outline =
        Outline::Box { center: Vec2::new(150.0, 200.0), half: Vec2::new(120.0, 30.0), angle: 0.3, thickness: 6.0 };
    w.walls.insert(
        ThingId(901),
        Wall { outline, solid: true, spin: 0.0, growth: 0.0, velocity: Vec2::ZERO, passes: false },
    );
    // Fine cells, so the grid is big enough to split.
    let spec = AirSpec { cell: 8.0, wind_speed: 80.0, gusts: 0.5, ..AirSpec::default() };
    let mut one = awake(spec.clone(), &w);
    let mut many = awake(spec, &w);
    assert!(one.size().0 * one.size().1 >= THREADED_CELLS, "{:?}", one.size());
    for k in 1..=12 {
        one.add_heat(Vec2::new(0.0, 100.0), 3.0);
        many.add_heat(Vec2::new(0.0, 100.0), 3.0);
        one.catch_up_to(k, &w, 1);
        many.catch_up_to(k, &w, 4);
    }
    assert_eq!(one.velocities(), many.velocities());
    assert_eq!(one.heat, many.heat);
}

#[test]
fn a_ball_gone_wrong_spoils_nothing() {
    let mut w = world();
    let mut b = Bodies::default();
    b.push(Vec2::ZERO, Vec2::new(f32::NAN, 1.0), 40.0);
    b.push(Vec2::new(f32::INFINITY, 0.0), Vec2::X, 40.0);
    b.push(Vec2::ZERO, Vec2::new(1e30, 0.0), 40.0);
    w.bodies.insert(ThingId(900), b);
    let mut air = awake(AirSpec::default(), &w);
    air.catch_up_to(30, &w, 1);
    assert!(air.velocities().iter().all(|v| v.is_finite() && v.length() <= w.limits().ceiling + 1.0));
}

#[test]
fn a_bigger_view_is_capped() {
    let mut w = world();
    w.video = Vec2::new(40_000.0, 40_000.0);
    let air = awake(AirSpec { cell: 8.0, ..AirSpec::default() }, &w);
    let (c, r) = air.size();
    assert!(c * r <= MOST_CELLS && c * r > MOST_CELLS / 2, "{c} × {r}");
}

/// The air step's time, for the build ledger (run with `--ignored --nocapture`, in release).
#[test]
#[ignore]
fn bench_the_air_step() {
    let mut w = world();
    for i in 0..30 {
        with_ball(&mut w, Vec2::new(i as f32 * 20.0 - 300.0, 0.0), Vec2::new(300.0, 100.0), 30.0);
    }
    for (name, cell) in [("Coarse", 32.0), ("Normal", 16.0), ("Fine", 8.0)] {
        let mut air = awake(AirSpec { cell, ..AirSpec::default() }, &w);
        let threads = crate::things::particles::step::threads();
        let started = std::time::Instant::now();
        air.catch_up_to(240, &w, threads);
        let ms = started.elapsed().as_secs_f64() * 1000.0 / 240.0;
        println!("air {name}: {:?} cells, {ms:.3} ms a step on {threads} threads", air.size());
    }
}
