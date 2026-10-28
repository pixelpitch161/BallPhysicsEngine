//! Soft bodies, balls bumping into each other, pockets and custom-shape arenas, run through the
//! simulation as a scene would.

use bpe_scene::{BallContact, Color, Container, Pocket, Scene, Spawner, Vec2};
use bpe_sim::{Event, EventKind, Playback, Simulation};

fn preset(name: &str) -> Scene {
    Scene::presets().into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no preset {name}")).scene
}

fn run(scene: &Scene, seconds: f64) -> (Simulation, Vec<Event>) {
    let mut sim = Simulation::new(scene);
    let mut events = Vec::new();
    sim.advance_to(seconds, &mut events);
    (sim, events)
}

fn ball(position: Vec2, velocity: Vec2, radius: f32) -> Spawner {
    Spawner::Single { position, velocity, radius, color: Color::WHITE, depth: 0.0, depth_velocity: 0.0 }
}

/// Every soft point (position, velocity) and ball (position, velocity, alive).
type State = (Vec<(Vec2, Vec2)>, Vec<(Vec2, Vec2, bool)>);

/// Every soft point and ball, for comparing two runs exactly.
fn state(sim: &Simulation) -> State {
    let soft = sim.soft().points.iter().map(|p| (p.position, p.velocity)).collect();
    let balls = sim.balls().iter().map(|b| (b.position, b.velocity, b.alive)).collect();
    (soft, balls)
}

const SOFT_PRESETS: [&str; 8] =
    ["Jelly", "Rope bridge", "Cloth", "Liquid", "Billiards", "Stacking", "Newton's cradle", "Heart arena"];

#[test]
fn seeking_back_lands_on_the_same_soft_bodies_as_playing_through() {
    for name in SOFT_PRESETS {
        let scene = preset(name);
        let mut playback = Playback::new(&scene);
        playback.seek(3.1);
        playback.seek(1.37);
        let mut fresh = Playback::new(&scene);
        fresh.seek(1.37);
        assert_eq!(state(playback.simulation()), state(fresh.simulation()), "{name}");
        assert_eq!(playback.events(), fresh.events(), "{name}");
    }
}

#[test]
fn liquid_comes_out_the_same_on_any_number_of_threads() {
    let scene = preset("Liquid");
    let on = |threads| {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        pool.install(|| {
            let (sim, events) = run(&scene, 1.5);
            (state(&sim), events)
        })
    };
    let one = on(1);
    assert_eq!(one, on(3));
    assert_eq!(one, on(4));
}

#[test]
fn soft_bodies_stay_whole_and_inside() {
    for name in SOFT_PRESETS {
        let scene = preset(name);
        let (sim, events) = run(&scene, 8.0);
        let soft = sim.soft();
        assert!(soft.points.iter().all(|p| p.position.is_finite() && p.velocity.is_finite()), "{name}");
        assert!(sim.balls().iter().all(|b| b.position.is_finite() && b.velocity.is_finite()), "{name}");
        // Nothing gets out of the frame.
        let reach = 1300.0;
        assert!(soft.points.iter().all(|p| p.position.length() < reach), "{name}");
        assert!(sim.balls().iter().all(|b| b.position.length() < reach), "{name}");
        assert!(events.windows(2).all(|w| w[0].time <= w[1].time), "{name}: events out of order");
        for b in 0..soft.bodies.len() {
            if soft.bodies[b].kind == bpe_sim::bodies::BodyKind::Jelly {
                let squash = soft.squash(b);
                assert!((0.5..1.5).contains(&squash), "{name}: jelly {b} squashed to {squash}");
            }
        }
    }
}

#[test]
fn soft_bodies_make_sounds_when_they_land() {
    for name in ["Jelly", "Rope bridge", "Liquid"] {
        let (_, events) = run(&preset(name), 6.0);
        let soft = events.iter().filter(|e| matches!(e.kind, EventKind::Soft { .. })).count();
        assert!(soft > 3, "{name}: only {soft} soft sounds");
        // Each body sounds at most every few hundredths of a second, not every step.
        assert!(soft < 6 * 60, "{name}: {soft} soft sounds is a buzz");
    }
}

