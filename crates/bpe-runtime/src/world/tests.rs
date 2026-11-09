use super::*;
use crate::plugins::Sims;
use crate::testing::{
    DOTS, DOTS2, TestDots, TestRoot, WALL, dots_in_ring, dots_in_sticky_ring, dots_pair, dots_pair_popping, dots_pile,
    reg, ring_with_rider,
};
use bpe_core::{Address, Attach, Ease, SlotId, Target, ThingId, Value, resolve};

fn root_setting(key: &str) -> Address {
    Address::own(ThingId::ROOT, key)
}

#[test]
fn a_moment_between_steps_is_drawn_and_undone_without_touching_the_physics() {
    let (scene, resolved, sims) = dots_pair(120.0, Target::Anything);
    let mut world = World::build(&scene, &resolved, &sims);
    let mut twin = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    let mut twin_log = Log::default();
    world.advance_to(0.1, &sims, &mut log);
    twin.advance_to(0.1, &sims, &mut twin_log);
    let before = world.bodies[&DOTS].position.clone();
    let previous = world.bodies[&DOTS].previous.clone();
    let between = world.set_between(0.25);
    for i in 0..before.len() {
        let want = previous[i] + (before[i] - previous[i]) * 0.25;
        assert!((world.bodies[&DOTS].position[i] - want).length() < 1e-4);
    }
    assert!(before.iter().zip(&previous).any(|(a, b)| a != b), "the balls moved in the step");
    world.restore_between(between);
    assert_eq!(world.bodies[&DOTS].position, before, "put back bit for bit");
    world.advance_to(0.5, &sims, &mut log);
    twin.advance_to(0.5, &sims, &mut twin_log);
    for (id, b) in &world.bodies {
        assert_eq!(b.position, twin.bodies[id].position);
        assert_eq!(b.velocity, twin.bodies[id].velocity);
    }
}

#[test]
fn a_lone_copy_steps_only_what_it_keeps_as_the_world_does() {
    let (scene, resolved, sims) = dots_pair(120.0, Target::Anything);
    let mut world = World::build(&scene, &resolved, &sims);
    world.keep_history(DOTS, crate::HistoryWant { seconds: 2.0, forever: false });
    let mut log = Log::default();
    world.advance_to(0.1, &sims, &mut log);
    let mut lone = world.lone(&|id| id != DOTS2);
    assert!(lone.bodies.contains_key(&DOTS) && !lone.bodies.contains_key(&DOTS2));
    assert!(lone.walls.contains_key(&WALL) && !lone.states.contains_key(&DOTS2));
    assert!(lone.history.is_empty() && !lone.order().contains(&DOTS2));
    assert_eq!(lone.kind(DOTS2), Some("dots"), "still named");
    // Before the two meet, the kept ball goes exactly where it goes in the whole world.
    let mut lone_log = Log::default();
    for _ in 0..60 {
        world.step(&sims, &mut log);
        lone.step(&sims, &mut lone_log);
        assert_eq!(world.bodies[&DOTS].position, lone.bodies[&DOTS].position);
    }
    assert!(world.fingerprint.is_none(), "nothing asked for it");
}

#[test]
fn lent_cards_move_a_ball_as_their_owner_does() {
    let (scene, resolved, sims) = dots_pair(120.0, Target::Anything);
    let world = World::build(&scene, &resolved, &sims);
    let mut lone = world.lone(&|id| id != DOTS2);
    lone.modules.remove(&DOTS);
    assert!(lone.card(DOTS, SlotId::Motion).is_none());
    lone.lend_cards(&world, DOTS2, DOTS);
    assert_eq!(lone.card(DOTS, SlotId::Motion).map(|c| c.kind), Some("motion.gravity"));
}

#[test]
fn two_runs_give_the_same_log_and_a_ball_bounces() {
    let (scene, resolved, sims) = dots_in_ring(1);
    let run = || {
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(5.0, &sims, &mut log);
        (w, log)
    };
    let (w1, log1) = run();
    let (_, log2) = run();
    assert_eq!(log1, log2);
    assert!(log1.len() >= 3, "a launched ball under gravity should hit the ring a few times in 5 s");
    assert_eq!(log1.events()[0].kind, EventKind::Hit);
    assert_eq!(log1.events()[0].a, Party { thing: DOTS, item: 0 });
    assert_eq!(log1.events()[0].b, Some(Party { thing: WALL, item: 0 }));
    assert!(w1.bodies[&DOTS].position[0].length() < 100.0);
    assert_eq!(w1.step, 1200);
    assert_eq!(w1.time(), 5.0);
    assert_eq!(w1.bodies[&DOTS].hits[0] as usize, log1.len());
}

