use super::*;
use crate::testing::run;
use crate::things::emitter::Directions;
use crate::things::emitter::test_scenes::{resolved, scene_with, shooter_and_catcher};
use bpe_core::{Target, ThingId};

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
fn a_catcher_notes_who_caught_each_ball_it_takes_out_and_when() {
    let (scene, shooter, catcher, kit) = shooter_and_catcher(3, |_| {});
    let (world, _) = run(&scene, &resolved(&scene, &kit), &kit, 1.0);
    let caught: Vec<_> = st(&world, shooter).rows.caught.iter().map(|c| c.map(|c| (c.by, c.step))).collect();
    assert_eq!(caught, vec![Some((catcher, 107)), Some((catcher, 102)), Some((catcher, 107))]);
    // Balls sent back out or let through are still there: nothing to note.
    for catches in [Catches::SendBackOut, Catches::PassThrough] {
        let (scene, shooter, _, kit) = shooter_and_catcher(3, |s| s.catches = catches);
        let (world, _) = run(&scene, &resolved(&scene, &kit), &kit, 1.0);
        assert!(st(&world, shooter).rows.caught.iter().all(Option::is_none), "{catches:?}");
    }
}

#[test]
fn removing_earlier_rounds_drops_their_balls_still_waiting_on_the_gap() {
    let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.fire_when = FireWhen::Every;
        s.every = 0.5;
        s.rounds = 2;
        s.count = 4;
        s.gap = 0.3;
        s.earlier = Earlier::Remove;
    });
    let r = resolved(&scene, &kit);
    let (world, _) = run(&scene, &r, &kit, 2.0);
    let s = st(&world, id);
    // Round 1's balls go at 0 and 0.3 s; the ones due at 0.6 and 0.9 s never go.
    assert_eq!(s.fired[0].launched, 2);
    assert_eq!(s.rows.round.iter().filter(|&&r| r == 1).count(), 2);
    assert!(s.pending.is_empty());
    let b = &world.bodies[&id];
    assert!((0..b.len()).all(|i| b.alive[i] == (s.rows.round[i] == 2)), "only round 2 is left");
}

#[test]
fn an_earlier_rounds_ball_launched_late_fades_with_its_round() {
    let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.fire_when = FireWhen::Every;
        s.every = 0.5;
        s.rounds = 2;
        s.count = 4;
        s.gap = 0.3;
        s.earlier = Earlier::FadeOut;
        s.fade_time = 2.0;
    });
    let r = resolved(&scene, &kit);
    let (world, _) = run(&scene, &r, &kit, 0.7);
    let s = st(&world, id);
    let half = world.steps_per_second as u64 / 2;
    let firsts: Vec<Option<u64>> =
        (0..s.rows.len()).filter(|&i| s.rows.round[i] == 1).map(|i| s.rows.fade_from[i]).collect();
    assert_eq!(firsts, vec![Some(half); 3], "the one launched at 0.6 s fades from round 2's start too");
}

#[test]
fn a_held_ball_gathers_no_speed_while_it_waits() {
    let (mut scene, shooter, _, kit) = shooter_and_catcher(1, |s| {
        s.catches = Catches::SendBackOut;
        s.return_way = ReturnWay::SameWayBack;
        s.return_delay = 0.5;
    });
    // A wind along its path still gets it to the catcher, and would push a held ball.
    scene.card_mut(shooter, bpe_core::SlotId::Motion).unwrap().params.set("wind", bpe_core::Value::Vec2([300.0, 0.0]));
    let r = resolved(&scene, &kit);
    let (world, _) = run(&scene, &r, &kit, 0.7);
    let b = &world.bodies[&shooter];
    assert!(b.held[0]);
    assert_eq!(b.velocity[0], Vec2::ZERO);
}