#[test]
fn pinned_rope_ends_stay_put_and_a_cut_rope_falls() {
    let scene = preset("Rope bridge");
    let (mut sim, _) = run(&scene, 2.0);
    let rope = &scene.bodies.ropes[0];
    let ends = |sim: &Simulation| {
        let body = &sim.soft().bodies[0];
        let points = &sim.soft().points;
        (points[body.points.start as usize].position, points[body.points.end as usize - 1].position)
    };
    assert_eq!(ends(&sim), (rope.from, rope.to));
    let lowest = |sim: &Simulation| sim.soft().points.iter().map(|p| p.position.y).fold(f32::MIN, f32::max);
    let before = lowest(&sim);
    assert!(sim.cut_soft(0, 0.5));
    assert!(!sim.cut_soft(0, 0.5), "a link can only be cut once");
    let mut events = Vec::new();
    sim.advance_to(4.0, &mut events);
    assert_eq!(ends(&sim), (rope.from, rope.to));
    assert!(lowest(&sim) > before + 100.0, "the cut halves should hang down");
    // Letting go of an end drops it.
    assert!(sim.pin_soft(0, 0, false));
    sim.advance_to(5.0, &mut events);
    assert!(ends(&sim).0.y > rope.from.y + 50.0);
}

fn two_balls_meeting(contact: BallContact) -> Scene {
    Scene {
        container: Container::Rect {
            center: Vec2::ZERO,
            size: Vec2::new(1000.0, 1000.0),
            thickness: 4.0,
            color: Color::WHITE,
        },
        spawners: vec![
            ball(Vec2::new(-200.0, 0.0), Vec2::new(300.0, 0.0), 30.0),
            ball(Vec2::new(200.0, 0.0), Vec2::new(-300.0, 0.0), 30.0),
        ],
        physics: bpe_scene::Physics {
            gravity: Vec2::ZERO,
            restitution: 1.0,
            ball_collisions: true,
            contact,
            ..bpe_scene::Physics::default()
        },
        ..Scene::default()
    }
}

#[test]
fn balls_meeting_make_one_ball_event_and_swap_speeds() {
    let scene = two_balls_meeting(BallContact { quality: 8, ..BallContact::default() });
    let (sim, events) = run(&scene, 0.9);
    let meetings: Vec<_> = events.iter().filter(|e| matches!(e.kind, EventKind::Ball(_))).collect();
    assert_eq!(meetings.len(), 1, "{meetings:?}");
    assert_eq!(meetings[0].kind, EventKind::Ball(1));
    assert_eq!(meetings[0].ball, 0);
    assert!((meetings[0].speed - 600.0).abs() < 30.0, "closing speed {}", meetings[0].speed);
    assert!((meetings[0].position.x).abs() < 15.0);
    // A perfect bounce between equal balls sends each back the way it came.
    let (a, b) = (sim.balls()[0].velocity, sim.balls()[1].velocity);
    assert!(a.x < -280.0 && b.x > 280.0, "{a} {b}");
}

#[test]
fn the_classic_solver_makes_no_ball_events() {
    let scene = two_balls_meeting(BallContact::default());
    let (_, events) = run(&scene, 0.9);
    assert!(!events.iter().any(|e| matches!(e.kind, EventKind::Ball(_))));
}

#[test]
fn pockets_swallow_balls() {
    let contact = BallContact {
        quality: 8,
        pockets: vec![Pocket { position: Vec2::new(400.0, 0.0), radius: 40.0 }],
        ..BallContact::default()
    };
    let mut scene = two_balls_meeting(contact);
    scene.spawners = vec![ball(Vec2::ZERO, Vec2::new(500.0, 0.0), 20.0)];
    let (sim, events) = run(&scene, 1.5);
    let kinds: Vec<_> = events.iter().filter(|e| e.ball == 0).map(|e| e.kind).collect();
    assert_eq!(kinds, [EventKind::Escape(0), EventKind::Remove]);
    assert!(!sim.balls()[0].alive);
    assert_eq!(sim.alive_balls(), 0);
}

#[test]
fn a_pile_settles_and_falls_asleep() {
    let scene = preset("Stacking");
    let (mut sim, _) = run(&scene, 20.0);
    assert!((0..sim.balls().len()).all(|i| sim.ball_asleep(i)), "the pile should be asleep");
    let before: Vec<_> = sim.balls().iter().map(|b| b.position).collect();
    let mut events = Vec::new();
    sim.advance_to(21.0, &mut events);
    let after: Vec<_> = sim.balls().iter().map(|b| b.position).collect();
    assert_eq!(before, after, "sleeping balls don't move");
    assert!(events.is_empty(), "a sleeping pile is silent");
}