#[test]
fn hit_fraction_matches_where_the_ball_crossed() {
    // A ring of radius 100 (2 px thick): a 5 px dot touches it with its centre at 94. It starts
    // 4 px short of that and moves 10 px a step, so it touches 0.4 of the way through.
    let (mut scene, _, sims) = dots_in_ring(1);
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
    let dots = scene.things.get_mut(&DOTS).unwrap();
    dots.transform.position = glam::Vec2::new(90.0, 0.0);
    dots.params.set("velocity", Value::Vec2([2400.0, 0.0]));
    let resolved = resolve(&scene, &reg());
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.step(&sims, &mut log);
    let hit = log.events().iter().find(|e| e.kind == EventKind::Hit).expect("it hits in the first step");
    assert!((hit.at - 0.4).abs() < 0.02, "at = {}", hit.at);
}

#[test]
fn a_ball_pair_hit_knows_where_in_the_step_it_was() {
    // Two 5 px dots with centres 31 px apart, closing at 200 px/s: they touch 21 px of
    // closing later, 25.2 steps in (each step closes 200/240 px).
    let (scene, resolved, sims) = dots_pair(15.5, Target::Anything);
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(1.0, &sims, &mut log);
    let hit = log.events().iter().find(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == DOTS2));
    let hit = hit.expect("the dots meet");
    let closing = 200.0 / 240.0;
    let exact = 21.0 / closing;
    let expected = exact - (hit.step - 1) as f32;
    assert!((hit.at - expected).abs() < 0.05, "step {} at {} expected {expected}", hit.step, hit.at);
}

#[test]
fn things_run_in_id_order_and_events_are_step_sorted() {
    let (scene, resolved, sims) = dots_in_ring(50);
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(3.0, &sims, &mut log);
    assert!(log.len() > 50);
    assert!(log.events().windows(2).all(|p| p[0].step <= p[1].step));
    assert!(log.events().windows(2).filter(|p| p[0].step == p[1].step).all(|p| p[0].a <= p[1].a));
}

#[test]
fn a_resting_touch_is_not_a_hit() {
    let (scene, resolved, sims) = dots_in_ring(1);
    let mut w = World::build(&scene, &resolved, &sims);
    // Put the ball on the floor of the ring, at rest.
    w.bodies.get_mut(&DOTS).unwrap().position[0] = Vec2::new(0.0, 94.0);
    w.bodies.get_mut(&DOTS).unwrap().velocity[0] = Vec2::ZERO;
    let mut log = Log::default();
    w.advance_to(1.0, &sims, &mut log);
    assert!(log.is_empty(), "{:?}", log.events());
    assert!(w.bodies[&DOTS].position[0].y <= 94.0 + 1e-3);
}

#[test]
fn the_wall_can_take_over_the_bounce() {
    let (scene, resolved, sims) = dots_in_sticky_ring(true);
    let mut w = World::build(&scene, &resolved, &sims);
    w.advance_to(2.0, &sims, &mut Log::default());
    assert!(w.bodies[&DOTS].velocity[0].length() < 50.0);
}

#[test]
fn without_taking_over_the_mover_decides() {
    let (scene, resolved, sims) = dots_in_sticky_ring(false);
    let mut w = World::build(&scene, &resolved, &sims);
    w.advance_to(2.0, &sims, &mut Log::default());
    assert!(w.bodies[&DOTS].velocity[0].length() > 50.0);
    assert_eq!(w.module_state(DOTS, SlotId::Bounce).map(|(k, _)| k), Some("bounce.bouncy"));
    assert_eq!(w.module_state(WALL, SlotId::Bounce).map(|(k, _)| k), Some("bounce.sticky"));
    assert!(w.module_state(WALL, SlotId::Look).is_none());
}

