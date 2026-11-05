use super::*;
use crate::testing::run;
use crate::things::emitter::{Directions, EmitterSettings, StartFrom};
use crate::{Kit, add_thing, kit, new_scene};
use bpe_core::{Resolved, Scene, Target, ThingId, resolve};

/// The new scene without its ball, with an emitter at `at` set by `f`; and `more` for anything
/// else. Gives back the emitter's id.
fn scene_with(at: Vec2, f: impl FnOnce(&mut EmitterSettings)) -> (Scene, ThingId, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    scene.remove_thing(crate::testing::BALLS).unwrap();
    let id = add_emitter(&mut scene, &kit, at, f);
    (scene, id, kit)
}

fn add_emitter(scene: &mut Scene, kit: &Kit, at: Vec2, f: impl FnOnce(&mut EmitterSettings)) -> ThingId {
    let id = add_thing(scene, &kit.registry, KIND, ThingId::ROOT).unwrap();
    let mut s = EmitterSettings::default();
    f(&mut s);
    let thing = scene.things.get_mut(&id).unwrap();
    thing.params = s.to_params();
    thing.transform.position = at;
    id
}

fn resolved(scene: &Scene, kit: &Kit) -> Resolved {
    let r = resolve(scene, &kit.registry);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    r
}

fn st(world: &World, id: ThingId) -> &EmitterState {
    emitter_state(world, id).expect("an emitter")
}

fn count(log: &Log, kind: EventKind) -> usize {
    log.events().iter().filter(|e| e.kind == kind).count()
}

#[test]
fn a_new_emitter_fires_one_round_of_ten_all_round_at_the_start() {
    let (scene, id, kit) = scene_with(Vec2::new(10.0, 20.0), |_| {});
    let r = resolved(&scene, &kit);
    let world = World::build(&scene, &r, &kit.sims);
    let b = &world.bodies[&id];
    assert_eq!(b.len(), 10);
    for i in 0..10 {
        assert_eq!(b.position[i], Vec2::new(10.0, 20.0));
        assert!((b.velocity[i].length() - 600.0).abs() < 1e-3);
        let want = (-90f32).to_radians() + std::f32::consts::TAU * i as f32 / 10.0;
        assert!(b.velocity[i].angle_to(Vec2::from_angle(want)).abs() < 1e-4, "ball {i}");
    }
    let s = st(&world, id);
    assert_eq!(s.rounds_fired, 1);
    assert_eq!(s.fired, vec![Fired { step: 0, round: 1, size: 10, launched: 10 }]);
    assert_eq!(s.rows.numbers, (0..10).collect::<Vec<_>>());
    assert!(s.rows.round.iter().all(|&r| r == 1));
    // At the start it fires once and never again.
    let (world, log) = run(&scene, &r, &kit, 3.0);
    assert_eq!(world.bodies[&id].len(), 10);
    assert_eq!(count(&log, EventKind::Spawn), 0, "a round at 0 s is there from the first frame");
}

#[test]
fn every_fires_rounds_that_grow_until_the_last() {
    let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.fire_when = FireWhen::Every;
        s.every = 1.0;
        s.rounds = 3;
        s.count = 2;
        s.grow_times = 2.0;
        s.grow_plus = 1;
    });
    let r = resolved(&scene, &kit);
    let (world, log) = run(&scene, &r, &kit, 4.5);
    let s = st(&world, id);
    let sizes: Vec<(u64, u32, u32)> = s.fired.iter().map(|f| (f.step, f.round, f.size)).collect();
    let rate = world.steps_per_second as u64;
    // 2, then 2×2+1, then 5×2+1.
    assert_eq!(sizes, vec![(0, 1, 2), (rate, 2, 5), (2 * rate, 3, 11)]);
    assert_eq!(world.bodies[&id].len(), 18);
    assert_eq!(count(&log, EventKind::Spawn), 16, "the later rounds' balls are logged as they go");
    let spawn = log.events().iter().find(|e| e.kind == EventKind::Spawn).unwrap();
    assert_eq!(spawn.step, rate);
    assert_eq!(spawn.b, Some(Party { thing: id, item: 2 }));
    // Numbers run on across rounds.
    assert_eq!(s.rows.numbers, (0..18).collect::<Vec<_>>());
}

