use super::*;
use crate::testing::SHAPE;
use crate::things::emitter::aim::check;
use crate::things::emitter::aim_cache::is_kept;
use crate::things::emitter::test_scenes::{add_emitter, resolved, scene_with};
use crate::things::emitter::{
    Aim, AimArrive, AimLeave, AimWay, CantReach, Catches, Directions, FireWhen, KIND, ReturnWay, SitsFocus,
};
use crate::{Kit, ShapeSettings};
use bpe_core::{Scene, Settings, SlotId, Value};

const DT: f32 = 1.0 / 240.0;

#[test]
fn a_pass_is_the_nearest_miss_of_each_arc_signed_by_its_side() {
    // Along y = 2 going +x, then after a hit back along y = -1 going -x: the target (the
    // origin) is on the same hand of both ways (y down), 2 px and 1 px off.
    let rel = [
        Vec2::new(-3.0, 2.0),
        Vec2::new(-1.0, 2.0),
        Vec2::new(1.0, 2.0),
        Vec2::new(3.0, 0.0),
        Vec2::new(1.0, -1.0),
        Vec2::new(-1.0, -1.0),
    ];
    let hits = [0, 0, 0, 1, 0, 0];
    let p = passes(&rel, &hits);
    assert_eq!(p.len(), 2);
    let first = p[0].unwrap();
    assert!((first.miss.abs() - 2.0).abs() < 1e-9 && (first.e - 1.5).abs() < 1e-9, "{first:?}");
    let second = p[1].unwrap();
    assert!((second.miss.abs() - 1.0).abs() < 1e-9 && (second.e - 4.5).abs() < 1e-9, "{second:?}");
    assert!(first.miss < 0.0 && second.miss < 0.0, "the same hand of both ways");
    // A step that ends in a hit is no pass.
    let p = passes(&rel[..4], &[0, 0, 0, 1]);
    assert_eq!(p.len(), 2);
    assert!(p[1].is_none());
}

#[test]
fn a_target_that_moves_is_passed_where_it_is_then() {
    // The ball goes along y = 0 at 1 px a step; the target walks along x at 1 px a step from x = 2
    // and sits 0.5 below: relative to it the ball stands still, so no pass; one at 2 px a step
    // passes it at step 2.
    let rel: Vec<Vec2> = (0..5).map(|j| Vec2::new(j as f32 * 2.0 - (2.0 + j as f32), -0.5)).collect();
    let p = passes(&rel, &[0; 5]);
    let pass = p[0].unwrap();
    assert!((pass.e - 2.0).abs() < 1e-9 && (pass.miss.abs() - 0.5).abs() < 1e-9, "{pass:?}");
}

/// The new scene (a circle of 508 at the origin, no ball) with `gravity`, an emitter at `at` set
/// by `f` whose balls fall by Gravity × 1, and a target emitter at `target` that doesn't fire.
fn bouncing(gravity: Vec2, at: Vec2, target: Vec2, f: impl FnOnce(&mut EmitterSettings)) -> (Scene, ThingId, Kit) {
    let (mut scene, me, kit) = scene_with(at, |_| {});
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([gravity.x, gravity.y]));
    let to = add_emitter(&mut scene, &kit, target, |s| s.emits = false);
    let mut s = EmitterSettings { aim: Aim::ThroughBounces, aim_target: to.0 as i32, ..EmitterSettings::default() };
    f(&mut s);
    scene.things.get_mut(&me).unwrap().params = s.to_params();
    scene.card_mut(me, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(1.0));
    (scene, me, kit)
}

/// `scene` built and stepped once: a round at 0 s aimed through bounces fires on the first step
/// (setting off from 0 s).
fn fired(scene: &Scene, kit: &Kit) -> World {
    fired_with(scene, &resolved(scene, kit), kit)
}

fn fired_with(scene: &Scene, resolved: &bpe_core::Resolved, kit: &Kit) -> World {
    let mut world = World::build(scene, resolved, &kit.sims);
    world.step(&kit.sims, &mut Log::default());
    world
}