#[test]
fn speeds_are_clamped_to_the_ceiling() {
    let (scene, resolved, sims) = dots_in_ring(1);
    let mut w = World::build(&scene, &resolved, &sims);
    w.bodies.get_mut(&DOTS).unwrap().velocity[0] = Vec2::new(1.0e9, 0.0);
    let mut log = Log::default();
    w.advance_to(1.0, &sims, &mut log);
    let p = w.bodies[&DOTS].position[0];
    assert!(p.is_finite() && p.length() < 100.0);
    assert!(w.bodies[&DOTS].velocity[0].length() <= w.limits().ceiling + 1.0);
    assert_eq!(w.limits().ceiling, 16.0 * 64.0 * 240.0);
}

/// A light probe that counts how often it is made.
struct CountingLight(std::sync::atomic::AtomicUsize);

impl crate::LightAt for CountingLight {
    fn level(&self, _: &World, _: &Log, _: ThingId, _: &bpe_core::LightSource, _: f64) -> f32 {
        1.0
    }
    fn ball(&self, _: &World, _: &Log, _: ThingId, _: usize, _: f64) -> crate::BallLight {
        crate::BallLight::default()
    }
    fn pull(&self, _: &World, _: &Log, _: ThingId, _: usize, _: f64, _: &crate::LightPull) -> Vec2 {
        Vec2::ZERO
    }
}

struct CountingMaker(std::sync::Arc<CountingLight>);

impl crate::LightMaker for CountingMaker {
    fn make(&self, _: &Scene, _: &Resolved) -> std::sync::Arc<dyn crate::LightAt> {
        self.0.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.0.clone()
    }
    fn fingerprint(&self, _: &Scene) -> u64 {
        7
    }
}

#[test]
fn a_world_makes_no_light_probe_unless_something_physical_follows_light() {
    let (scene, resolved, mut sims) = dots_in_ring(3);
    let counter = std::sync::Arc::new(CountingLight(std::sync::atomic::AtomicUsize::new(0)));
    let before = fingerprint(&scene, &resolved, &sims);
    sims.light = Some(std::sync::Arc::new(CountingMaker(counter.clone())));
    let mut world = World::build(&scene, &resolved, &sims);
    world.advance_to(0.5, &sims, &mut Log::default());
    assert!(world.light().is_none());
    assert_eq!(counter.0.load(std::sync::atomic::Ordering::Relaxed), 0);
    assert_eq!(fingerprint(&scene, &resolved, &sims), before, "light sensing off: the run is as it was");
    assert!(!needs_light(&scene, &resolved, &sims, None));
}

#[test]
fn look_changes_keep_the_fingerprint() {
    let (mut scene, resolved, sims) = dots_in_ring(1);
    let before = fingerprint(&scene, &resolved, &sims);
    scene.things.get_mut(&DOTS).unwrap().params.set("tint", Value::Number(3.0));
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("tint", Value::Number(3.0));
    let resolved = resolve(&scene, &reg());
    assert_eq!(fingerprint(&scene, &resolved, &sims), before);
    scene.things.get_mut(&DOTS).unwrap().transform.position.x += 1.0;
    let resolved = resolve(&scene, &reg());
    let moved = fingerprint(&scene, &resolved, &sims);
    assert_ne!(moved, before);
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 1.0]));
    let resolved = resolve(&scene, &reg());
    assert_ne!(fingerprint(&scene, &resolved, &sims), moved);
}

#[test]
fn a_gravity_track_changes_the_world_at_its_time() {
    let (mut scene, resolved, sims) = dots_in_ring(1);
    let reg = reg();
    let before = fingerprint(&scene, &resolved, &sims);
    let gravity = root_setting("gravity");
    let first = scene.set_key(&reg, &gravity, 0.0, Value::Vec2([0.0, 500.0])).unwrap();
    scene.set_key(&reg, &gravity, 1.0, Value::Vec2([0.0, -500.0])).unwrap();
    assert!(scene.set_ease(&gravity, first, Ease::Hold));
    let resolved = resolve(&scene, &reg);
    assert_ne!(fingerprint(&scene, &resolved, &sims), before, "a physics key changes the run");
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(0.5, &sims, &mut log);
    assert_eq!(w.gravity, Vec2::new(0.0, 500.0));
    w.advance_to(1.5, &sims, &mut log);
    assert_eq!(w.gravity, Vec2::new(0.0, -500.0));
    // The resolved scene the world was built from is untouched.
    assert_eq!(resolved.things[&ThingId::ROOT].params.get("gravity"), Some(&Value::Vec2([0.0, 500.0])));
}

