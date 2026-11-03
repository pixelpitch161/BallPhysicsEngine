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
    // Wet paint that bleeds and drips, and brushes that pick it up.
    canvas_params.set("bleed", Value::Number(0.5));
    canvas_params.set("drips", Value::Number(0.5));
    canvas_params.set("dry_time", Value::Number(2.0));
    let brushes = bpe_core::resolve_slot(&scene, &kit.registry, ThingId(2), SlotId::Marks, &mut Vec::new())
        .expect("the balls' brushes")
        .module;
    scene.library.modules.get_mut(&brushes).unwrap().params.set("paint_pickup", Value::Number(0.5));
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

/// Skins keep up: 50 balls in a ring arena throwing Spread splats cost under a millisecond of
/// skin work a step, on average.
#[test]
#[ignore = "a speed check, run with --release"]
fn skin_step_is_quick() {
    use bpe_core::{SlotId, Value};
    use bpe_kit::things::canvas::skin_state;
    const STEPS: u32 = 1200;

    let kit = bpe_kit::kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    let balls = scene.things.get_mut(&ThingId(2)).expect("the balls");
    let mut settings = BallsSettings::from_params(&balls.params);
    (settings.arrangement, settings.count, settings.ring_radius, settings.radius) =
        (bpe_kit::Arrangement::Ring, 50, 300.0, 10.0);
    balls.params = settings.to_params();
    let brush = bpe_kit::MarksBrushes {
        paint_on: false,
        splat_on: true,
        splat_skin: bpe_kit::SplatSkin::Spread,
        splat_min_speed: 0.0,
        ..bpe_kit::MarksBrushes::default()
    };
    let module = scene.add_module(&kit.registry, "marks.brushes", "Spread").expect("the brushes kind");
    scene.library.modules.get_mut(&module).unwrap().params = brush.to_params();
    scene.link(&kit.registry, ThingId(2), SlotId::Marks, module).expect("the balls take brushes");
    let arena = &mut scene.things.get_mut(&ThingId(1)).unwrap().params;
    arena.set("skin_on", Value::Bool(true));
    arena.set("skin_drips", Value::Number(0.5));
    arena.set("skin_dry_time", Value::Number(2.0));
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);

    let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
    let mut log = bpe_runtime::Log::default();
    let sim = kit.sims.thing("arena").expect("the arena sim");
    let mut total = Duration::ZERO;
    for _ in 0..STEPS {
        world.step(&kit.sims, &mut log);
        total += timed(|| sim.after_step(ThingId(1), &mut world, &mut log));
    }
    let mean = total / STEPS;
    println!("skin marking under 50 balls with Spread splats: mean {mean:?} a step, {} marks", log.marks().len());
    assert!(skin_state(&world, ThingId(1)).is_some(), "the arena has a skin");
    assert!(log.marks().len() > 100, "the balls painted: {} marks", log.marks().len());
    assert!(mean < Duration::from_millis(1), "a skin step takes {mean:?}");
}

/// The mean time one `after_step` of the canvas takes over a minute of steps of the Marks demo
/// called `name`.
fn demo_canvas_step(name: &str) -> (Duration, f32) {
    const STEPS: u32 = 600;
    let kit = bpe_kit::kit();
    let scene = scenes::marks(&kit.registry).into_iter().find(|(n, _)| n == name).expect("the demo").1;
    let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let canvas = *scene.things.iter().find(|(_, t)| t.kind == "canvas").expect("a canvas").0;
    let mut world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
    let mut log = bpe_runtime::Log::default();
    let sim = kit.sims.thing("canvas").expect("the canvas sim");
    let mut total = Duration::ZERO;
    for _ in 0..STEPS {
        let state = world.states.remove(&canvas).expect("the canvas state");
        world.step(&kit.sims, &mut log);
        world.states.insert(canvas, state);
        total += timed(|| sim.after_step(canvas, &mut world, &mut log));
    }
    // How much the balls did: ground scraped away, or claimed.
    let n = world.canvas_numbers_of(canvas).expect("the canvas reported");
    (total / STEPS, n.revealed + n.shares.iter().sum::<f32>())
}

/// Scraping sand and wiping fog cost under a millisecond of canvas work a step, on average.
#[test]
#[ignore = "a speed check, run with --release"]
fn surface_step_is_quick() {
    for name in ["Sand garden", "Fog glass"] {
        let (mean, done) = demo_canvas_step(name);
        println!("{name}: canvas step mean {mean:?}, {done:.3} of the canvas worked");
        assert!(done > 0.005, "{name}: the balls worked the canvas ({done})");
        assert!(mean < Duration::from_millis(1), "{name}: a canvas step takes {mean:?}");
    }
}

/// A colour war's claims cost under a millisecond of canvas work a step, on average.
#[test]
#[ignore = "a speed check, run with --release"]
fn claims_step_is_quick() {
    let (mean, done) = demo_canvas_step("Territory");
    println!("Territory: canvas step mean {mean:?}, {done:.3} of the canvas claimed");
    assert!(done > 0.005, "the balls claimed ground ({done})");
    assert!(mean < Duration::from_millis(1), "a canvas step takes {mean:?}");
}
