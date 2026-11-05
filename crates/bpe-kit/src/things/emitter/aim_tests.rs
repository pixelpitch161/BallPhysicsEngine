use super::*;
use crate::testing::SHAPE;
use crate::things::emitter::sim::EmitterState;
use crate::things::emitter::test_scenes::{add_emitter, resolved, scene_with, shooter_and_catcher};
use crate::things::emitter::{Catches, Directions, FireWhen, ReturnWay, SitsFocus, StartFrom};
use crate::{Kit, ShapeSettings, add_thing};
use bpe_core::{Attach, Scene, Settings, Value, resolve};
use bpe_runtime::{EventKind, Log};

const DT: f32 = 1.0 / 240.0;

/// Moves a ball the way the world does, one substep a step, for `n` steps.
fn simulate(p0: Vec2, v0: Vec2, a: Vec2, n: u64) -> Vec2 {
    let (mut p, mut v) = (p0, v0);
    for _ in 0..n {
        v += a * DT;
        p += v * DT;
    }
    p
}

#[test]
fn straight_in_lands_on_the_point_in_the_worlds_own_steps() {
    for a in [Vec2::ZERO, Vec2::new(0.0, 980.0), Vec2::new(300.0, -500.0)] {
        for n in [1u64, 7, 240, 481] {
            let delta = Vec2::new(420.0, -130.0);
            let v = straight_in(delta, a, n, DT);
            let end = simulate(Vec2::ZERO, v, a, n);
            assert!((end - delta).length() < 0.05, "a {a} n {n}: {end}");
            assert!((path_at(Vec2::ZERO, v, a, n as f64, DT) - end).length() < 0.05);
        }
    }
}

#[test]
fn the_path_between_steps_is_straight_as_the_world_moves() {
    let (v, a) = (Vec2::new(200.0, -300.0), Vec2::new(0.0, 980.0));
    let (p3, p4) = (simulate(Vec2::ZERO, v, a, 3), simulate(Vec2::ZERO, v, a, 4));
    let mid = path_at(Vec2::ZERO, v, a, 3.25, DT);
    assert!((mid - p3.lerp(p4, 0.25)).length() < 1e-3, "{mid} {p3} {p4}");
}

#[test]
fn keeping_a_direction_under_gravity_passes_through_the_point() {
    let a = Vec2::new(0.0, 980.0);
    let delta = Vec2::new(400.0, 0.0);
    for deg in [-80.0f32, -60.0, -45.0, -20.0] {
        let u = Vec2::from_angle(deg.to_radians());
        let k = keep_way(delta, a, u, 600.0, DT).unwrap_or_else(|| panic!("{deg}° reaches"));
        let at = path_at(Vec2::ZERO, u * k.speed, a, k.steps, DT);
        assert!((at - delta).length() < 1e-2, "{deg}°: {at} after {} steps", k.steps);
    }
    // Pointing down and away, or up and backwards, it can't.
    assert_eq!(keep_way(delta, a, Vec2::from_angle(30f32.to_radians()), 600.0, DT), None);
    assert_eq!(keep_way(delta, a, Vec2::from_angle(-120f32.to_radians()), 600.0, DT), None);
}

#[test]
fn without_a_pull_only_a_way_through_the_point_reaches_at_speed() {
    let delta = Vec2::new(300.0, 0.0);
    let k = keep_way(delta, Vec2::ZERO, Vec2::X, 600.0, DT).unwrap();
    assert_eq!(k.speed, 600.0);
    assert!((k.steps - 120.0).abs() < 1e-4, "{}", k.steps);
    assert_eq!(keep_way(delta, Vec2::ZERO, Vec2::Y, 600.0, DT), None);
    assert_eq!(keep_way(delta, Vec2::ZERO, -Vec2::X, 600.0, DT), None, "not backwards");
    // A pull along the way only: it gets there sooner.
    let k = keep_way(delta, Vec2::new(500.0, 0.0), Vec2::X, 600.0, DT).unwrap();
    let at = path_at(Vec2::ZERO, Vec2::X * 600.0, Vec2::new(500.0, 0.0), k.steps, DT);
    assert!(k.steps < 120.0 && (at - delta).length() < 1e-2, "{at} {}", k.steps);
}