/// A key at 0 s holds from the start: the world is built with it, not with the setting
/// under it.
#[test]
fn a_key_at_the_start_is_there_before_the_first_step() {
    let (mut scene, _, sims) = dots_in_ring(1);
    let reg = reg();
    let gravity = root_setting("gravity");
    scene.set_key(&reg, &gravity, 0.0, Value::Vec2([0.0, 100.0])).unwrap();
    scene.set_key(&reg, &gravity, 1.0, Value::Vec2([0.0, 100.0])).unwrap();
    let resolved = resolve(&scene, &reg);
    let w = World::build(&scene, &resolved, &sims);
    assert_eq!(w.gravity, Vec2::new(0.0, 100.0));
    assert_eq!(w.limits().resting, 100.0 * w.dt() * 4.0 + 1.0);
}

/// A frame's time, `index / fps`, lands on its own step even when the product rounds just
/// under it (2.05 s × 240 is 491.99999999999994), so the export and the editor agree.
#[test]
fn a_frame_time_lands_on_its_own_step() {
    let (scene, resolved, sims) = dots_in_ring(1);
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(123.0 / 60.0, &sims, &mut log);
    assert_eq!(w.step, 492);
    for index in 0..2000u64 {
        let time = index as f64 / 60.0;
        assert_eq!(crate::step_at(time, 240), index * 4, "frame {index}");
        let time = index as f64 / 30.0;
        assert_eq!(crate::step_at(time, 240), index * 8, "frame {index} at 30 fps");
    }
    let mut playback = crate::Playback::new(&scene, &resolved, std::sync::Arc::new(sims));
    playback.seek(2.05);
    assert_eq!(playback.world().step, 492);
}

#[test]
fn a_look_track_leaves_the_fingerprint_alone() {
    let (mut scene, resolved, sims) = dots_in_ring(1);
    let reg = reg();
    let before = fingerprint(&scene, &resolved, &sims);
    let tint = root_setting("tint");
    scene.set_key(&reg, &tint, 0.0, Value::Number(1.0)).unwrap();
    scene.set_key(&reg, &tint, 2.0, Value::Number(9.0)).unwrap();
    let resolved = resolve(&scene, &reg);
    assert_eq!(fingerprint(&scene, &resolved, &sims), before);
    assert!(physics_animation(&scene, &resolved, &sims).is_none());
    let mut w = World::build(&scene, &resolved, &sims);
    w.advance_to(1.0, &sims, &mut Log::default());
    assert_eq!(w.gravity, Vec2::new(0.0, 500.0));
}

#[test]
fn after_step_runs_once_per_step_with_the_log() {
    let (mut scene, _, sims) = dots_in_ring(1);
    let reg = reg();
    let counter = scene.add_thing(&reg, "counter", ThingId::ROOT).unwrap();
    let resolved = resolve(&scene, &reg);
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(1.0, &sims, &mut log);
    let counts: Vec<&Event> =
        log.events().iter().filter(|e| e.kind == EventKind::Hit && e.a.thing == counter).collect();
    assert_eq!(counts.len(), 240);
    assert_eq!(counts.first().unwrap().step, 1);
    assert_eq!(counts.last().unwrap().step, 240);
    assert!(counts.iter().all(|e| e.a == Party { thing: counter, item: 0 }));
    assert!(log.events().windows(2).all(|p| p[0].step <= p[1].step), "after_step events keep the log sorted");
}

#[test]
fn a_rider_starts_at_its_parents_pose_with_its_speed() {
    let (scene, resolved, sims) = ring_with_rider(Vec2::new(50.0, 0.0), true);
    let w = World::build(&scene, &resolved, &sims);
    assert_eq!(w.bodies[&DOTS].position[0], Vec2::new(50.0, 0.0));
    // The rim at (50, 0) of a ring turning clockwise at 1 rad/s moves down the screen at
    // 50 px/s, on top of the dots' own launch speed of (300, -200).
    let v = w.bodies[&DOTS].velocity[0];
    assert!((v.y - (-200.0 + 50.0)).abs() < 1e-3 && (v.x - 300.0).abs() < 1e-3, "{v:?}");
    assert_eq!(w.poses[&WALL].spin, 1.0);
    assert_eq!(w.anchor(DOTS).angle, 0.0);
}