#[test]
fn a_heart_arena_keeps_every_ball_inside() {
    let scene = preset("Heart arena");
    let (sim, events) = run(&scene, 12.0);
    let place = sim.arena_placement().unwrap();
    let Container::Custom { shape, .. } = &scene.container else { panic!() };
    let lines = scene.shape(shape).unwrap().flatten(place.size);
    for ball in sim.balls() {
        let local = place.invert(ball.position);
        assert!(bpe_scene::shapes::inside(&lines, local), "ball escaped the heart at {}", ball.position);
    }
    assert!(events.iter().filter(|e| e.kind == EventKind::Wall).count() > 100);
}

#[test]
fn spinning_balls_turn() {
    let mut scene = preset("Billiards");
    scene.physics.contact.spin = true;
    let (sim, _) = run(&scene, 1.0);
    assert!((0..sim.balls().len()).any(|i| sim.ball_angle(i) != 0.0));
}

/// Random variations of the soft-body and ball-bumping presets, with their soft things and
/// bumping settings rolled too: nothing panics, nothing turns into NaN or leaves the frame,
/// and a second run gives the same log. Slow, so it only runs when asked:
/// `cargo test --release -p bpe-sim --test soft -- --ignored` (`FUZZ_SEEDS`, `FUZZ_SECONDS`).
#[test]
#[ignore]
fn rolled_soft_scenes_stay_sane_and_repeatable() {
    use bpe_scene::{ClothPin, Dice, Group};
    let number = |name: &str, default: f64| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
    let seeds = number("FUZZ_SEEDS", 500.0) as u64;
    let seconds = number("FUZZ_SECONDS", 10.0);
    let mut failures = Vec::new();
    for seed in 0..seeds {
        let name = SOFT_PRESETS[seed as usize % SOFT_PRESETS.len()];
        let base = preset(name);
        let groups: Vec<Group> = (0..base.spawners.len())
            .map(Group::Balls)
            .chain([Group::Shape, Group::Movement, Group::World, Group::Bounces, Group::NewBalls])
            .collect();
        let mut scene = base.variation(&groups, seed, 1.0);
        let mut dice = Dice::new(seed ^ 0xb0d1e5);
        let contact = &mut scene.physics.contact;
        if dice.chance(0.5) {
            scene.physics.ball_collisions = true;
            contact.quality = dice.int(0, 32);
            contact.friction = dice.range(0.0, 1.5);
            contact.spin = dice.chance(0.5);
            contact.sleep = dice.chance(0.7);
        }
        for jelly in &mut scene.bodies.jelly {
            jelly.squishiness = dice.range(0.0, 1.0);
            jelly.radius = dice.range(8.0, 200.0);
            jelly.points = dice.int(3, 70);
        }
        for rope in &mut scene.bodies.ropes {
            rope.stiffness = dice.range(0.0, 1.0);
            rope.slack = dice.range(0.2, 3.0);
            rope.points = dice.int(1, 300);
        }
        for cloth in &mut scene.bodies.cloths {
            cloth.stiffness = dice.range(0.0, 1.0);
            cloth.pin = dice.pick(&[ClothPin::None, ClothPin::TopRow, ClothPin::TopCorners]);
        }
        if scene.bodies.fluid.count > 0 {
            scene.bodies.fluid.count = dice.int(1, 6000);
            scene.bodies.fluid.radius = dice.range(2.0, 20.0);
            scene.bodies.fluid.thickness = dice.range(0.0, 1.0);
        }
        scene.bodies.iterations = dice.int(1, 24);
        let label = format!("{name} seed {seed}");
        let (sim, events) = run(&scene, seconds);
        let soft = sim.soft();
        if soft.points.iter().any(|p| !p.position.is_finite() || !p.velocity.is_finite()) {
            failures.push(format!("{label}: a soft point is not a number"));
        }
        if sim.balls().iter().any(|b| !b.position.is_finite() || !b.velocity.is_finite()) {
            failures.push(format!("{label}: a ball is not a number"));
        }
        let rings = matches!(scene.container, Container::Rings { .. });
        if !rings && soft.points.iter().any(|p| p.position.length() > 3000.0) {
            failures.push(format!("{label}: a soft point left the frame"));
        }
        if !rings && sim.balls().iter().any(|b| b.alive && b.position.length() > 3000.0) {
            failures.push(format!("{label}: a ball left the frame"));
        }
        if events.windows(2).any(|w| w[1].time < w[0].time) {
            failures.push(format!("{label}: events out of time order"));
        }
        if seed % 5 == 0 && run(&scene, seconds).1 != events {
            failures.push(format!("{label}: a second run gave a different log"));
        }
    }
    assert!(failures.is_empty(), "{} problems:\n{}", failures.len(), failures.join("\n"));
}