/// A blank scene (no shape) with gravity `g`, an emitter at the origin set by `f` whose balls
/// fall by Gravity × `scale`, and a still target emitter at `target`.
fn aimed(g: Vec2, scale: f32, target: Vec2, f: impl FnOnce(&mut EmitterSettings)) -> (Scene, ThingId, ThingId, Kit) {
    let (mut scene, me, kit) = scene_with(Vec2::ZERO, |_| {});
    scene.remove_thing(SHAPE).unwrap();
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([g.x, g.y]));
    let to = add_emitter(&mut scene, &kit, target, |s| s.emits = false);
    let mut s = EmitterSettings::default();
    f(&mut s);
    s.aim_target = to.0 as i32;
    scene.things.get_mut(&me).unwrap().params = s.to_params();
    scene.card_mut(me, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(scale as f64));
    (scene, me, to, kit)
}

fn st(world: &World, id: ThingId) -> &EmitterState {
    emitter_state(world, id).expect("an emitter")
}

/// Steps `world` to `until` (a step), giving each live ball of `me` the nearest it came to the
/// moving point `target(world)` within one step either side of `around`.
fn nearest(
    world: &mut World,
    kit: &Kit,
    log: &mut Log,
    me: ThingId,
    around: u64,
    target: impl Fn(&World) -> Vec2,
) -> Vec<f32> {
    while world.step + 1 < around {
        world.step(&kit.sims, log);
    }
    let mut best = vec![f32::INFINITY; world.bodies[&me].len()];
    while world.step < around + 1 {
        world.step(&kit.sims, log);
        let t = target(world);
        let b = &world.bodies[&me];
        best.resize(b.len(), f32::INFINITY);
        for (i, best) in best.iter_mut().enumerate() {
            if !b.alive[i] {
                continue;
            }
            // The nearest point of this step's straight move.
            let (from, to) = (b.previous[i], b.position[i]);
            let d = to - from;
            let s =
                if d.length_squared() > 0.0 { ((t - from).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
            *best = best.min((from + d * s - t).length());
        }
    }
    best
}

#[test]
fn straight_in_from_a_ring_all_arrive_on_time_without_gravity() {
    let target = Vec2::new(300.0, -200.0);
    let (scene, me, _, kit) = aimed(Vec2::ZERO, 0.0, target, |s| {
        s.count = 12;
        s.start_from = StartFrom::Ring;
        s.start_radius = 80.0;
        s.aim = Aim::StraightIn;
        s.arrive_after = 1.5;
    });
    let mut world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    assert_eq!(world.bodies[&me].len(), 12, "the round at 0 s is there from the start");
    world.advance_to(1.5, &kit.sims, &mut Log::default());
    for (i, p) in world.bodies[&me].position.iter().enumerate() {
        assert!((*p - target).length() < 0.5, "ball {i} at {p}");
    }
}

#[test]
fn straight_in_with_gravity_wind_and_a_gap_all_arrive_on_time() {
    let target = Vec2::new(-250.0, 150.0);
    let (mut scene, me, _, kit) = aimed(Vec2::new(0.0, 980.0), 1.0, target, |s| {
        s.count = 8;
        s.start_from = StartFrom::Ring;
        s.start_radius = 60.0;
        s.gap = 0.05;
        s.aim = Aim::StraightIn;
        s.arrive_after = 1.25;
        s.fire_when = FireWhen::AtStart;
        s.start_delay = 0.5;
    });
    scene.card_mut(me, SlotId::Motion).unwrap().params.set("wind", Value::Vec2([80.0, 0.0]));
    let mut world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    let mut log = Log::default();
    world.advance_to(1.75, &kit.sims, &mut log);
    assert_eq!(st(&world, me).fired[0].step, 120);
    assert_eq!(world.bodies[&me].len(), 8);
    for (i, p) in world.bodies[&me].position.iter().enumerate() {
        assert!((*p - target).length() < 0.5, "ball {i} at {p}");
    }
    // Each left on its own Gap slot.
    let spawns: Vec<u64> = log.events().iter().filter(|e| e.kind == EventKind::Spawn).map(|e| e.step).collect();
    assert_eq!(spawns, (0..8).map(|j| 120 + 12 * j).collect::<Vec<u64>>());
}

#[test]
fn keeping_their_directions_under_gravity_they_pass_the_target_together() {
    let target = Vec2::new(400.0, 0.0);
    let set = |s: &mut EmitterSettings| {
        s.count = 9;
        s.directions = Directions::Fan;
        s.aim_angle = -90.0;
        s.fan_angle = 120.0;
        s.aim = Aim::KeepMyDirections;
        s.arrive_after = 2.0;
    };
    let (scene, me, _, kit) = aimed(Vec2::new(0.0, 980.0), 1.0, target, set);
    let mut world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    let mut log = Log::default();
    let best = nearest(&mut world, &kit, &mut log, me, 480, |_| target);
    let fired = st(&world, me).fired[0];
    assert!(fired.launched >= 3 && fired.launched < 9, "the ones tilted right reach: {fired:?}");
    assert_eq!(best.len(), fired.launched as usize);
    for (i, d) in best.iter().enumerate() {
        assert!(*d < 0.5, "ball {i} came no nearer than {d} px");
    }
    // They left at their own times, after the round fired, none of them at once.
    assert!(log.events().iter().filter(|e| e.kind == EventKind::Spawn).all(|e| e.step > 0));

    // Fire anyway sends every one.
    let (scene, me, _, kit) = aimed(Vec2::new(0.0, 980.0), 1.0, target, |s| {
        set(s);
        s.cant_reach = CantReach::FireAnyway;
    });
    let (world, _) = crate::testing::run(&scene, &resolved(&scene, &kit), &kit, 2.5);
    assert_eq!(st(&world, me).fired[0].launched, 9);
}

#[test]
fn keeping_their_directions_without_gravity_only_the_one_pointing_at_it_goes_at_speed() {
    let target = Vec2::new(300.0, 0.0);
    let (scene, me, _, kit) = aimed(Vec2::ZERO, 0.0, target, |s| {
        s.count = 8;
        s.aim = Aim::KeepMyDirections;
        s.arrive_after = 1.0;
        s.speed = 600.0;
    });
    let mut world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    let mut log = Log::default();
    let best = nearest(&mut world, &kit, &mut log, me, 240, |_| target);
    assert_eq!(best.len(), 1, "only ball 2 points right");
    assert!(best[0] < 0.5, "{}", best[0]);
    let spawn = log.events().iter().find(|e| e.kind == EventKind::Spawn).unwrap();
    assert!((spawn.speed - 600.0).abs() < 1e-3);
    // 300 px at 600 px/s is half a second: it leaves half a second before the arrival.
    assert_eq!(spawn.step, 121);
}

#[test]
fn aimed_at_the_other_focus_of_a_spinning_shrinking_ellipse_they_meet_it() {
    let (mut scene, me, kit) = scene_with(Vec2::ZERO, |_| {});
    let mut oval = ShapeSettings::ellipse(900.0, 1300.0, 4.0);
    oval.spin = 25.0;
    oval.shrink = 0.08;
    oval.min_size = 300.0;
    let shape = scene.things.get_mut(&SHAPE).unwrap();
    shape.params = oval.to_params();
    shape.transform.position = Vec2::new(540.0, 960.0);
    let mut s = EmitterSettings {
        count: 6,
        start_from: StartFrom::Ring,
        start_radius: 20.0,
        sits_on: SHAPE.0 as i32,
        sits_focus: SitsFocus::Focus1,
        aim: Aim::StraightIn,
        arrive_after: 1.0,
        fire_when: FireWhen::Every,
        every: 1.5,
        rounds: 2,
        ..EmitterSettings::default()
    };
    s.aim_target = 0;
    scene.things.get_mut(&me).unwrap().params = s.to_params();
    let mut world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    let mut log = Log::default();
    let other = |w: &World| crate::things::shape::foci(w, SHAPE).unwrap()[1];
    let first = nearest(&mut world, &kit, &mut log, me, 240, other);
    assert!(first.iter().all(|d| *d < 0.5), "round 1: {first:?}");
    // The second round fires from a spawn at 1.5 s, the shape smaller and turned further.
    let second = nearest(&mut world, &kit, &mut log, me, 360 + 240, other);
    assert_eq!(second.len(), 12);
    assert!(second[6..].iter().all(|d| *d < 0.5), "round 2: {second:?}");
}

#[test]
fn a_target_riding_a_moving_ball_is_met_where_it_will_be() {
    let (mut scene, me, kit) = scene_with(Vec2::ZERO, |_| {});
    scene.remove_thing(SHAPE).unwrap();
    let ball = add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
    let carrier = scene.things.get_mut(&ball).unwrap();
    carrier.transform.position = Vec2::new(200.0, -300.0);
    carrier.params.set("velocity", Value::Vec2([150.0, 40.0]));
    scene.card_mut(ball, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(0.0));
    let target = add_emitter(&mut scene, &kit, Vec2::ZERO, |s| s.emits = false);
    let rider = scene.things.get_mut(&target).unwrap();
    rider.parent = Some(ball);
    rider.attach = Attach::Ride { offset: Vec2::ZERO, turn_with_parent: false, item: Some(0) };
    let s = EmitterSettings {
        count: 5,
        start_from: StartFrom::Ring,
        start_radius: 40.0,
        aim: Aim::StraightIn,
        aim_target: target.0 as i32,
        arrive_after: 1.0,
        start_delay: 0.25,
        ..EmitterSettings::default()
    };
    scene.things.get_mut(&me).unwrap().params = s.to_params();
    scene.card_mut(me, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(0.0));
    let r = resolve(&scene, &kit.registry);
    let mut world = World::build(&scene, &r, &kit.sims);
    let mut log = Log::default();
    // Its middle is where the world has its pose (a rider follows its ball a step behind).
    let best = nearest(&mut world, &kit, &mut log, me, 60 + 240, |w| w.poses[&target].position);
    assert_eq!(best.len(), 5);
    assert!(best.iter().all(|d| *d < 0.5), "{best:?}");
}

#[test]
fn too_far_to_reach_under_the_ceiling_is_skipped_or_fired_anyway() {
    let target = Vec2::new(30000.0, 0.0);
    let set = |s: &mut EmitterSettings| {
        s.count = 4;
        s.aim = Aim::StraightIn;
        s.arrive_after = 0.1;
    };
    let (scene, me, _, kit) = aimed(Vec2::ZERO, 0.0, target, set);
    let world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    assert_eq!(world.bodies[&me].len(), 0, "300 000 px/s is past the ceiling");
    assert_eq!(st(&world, me).fired[0].launched, 0);
    let (scene, me, _, kit) = aimed(Vec2::ZERO, 0.0, target, |s| {
        set(s);
        s.cant_reach = CantReach::FireAnyway;
    });
    let world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    assert_eq!(world.bodies[&me].len(), 4, "unaimed, at Speed");
    assert!((world.bodies[&me].velocity[0].length() - 600.0).abs() < 1e-3);
}

#[test]
fn no_target_fires_as_if_aim_were_off() {
    let (mut scene, me, _, kit) = aimed(Vec2::ZERO, 0.0, Vec2::new(100.0, 0.0), |s| s.aim = Aim::StraightIn);
    scene.things.get_mut(&me).unwrap().params.set("aim_target", Value::Int(0));
    let world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    assert_eq!(world.bodies[&me].len(), 10);
    assert!((world.bodies[&me].velocity[3].length() - 600.0).abs() < 1e-3);
    assert_eq!(check(&world, me).map(|c| c.no_target), Some(true));
}

#[test]
fn a_caught_ball_sent_out_my_way_reaches_the_target_on_time() {
    let (mut scene, _, catcher, kit) = shooter_and_catcher(1, |s| {
        s.catches = Catches::SendBackOut;
        s.return_way = ReturnWay::OutMyWay;
        s.aim = Aim::StraightIn;
        s.arrive_after = 1.0;
    });
    scene.remove_thing(SHAPE).unwrap();
    let target = add_emitter(&mut scene, &kit, Vec2::new(200.0, 400.0), |s| s.emits = false);
    scene.things.get_mut(&catcher).unwrap().params.set("aim_target", Value::Int(target.0 as i64));
    let shooter = scene.order.iter().copied().find(|id| *id != catcher && *id != target && *id != ThingId::ROOT);
    let shooter = shooter.unwrap();
    // The catcher's own balls would fall; the shooter's, the one caught, don't.
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 600.0]));
    scene.card_mut(catcher, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(1.0));
    scene.card_mut(shooter, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(0.0));
    let mut world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    let mut log = Log::default();
    while !log.events().iter().any(|e| e.kind == EventKind::Enter) {
        world.step(&kit.sims, &mut log);
        assert!(world.step < 600, "it is caught");
    }
    let caught = world.step;
    world.advance_to((caught + 1 + 240) as f64 / 240.0, &kit.sims, &mut log);
    let p = world.bodies[&shooter].position[0];
    assert!((p - Vec2::new(200.0, 400.0)).length() < 0.5, "{p}");
}

