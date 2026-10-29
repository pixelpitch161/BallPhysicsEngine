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