#[test]
fn a_rider_without_a_parent_pose_uses_its_own_transform() {
    let (mut scene, _, _) = ring_with_rider(Vec2::new(50.0, 0.0), true);
    scene.things.get_mut(&DOTS).unwrap().transform.position = Vec2::new(7.0, 8.0);
    let resolved = resolve(&scene, &reg());
    // Without a wall sim the parent builds nothing and has no pose.
    let mut thin = Sims::default();
    thin.add_thing(TestRoot);
    thin.add_thing(TestDots);
    let w = World::build(&scene, &resolved, &thin);
    assert_eq!(w.bodies[&DOTS].position[0], Vec2::new(7.0, 8.0));
    assert_eq!(w.anchor(ThingId(99)), Pose::default());
    assert_eq!(w.anchor(ThingId::ROOT), Pose::default());
}

#[test]
fn build_and_scripted_follow_tree_order_not_id_order() {
    // A parent added after its child in id order must still be placed first.
    let (mut scene, _, sims) = ring_with_rider(Vec2::new(50.0, 0.0), false);
    let reg = reg();
    let late = scene.add_thing(&reg, "wall", ThingId::ROOT).unwrap();
    scene.things.get_mut(&late).unwrap().transform.position = Vec2::new(300.0, 0.0);
    scene.set_parent(DOTS, late, Attach::Ride { offset: Vec2::ZERO, turn_with_parent: false, item: None }).unwrap();
    let resolved = resolve(&scene, &reg);
    let mut w = World::build(&scene, &resolved, &sims);
    assert_eq!(w.bodies[&DOTS].position[0], Vec2::new(300.0, 0.0));
    assert_eq!(w.order, vec![ThingId::ROOT, WALL, late, DOTS]);
    w.step(&sims, &mut Log::default());
    assert_eq!(w.poses[&late].position, Vec2::new(300.0, 0.0));
}

#[test]
fn two_balls_heading_at_each_other_bounce_and_log_one_hit() {
    let (scene, resolved, sims) = dots_pair(30.0, Target::Anything);
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(1.0, &sims, &mut log);
    let hits: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Hit).collect();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!((hits[0].a.thing, hits[0].b.map(|b| b.thing)), (DOTS, Some(DOTS2)));
    assert!((hits[0].speed - 200.0).abs() < 5.0, "{:?}", hits[0]);
    assert!((hits[0].normal - Vec2::new(-1.0, 0.0)).length() < 1e-3, "from the second ball towards the first");
    let (a, b) = (w.bodies[&DOTS].velocity[0], w.bodies[&DOTS2].velocity[0]);
    assert!(a.x < -90.0 && b.x > 90.0, "{a:?} {b:?}");
    assert_eq!((w.bodies[&DOTS].hits[0], w.bodies[&DOTS2].hits[0]), (1, 1));
}

#[test]
fn a_target_that_does_not_match_lets_balls_pass_through() {
    let (scene, resolved, sims) = dots_pair(30.0, Target::Tag("hard".into()));
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(1.0, &sims, &mut log);
    assert!(log.is_empty());
    assert!(w.bodies[&DOTS].position[0].x > 0.0);
    let (scene, resolved, sims) = dots_pair(30.0, Target::Tag("soft".into()));
    let mut w = World::build(&scene, &resolved, &sims);
    w.advance_to(1.0, &sims, &mut Log::default());
    // Only the second ball is tagged soft: the first wants it, it doesn't want the first.
    assert!(w.bodies[&DOTS].position[0].x > 0.0);
    assert_eq!(w.matching(&Target::Tag("soft".into()), DOTS), vec![DOTS2]);
    assert_eq!(w.matching(&Target::Kind("dots".into()), DOTS), vec![DOTS, DOTS2]);
    assert!(w.matching(&Target::Thing(ThingId(99)), DOTS).is_empty());
    // Both sides pointing at the other's thing: they bump.
    let (scene, resolved, sims) = dots_pair(30.0, Target::Kind("dots".into()));
    let mut w = World::build(&scene, &resolved, &sims);
    w.advance_to(1.0, &sims, &mut Log::default());
    assert!(w.bodies[&DOTS].position[0].x < 0.0);
}