#[test]
fn a_ball_sent_back_by_one_catcher_isnt_caught_again_by_another_over_it() {
    let (mut scene, shooter, _, kit) = shooter_and_catcher(1, |s| {
        s.catches = Catches::SendBackOut;
        s.return_way = ReturnWay::SameWayBack;
    });
    crate::things::emitter::test_scenes::add_emitter(&mut scene, &kit, Vec2::new(200.0, 0.0), |s| {
        s.emits = false;
        s.catches = Catches::SendBackOut;
        s.return_way = ReturnWay::SameWayBack;
        s.catch_size = 30.0;
    });
    let r = resolved(&scene, &kit);
    let (world, log) = run(&scene, &r, &kit, 0.6);
    assert_eq!(count(&log, EventKind::Enter), 1);
    assert!(world.bodies[&shooter].velocity[0].x < -399.0, "on its way back");
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

/// The emitter's own focus test: on focus 1 of an ellipse, with no gravity and its balls
/// touching walls with their middles, one round all passes focus 2 together.
#[test]
fn from_one_focus_of_an_ellipse_a_round_meets_at_the_other() {
    use crate::testing::SHAPE;
    use crate::things::emitter::SitsFocus;
    let (mut scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.sits_on = SHAPE.0 as i32;
        s.sits_focus = SitsFocus::Focus1;
        s.count = 72;
        s.radius = 1.0;
        s.speed = 600.0;
    });
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", bpe_core::Value::Vec2([0.0, 0.0]));
    let shape = scene.things.get_mut(&SHAPE).unwrap();
    shape.params = bpe_core::Settings::to_params(&crate::ShapeSettings::ellipse(900.0, 1300.0, 8.0));
    shape.transform.position = Vec2::ZERO;
    scene.card_mut(id, bpe_core::SlotId::Bounce).unwrap().params.set("touch", bpe_core::Value::Choice("middle".into()));
    let r = resolved(&scene, &kit);
    let c = (650f32 * 650.0 - 450.0 * 450.0).sqrt();
    let (from, to) = (Vec2::new(0.0, -c), Vec2::new(0.0, c));
    let mut w = World::build(&scene, &r, &kit.sims);
    assert_eq!(w.poses[&id].position, from);
    let mut log = Log::default();
    let rate = w.steps_per_second as f64;
    let arrive = (1300.0 - 1.0) / 600.0;
    let mut best = vec![(f32::INFINITY, 0u64); 72];
    let mut last = w.bodies[&id].position.clone();
    let mut step = 0u64;
    while (step as f64) < (arrive + 0.2) * rate {
        step += 1;
        w.advance_to(step as f64 / rate, &kit.sims, &mut log);
        let now = &w.bodies[&id].position;
        for (i, (a, b)) in last.iter().zip(now).enumerate() {
            let (_, _, d) = bpe_core::shape_path::nearest_on_segment(*a, *b, to);
            if d < best[i].0 {
                best[i] = (d, step);
            }
        }
        last = now.clone();
    }
    let worst = best.iter().map(|b| b.0).fold(0.0, f32::max);
    assert!(worst < 0.5, "every ball passes the far focus: worst {worst}");
    let mut steps: Vec<u64> = best.iter().map(|b| b.1).collect();
    steps.sort_unstable();
    assert!(steps[71] <= steps[0] + 2, "together: {} to {}", steps[0], steps[71]);
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
    // A key on How many lays the gradient's round out again.
    later.params.set("count", bpe_core::Value::Int(6));
    EmitterSim.settings_changed(id, &later, &mut world);
    assert_eq!(st(&world, id).layout.len(), 6);
}

#[test]
fn marker_settings_dont_touch_the_physics() {
    for key in [
        "marker",
        "marker_size",
        "marker_colour",
        "marker_glow",
        "flash_fire",
        "ripple_far",
        "vary_colour",
        "leave_trails",
    ] {
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

#[test]
fn the_timeline_shows_rounds_fired_and_those_still_to_come() {
    let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.fire_when = FireWhen::Every;
        s.every = 1.0;
        s.rounds = 4;
        s.count = 3;
        s.grow_plus = 2;
    });
    let r = resolved(&scene, &kit);
    let (world, _) = run(&scene, &r, &kit, 1.5);
    let marks = round_marks(&world, id, 10.0);
    let got: Vec<(f64, u32, u32, bool)> = marks.iter().map(|m| (m.time, m.round, m.size, m.fired)).collect();
    assert_eq!(got, vec![(0.0, 1, 3, true), (1.0, 2, 5, true), (2.0, 3, 7, false), (3.0, 4, 9, false)]);
    assert_eq!(round_marks(&world, id, 2.5).len(), 3, "only up to the end of the clip");
    let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.fire_when = FireWhen::WhenAWaveLands;
        s.wave_first = false;
    });
    let r = resolved(&scene, &kit);
    assert!(round_marks(&World::build(&scene, &r, &kit.sims), id, 10.0).is_empty(), "it can't know when");
}

#[test]
fn each_ball_knows_its_place_in_the_spreads() {
    let (scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.fire_when = FireWhen::Every;
        s.every = 0.5;
        s.rounds = 3;
        s.count = 4;
    });
    let r = resolved(&scene, &kit);
    let (world, _) = run(&scene, &r, &kit, 1.2);
    let spreads: Vec<bpe_core::BallSpread> = (0..12).map(|row| spread_in(&world, id, row).unwrap()).collect();
    assert_eq!(spreads.iter().map(|s| s.round).collect::<Vec<_>>()[..], [[0.0; 4], [0.5; 4], [1.0; 4]].concat()[..]);
    assert_eq!(spreads.iter().map(|s| s.number).collect::<Vec<_>>(), (0..12).collect::<Vec<_>>());
    assert!(spreads.iter().all(|s| s.count == 12 && !s.alone));
    // Four all round from straight up: up, right, down, left.
    let launch: Vec<f32> = spreads[..4].iter().map(|s| s.launch).collect();
    for (got, want) in launch.iter().zip([0.75, 0.0, 0.25, 0.5]) {
        assert!((got - want).abs() < 1e-4 || (got - want).abs() > 0.9999, "{launch:?}");
    }
    assert_eq!(spreads[3].place, 0.0, "the left one");
    assert_eq!(spreads[1].place, 1.0, "the right one");
    assert!(spread_in(&world, id, 12).is_none());
}