/// Straight in, with rounds still to come after the first.
fn rounds_to_come(s: &mut EmitterSettings) {
    s.aim = Aim::StraightIn;
    s.fire_when = FireWhen::Every;
    s.rounds = 3;
}

#[test]
fn the_check_warns_about_drag_and_walls_in_the_way() {
    let target = Vec2::new(300.0, 0.0);
    let (mut scene, me, _, kit) = aimed(Vec2::ZERO, 0.0, target, rounds_to_come);
    let world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    let quiet = check(&world, me).unwrap();
    assert_eq!(quiet, AimCheck { of: 10, skip: true, ..AimCheck::default() });
    scene.card_mut(me, SlotId::Motion).unwrap().params.set("drag", Value::Number(0.5));
    let world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    assert!(check(&world, me).unwrap().in_the_way, "drag");
    // A circle between them.
    let (mut scene, me, _, kit) = aimed(Vec2::ZERO, 0.0, target, rounds_to_come);
    let wall = add_thing(&mut scene, &kit.registry, "shape", ThingId::ROOT).unwrap();
    let w = scene.things.get_mut(&wall).unwrap();
    w.params = ShapeSettings::circle(40.0, 4.0).to_params();
    w.transform.position = Vec2::new(150.0, 0.0);
    let world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    assert!(check(&world, me).unwrap().in_the_way, "a wall");
}