#[test]
fn a_fast_ball_never_crosses_a_thin_band_from_either_side() {
    for start_outside in [false, true] {
        let (mut scene, _, sims) = dots_in_ring(1);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
        scene.things.get_mut(&WALL).unwrap().params.set("radius", Value::Number(100.0));
        let dots = scene.things.get_mut(&DOTS).unwrap();
        let velocity = if start_outside { [-6000.0, 1300.0] } else { [6000.0, 2500.0] };
        dots.params.set("velocity", Value::Vec2(velocity));
        if start_outside {
            dots.transform.position = Vec2::new(300.0, 0.0);
        }
        let resolved = resolve(&scene, &reg());
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        let mut hits = 0;
        for k in 1..=480 {
            w.advance_to(k as f64 / 240.0, &sims, &mut log);
            let p = w.bodies[&DOTS].position[0];
            // The ring is 2 px thick at radius 100 and the dot's radius is 5.
            if start_outside {
                assert!(p.length() >= 106.0 - 1e-2, "step {k}: the dot got in, at {p}");
            } else {
                assert!(p.length() <= 94.0 + 1e-2, "step {k}: the dot got out, at {p}");
            }
            hits = log.events().len();
        }
        assert!(hits > 0, "it hit the band (outside {start_outside})");
    }
}

#[test]
fn pair_collisions_are_deterministic_in_a_pile() {
    let (mut scene, _, sims) = dots_pile(400);
    // Wide enough that the whole block of dots starts inside it: a dot that starts outside a
    // hollow wall stays outside.
    scene.things.get_mut(&WALL).unwrap().params.set("radius", Value::Number(175.0));
    let resolved = resolve(&scene, &reg());
    let run = || {
        let mut w = World::build(&scene, &resolved, &sims);
        let mut log = Log::default();
        w.advance_to(3.0, &sims, &mut log);
        (w, log)
    };
    let (w1, l1) = run();
    let (w2, l2) = run();
    assert_eq!(l1.hash(), l2.hash());
    assert_eq!(w1.bodies[&DOTS].position, w2.bodies[&DOTS].position);
    assert!(l1.events().iter().any(|e| e.b.is_some_and(|b| b.thing == DOTS)), "balls hit each other");
    assert!(w1.bodies[&DOTS].position.iter().all(|p| p.is_finite() && p.length() < 175.0));
    assert!(l1.events().windows(2).all(|p| p[0].step <= p[1].step));
}

#[test]
fn a_pile_of_balls_settles_instead_of_boiling() {
    let (scene, resolved, sims) = dots_pile(60);
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(4.0, &sims, &mut log);
    let bodies = &w.bodies[&DOTS];
    let average = bodies.velocity.iter().map(|v| v.length()).sum::<f32>() / bodies.len() as f32;
    let late = log.events().iter().filter(|e| e.step > 3 * w.steps_per_second as u64).count();
    assert!(average < 10.0, "a pile at rest barely moves, not {average} px/s");
    assert!(late < 10, "a pile at rest stops ticking, not {late} hits in its fourth second");
    // The ring is 2 px thick and the dots 5 px wide: none sinks into the floor.
    let farthest = bodies.position.iter().map(|p| p.length()).fold(0.0f32, f32::max);
    assert!(farthest <= 144.5, "nothing sinks through the floor: {farthest}");
}

#[test]
fn a_ball_pops_after_its_hits_and_logs_a_remove() {
    let (scene, resolved, sims) = dots_pair_popping(2);
    let mut w = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    w.advance_to(5.0, &sims, &mut log);
    let removes: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Remove).collect();
    assert_eq!(removes.len(), 1, "{removes:?}");
    assert_eq!(removes[0].a.thing, DOTS);
    assert!(!w.bodies[&DOTS].alive[0]);
    assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Hit && e.a.thing == DOTS).count(), 2);
    assert!(w.bodies[&DOTS2].alive[0], "the other ball never pops");
}

/// A held ball stays where it is through gravity, the wall it sits on and a ball heading
/// into it, and logs nothing; let go, it moves again.
#[test]
fn a_held_ball_stays_put_through_gravity_walls_and_pairs() {
    let (scene, resolved, sims) = dots_pair_popping(100);
    let mut w = World::build(&scene, &resolved, &sims);
    let held = w.bodies[&DOTS2].position[0];
    w.bodies.get_mut(&DOTS2).unwrap().held[0] = true;
    let mut log = Log::default();
    w.advance_to(3.0, &sims, &mut log);
    assert_eq!(w.bodies[&DOTS2].position[0], held, "never moved");
    assert!(log.events().iter().all(|e| e.a.thing != DOTS2 && e.b.is_none_or(|b| b.thing != DOTS2)), "{log:?}");
    assert!(log.events().iter().any(|e| e.a.thing == DOTS), "the other ball still bounces");
    w.bodies.get_mut(&DOTS2).unwrap().held[0] = false;
    w.advance_to(3.5, &sims, &mut log);
    assert_ne!(w.bodies[&DOTS2].position[0], held, "let go, it falls");
}

