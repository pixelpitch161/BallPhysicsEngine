//! The all-drops engine's behaviour (`docs/superpowers/plans/water-mode/core-c1-engine.md`).

use std::sync::Arc;

use bpe_water::drops::LEAST_WALL;
use bpe_water::probe::{self, Stats};
use bpe_water::scenes::{self, Scene};
use bpe_water::{Drop, Feel, Layout, Numbers, Pour, STOPS, Solid, SolidField, WATER, Water, WaterGpu};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

/// A pool stays in its box, keeps its height and comes to rest.
#[test]
fn a_pool_stays_in_its_box_and_keeps_its_height() {
    let Some(gpu) = card() else { return };
    let scene = scenes::still_box(12.0, 0.3);
    let mut run = scene.start(&gpu, Numbers::for_cell(12.0));
    let layout = run.water.layout();
    let before = Stats::of(&run.water.read(), &layout);
    assert!(before.drops > 2000, "{before:?}");
    run.run_to(1.0);
    let drops = run.water.read();
    let after = Stats::of(&drops, &layout);
    assert_eq!(after.broken, 0);
    for d in &drops {
        let p = layout.to_world(Vec2::from(d.x));
        assert!(p.x.abs() < 300.0 && p.y.abs() < 450.0, "a drop left the box: {p}");
    }
    let floor = 450.0 - 6.0;
    let kept = (floor - after.top) / (floor - before.top);
    assert!((0.93..1.05).contains(&kept), "the pool keeps its height: {kept}");
    assert!(after.rms < 60.0, "and comes to rest: {} px/s", after.rms);
}

/// A run repeats, and a copy made half way plays on the same.
#[test]
fn two_runs_and_a_copy_are_the_same() {
    let Some(gpu) = card() else { return };
    let scene = scenes::weird(12.0);
    let numbers = Numbers::for_cell(12.0);
    let mut a = scene.start(&gpu, numbers);
    a.run_to(0.5);
    let mut b = scene.start(&gpu, numbers);
    b.run_to(0.25);
    let mut c = b.copy();
    b.run_to(0.5);
    c.run_to(0.5);
    let first = a.water.read();
    assert!(first.len() > 100, "{}", first.len());
    assert_eq!(probe::print_of(&first), probe::print_of(&b.water.read()), "twice from the start");
    assert_eq!(probe::print_of(&first), probe::print_of(&c.water.read()), "from a copy made half way");
}

/// A 600 x 900 box with nothing in it, at `cell`-pixel cells.
fn empty_box(cell: f32) -> (Layout, SolidField) {
    let half = Vec2::new(300.0, 450.0);
    let layout = Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), cell);
    let field = SolidField::build_least(layout, &[Solid::rect(-half, half, false, 3.0)], LEAST_WALL * cell);
    (layout, field)
}

/// Many drops born on one point take their room and repeat: with no cap on how many drops a bin
/// sorts by serial (the trial's 192 broke this).
#[test]
fn a_heap_on_one_point_spreads_and_repeats() {
    let Some(gpu) = card() else { return };
    let play = || {
        let (layout, field) = empty_box(12.0);
        let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(12.0), &[Feel::default()]);
        let at = layout.to_grid(Vec2::new(3.3, -100.7));
        let heap: Vec<Drop> = (0..2000).map(|k| Drop::new(at, Vec2::ZERO, STOPS, k, WATER)).collect();
        water.add(&heap);
        for _ in 0..30 {
            water.frame();
        }
        (layout, water.read())
    };
    let (layout, drops) = play();
    assert_eq!(drops.len(), 2000);
    for d in &drops {
        let p = layout.to_world(Vec2::from(d.x));
        assert!(p.is_finite() && p.x.abs() < 300.0 && p.y.abs() < 450.0, "a drop left the box: {p}");
    }
    // (A heap that bursts throws a few pairs of drops off together, under-full, so the pressure does
    // not part them: a droplet of two. Every other drop has its room.)
    let mut places: Vec<Vec2> = drops.iter().map(|d| Vec2::from(d.x)).collect();
    places.sort_by(|a, b| a.x.total_cmp(&b.x));
    let mut close = 0;
    for (i, p) in places.iter().enumerate() {
        for q in &places[i + 1..] {
            if q.x - p.x >= 0.05 {
                break;
            }
            if p.distance(*q) < 0.05 {
                close += 1;
            }
        }
    }
    assert!(close * 100 <= drops.len(), "the heap is parted: {close} pairs of drops within 0.05 cell");
    assert_eq!(probe::print_of(&drops), probe::print_of(&play().1), "twice the same");
}

/// One drop with nothing near it falls as a stone does.
#[test]
fn a_lone_drop_falls_freely() {
    let Some(gpu) = card() else { return };
    let layout = Layout::cover(Vec2::new(-200.0, -400.0), Vec2::new(200.0, 400.0), 8.0);
    let field = SolidField::build(layout, &[]);
    let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(8.0), &[Feel::default()]);
    let from = Vec2::new(0.0, -300.0);
    water.add(&[Drop::new(layout.to_grid(from), Vec2::ZERO, STOPS, 0, WATER)]);
    for _ in 0..15 {
        water.frame();
    }
    let d = water.read()[0];
    let fell = layout.to_world(Vec2::from(d.x)).y - from.y;
    let t = 0.25;
    let free = 0.5 * scenes::GRAVITY.y * t * t;
    assert!((fell - free).abs() < 0.01 * free, "fell {fell} px, a stone {free}");
    assert!((d.v[1] * layout.cell - scenes::GRAVITY.y * t).abs() < 0.01 * scenes::GRAVITY.y * t, "{:?}", d.v);
    assert!(d.v[0].abs() < 1e-4);
}

/// Drops thrown at 2,000 px/s straight at a 3-pixel wall never cross it.
#[test]
fn a_thin_wall_holds_a_fast_pour() {
    let Some(gpu) = card() else { return };
    let mut scene = scenes::boxed(8.0, 600.0, 3.0, 0.0);
    scene.fills.clear();
    scene.pours = vec![Pour { place: Vec2::new(0.0, -200.0), aim: 0.0, width: 24.0, speed: 2000.0, ..Pour::default() }];
    let mut run = scene.start(&gpu, Numbers::for_cell(8.0));
    run.run_to(1.0);
    assert!(run.water.count() > 1000, "the pour pours: {}", run.water.count());
    let layout = run.water.layout();
    let half = Vec2::new(300.0, 450.0);
    let out = run.water.read().iter().filter(|d| layout.to_world(Vec2::from(d.x)).abs().cmpgt(half).any()).count();
    assert_eq!(out, 0, "no drop is past the wall's line");
}

/// A body with no drops plays on.
#[test]
fn an_empty_body_moves_on() {
    let Some(gpu) = card() else { return };
    let scene = Scene { fills: Vec::new(), ..scenes::still_box(12.0, 0.3) };
    let mut run = scene.start(&gpu, Numbers::for_cell(12.0));
    for _ in 0..10 {
        run.frame();
    }
    assert_eq!(run.water.count(), 0);
    assert_eq!(run.water.frames(), 10);
    assert!(run.water.read().is_empty());
}
