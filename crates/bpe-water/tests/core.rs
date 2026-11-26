//! The water core's own checks (Water mode parts 2a and 2a-2): pools rest and keep their height,
//! walls hold, narrow openings flow, a run repeats exactly, and the drops' elbow room leaves still
//! water alone and never bursts.

use std::sync::Arc;

use bpe_water::grid::{Feel, Numbers, Water, WaterGpu};
use bpe_water::probe::{self, Spacing, Stats};
use bpe_water::scenes::{self, Scene};
use bpe_water::{Drop, Layout, Pour, STOPS, Solid, SolidField, WATER};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

#[test]
fn still_water_rests() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::still_box(8.0, 0.3).start_grid(&gpu, Numbers::default());
    run.run_to(2.0);
    let s = Stats::of(&run.water.read(), &run.water.layout());
    assert_eq!(s.broken, 0);
    assert!(s.rms < 3.0, "still water is at rest: {} px/s", s.rms);
    assert!(s.fastest < 60.0, "and no drop of it is flung about: {} px/s", s.fastest);
}

#[test]
fn a_pool_keeps_its_height() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::still_box(8.0, 0.3).start_grid(&gpu, Numbers::default());
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
        let mut run = scenes::weird(8.0).start_grid(&gpu, Numbers::default());
        run.run_to(0.6);
        probe::print_of(&run.water.read())
    };
    assert_eq!(play(), play(), "the same scene played twice gives the same drops, to the bit");
}

#[test]
fn a_copy_plays_on_the_same() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::weird(8.0).start_grid(&gpu, Numbers::default());
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

fn outside(run: &scenes::GridRunning, half: Vec2) -> usize {
    let layout = run.water.layout();
    run.water.read().iter().filter(|d| layout.to_world(Vec2::from(d.x)).abs().cmpgt(half).any()).count()
}

#[test]
fn a_thin_wall_holds_a_fast_pour() {
    let Some(gpu) = card() else { return };
    // Straight at the side wall at 2,500 px/s: over a cell a liquid step.
    let mut run = pour_in_a_box(0.0, 2500.0).start_grid(&gpu, Numbers::default());
    run.run_to(1.0);
    assert!(run.water.count() > 1000, "the pour pours: {}", run.water.count());
    assert_eq!(outside(&run, Vec2::new(300.0, 450.0)), 0, "no drop is past the wall's line");
}

#[test]
fn a_full_box_stays_shut() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::boxed(8.0, 240.0, 3.0, 0.97).start_grid(&gpu, Numbers::default());
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
    let mut run = scene.start_grid(&gpu, Numbers::default());
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
    let mut run = scenes::thin_pour(8.0, 2.0).start_grid(&gpu, Numbers::default());
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

/// Elbow room is for water on the move: a pool at rest is left exactly as it lies. A settled pool
/// plays on the same to the bit with it and without it.
#[test]
fn still_water_is_left_alone() {
    let Some(gpu) = card() else { return };
    let without = Numbers { apart: 0.0, ..Numbers::default() };
    let mut run = scenes::still_box(8.0, 0.3).start_grid(&gpu, without);
    // (Its first moments are the fill's own drops finding their places, some at over 50 px/s.)
    run.run_to(0.5);
    let mut with = run.copy();
    with.water.set_numbers(Numbers::default());
    run.run_to(2.0);
    with.run_to(2.0);
    assert_eq!(probe::print_of(&with.water.read()), probe::print_of(&run.water.read()));
}

/// An empty box with room to fall in, and `drops` put in it.
fn in_the_air(gpu: &Arc<WaterGpu>, numbers: Numbers, drops: &[Drop]) -> Water {
    let layout = Layout::cover(Vec2::new(-200.0, -400.0), Vec2::new(200.0, 400.0), 8.0);
    let field = SolidField::build(layout, &[]);
    let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, numbers, &[Feel::default()]);
    water.add(drops);
    water
}