#[test]
fn tags_change_the_fingerprint() {
    let (mut scene, resolved, sims) = dots_in_ring(1);
    let before = fingerprint(&scene, &resolved, &sims);
    scene.things.get_mut(&DOTS).unwrap().tags.insert("x".into());
    assert_ne!(fingerprint(&scene, &resolve(&scene, &reg()), &sims), before);
}

#[test]
fn a_trail_keeps_a_history_that_follows_the_balls() {
    let (mut scene, resolved, sims) = dots_in_ring(2);
    let plain = World::build(&scene, &resolved, &sims);
    assert!(plain.history.is_empty());
    let before = fingerprint(&scene, &resolved, &sims);
    crate::testing::give(&mut scene, DOTS, SlotId::Trail, "trail.test", &[]);
    let resolved = resolve(&scene, &reg());
    // Keeping a history is a rebuild, but how the trail looks is not.
    let with_trail = fingerprint(&scene, &resolved, &sims);
    assert_ne!(with_trail, before);
    scene.things.get_mut(&DOTS).unwrap().slots.get_mut(&SlotId::Trail).unwrap().params.set("width", Value::Number(3.0));
    let resolved = resolve(&scene, &reg());
    assert_eq!(fingerprint(&scene, &resolved, &sims), with_trail);
    // A longer trail keeps a longer history, and that is a rebuild.
    fn trail(scene: &mut Scene) -> &mut bpe_core::Slot {
        scene.things.get_mut(&DOTS).unwrap().slots.get_mut(&SlotId::Trail).unwrap()
    }
    trail(&mut scene).params.set("seconds", Value::Number(5.0));
    let long = resolve(&scene, &reg());
    assert_ne!(fingerprint(&scene, &long, &sims), with_trail);
    assert_eq!(World::build(&scene, &long, &sims).history[&DOTS].cap(), 5 * 60 + 1);
    trail(&mut scene).params.set("seconds", Value::Number(0.5));
    let resolved = resolve(&scene, &reg());

    let mut world = World::build(&scene, &resolved, &sims);
    assert!(world.history.contains_key(&DOTS));
    assert!(world.history[&DOTS].is_empty());
    let mut log = Log::default();
    world.advance_to(1.0, &sims, &mut log);
    let history = &world.history[&DOTS];
    assert_eq!(history.len(), 60);
    assert_eq!(history.points(1).next(), Some(world.bodies[&DOTS].position[1]));
    assert!(history.points(0).nth(30).unwrap() != world.bodies[&DOTS].position[0]);
    assert!(world.memory() >= plain.memory() + 2 * 60 * 8);
    // The same scene without the history steps the same.
    let mut bare = World::build(&scene, &resolved, &sims);
    bare.history.clear();
    let mut bare_log = Log::default();
    bare.advance_to(1.0, &sims, &mut bare_log);
    assert_eq!(bare.bodies, world.bodies);
    assert_eq!(bare_log.events(), log.events());
}

/// A world nobody draws (the sound engine's, the particle thread's) keeps no trail paths and
/// steps exactly as a drawn one.
#[test]
fn an_unseen_world_keeps_no_trail_paths_and_steps_the_same() {
    let (mut scene, _, sims) = dots_in_ring(12);
    crate::testing::give(&mut scene, DOTS, SlotId::Trail, "trail.test", &[]);
    let resolved = resolve(&scene, &reg());
    let mut drawn = World::build(&scene, &resolved, &sims);
    let mut unseen = World::build_unseen(&scene, &resolved, &sims);
    assert!(drawn.history.contains_key(&DOTS));
    assert!(unseen.history.is_empty());
    let (mut drawn_log, mut unseen_log) = (Log::default(), Log::default());
    drawn.advance_to(5.0, &sims, &mut drawn_log);
    unseen.advance_to(5.0, &sims, &mut unseen_log);
    assert_eq!(unseen.bodies, drawn.bodies);
    assert_eq!(unseen_log.hash(), drawn_log.hash());
    assert!(unseen.memory() < drawn.memory());
}