/// Sets emitter `id`'s own `key` varying to `to` by `by`.
fn vary_own(scene: &mut bpe_core::Scene, id: ThingId, key: &str, to: f64, by: &str) {
    let params = &mut scene.things.get_mut(&id).unwrap().params;
    params.set(&format!("{key}~to"), bpe_core::Value::Number(to));
    params.set(&format!("{key}~by"), bpe_core::Value::Choice(by.into()));
}

#[test]
fn the_emitter_s_size_and_speed_may_vary_its_other_settings_not() {
    let kit = crate::kit();
    let kind = kit.registry.thing(crate::things::emitter::KIND).unwrap();
    assert_eq!(kind.vary("radius"), Some(bpe_core::VaryUse::Live));
    assert_eq!(kind.vary("speed"), Some(bpe_core::VaryUse::AtLaunch));
    assert_eq!(kind.vary("count"), None);
}

#[test]
fn a_size_varied_by_number_gives_each_fired_ball_its_own_size() {
    let (mut scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.count = 4;
        s.radius = 10.0;
    });
    vary_own(&mut scene, id, "radius", 40.0, "number");
    let r = resolved(&scene, &kit);
    let world = World::build(&scene, &r, &kit.sims);
    assert_eq!(world.bodies[&id].radius, vec![10.0, 20.0, 30.0, 40.0]);
    assert_eq!(st(&world, id).own_size, vec![10.0, 20.0, 30.0, 40.0]);
}

#[test]
fn a_speed_varied_by_number_fires_each_ball_at_its_own_speed() {
    let (mut scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.count = 3;
        s.speed = 100.0;
    });
    vary_own(&mut scene, id, "speed", 300.0, "number");
    let r = resolved(&scene, &kit);
    let world = World::build(&scene, &r, &kit.sims);
    let speeds: Vec<f32> = world.bodies[&id].velocity.iter().map(|v| v.length()).collect();
    for (got, want) in speeds.iter().zip([100.0, 200.0, 300.0]) {
        assert!((got - want).abs() < 1e-2, "{speeds:?}");
    }
}

#[test]
fn a_size_varied_by_round_gives_each_round_its_own_size() {
    let (mut scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.fire_when = FireWhen::Every;
        s.every = 0.5;
        s.rounds = 3;
        s.count = 2;
        s.radius = 10.0;
    });
    vary_own(&mut scene, id, "radius", 30.0, "round");
    let r = resolved(&scene, &kit);
    let (world, _) = run(&scene, &r, &kit, 1.2);
    assert_eq!(world.bodies[&id].radius, vec![10.0, 10.0, 20.0, 20.0, 30.0, 30.0]);
}

#[test]
fn keys_on_the_to_end_resize_the_balls_already_fired() {
    let (mut scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.count = 2;
        s.radius = 10.0;
        s.speed = 0.0;
    });
    vary_own(&mut scene, id, "radius", 20.0, "number");
    let to = bpe_core::Address { thing: id, slot: None, key: "radius~to".into() };
    scene.set_key(&kit.registry, &to, 0.0, bpe_core::Value::Number(20.0)).unwrap();
    scene.set_key(&kit.registry, &to, 1.0, bpe_core::Value::Number(30.0)).unwrap();
    let r = resolved(&scene, &kit);
    let mut world = World::build(&scene, &r, &kit.sims);
    assert_eq!(world.bodies[&id].radius, vec![10.0, 20.0]);
    world.advance_to(1.5, &kit.sims, &mut Log::default());
    assert_eq!(world.bodies[&id].radius, vec![10.0, 30.0]);
}

#[test]
fn a_size_varied_by_hits_grows_as_its_balls_are_caught() {
    let (mut scene, shooter, catcher, kit) = shooter_and_catcher(1, |s| s.catches = Catches::SendBackOut);
    scene.things.get_mut(&shooter).unwrap().params.set("radius", bpe_core::Value::Number(5.0));
    vary_own(&mut scene, shooter, "radius", 15.0, "hits");
    let r = resolved(&scene, &kit);
    let (world, _) = run(&scene, &r, &kit, 1.0);
    let caught = st(&world, shooter).rows.arrivals[0];
    assert!(caught >= 1, "the ball reached the catcher");
    let _ = catcher;
    assert!(world.bodies[&shooter].radius[0] > 5.0, "{:?}", world.bodies[&shooter].radius);
}

#[test]
fn an_endless_emitter_s_size_by_number_runs_over_the_balls_fired_so_far() {
    let (mut scene, id, kit) = scene_with(Vec2::ZERO, |s| {
        s.fire_when = FireWhen::Every;
        s.every = 0.5;
        s.endless = true;
        s.count = 1;
        s.radius = 10.0;
        s.speed = 0.0;
    });
    vary_own(&mut scene, id, "radius", 20.0, "number");
    let r = resolved(&scene, &kit);
    assert_eq!(st(&World::build(&scene, &r, &kit.sims), id).planned, 0, "endless");
    let (world, _) = run(&scene, &r, &kit, 1.2);
    // Three fired so far: the first at Size, the last at the To end.
    assert_eq!(world.bodies[&id].radius, vec![10.0, 15.0, 20.0]);
}