#[test]
fn the_next_rounds_paths_end_on_the_target() {
    let target = Vec2::new(300.0, -100.0);
    let (scene, me, _, kit) = aimed(Vec2::new(0.0, 980.0), 1.0, target, |s| {
        s.aim = Aim::StraightIn;
        s.start_from = StartFrom::Ring;
        s.fire_when = FireWhen::Every;
        s.rounds = 3;
    });
    let world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    let (aim, balls) = next_round(&world, me, 400);
    let aim = aim.unwrap();
    assert_eq!(balls.len(), 10);
    for b in &balls {
        assert!(b.reaches);
        let path = path_points(&aim, b, PATH_POINTS);
        assert_eq!(path.len(), PATH_POINTS);
        assert!((path[0] - b.start).length() < 1e-3);
        assert!((*path.last().unwrap() - target).length() < 0.5, "{:?}", path.last());
    }
}

#[test]
fn rows_that_dont_steer_an_aim_hide() {
    let shown = |s: &EmitterSettings, key: &str| {
        let p = s.to_params();
        let defs = EmitterSettings::defs();
        let get = |k: &str| p.get(k).cloned().or_else(|| defs.iter().find(|d| d.key == k).map(|d| d.default.clone()));
        defs.iter().find(|d| d.key == key).unwrap().applies(Some(KIND), &get)
    };
    let off = EmitterSettings { gap: 0.1, ..EmitterSettings::default() };
    assert!(
        shown(&off, "vary_speed") && shown(&off, "order") && !shown(&off, "arrive_after") && !shown(&off, "cant_reach")
    );
    let straight = EmitterSettings { aim: Aim::StraightIn, ..off.clone() };
    for key in ["speed", "vary_speed", "directions", "aim_angle"] {
        assert!(!shown(&straight, key), "{key}");
    }
    assert!(shown(&straight, "gap") && shown(&straight, "order") && shown(&straight, "arrive_after"));
    let ring = EmitterSettings { start_from: StartFrom::Ring, directions: Directions::Fan, ..straight.clone() };
    assert!(shown(&ring, "directions") && shown(&ring, "aim_angle") && shown(&ring, "fan_angle"));
    let line = EmitterSettings { start_from: StartFrom::Line, ..straight.clone() };
    assert!(!shown(&line, "directions") && shown(&line, "aim_angle"));
    let keep = EmitterSettings { aim: Aim::KeepMyDirections, directions: Directions::Fan, ..off.clone() };
    assert!(shown(&keep, "speed") && shown(&keep, "fan_angle") && shown(&keep, "cant_reach"));
    assert!(!shown(&keep, "vary_speed") && !shown(&keep, "gap") && !shown(&keep, "order"));
}