#[test]
fn endless_rounds_keep_going_and_earlier_rounds_can_go() {
    let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.fire_when = FireWhen::Every;
        s.every = 0.5;
        s.endless = true;
        s.count = 3;
        s.earlier = Earlier::Remove;
    });
    let r = resolved(&scene, &kit);
    let (world, log) = run(&scene, &r, &kit, 2.6);
    let s = st(&world, id);
    assert_eq!(s.rounds_fired, 6);
    let b = &world.bodies[&id];
    assert_eq!(b.alive_count(), 3, "only the newest round is left");
    assert_eq!(count(&log, EventKind::Remove), 15);
    assert!((0..b.len()).filter(|&i| b.alive[i]).all(|i| s.rows.round[i] == 6));
}

#[test]
fn faded_rounds_go_once_their_fade_is_over() {
    let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.fire_when = FireWhen::Every;
        s.every = 1.0;
        s.rounds = 2;
        s.count = 4;
        s.earlier = Earlier::FadeOut;
        s.fade_time = 0.5;
    });
    let r = resolved(&scene, &kit);
    let rate = 240;
    let (world, _) = run(&scene, &r, &kit, 1.25);
    let s = st(&world, id);
    assert!((0..4).all(|i| s.rows.fade_from[i] == Some(rate)), "{:?}", s.rows.fade_from);
    assert_eq!(world.bodies[&id].alive_count(), 8, "still fading");
    let (world, _) = run(&scene, &r, &kit, 1.6);
    assert_eq!(world.bodies[&id].alive_count(), 4);
}

#[test]
fn most_balls_cuts_a_round_short() {
    let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.count = 50;
        s.most_balls = 20;
    });
    let r = resolved(&scene, &kit);
    let world = World::build(&scene, &r, &kit.sims);
    assert_eq!(world.bodies[&id].len(), 20);
}

#[test]
fn a_gap_fires_the_balls_one_after_another() {
    let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.count = 4;
        s.gap = 0.25;
    });
    let r = resolved(&scene, &kit);
    let world = World::build(&scene, &r, &kit.sims);
    assert_eq!(world.bodies[&id].len(), 1);
    let (world, log) = run(&scene, &r, &kit, 0.8);
    assert_eq!(world.bodies[&id].len(), 4);
    let steps: Vec<u64> = log.events().iter().filter(|e| e.kind == EventKind::Spawn).map(|e| e.step).collect();
    assert_eq!(steps, vec![60, 120, 180]);
}

/// An emitter at the origin firing `count` balls one way to the right at a catcher 200 px off,
/// set by `f`.
fn shooter_and_catcher(count: u32, f: impl FnOnce(&mut EmitterSettings)) -> (Scene, ThingId, ThingId, Kit) {
    let (mut scene, shooter, kit) = scene_with(Vec2::ZERO, |s| {
        s.count = count;
        s.directions = Directions::OneWay;
        s.aim_angle = 0.0;
        s.start_from = StartFrom::Line;
        s.start_length = 40.0;
        s.speed = 400.0;
    });
    let catcher = add_emitter(&mut scene, &kit, Vec2::new(200.0, 0.0), |s| {
        s.emits = false;
        s.catches = Catches::Remove;
        s.catch_size = 30.0;
        f(s);
    });
    (scene, shooter, catcher, kit)
}