/// A fast ball's history runs through every bounce: a trail drawn through it turns where the
/// ball really turned instead of cutting the corner between two samples.
#[test]
fn a_history_runs_through_every_bounce() {
    let (mut scene, _, sims) = dots_in_ring(1);
    scene.things.get_mut(&DOTS).unwrap().params.set("velocity", Value::Vec2([2600.0, -1700.0]));
    crate::testing::give(&mut scene, DOTS, SlotId::Trail, "trail.test", &[("seconds", Value::Number(2.0))]);
    let resolved = resolve(&scene, &reg());
    let mut world = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    world.advance_to(1.5, &sims, &mut log);
    let history = &world.history[&DOTS];
    let radius = world.bodies[&DOTS].radius[0];
    let trace: Vec<(f64, Vec2)> = history.trace(0).collect();
    assert!(trace.windows(2).all(|w| w[0].0 > w[1].0), "newest first, in order");
    let oldest = trace.last().unwrap().0;
    let hits: Vec<&Event> = log
        .events()
        .iter()
        .filter(|e| e.kind == EventKind::Hit && e.a.thing == DOTS && e.step as f64 > oldest + 1.0)
        .collect();
    assert!(hits.len() >= 5, "{} bounces", hits.len());
    for e in hits {
        // Where the ball's centre was as it touched.
        let centre = e.point + e.normal * radius;
        let nearest = trace.iter().map(|(_, p)| (*p - centre).length()).fold(f32::INFINITY, f32::min);
        assert!(nearest < 1.0, "the bounce on step {} is {nearest} px from the history", e.step);
    }
}

#[test]
fn a_thing_without_a_sim_is_skipped() {
    let (mut scene, _, sims) = dots_in_ring(1);
    let reg = reg();
    let extra = scene.add_thing(&reg, "wall", ThingId::ROOT).unwrap();
    scene.things.get_mut(&extra).unwrap().kind = "scene".into();
    let mut thin = Sims::default();
    thin.add_thing(TestDots);
    let resolved = resolve(&scene, &reg);
    let w = World::build(&scene, &resolved, &thin);
    assert!(w.walls.is_empty());
    assert_eq!(w.bodies.len(), 1);
    assert_eq!(w.steps_per_second, 240);
    drop(sims);
}

#[test]
fn targets_resolve_by_tag_kind_thing_and_me() {
    let (mut scene, _, sims) = dots_in_ring(1);
    scene.things.get_mut(&DOTS).unwrap().tags.insert("red".into());
    let resolved = resolve(&scene, &reg());
    let w = World::build(&scene, &resolved, &sims);
    assert_eq!(w.matching(&Target::Tag("red".into()), WALL), vec![DOTS]);
    assert_eq!(w.matching(&Target::Tag("blue".into()), WALL), vec![]);
    assert_eq!(w.matching(&Target::Kind("wall".into()), DOTS), vec![WALL]);
    assert_eq!(w.matching(&Target::Thing(DOTS), WALL), vec![DOTS]);
    assert_eq!(w.matching(&Target::Thing(ThingId(99)), WALL), vec![]);
    assert_eq!(w.matching(&Target::Me, WALL), vec![WALL]);
    assert_eq!(w.matching(&Target::Anything, WALL), vec![ThingId::ROOT, WALL, DOTS]);
    assert_eq!(w.kind(DOTS), Some("dots"));
    assert_eq!(w.kind(ThingId(99)), None);
    assert!(w.tags(DOTS).contains("red"));
    assert!(w.tags(WALL).is_empty());
}

#[test]
fn lent_states_are_there_for_drawing_and_gone_after() {
    let (scene, resolved, sims) = dots_in_ring(3);
    let mut w = World::build(&scene, &resolved, &sims);
    let before: Vec<ThingId> = w.states.keys().copied().collect();
    let lent = w.lend_states(vec![(ThingId(77), Box::new(5u32) as Box<dyn ThingState>)]);
    assert_eq!(*crate::state::<u32>(w.states[&ThingId(77)].as_ref()), 5);
    w.take_back(lent);
    assert_eq!(w.states.keys().copied().collect::<Vec<_>>(), before);
    assert!(w.beside.is_empty(), "every test kind steps with the physics");
    assert!(matches!(main_part(&scene, &resolved, &sims), std::borrow::Cow::Borrowed(_)));
}
