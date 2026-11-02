//! The new engine against the old one on ten thousand balls. Ignored by default: run with
//! `cargo test --release -p bpe-kit --test speed -- --ignored --nocapture`.

use bpe_core::{Settings, ThingId, resolve};
use bpe_kit::{BallsSettings, scenes};
use std::time::{Duration, Instant};

/// A 100 × 100 block of small balls in the old engine's reference reel.
fn old_scene() -> bpe_scene::Scene {
    let mut scene = bpe_scene::Scene::default();
    match &mut scene.spawners[0] {
        bpe_scene::Spawner::Grid { columns, rows, spacing, radius, .. } => {
            (*columns, *rows, *spacing, *radius) = (100, 100, 9.0, 4.0);
        }
        other => panic!("the reference reel starts with a grid, not {other:?}"),
    }
    scene
}

/// The same block in the new engine's reel.
fn new_scene(reg: &bpe_core::Registry) -> bpe_core::Scene {
    let mut scene = scenes::stage2(reg).remove(1).1;
    let block = scene.things.get_mut(&ThingId(2)).expect("the reel's block");
    let mut settings = BallsSettings::from_params(&block.params);
    (settings.columns, settings.rows, settings.spacing, settings.radius) = (100, 100, 9.0, 4.0);
    block.params = settings.to_params();
    scene
}

fn timed(f: impl FnOnce()) -> Duration {
    let started = Instant::now();
    f();
    started.elapsed()
}

#[test]
#[ignore = "a speed comparison, run with --release"]
fn ten_thousand_balls_step_no_slower_than_the_old_engine() {
    let old_scene = old_scene();
    let mut old = bpe_sim::Playback::new(&old_scene);
    old.seek(1.0);
    let old_time = timed(|| old.seek(5.0));

    let kit = bpe_kit::kit();
    let scene = new_scene(&kit.registry);
    let resolved = resolve(&scene, &kit.registry);
    let mut new = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
    new.seek(1.0);
    let new_time = timed(|| new.seek(5.0));

    let balls = new.world().bodies.values().map(|b| b.len()).sum::<usize>();
    println!("stepping {balls} balls from 1 s to 5 s: old {old_time:?}, new {new_time:?}");
    assert!(new_time <= old_time.mul_f64(1.1), "new {new_time:?} is slower than old {old_time:?}");
}

#[test]
#[ignore = "a speed comparison, run with --release"]
fn ten_thousand_balls_build_a_frame_no_slower() {
    const ROUNDS: u32 = 20;

    let old_scene = old_scene();
    let mut old = bpe_sim::Playback::new(&old_scene);
    old.seek(5.0);
    let mut old_frame = bpe_render::FrameData::default();
    old_frame.fill(&old_scene, &old, 1.0);
    let old_time = timed(|| {
        for _ in 0..ROUNDS {
            old_frame.fill(&old_scene, &old, 1.0);
        }
    }) / ROUNDS;

    let kit = bpe_kit::kit();
    let scene = new_scene(&kit.registry);
    let resolved = resolve(&scene, &kit.registry);
    let mut new = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
    new.seek(5.0);
    let mut frame = bpe_gfx::Frame::default();
    let build = |frame: &mut bpe_gfx::Frame| {
        bpe_gfx::build_frame(&scene, &resolved, new.world(), new.log(), 5.0, &kit.draws, frame);
    };
    build(&mut frame);
    let new_time = timed(|| {
        for _ in 0..ROUNDS {
            build(&mut frame);
        }
    }) / ROUNDS;

    println!("building a frame of {} balls at 5 s: old {old_time:?}, new {new_time:?}", frame.balls.len());
    assert_eq!(frame.balls.len(), 10_000);
    assert!(new_time <= old_time.mul_f64(1.1), "new {new_time:?} is slower than old {old_time:?}");
}

/// The canvas keeps up: 20 balls painting trails on a full-size canvas cost under a millisecond
/// of marking a step, on average.
#[test]
#[ignore = "a speed check, run with --release"]
fn canvas_step_is_quick() {
    use bpe_core::{SlotId, Value};
    const STEPS: u32 = 600;

    let kit = bpe_kit::kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    let balls = scene.things.get_mut(&ThingId(2)).expect("the balls");
    let mut settings = BallsSettings::from_params(&balls.params);
    (settings.arrangement, settings.count, settings.ring_radius, settings.radius) =
        (bpe_kit::Arrangement::Ring, 20, 300.0, 14.0);
    balls.params = settings.to_params();
    let canvas = bpe_kit::add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).expect("a canvas");
    let canvas_params = &mut scene.things.get_mut(&canvas).unwrap().params;
    canvas_params.set("fit", Value::Choice("custom".into()));
    canvas_params.set("size", Value::Vec2([1080.0, 1920.0]));
    assert!(scene.things[&ThingId(2)].slots.contains_key(&SlotId::Marks), "the balls paint");
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);

    let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
    let mut log = bpe_runtime::Log::default();
    let sim = kit.sims.thing("canvas").expect("the canvas sim");
    let mut total = Duration::ZERO;
    for _ in 0..STEPS {
        // Keep the canvas's own turn out of the step, so only its marking is timed.
        let state = world.states.remove(&canvas).expect("the canvas state");
        world.step(&kit.sims, &mut log);
        world.states.insert(canvas, state);
        total += timed(|| sim.after_step(canvas, &mut world, &mut log));
    }
    let mean = total / STEPS;
    println!("marking a 1080 x 1920 canvas under 20 painting balls: mean {mean:?} a step, {} marks", log.marks().len());
    assert!(log.marks().len() > 100, "the balls painted: {} marks", log.marks().len());
    assert!(mean < Duration::from_millis(1), "a canvas step takes {mean:?}");
}