#[test]
fn a_catcher_takes_out_the_balls_that_reach_it_and_counts_one_wave() {
    let (scene, shooter, catcher, kit) = shooter_and_catcher(3, |_| {});
    let r = resolved(&scene, &kit);
    let (world, log) = run(&scene, &r, &kit, 1.0);
    let enters: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Enter).collect();
    assert_eq!(enters.len(), 3);
    for e in &enters {
        assert_eq!(e.a.thing, shooter);
        assert_eq!(e.b, Some(Party { thing: catcher, item: 1 }));
        assert!((e.point.distance(Vec2::new(200.0, 0.0)) - 30.0).abs() < 0.5, "{:?}", e.point);
    }
    // The middle one 170 px off at 400 px/s, a little over 0.42 s; the side ones 22 px further.
    assert_eq!(enters.iter().map(|e| (e.a.item, e.step)).collect::<Vec<_>>(), vec![(1, 102), (0, 107), (2, 107)]);
    assert_eq!(world.bodies[&shooter].alive_count(), 0);
    let s = st(&world, catcher);
    assert_eq!(s.landings.len(), 1, "one round arriving together is one wave");
    assert_eq!(s.landings[0].owner, shooter);
    assert_eq!(st(&world, shooter).rows.arrivals, vec![1, 1, 1]);
}

#[test]
fn passing_through_lets_the_balls_fly_on_but_they_still_arrive() {
    let (scene, shooter, catcher, kit) = shooter_and_catcher(1, |s| s.catches = Catches::PassThrough);
    let r = resolved(&scene, &kit);
    let (world, log) = run(&scene, &r, &kit, 1.0);
    assert_eq!(count(&log, EventKind::Enter), 1);
    assert_eq!(world.bodies[&shooter].alive_count(), 1);
    assert!(world.bodies[&shooter].position[0].x > 300.0);
    assert_eq!(st(&world, catcher).landings.len(), 1);
}

#[test]
fn sent_back_the_same_way_with_a_delay_waits_then_goes_back() {
    let (scene, shooter, _, kit) = shooter_and_catcher(1, |s| {
        s.catches = Catches::SendBackOut;
        s.return_way = ReturnWay::SameWayBack;
        s.return_delay = 0.5;
    });
    let r = resolved(&scene, &kit);
    let (world, _) = run(&scene, &r, &kit, 0.7);
    let b = &world.bodies[&shooter];
    assert!(b.held[0]);
    assert!((b.position[0] - Vec2::new(170.0, 0.0)).length() < 0.5, "held where it came in: {:?}", b.position[0]);
    let (world, _) = run(&scene, &r, &kit, 1.2);
    let b = &world.bodies[&shooter];
    assert!(!b.held[0]);
    assert!(b.velocity[0].x < -399.0, "going back: {:?}", b.velocity[0]);
    assert!(b.position[0].x < 170.0);
}

#[test]
fn out_my_way_fires_a_caught_ball_the_catchers_way() {
    let (scene, shooter, _, kit) = shooter_and_catcher(1, |s| {
        s.catches = Catches::SendBackOut;
        s.return_way = ReturnWay::OutMyWay;
        s.directions = Directions::OneWay;
        s.aim_angle = 90.0;
        s.speed = 300.0;
    });
    let r = resolved(&scene, &kit);
    let (world, _) = run(&scene, &r, &kit, 0.6);
    let v = world.bodies[&shooter].velocity[0];
    assert!((v - Vec2::new(0.0, 300.0)).length() < 1e-2, "{v:?}");
}

#[test]
fn a_wave_landing_fires_the_next_round_and_chains() {
    // A fires at B; B fires (back at A) one round when a wave of A's lands on it.
    let (mut scene, a, b, kit) = shooter_and_catcher(2, |s| {
        s.emits = true;
        s.fire_when = FireWhen::WhenAWaveLands;
        s.wave_first = false;
        s.directions = Directions::OneWay;
        s.aim_angle = 180.0;
        s.count = 1;
        s.rounds = 5;
    });
    scene.things.get_mut(&a).unwrap().params.set("catches", bpe_core::Value::Choice("remove".into()));
    let r = resolved(&scene, &kit);
    let (world, log) = run(&scene, &r, &kit, 0.3);
    assert_eq!(st(&world, b).rounds_fired, 0, "it waits for a wave");
    let (world, _) = run(&scene, &r, &kit, 1.5);
    let sb = st(&world, b);
    assert_eq!(sb.rounds_fired, 1);
    assert_eq!(sb.fired[0].step, sb.landings[0].step + 1, "the step after the wave lands");
    // B's ball came back to A and was caught there.
    assert_eq!(st(&world, a).landings.len(), 1);
    assert_eq!(st(&world, a).landings[0].owner, b);
    drop(log);
}