/// Many drops born on one point (a pour into a corner does it): they take their room without a
/// burst. (With no elbow room they stay on that one point for good: drops at one place gather the
/// same speed, so nothing the grid does can part them.)
#[test]
fn a_heap_on_one_spot_spreads_without_a_burst() {
    let Some(gpu) = card() else { return };
    let play = |numbers: Numbers| {
        let layout = Layout::cover(Vec2::new(-200.0, -400.0), Vec2::new(200.0, 400.0), 8.0);
        let at = layout.to_grid(Vec2::new(3.3, -300.7));
        let heap: Vec<Drop> = (0..400).map(|k| Drop::new(at, Vec2::new(0.0, 600.0) / 8.0, STOPS, k, WATER)).collect();
        let mut water = in_the_air(&gpu, numbers, &heap);
        for _ in 0..48 {
            water.step();
        }
        let drops = water.read();
        let s = Stats::of(&drops, &layout);
        let (mut lo, mut hi) = (Vec2::MAX, Vec2::MIN);
        for d in &drops {
            lo = lo.min(Vec2::from(d.x));
            hi = hi.max(Vec2::from(d.x));
        }
        (s, (hi - lo).max_element(), Spacing::of(&drops))
    };
    let (with, span, spacing) = play(Numbers::default());
    let (without, span_without, _) = play(Numbers { apart: 0.0, ..Numbers::default() });
    println!("a heap of 400: {span} cells across, fastest {} px/s, {spacing:?}", with.fastest);
    println!("with no elbow room: {span_without} cells across, fastest {} px/s", without.fastest);
    assert_eq!((with.broken, with.drops), (0, 400));
    // (400 drops are 100 cells of liquid: a patch 10 cells across at rest.)
    assert!(span > 5.0, "the heap has spread: {span} cells across");
    assert!(span < 30.0, "and has not been blown apart: {span} cells across");
    assert!(spacing.close < 0.2, "its drops have their room: {spacing:?}");
    // A free fall from 600 px/s for 48 steps is at 1,400 px/s.
    assert!(with.fastest < 1400.0 + 500.0, "none was thrown: the fastest is at {} px/s", with.fastest);
}

/// A drop at the grid's very edge, or off it, among a pour: nothing reads or writes outside the
/// spots, the drops that step stay numbers and none is lost.
#[test]
fn drops_at_the_grids_edge_are_safe() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::weird(8.0).start_grid(&gpu, Numbers::default());
    let layout = run.water.layout();
    let (cols, rows) = (layout.cols as f32, layout.rows as f32);
    let fast = Vec2::new(40.0, 40.0);
    let odd: Vec<Drop> =
        [(0.0, 0.0), (cols - 1.0, rows - 1.0), (-0.4, 3.0), (cols + 5.0, rows + 5.0), (3.0, rows - 0.999)]
            .iter()
            .enumerate()
            .map(|(k, &(x, y))| Drop::new(Vec2::new(x, y), fast, 0, 9000 + k as u32, WATER))
            .collect();
    run.water.add(&odd);
    for _ in 0..60 {
        run.step();
    }
    let drops = run.water.read();
    // The two on the grid's corners step with the rest (and, not stopping at the edge, may leave
    // the grid): they are numbers still.
    let on_grid = |d: &Drop| d.x[0] >= 0.0 && d.x[0] <= cols - 1.0 && d.x[1] >= 0.0 && d.x[1] <= rows - 1.0;
    for serial in [9000, 9001] {
        let d = drops.iter().find(|d| d.serial == serial).expect("the corner drop is kept");
        assert!(Vec2::from(d.x).is_finite() && Vec2::from(d.v).is_finite(), "{d:?}");
        assert!(Vec2::from(d.x) != Vec2::ZERO, "it has stepped: {d:?}");
    }
    // The ones off the grid are kept as they were (they do not step).
    for serial in [9002, 9003] {
        let d = drops.iter().find(|d| d.serial == serial).expect("the drop off the grid is kept");
        assert!(!on_grid(d) && Vec2::from(d.v) == fast, "{d:?}");
    }
    let pour: Vec<&Drop> = drops.iter().filter(|d| d.serial >= 1 << 24).collect();
    assert!(pour.len() > 50, "the pour pours: {}", pour.len());
    assert!(pour.iter().all(|d| Vec2::from(d.x).is_finite() && Vec2::from(d.v).is_finite()));
    assert!(pour.iter().all(|d| on_grid(d)));
}

#[test]
fn no_elbow_room_still_runs() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::weird(8.0).start_grid(&gpu, Numbers { apart: 0.0, ..Numbers::default() });
    run.run_to(0.3);
    let s = Stats::of(&run.water.read(), &run.water.layout());
    assert!(s.drops > 100, "{}", s.drops);
    assert_eq!(s.broken, 0);
}
