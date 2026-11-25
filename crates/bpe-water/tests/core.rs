//! The water core's own checks (Water mode part 2a): pools rest and keep their height, walls hold,
//! narrow openings flow, and a run repeats exactly.

use std::sync::Arc;

use bpe_water::probe::{self, Stats};
use bpe_water::scenes::{self, Scene};
use bpe_water::{Numbers, Pour, Solid, WaterGpu};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

#[test]
fn still_water_rests() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::still_box(8.0, 0.3).start(&gpu, Numbers::default());
    run.run_to(2.0);
    let s = Stats::of(&run.water.read(), &run.water.layout());
    assert_eq!(s.broken, 0);
    assert!(s.rms < 3.0, "still water is at rest: {} px/s", s.rms);
    assert!(s.fastest < 60.0, "and no drop of it is flung about: {} px/s", s.fastest);
}

#[test]
fn a_pool_keeps_its_height() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::still_box(8.0, 0.3).start(&gpu, Numbers::default());
    let layout = run.water.layout();
    let before = Stats::of(&run.water.read(), &layout);
    run.run_to(3.0);
    let after = Stats::of(&run.water.read(), &layout);
    assert_eq!(after.drops, before.drops, "no drop is lost");
    // (Its surface is 270 px above its floor; a cell is 8.)
    assert!(
        (after.top - before.top).abs() < 6.0,
        "the pool's top stays where it was: {} then {}",
        before.top,
        after.top
    );
}

#[test]
fn two_runs_are_the_same() {
    let Some(gpu) = card() else { return };
    let play = || {
        let mut run = scenes::weird(8.0).start(&gpu, Numbers::default());
        run.run_to(0.6);
        probe::print_of(&run.water.read())
    };
    assert_eq!(play(), play(), "the same scene played twice gives the same drops, to the bit");
}

#[test]
fn a_copy_plays_on_the_same() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::weird(8.0).start(&gpu, Numbers::default());
    run.run_to(0.3);
    let mut copy = run.copy();
    run.run_to(0.6);
    copy.run_to(0.6);
    assert_eq!(probe::print_of(&run.water.read()), probe::print_of(&copy.water.read()));
}

/// A box 600 wide with a 3-pixel wall and a pour inside it, aimed `aim` degrees at `speed`.
fn pour_in_a_box(aim: f32, speed: f32) -> Scene {
    let mut scene = scenes::boxed(8.0, 600.0, 3.0, 0.0);
    scene.fills.clear();
    scene.pours = vec![Pour { place: Vec2::new(0.0, -200.0), aim, width: 24.0, speed, ..Pour::default() }];
    scene
}

fn outside(run: &scenes::Running, half: Vec2) -> usize {
    let layout = run.water.layout();
    run.water.read().iter().filter(|d| layout.to_world(Vec2::from(d.x)).abs().cmpgt(half).any()).count()
}

#[test]
fn a_thin_wall_holds_a_fast_pour() {
    let Some(gpu) = card() else { return };
    // Straight at the side wall at 2,500 px/s: over a cell a liquid step.
    let mut run = pour_in_a_box(0.0, 2500.0).start(&gpu, Numbers::default());
    run.run_to(1.0);
    assert!(run.water.count() > 1000, "the pour pours: {}", run.water.count());
    assert_eq!(outside(&run, Vec2::new(300.0, 450.0)), 0, "no drop is past the wall's line");
}

#[test]
fn a_full_box_stays_shut() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::boxed(8.0, 240.0, 3.0, 0.97).start(&gpu, Numbers::default());
    let n = run.water.count();
    run.run_to(1.5);
    assert_eq!(run.water.count(), n);
    assert_eq!(outside(&run, Vec2::new(120.0, 450.0)), 0, "a box full to the brim leaks nothing");
    let s = Stats::of(&run.water.read(), &run.water.layout());
    assert!(s.rms < 6.0, "and is at rest: {} px/s", s.rms);
}

#[test]
fn water_runs_through_a_narrow_opening() {
    let Some(gpu) = card() else { return };
    // A pool over a floor with an opening 16 px wide (two cells), with room under it.
    let mut scene = scenes::boxed(8.0, 400.0, 16.0, 0.0);
    let shelf = |x0: f32, x1: f32| Solid::rect(Vec2::new(x0, 0.0), Vec2::new(x1, 24.0), true, 0.0);
    scene.solids.push(shelf(-200.0, -8.0));
    scene.solids.push(shelf(8.0, 200.0));
    scene.fills = vec![(Vec2::new(-190.0, -160.0), Vec2::new(190.0, -2.0))];
    let mut run = scene.start(&gpu, Numbers::default());
    let n = run.water.count();
    run.run_to(2.0);
    let layout = run.water.layout();
    let under = run.water.read().iter().filter(|d| layout.to_world(Vec2::from(d.x)).y > 30.0).count();
    assert!(under * 20 > n as usize, "a twentieth of the pool is through a two-cell opening in 2 s: {under} of {n}");
    assert_eq!(outside(&run, Vec2::new(200.0, 450.0)), 0);
}

/// The trial's fault: empty air braked fast drops, so a thin pour piled up on its own gaps.
#[test]
fn the_air_does_not_brake_a_pour() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::thin_pour(8.0, 2.0).start(&gpu, Numbers::default());
    run.run_to(1.0);
    let layout = run.water.layout();
    let (top, _) = scenes::THIN_POUR;
    // 1,400 px below the spout a free fall from 848.5 px/s is at 3,451 px/s.
    let (from, to) = (top + 1300.0, top + 1500.0);
    let falls: Vec<f32> = run
        .water
        .read()
        .iter()
        .filter(|d| (from..to).contains(&layout.to_world(Vec2::from(d.x)).y))
        .map(|d| d.v[1] * layout.cell)
        .collect();
    assert!(falls.len() > 10, "the pour reaches that far down: {}", falls.len());
    let mean = falls.iter().sum::<f32>() / falls.len() as f32;
    let free = (scenes::POUR_SPEED * scenes::POUR_SPEED + 2.0 * scenes::GRAVITY.y * 1400.0).sqrt();
    assert!((mean / free - 1.0).abs() < 0.04, "it falls freely: {mean} px/s against {free}");
}