#[test]
fn a_ball_thrown_across_is_met_where_it_falls_to() {
    let (mut scene, me, kit) = scene_with(Vec2::ZERO, |_| {});
    scene.remove_thing(SHAPE).unwrap();
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 600.0]));
    let ball = add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
    let thrown = scene.things.get_mut(&ball).unwrap();
    thrown.transform.position = Vec2::new(-300.0, -200.0);
    thrown.params.set("velocity", Value::Vec2([250.0, -300.0]));
    scene.card_mut(ball, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(1.0));
    let s = EmitterSettings {
        count: 4,
        start_from: StartFrom::Ring,
        start_radius: 30.0,
        aim: Aim::StraightIn,
        aim_target: ball.0 as i32,
        arrive_after: 1.0,
        start_delay: 0.5,
        ..EmitterSettings::default()
    };
    scene.things.get_mut(&me).unwrap().params = s.to_params();
    scene.card_mut(me, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(0.0));
    let mut world = World::build(&scene, &resolve(&scene, &kit.registry), &kit.sims);
    let mut log = Log::default();
    let best = nearest(&mut world, &kit, &mut log, me, 120 + 240, |w| w.bodies[&ball].position[0]);
    assert_eq!(best.len(), 4);
    assert!(best.iter().all(|d| *d < 0.5), "{best:?}");
    assert!(check(&world, me).unwrap().target_moves);
}

#[test]
fn a_catcher_that_doesnt_fire_shows_no_round_to_come() {
    let (scene, me, _, kit) = aimed(Vec2::ZERO, 0.0, Vec2::new(200.0, 0.0), |s| {
        s.aim = Aim::StraightIn;
        s.emits = false;
    });
    let world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    assert!(next_round(&world, me, 400).1.is_empty());
    assert_eq!(check(&world, me).unwrap().cant_reach, 0);
}