#[test]
fn catching_its_own_balls_only_leaves_others_alone() {
    let (scene, shooter, _, kit) = shooter_and_catcher(1, |s| s.catch_from = CatchFrom::ItsOwn);
    let r = resolved(&scene, &kit);
    let (world, log) = run(&scene, &r, &kit, 1.0);
    assert_eq!(count(&log, EventKind::Enter), 0);
    assert_eq!(world.bodies[&shooter].alive_count(), 1);
    let (scene, _, catcher, kit) = shooter_and_catcher(1, |s| {
        s.catch_from = CatchFrom::Chosen;
        s.catch_things = Target::Thing(crate::testing::SHAPE);
    });
    let r = resolved(&scene, &kit);
    let (world, _) = run(&scene, &r, &kit, 1.0);
    assert!(st(&world, catcher).landings.is_empty());
}

#[test]
fn it_sits_on_a_circles_middle_and_follows_it() {
    let (mut scene, id, kit) = scene_with(Vec2::new(300.0, 300.0), |s| s.sits_on = crate::testing::SHAPE.0 as i32);
    scene.things.get_mut(&crate::testing::SHAPE).unwrap().transform.position = Vec2::new(-40.0, 25.0);
    let r = resolved(&scene, &kit);
    let world = World::build(&scene, &r, &kit.sims);
    assert_eq!(world.poses[&id].position, Vec2::new(-40.0, 25.0));
    assert!(world.bodies[&id].position.iter().all(|p| *p == Vec2::new(-40.0, 25.0)));
}

#[test]
fn the_same_scene_makes_the_same_log() {
    let make = || {
        let (scene, _, _, kit) = shooter_and_catcher(6, |s| {
            s.catches = Catches::SendBackOut;
            s.return_way = ReturnWay::OutMyWay;
            s.directions = Directions::Random;
        });
        let r = resolved(&scene, &kit);
        run(&scene, &r, &kit, 3.0).1.hash()
    };
    assert_eq!(make(), make());
}

#[test]
fn a_key_on_size_resizes_its_balls() {
    let (scene, id, kit) = scene_with(Vec2::ZERO, |_| {});
    let r = resolved(&scene, &kit);
    let mut world = World::build(&scene, &r, &kit.sims);
    let mut later = r.things[&id].clone();
    later.params.set("radius", bpe_core::Value::Number(12.0));
    EmitterSim.settings_changed(id, &later, &mut world);
    assert!(world.bodies[&id].radius.iter().all(|&r| r == 12.0));
}

#[test]
fn marker_settings_dont_touch_the_physics() {
    for key in ["marker", "marker_size", "marker_colour", "marker_glow", "flash_fire", "ripple_far", "vary_colour"] {
        assert!(!EmitterSim.affects_physics(key), "{key}");
    }
    for key in ["speed", "count", "catches", "vary_size", "rounds"] {
        assert!(EmitterSim.affects_physics(key), "{key}");
    }
}

#[test]
fn the_entry_point_is_where_the_path_first_meets_the_circle() {
    let c = Vec2::ZERO;
    assert_eq!(entry(Vec2::new(-20.0, 0.0), Vec2::new(0.0, 0.0), c, 10.0), Some(Vec2::new(-10.0, 0.0)));
    assert_eq!(entry(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0), c, 10.0), None, "already inside");
    assert_eq!(entry(Vec2::new(-20.0, 20.0), Vec2::new(20.0, 20.0), c, 10.0), None, "passes by");
    assert!(entry(Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0), c, 10.0).is_some(), "straight through");
}