fn st(world: &World, id: ThingId) -> &EmitterState {
    emitter_state(world, id).expect("an emitter")
}

/// Steps `world` up to one step past `around`, giving each ball of `me` the nearest it came to
/// `target` within a step either side of `around`, and how many wall hits it had before.
fn nearest(world: &mut World, kit: &Kit, log: &mut Log, me: ThingId, around: u64, target: Vec2) -> Vec<(f32, u32)> {
    while world.step + 1 < around {
        world.step(&kit.sims, log);
    }
    let mut best = vec![f32::INFINITY; world.bodies[&me].len()];
    let hits: Vec<u32> = (0..world.bodies[&me].len()).map(|i| world.bodies[&me].hits[i]).collect();
    while world.step < around + 1 {
        world.step(&kit.sims, log);
        let b = &world.bodies[&me];
        best.resize(b.len(), f32::INFINITY);
        for (i, best) in best.iter_mut().enumerate() {
            if !b.alive[i] {
                continue;
            }
            let (from, to) = (b.previous[i], b.position[i]);
            let d = to - from;
            let s = if d.length_squared() > 0.0 {
                ((target - from).dot(d) / d.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            *best = best.min((from + d * s - target).length());
        }
    }
    best.into_iter().zip(hits.into_iter().chain(std::iter::repeat(0))).collect()
}

/// Up and away in a fan from the left, under gravity, to a target on the right that the circle's
/// top bounces them down onto.
fn fan_in_a_circle(f: impl FnOnce(&mut EmitterSettings)) -> (Scene, ThingId, Kit) {
    bouncing(Vec2::new(0.0, 980.0), Vec2::new(-250.0, 200.0), Vec2::new(250.0, 200.0), |s| {
        s.count = 6;
        s.directions = Directions::Fan;
        s.aim_angle = -80.0;
        s.fan_angle = 40.0;
        s.speed = 1200.0;
        s.arrive_after = 1.5;
        f(s);
    })
}

/// The round's balls that get there with the others.
fn reaching(aims: &RoundAim) -> usize {
    aims.balls.iter().filter(|b| b.as_ref().is_some_and(|b| b.reaches)).count()
}

#[test]
fn turning_each_ball_they_all_leave_together_and_meet_after_a_bounce() {
    let (scene, me, kit) = fan_in_a_circle(|_| {});
    let mut world = fired(&scene, &kit);
    assert!(world.fingerprint.is_some(), "aiming through bounces asks for the fingerprint");
    let aims = st(&world, me).aimed.last().cloned().expect("the round at 0 s was aimed");
    assert_eq!(reaching(&aims), 6, "every ball of the fan finds a way: {:?}", aims.balls);
    for b in aims.balls.iter().flatten() {
        assert_eq!(b.leave, 1, "all leave on the round's step");
        assert!(b.velocity.length() <= 1200.5, "none faster than Speed: {}", b.velocity.length());
    }
    assert_eq!(world.bodies[&me].alive_count(), 6);
    let mut log = Log::default();
    let near = nearest(&mut world, &kit, &mut log, me, aims.arrive, Vec2::new(250.0, 200.0));
    assert_eq!(near.len(), 6);
    for (d, hits) in &near {
        assert!(*d < ROUGH as f32, "{near:?}");
        assert!(*hits >= 1, "bounced first: {near:?}");
    }
    // At the target together, not merely passing it: each is there at the round's moment.
    let b = &world.bodies[&me];
    let at = aims.meet;
    assert!(world.step == aims.arrive + 1);
    for i in 0..6 {
        assert!((b.previous[i] - at).length() < ROUGH as f32 + b.velocity[i].length() * DT, "{i}");
    }
}

#[test]
fn keeping_their_directions_under_gravity_only_those_that_can_meet_go() {
    let (scene, me, kit) = fan_in_a_circle(|s| s.aim_way = AimWay::KeepMine);
    let mut world = fired(&scene, &kit);
    let aims = st(&world, me).aimed.last().cloned().unwrap();
    let going = reaching(&aims);
    assert!(going >= 1, "the latest at least: {:?}", aims.balls);
    assert_eq!(aims.balls.iter().flatten().count(), going, "the rest are skipped");
    for b in aims.balls.iter().flatten() {
        assert_eq!(b.leave, 1);
    }
    let c = check(&world, me).unwrap();
    assert!(c.bends_keep && c.untimed, "{c:?}");
    assert_eq!(c.cant_reach as usize, 6 - going);
    let mut log = Log::default();
    let near = nearest(&mut world, &kit, &mut log, me, aims.arrive, Vec2::new(250.0, 200.0));
    assert_eq!(near.len(), going);
    assert!(near.iter().all(|(d, _)| *d < ROUGH as f32), "{near:?}");
}

#[test]
fn arriving_after_a_time_they_all_meet_then() {
    // Through one bounce at Speed they take a little longer: they slow down and turn.
    let (scene, me, kit) = fan_in_a_circle(|s| {
        s.aim_arrive = AimArrive::After;
        s.arrive_after = 1.55;
    });
    let mut world = fired(&scene, &kit);
    let aims = st(&world, me).aimed.last().cloned().unwrap();
    // Fired on the first step, there 1.55 s later.
    assert_eq!(aims.arrive, 373);
    assert_eq!(reaching(&aims), 6, "{:?}", aims.balls);
    let mut log = Log::default();
    let near = nearest(&mut world, &kit, &mut log, me, 373, Vec2::new(250.0, 200.0));
    assert_eq!(near.len(), aims.balls.iter().flatten().count());
    assert!(near.iter().all(|(d, _)| *d < ROUGH as f32), "{near:?}");
}

#[test]
fn leaving_when_needed_each_sets_off_so_it_passes_with_the_others() {
    let (scene, me, kit) = fan_in_a_circle(|s| {
        s.aim_leave = AimLeave::WhenNeeded;
        s.aim_arrive = AimArrive::After;
    });
    let mut world = fired(&scene, &kit);
    let aims = st(&world, me).aimed.last().cloned().expect("the round at 0 s was aimed");
    let going = reaching(&aims);
    assert!(going >= 4, "most of the fan finds a way: {:?}", aims.balls);
    let leaves: Vec<u64> = aims.balls.iter().flatten().map(|b| b.leave).collect();
    assert!(leaves.iter().any(|l| *l != leaves[0]), "they leave when needed: {leaves:?}");
    let mut log = Log::default();
    let near = nearest(&mut world, &kit, &mut log, me, 361, Vec2::new(250.0, 200.0));
    assert_eq!(near.len(), going, "the ones without a way are skipped");
    for (d, hits) in &near {
        assert!(*d < 0.5, "{near:?}");
        assert!(*hits >= 1, "bounced first: {near:?}");
    }
}

#[test]
fn no_way_has_more_bounces_than_at_most_and_flights_stop_there() {
    let (scene, me, kit) = fan_in_a_circle(|s| s.aim_bounces_most = 1);
    let mut world = fired(&scene, &kit);
    let aims = st(&world, me).aimed.last().cloned().unwrap();
    assert!(reaching(&aims) >= 1);
    let mut log = Log::default();
    let near = nearest(&mut world, &kit, &mut log, me, aims.arrive, Vec2::new(250.0, 200.0));
    assert!(near.iter().all(|(d, hits)| *d < ROUGH as f32 && *hits == 1), "{near:?}");
    // A flight ends at its second bounce.
    let world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    let aim = Aimer::of(&world, me, &st(&world, me).settings, 0).unwrap();
    let base = Lone::new(&world, me, me, 10.0, 0);
    let p0 = world.poses[&me].position;
    let flight = fly(&base, &aim, p0, Vec2::new(0.0, -1200.0), MOST_STEPS as u64, 1);
    assert_eq!(flight.bounces, 1);
    assert!(flight.places.len() < 240 * 5, "{}", flight.places.len());
}

#[test]
fn the_fewest_bounces_win_and_none_may_go_straight() {
    let (scene, me, kit) = fan_in_a_circle(|s| {
        s.aim_bounces = 0;
        s.aim_angle = -45.0;
        s.fan_angle = 20.0;
        s.speed = 700.0;
    });
    let mut world = fired(&scene, &kit);
    let aims = st(&world, me).aimed.last().cloned().unwrap();
    // A 45° lob onto the target needs no bounce.
    let mut log = Log::default();
    let near = nearest(&mut world, &kit, &mut log, me, aims.arrive, Vec2::new(250.0, 200.0));
    assert_eq!(near.len(), aims.balls.iter().flatten().count());
    assert!(near.iter().all(|(d, _)| *d < ROUGH as f32), "{near:?}");
    assert!(near.iter().any(|(_, hits)| *hits == 0), "a straight lob is allowed: {near:?}");
}

#[test]
fn from_one_focus_of_an_ellipse_they_all_go_at_speed_and_meet_at_the_other() {
    for way in [AimWay::KeepMine, AimWay::AdjustEachBall] {
        focus_to_focus(way);
    }
}

fn focus_to_focus(way: AimWay) {
    let (mut scene, me, kit) = bouncing(Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, |s| {
        s.count = 16;
        s.directions = Directions::AllRound;
        s.speed = 600.0;
        s.aim_target = 0;
        s.aim_way = way;
    });
    // Sit it on the first focus of an upright ellipse; aim at the other.
    let shape = scene.things.get_mut(&SHAPE).unwrap();
    shape.params = ShapeSettings::ellipse(900.0, 1300.0, 4.0).to_params();
    let mut s = EmitterSettings::from_params(&scene.things[&me].params);
    s.sits_on = SHAPE.0 as i32;
    s.sits_focus = SitsFocus::Focus1;
    scene.things.get_mut(&me).unwrap().params = s.to_params();
    scene.card_mut(me, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(0.0));
    scene.card_mut(me, SlotId::Bounce).unwrap().params.set("touch", Value::Choice("middle".into()));
    scene.card_mut(me, SlotId::Bounce).unwrap().params.set("bounciness", Value::Number(1.0));
    let mut world = fired(&scene, &kit);
    let aims = st(&world, me).aimed.last().cloned().unwrap();
    let balls: Vec<&BallAim> = aims.balls.iter().flatten().collect();
    assert_eq!(balls.len(), 16);
    for (k, b) in balls.iter().enumerate() {
        assert!(b.reaches);
        assert!((b.velocity.length() - 600.0).abs() < 1.0, "{way:?} {k} at Speed: {}", b.velocity.length());
        assert_eq!(b.leave, 1, "they leave together");
    }
    // Every way from focus to focus through one bounce is as long (the ellipse's long axis,
    // 1300): they get there together at Speed.
    let t = (aims.arrive as f32) * DT;
    assert!((t - 1300.0 / 600.0).abs() < 0.02, "{t}");
    let other = crate::things::shape::foci(&world, SHAPE).unwrap()[1];
    let mut log = Log::default();
    let near = nearest(&mut world, &kit, &mut log, me, aims.arrive, other);
    assert_eq!(near.len(), 16);
    assert!(near.iter().all(|(d, _)| *d < ROUGH as f32), "{way:?} {near:?}");
}

#[test]
fn in_a_spinning_square_the_way_is_checked_from_when_it_leaves() {
    let (mut scene, me, kit) = fan_in_a_circle(|s| s.count = 5);
    let mut square = ShapeSettings::polygon(4, 560.0, 4.0);
    square.spin = 2.0;
    scene.things.get_mut(&SHAPE).unwrap().params = square.to_params();
    let mut world = fired(&scene, &kit);
    let aims = st(&world, me).aimed.last().cloned().unwrap();
    let reaching = reaching(&aims);
    assert!(reaching >= 1, "{:?}", aims.balls);
    let mut log = Log::default();
    let near = nearest(&mut world, &kit, &mut log, me, aims.arrive, Vec2::new(250.0, 200.0));
    let met = near.iter().filter(|(d, _)| *d < ROUGH as f32).count();
    assert_eq!(met, reaching, "{near:?}");
}

#[test]
fn with_no_way_in_time_they_are_skipped_or_go_anyway_at_speed() {
    let build = |cant: CantReach| {
        let (scene, me, kit) = fan_in_a_circle(|s| {
            s.arrive_after = 0.1;
            s.aim_arrive = AimArrive::After;
            s.cant_reach = cant;
            s.aim_bounces = 3;
        });
        (World::build(&scene, &resolved(&scene, &kit), &kit.sims), me, kit)
    };
    let (mut world, me, kit) = build(CantReach::Skip);
    let mut log = Log::default();
    world.step(&kit.sims, &mut log);
    assert_eq!(world.bodies[&me].alive_count(), 0);
    assert_eq!(check(&world, me).map(|c| (c.cant_reach, c.of, c.skip)), Some((6, 6, true)));
    let (mut world, me, kit) = build(CantReach::FireAnyway);
    world.step(&kit.sims, &mut log);
    assert_eq!(world.bodies[&me].alive_count(), 6);
    assert!((world.bodies[&me].velocity[0].length() - 1200.0).abs() < 30.0, "{}", world.bodies[&me].velocity[0]);
}

#[test]
fn a_scene_aims_once_and_every_world_of_it_shares_the_answer() {
    let (scene, me, kit) = fan_in_a_circle(|s| s.count = 2);
    let r = resolved(&scene, &kit);
    let one = fired_with(&scene, &r, &kit);
    let two = fired_with(&scene, &r, &kit);
    let (a, b) = (st(&one, me).aimed.last().unwrap(), st(&two, me).aimed.last().unwrap());
    assert!(Arc::ptr_eq(a, b), "the second build took the kept answer");
    assert!(
        is_kept(one.fingerprint.unwrap(), me, What::Round { round: 1, fire: 1 }),
        "a round at 0 s fires on the first step"
    );
    // The same run either way.
    let run = |mut w: World| {
        let mut log = Log::default();
        w.advance_to(2.0, &kit.sims, &mut log);
        log.hash()
    };
    assert_eq!(run(one), run(two));
}

#[test]
fn later_rounds_aim_from_where_the_world_is_then() {
    let (scene, me, kit) = fan_in_a_circle(|s| {
        s.count = 2;
        s.fire_when = FireWhen::Every;
        s.every = 2.0;
        s.rounds = 2;
        s.arrive_after = 1.2;
    });
    let mut world = fired(&scene, &kit);
    let mut log = Log::default();
    let target = Vec2::new(250.0, 200.0);
    let first = st(&world, me).aimed.last().cloned().unwrap();
    nearest(&mut world, &kit, &mut log, me, first.arrive, target);
    while st(&world, me).aimed.last().unwrap().round < 2 {
        world.step(&kit.sims, &mut log);
        assert!(world.step < 1200, "the second round fires");
    }
    let aims = st(&world, me).aimed.last().cloned().unwrap();
    assert_eq!(aims.fire, 481);
    let second = nearest(&mut world, &kit, &mut log, me, aims.arrive, target);
    let reaching = reaching(&aims);
    let met = second.iter().filter(|(d, _)| *d < ROUGH as f32).count();
    assert!(reaching >= 1 && met >= reaching, "{second:?} {:?}", aims.balls);
}

#[test]
fn a_caught_ball_sent_out_my_way_is_aimed_through_a_bounce() {
    // A shooter fires one ball right into a catcher that sends it out its way, up, through a
    // bounce off the circle down onto a target.
    let (mut scene, shooter, kit) = scene_with(Vec2::new(-200.0, 300.0), |s| {
        s.count = 1;
        s.directions = Directions::OneWay;
        s.aim_angle = 0.0;
        s.speed = 500.0;
    });
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 600.0]));
    scene.card_mut(shooter, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(1.0));
    let target = add_emitter(&mut scene, &kit, Vec2::new(250.0, 100.0), |s| s.emits = false);
    let catcher = add_emitter(&mut scene, &kit, Vec2::new(-100.0, 300.0), |s| {
        s.emits = false;
        s.catches = Catches::SendBackOut;
        s.catch_size = 30.0;
        s.return_way = ReturnWay::OutMyWay;
        s.directions = Directions::OneWay;
        s.aim_angle = -20.0;
        s.speed = 800.0;
        s.aim = Aim::ThroughBounces;
        s.aim_target = target.0 as i32;
    });
    // The caught ball falls by the shooter's cards, not the catcher's (which has none).
    scene.card_mut(catcher, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(0.0));
    let mut world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    let mut log = Log::default();
    while !log.events().iter().any(|e| e.kind == EventKind::Enter) {
        world.step(&kit.sims, &mut log);
        assert!(world.step < 600, "it is caught");
    }
    let caught = world.step;
    assert!(world.bodies[&shooter].alive[0], "it found a way and goes");
    let before = world.bodies[&shooter].hits[0];
    // As soon as it can: the nearest it comes within three seconds, and its bounces by then.
    let target = Vec2::new(250.0, 100.0);
    let mut best = (f32::INFINITY, 0);
    while world.step < caught + 720 {
        world.step(&kit.sims, &mut log);
        let b = &world.bodies[&shooter];
        let (from, to) = (b.previous[0], b.position[0]);
        let d = to - from;
        let s =
            if d.length_squared() > 0.0 { ((target - from).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        let near = (from + d * s - target).length();
        if near < best.0 {
            best = (near, b.hits[0]);
        }
    }
    assert!(best.0 < ROUGH as f32, "{best:?}");
    assert!(best.1 > before, "it bounced on the way");
    assert_eq!(world.kind(catcher), Some(KIND));
}

#[test]
fn every_flight_count_stays_in_the_budget() {
    // A target no way reaches (outside the circle): every ball spends its flights and stops.
    let (scene, me, kit) = bouncing(Vec2::new(0.0, 980.0), Vec2::new(-250.0, 200.0), Vec2::new(900.0, 0.0), |s| {
        s.count = 2;
        s.arrive_after = 0.5;
    });
    let world = World::build(&scene, &resolved(&scene, &kit), &kit.sims);
    let aim = Aimer::of(&world, me, &st(&world, me).settings, 0).unwrap();
    let base = Lone::new(&world, me, me, 10.0, 0);
    let s = &st(&world, me).settings;
    let started = std::time::Instant::now();
    let ball = aim_ball(&base, &aim, s, Shot::Emitter { offset: Vec2::ZERO, way: -1.0 }, 0);
    assert_eq!(ball, None);
    // 40 flights, each ending at its eleventh bounce: quick even unoptimised.
    assert!(started.elapsed().as_secs_f32() < 5.0);
}

#[test]
fn through_bounces_shows_its_rows() {
    let shown = |s: &EmitterSettings, key: &str| {
        let p = s.to_params();
        let defs = EmitterSettings::defs();
        let get = |k: &str| p.get(k).cloned().or_else(|| defs.iter().find(|d| d.key == k).map(|d| d.default.clone()));
        defs.iter().find(|d| d.key == key).unwrap().applies(Some(KIND), &get)
    };
    let through = EmitterSettings { aim: Aim::ThroughBounces, gap: 0.2, ..EmitterSettings::default() };
    let rows = ["speed", "directions", "aim_angle", "cant_reach", "aim_bounces", "aim_bounces_most", "aim_arrive"];
    for key in rows.into_iter().chain(["aim_leave", "aim_way", "aim_turn"]) {
        assert!(shown(&through, key), "{key}");
    }
    for key in ["gap", "order", "vary_speed", "arrive_after"] {
        assert!(!shown(&through, key), "{key}");
    }
    let after = EmitterSettings { aim_arrive: AimArrive::After, ..through.clone() };
    assert!(shown(&after, "arrive_after"));
    let keep = EmitterSettings { aim_way: AimWay::KeepMine, ..through.clone() };
    assert!(shown(&keep, "aim_way") && !shown(&keep, "aim_turn"));
    let needed = EmitterSettings { aim_leave: AimLeave::WhenNeeded, ..through.clone() };
    assert!(!shown(&needed, "aim_way") && !shown(&needed, "aim_turn"));
    let keep = EmitterSettings { aim: Aim::KeepMyDirections, ..through.clone() };
    assert!(!shown(&keep, "aim_bounces"));
    let straight = EmitterSettings { aim: Aim::StraightIn, ..through };
    assert!(shown(&straight, "gap") && shown(&straight, "order"));
}

#[test]
fn a_target_that_catches_takes_the_ball_on_its_first_way_by() {
    // Straight at the target along the middle line, at least one bounce: the way back off the
    // circle's side passes it again, unless it catches the ball the first time by.
    let aim_for = |catches: Catches| {
        let (mut scene, me, kit) = bouncing(Vec2::ZERO, Vec2::new(-250.0, 0.0), Vec2::new(250.0, 0.0), |s| {
            s.count = 1;
            s.directions = Directions::OneWay;
            s.aim_angle = 0.0;
            s.speed = 500.0;
            s.aim_bounces = 1;
            // Along its own line: turned, it could find a way round the catcher.
            s.aim_way = AimWay::KeepMine;
        });
        let to = ThingId(EmitterSettings::from_params(&scene.things[&me].params).aim_target as u32);
        let mut t = EmitterSettings::from_params(&scene.things[&to].params);
        t.catches = catches;
        t.catch_size = 20.0;
        scene.things.get_mut(&to).unwrap().params = t.to_params();
        let world = fired(&scene, &kit);
        st(&world, me).aimed.last().cloned().unwrap().balls[0].as_ref().map(|b| b.reaches)
    };
    assert_eq!(aim_for(Catches::Nothing), Some(true), "back off the wall through the target");
    assert_eq!(aim_for(Catches::PassThrough), Some(true), "passing through lets it fly on");
    assert_eq!(aim_for(Catches::Remove), None, "caught on the way out: no way after a bounce");
}

#[test]
fn from_a_ring_each_ball_finds_its_own_way_to_be_there_together() {
    // Each ball sets off from its own place: no way can be shared, each is worked out alone.
    let (scene, me, kit) = fan_in_a_circle(|s| {
        s.start_from = crate::things::emitter::StartFrom::Ring;
        s.start_radius = 40.0;
    });
    let mut world = fired(&scene, &kit);
    let aims = st(&world, me).aimed.last().cloned().unwrap();
    let going = reaching(&aims);
    assert!(going >= 4, "{:?}", aims.balls.iter().map(|b| b.as_ref().map(|b| b.velocity)).collect::<Vec<_>>());
    let mut log = Log::default();
    let near = nearest(&mut world, &kit, &mut log, me, aims.arrive, Vec2::new(250.0, 200.0));
    assert_eq!(near.len(), aims.balls.iter().flatten().count());
    assert!(near.iter().all(|(d, _)| *d < ROUGH as f32), "{near:?}");
}

#[test]
fn with_no_walls_to_bounce_off_the_search_gives_up_quickly() {
    // Nothing to bounce off and no time limit: each flight stops after ten quiet seconds, not two
    // minutes, and the balls (no way with a bounce) are skipped.
    let (mut scene, me, kit) = bouncing(Vec2::ZERO, Vec2::new(-250.0, 0.0), Vec2::new(250.0, 0.0), |s| s.count = 4);
    scene.remove_thing(SHAPE).unwrap();
    let started = std::time::Instant::now();
    let world = fired(&scene, &kit);
    assert_eq!(world.bodies[&me].alive_count(), 0);
    assert!(started.elapsed().as_secs_f32() < 20.0, "{:?}", started.elapsed());
}

/// A big round all round from the left of the circle (no gravity) at a target on the right,
/// with up to 50 bounces, set by `f`: how many are aimed, when they meet and how near each aimed
/// ball really comes to the target then (with how many bounces), in ball order.
fn big_round(count: u32, f: impl FnOnce(&mut EmitterSettings)) -> (usize, u64, Vec<(f32, u32)>) {
    let target = Vec2::new(150.0, 120.0);
    let (scene, me, kit) = bouncing(Vec2::ZERO, Vec2::new(-200.0, 0.0), target, |s| {
        s.count = count;
        s.aim_bounces_most = 50;
        f(s);
    });
    let mut world = fired(&scene, &kit);
    let aims = st(&world, me).aimed.last().cloned().unwrap();
    for b in aims.balls.iter().flatten() {
        assert_eq!(b.leave, 1, "all leave on the round's step");
    }
    let mut log = Log::default();
    let near = nearest(&mut world, &kit, &mut log, me, aims.arrive, target);
    // The balls fired, in ball order (the skipped ones aren't).
    let fired: Vec<&Option<BallAim>> = aims.balls.iter().filter(|b| b.is_some()).collect();
    let reached = fired.iter().zip(&near).filter(|(b, _)| b.as_ref().is_some_and(|b| b.reaches)).map(|(_, n)| *n).collect();
    (reaching(&aims), aims.arrive, reached)
}

#[test]
fn a_big_round_turning_each_ball_finds_every_ball_a_way() {
    // Part 4b aimed 370 of 500 here: only the first three arcs were looked at, and a ball whose
    // soonest way was much sooner than the round's moment couldn't slow down enough.
    let (aimed, arrive, near) = big_round(150, |_| {});
    assert!(aimed >= 147, "{aimed} of 150");
    assert!(arrive > 240, "the latest takes a while: {arrive}");
    assert!(near.iter().all(|(d, _)| *d < ROUGH as f32 + 0.5), "{near:?}");
    assert!(near.iter().all(|(_, hits)| *hits >= 1), "{near:?}");
}

#[test]
fn keeping_their_directions_ways_with_many_bounces_count_too() {
    // Part 4b aimed 6 of 500 balls keeping their directions: it never looked past three arcs.
    let (aimed, arrive, near) = big_round(120, |s| {
        s.aim_way = AimWay::KeepMine;
        s.aim_arrive = AimArrive::After;
        s.arrive_after = 10.0;
    });
    assert_eq!(arrive, 2401);
    assert!(aimed >= 40, "{aimed} of 120");
    assert!(near.iter().all(|(d, _)| *d < ROUGH as f32 + 0.5), "{near:?}");
    assert!(near.iter().any(|(_, hits)| *hits > 4), "some take many bounces: {near:?}");
}

/// The 500-ball rounds Aedan tried (slow: run by hand).
#[test]
#[ignore]
fn five_hundred_balls() {
    let t0 = std::time::Instant::now();
    let (aimed, arrive, near) = big_round(500, |_| {});
    eprintln!("turning: {aimed} of 500 at {arrive} in {:.1} s", t0.elapsed().as_secs_f64());
    assert!(aimed >= 495);
    assert!(near.iter().all(|(d, _)| *d < ROUGH as f32 + 0.5));
    let t0 = std::time::Instant::now();
    let (aimed, arrive, near) = big_round(500, |s| s.aim_way = AimWay::KeepMine);
    eprintln!("keeping: {aimed} of 500 at {arrive} in {:.1} s", t0.elapsed().as_secs_f64());
    assert!(aimed >= 60);
    assert!(near.iter().all(|(d, _)| *d < ROUGH as f32 + 0.5));
}
