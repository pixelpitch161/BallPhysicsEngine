//! The all-drops engine's behaviour (`docs/superpowers/plans/water-mode/core-c1-engine.md`).

use std::sync::Arc;

use bpe_water::drops::LEAST_WALL;
use bpe_water::drops::numbers::FRAME;
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

/// The clock takes the floor's steps for still water and more for fast water.
#[test]
fn the_clock_takes_more_steps_for_faster_water() {
    let Some(gpu) = card() else { return };
    let mut still = scenes::still_box(8.0, 0.3).start(&gpu, Numbers::for_cell(8.0));
    still.run_to(1.0);
    assert_eq!(still.water.steps_last_frame(), 16, "still water takes the floor's steps");

    let layout = Layout::cover(Vec2::new(-400.0, -800.0), Vec2::new(400.0, 800.0), 8.0);
    let field = SolidField::build(layout, &[]);
    let mut water = Water::new(gpu.clone(), &field, Vec2::ZERO, Numbers::for_cell(8.0), &[Feel::default()]);
    let fired: Vec<Drop> = (0..50)
        .map(|k| {
            Drop::new(
                layout.to_grid(Vec2::new(-200.0 + 8.0 * k as f32, -700.0)),
                Vec2::new(0.0, 750.0),
                STOPS,
                0,
                WATER,
            )
        })
        .collect();
    water.add(&fired);
    let mut steps = Vec::new();
    for _ in 0..6 {
        water.frame();
        steps.push(water.steps_last_frame());
    }
    // (6000 px/s at 8 px cells: ceil(6000 / 60 / (1.3 x 4)) = 20 steps a frame.)
    assert!(steps.iter().all(|n| *n == 20), "fast water gets its steps every frame: {steps:?}");
}

/// Drops that leave the course are let go, and no drop still in it is lost with them.
#[test]
fn drops_that_leave_are_counted_out() {
    let Some(gpu) = card() else { return };
    let layout = Layout::cover(Vec2::new(-300.0, -300.0), Vec2::new(300.0, 300.0), 8.0);
    let field = SolidField::build(layout, &[]);
    let leaving: Vec<Drop> = (0..500)
        .map(|k| {
            let at = Vec2::new(-250.0 + (k % 50) as f32 * 10.0, 100.0 + (k / 50) as f32 * 10.0);
            Drop::new(layout.to_grid(at), Vec2::new(0.0, 2000.0) / 8.0, STOPS, 0, WATER)
        })
        .collect();
    let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(8.0), &[Feel::default()]);
    water.set_edges_shut(false);
    water.add(&leaving);
    for _ in 0..60 {
        water.frame();
    }
    assert_eq!(water.count(), 0, "every drop has left and is let go");

    // A pool in a box inside the course, and drops leaving past it all the while.
    let (layout, field) = {
        let layout = Layout::cover(Vec2::new(-300.0, -300.0), Vec2::new(300.0, 300.0), 8.0);
        let walls = [Solid::rect(Vec2::new(-100.0, -100.0), Vec2::new(100.0, 100.0), false, 3.0)];
        (layout, SolidField::build_least(layout, &walls, LEAST_WALL * 8.0))
    };
    let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(8.0), &[Feel::default()]);
    water.set_edges_shut(false);
    let pool = bpe_water::fill(&layout, &field, Vec2::new(-90.0, 0.0), Vec2::new(90.0, 90.0), STOPS, 0);
    let kept = water.add(&pool);
    assert!(kept > 200);
    for f in 0..60 {
        let side = if f % 2 == 0 { -200.0 } else { 200.0 };
        let wave: Vec<Drop> = (0..20)
            .map(|k| {
                Drop::new(
                    layout.to_grid(Vec2::new(side, -250.0 + 5.0 * k as f32)),
                    Vec2::new(0.0, 3000.0) / 8.0,
                    STOPS,
                    0,
                    WATER,
                )
            })
            .collect();
        water.add(&wave);
        water.frame();
    }
    for _ in 0..30 {
        water.frame();
    }
    let drops = water.read();
    let mut pool_serials: Vec<u32> = drops.iter().map(|d| d.serial).filter(|s| *s < kept).collect();
    pool_serials.sort();
    assert_eq!(pool_serials, (0..kept).collect::<Vec<_>>(), "every pool drop is still there");
    assert_eq!(water.count(), kept, "and only they are");
}

/// The drops a pour of `width` makes at rest spacing on `layout`, as water: `count × s²`.
fn poured_area(count: u32, cell: f32) -> f32 {
    count as f32 * (cell * 0.5) * (cell * 0.5)
}

/// A tap pours exactly its width times its speed.
#[test]
fn a_tap_pours_its_amount() {
    let Some(gpu) = card() else { return };
    for cell in [12.0, 8.0] {
        let scene = scenes::thin_pour(cell, 2.0);
        let pour = scene.pours[0];
        let mut run = scene.start(&gpu, Numbers::for_cell(cell));
        run.run_to(1.0);
        let want = pour.width * pour.speed * (1.0 - pour.from);
        let got = poured_area(run.water.count(), cell);
        assert!((got / want - 1.0).abs() < 0.02, "at {cell} px: {got} of {want} px²");
    }
}

/// A tap's drops set off straight: nothing thrown sideways at the spout.
#[test]
fn a_tap_is_born_even() {
    let Some(gpu) = card() else { return };
    let scene = scenes::thin_pour(8.0, 2.4);
    let pour = scene.pours[0];
    let mut run = scene.start(&gpu, Numbers::for_cell(8.0));
    run.run_to(0.5);
    let layout = run.water.layout();
    let near: Vec<Drop> = run
        .water
        .read()
        .into_iter()
        .filter(|d| layout.to_world(Vec2::from(d.x)).distance(pour.place) < 10.0 * 4.0)
        .collect();
    assert!(near.len() > 20, "{}", near.len());
    for d in &near {
        let sideways = d.v[0].abs() * layout.cell;
        assert!(sideways <= 0.05 * pour.speed, "a drop by the spout goes sideways at {sideways} px/s");
    }
}

/// A thin pour at the finest detail with the strongest skin, at the most steps a frame, stays a
/// stream: none of it is thrown back above the spout or out to the side. (The skin's bend is only
/// calm up to about 40 steps a frame: at the 75 this pour once took, drops flew 1400 px/s sideways
/// and 30 px up.)
#[test]
fn a_pour_stays_calm_at_the_most_steps_a_frame() {
    let Some(gpu) = card() else { return };
    let mut scene = scenes::thin_pour(2.0, 7.0);
    for feel in &mut scene.feels {
        feel.surface_pull = 1.0;
    }
    let pour = scene.pours[0];
    let numbers = Numbers { floor: 1024, ..Numbers::for_cell(2.0) };
    let mut run = scene.start(&gpu, numbers);
    for frame in 0..10 {
        run.frame();
        let layout = run.water.layout();
        for d in run.water.read() {
            let at = layout.to_world(Vec2::from(d.x));
            let sideways = d.v[0].abs() * layout.cell;
            assert!(
                at.y >= pour.place.y - layout.cell,
                "frame {frame}: a drop {} px above the spout",
                pour.place.y - at.y
            );
            assert!(sideways <= 0.75 * pour.speed, "frame {frame}: a drop goes sideways at {sideways} px/s");
        }
    }
    assert_eq!(run.water.steps_last_frame(), numbers.most_steps);
}

/// How wide the stream is `below` pixels under its spout: its outer drops' distance apart, and a
/// drop's own spacing.
fn stream_width(run: &scenes::Running, pour: &Pour, below: f32) -> f32 {
    let layout = run.water.layout();
    let s = layout.cell * 0.5;
    let xs: Vec<f32> = run
        .water
        .read()
        .iter()
        .map(|d| layout.to_world(Vec2::from(d.x)))
        .filter(|p| (p.y - (pour.place.y + below)).abs() <= s * 0.5)
        .map(|p| p.x)
        .collect();
    assert!(!xs.is_empty(), "the stream reaches {below} px down");
    let (lo, hi) = xs.iter().fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
    hi - lo + s
}

/// Half the Rate is a stream half as wide.
#[test]
fn a_turned_down_tap_is_narrower() {
    let Some(gpu) = card() else { return };
    let width = |rate: f32| {
        let mut scene = scenes::thin_pour(8.0, 4.0);
        scene.pours[0].width *= rate;
        let pour = scene.pours[0];
        let mut run = scene.start(&gpu, Numbers::for_cell(8.0));
        run.run_to(0.3);
        stream_width(&run, &pour, 5.0 * 4.0)
    };
    let (full, half) = (width(1.0), width(0.5));
    assert!((half - full * 0.5).abs() <= 4.0, "{half} px at half the rate, {full} px at the full rate");
}

/// A spout under the surface makes no drops where there is no room: it owes a few rows at most.
#[test]
fn a_submerged_spout_holds_back() {
    let Some(gpu) = card() else { return };
    let mut scene = scenes::still_box(12.0, 0.6);
    let pour = Pour { place: Vec2::new(0.0, 300.0), width: 24.0, speed: scenes::POUR_SPEED, ..Pour::default() };
    scene.pours = vec![pour];
    let numbers = Numbers::for_cell(12.0);
    let mut run = scene.start(&gpu, numbers);
    let before = run.water.count();
    run.run_to(1.0);
    let layout = run.water.layout();
    let columns = (pour.width / (layout.cell * 0.5)).ceil() as u32;
    let first_two = (pour.speed * 2.0 * FRAME / pour.pitch(&layout)).ceil() as u32 + 1;
    let most = (numbers.held_rows + first_two) * columns;
    let grew = run.water.count() - before;
    assert!(grew <= most, "the spout made {grew} drops under the surface (at most {most})");
}

/// A run with a pour, copied at frame 7 (the pour part way through its rows), plays on the same.
#[test]
fn a_copy_with_pours_plays_on_the_same() {
    let Some(gpu) = card() else { return };
    let scene = scenes::thin_pour(8.0, 2.4);
    let numbers = Numbers::for_cell(8.0);
    let mut a = scene.start(&gpu, numbers);
    a.run_to(0.5);
    let mut b = scene.start(&gpu, numbers);
    for _ in 0..7 {
        b.frame();
    }
    let mut c = b.copy();
    b.run_to(0.5);
    c.run_to(0.5);
    let first = a.water.read();
    assert!(first.len() > 100, "{}", first.len());
    assert_eq!(probe::print_of(&first), probe::print_of(&b.water.read()), "twice from the start");
    assert_eq!(probe::print_of(&first), probe::print_of(&c.water.read()), "from a copy made at frame 7");
}

/// A run whose steps a frame change (a burst of fast drops onto a slab) while a pour sends drops
/// out of the course all the while plays the same twice and from a copy made part way.
#[test]
fn a_run_whose_steps_change_and_whose_drops_leave_repeats() {
    let Some(gpu) = card() else { return };
    let cell = 12.0;
    let layout = Layout::cover(Vec2::new(-300.0, -1200.0), Vec2::new(300.0, 1200.0), cell);
    let slab = [Solid::rect(Vec2::new(-260.0, 900.0), Vec2::new(260.0, 1000.0), true, 0.0)];
    let field = SolidField::build_least(layout, &slab, LEAST_WALL * cell);
    let start = || {
        let mut w = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(cell), &[Feel::default()]);
        w.set_edges_shut(false);
        let fired: Vec<Drop> = (0..600)
            .map(|k| {
                let at = Vec2::new(-150.0 + (k % 50) as f32 * 6.0, -1100.0 + (k / 50) as f32 * 6.0);
                Drop::new(layout.to_grid(at), Vec2::new(0.0, 2500.0) / cell, STOPS, 0, WATER)
            })
            .collect();
        w.add(&fired);
        // (Out of the side: these drops leave the course.)
        w.set_pours(&[Pour { place: Vec2::new(200.0, 0.0), aim: 0.0, width: 24.0, speed: 600.0, ..Pour::default() }]);
        w
    };
    let mut a = start();
    let mut steps = Vec::new();
    let mut left = 0;
    for _ in 0..40 {
        a.frame();
        steps.push(a.steps_last_frame());
        left = left.max(a.count() as usize - a.read().len());
    }
    assert!(steps.iter().max() > steps.iter().min(), "the steps a frame change: {steps:?}");
    assert!(left > 20, "drops leave the course: at most {left} out at once");
    let mut b = start();
    for _ in 0..13 {
        b.frame();
    }
    let mut c = b.copy();
    for _ in 13..40 {
        b.frame();
        c.frame();
    }
    let first = probe::print_of(&a.read());
    assert_eq!(first, probe::print_of(&b.read()), "twice from the start");
    assert_eq!(first, probe::print_of(&c.read()), "from a copy made at frame 13");
}

/// A fill starts stacked on the floor at its resting spacing: its lowest row a quarter cell off the
/// floor, its rows and columns half a cell apart, and no drop nearer a solid than that.
/// (Starting without a bounce is part C2's, with the fill's pressure worked out before the first
/// frame: here the pool squeezes a few per cent under its weight and bounces for about 0.2 s.)
#[test]
fn a_fill_is_stacked_on_the_floor() {
    let Some(gpu) = card() else { return };
    let numbers = Numbers::for_cell(12.0);
    let run = scenes::still_box(12.0, 0.3).start(&gpu, numbers);
    let layout = run.water.layout();
    let drops = run.water.read();
    let off = |d: &Drop| run.field.at(layout.to_world(Vec2::from(d.x))) / layout.cell;
    assert!(drops.iter().all(|d| off(d) >= numbers.wall_off - 1e-3), "every drop is clear of the solids");
    let lowest = drops.iter().map(|d| d.x[1]).fold(f32::MIN, f32::max);
    let floor_row: Vec<&Drop> = drops.iter().filter(|d| (d.x[1] - lowest).abs() < 0.1).collect();
    assert!(floor_row.len() > 40, "{}", floor_row.len());
    for d in &floor_row {
        assert!((off(d) - numbers.wall_off).abs() < 1e-3, "the lowest row rests {} cell off the floor", off(d));
    }
    // (Each column stacked on its own face: down a column, the drops are half a cell apart.)
    let mut columns: std::collections::BTreeMap<i64, Vec<f32>> = Default::default();
    for d in &drops {
        columns.entry((d.x[0] * 2.0).floor() as i64).or_default().push(d.x[1]);
    }
    assert!(columns.len() > 40, "{}", columns.len());
    for ys in columns.values_mut() {
        ys.sort_by(f32::total_cmp);
        assert!(ys.windows(2).all(|w| (w[1] - w[0] - 0.5).abs() < 1e-3), "a column's drops half a cell apart");
    }
}

/// A step's end joined with the next step's fall (one pass fewer a step) plays the same water to
/// the bit, births and all.
#[test]
fn joining_a_steps_end_to_the_next_fall_changes_nothing() {
    let Some(gpu) = card() else { return };
    let scene = scenes::thin_pour(8.0, 2.4);
    let play = |join| {
        let mut run = scene.start(&gpu, Numbers { join, ..Numbers::for_cell(8.0) });
        run.run_to(0.5);
        run.water.read()
    };
    let (joined, apart) = (play(1), play(0));
    assert!(joined.len() > 200, "{}", joined.len());
    assert_eq!(probe::print_of(&joined), probe::print_of(&apart));
}

/// The numbers for `cell` with the deep part on (`deep` 1) or never (0).
fn deep_numbers(cell: f32, deep: u32) -> Numbers {
    Numbers { deep, ..Numbers::for_cell(cell) }
}

/// The share of its height a still box's pool keeps from the start to `seconds` (from the floor's
/// face to the liquid's top), and its drops then.
fn height_kept(scene: &Scene, gpu: &Arc<WaterGpu>, numbers: Numbers, seconds: f32) -> (f32, Vec<Drop>) {
    let floor = 450.0 - 0.5 * scene.layout.cell.max(3.0);
    let mut run = scene.start(gpu, numbers);
    let layout = run.water.layout();
    let top0 = Stats::of(&run.water.read(), &layout).top;
    run.run_to(seconds);
    let drops = run.water.read();
    let top = Stats::of(&drops, &layout).top;
    ((floor - top) / (floor - top0), drops)
}

/// A deep pool keeps its height with the deep part, and does not without it (the part does the
/// work).
#[test]
fn a_deep_pool_keeps_its_height() {
    let Some(gpu) = card() else { return };
    let scene = scenes::still_box(12.0, 0.6);
    let (kept, drops) = height_kept(&scene, &gpu, deep_numbers(12.0, 1), 2.0);
    let layout = scene.layout;
    assert!(kept >= 0.985, "the pool keeps its height with the deep part: {kept}");
    assert_eq!(Stats::of(&drops, &layout).broken, 0);
    for d in &drops {
        let p = layout.to_world(Vec2::from(d.x));
        assert!(p.x.abs() < 300.0 && p.y.abs() < 450.0, "a drop left the box: {p}");
    }
    let (without, _) = height_kept(&scene, &gpu, deep_numbers(12.0, 0), 2.0);
    assert!(without < 0.99, "and sinks without it: {without}");
}

/// Water with nothing deep in it plays the same to the bit with the deep part on or never.
#[test]
fn the_deep_part_leaves_thin_water_alone() {
    let Some(gpu) = card() else { return };
    for scene in [scenes::weird(12.0), scenes::thin_pour(12.0, 2.0)] {
        let mut on = scene.start(&gpu, deep_numbers(12.0, 1));
        let mut off = scene.start(&gpu, deep_numbers(12.0, 0));
        on.run_to(0.5);
        off.run_to(0.5);
        let drops = on.water.read();
        assert!(drops.len() > 100, "{}: {}", scene.name, drops.len());
        assert_eq!(probe::print_of(&drops), probe::print_of(&off.water.read()), "{}", scene.name);
    }
    // (Drops added run the deep part on the two frames after, before any report: with nothing
    // deep those frames still leave every drop alone.)
    let scene = scenes::thin_pour(12.0, 2.0);
    let mut on = scene.start(&gpu, deep_numbers(12.0, 1));
    let mut off = scene.start(&gpu, deep_numbers(12.0, 0));
    let layout = on.water.layout();
    let blob: Vec<Drop> = (0..9)
        .map(|k| {
            let at = Vec2::new(150.0 + 6.0 * (k % 3) as f32, -100.0 + 6.0 * (k / 3) as f32);
            Drop::new(layout.to_grid(at), Vec2::ZERO, STOPS, 10_000 + k, WATER)
        })
        .collect();
    for run in [&mut on, &mut off] {
        run.run_to(5.0 * FRAME);
        run.water.add(&blob);
        run.run_to(0.5);
    }
    assert!(on.water.deep_frames() >= 2, "{}", on.water.deep_frames());
    assert_eq!(probe::print_of(&on.water.read()), probe::print_of(&off.water.read()), "with drops added");
}

/// A copy made while the deep part runs (a pour landing in a deep pool) plays on the same.
#[test]
fn a_copy_of_a_deep_pool_plays_on_the_same() {
    let Some(gpu) = card() else { return };
    let mut scene = scenes::still_box(12.0, 0.6);
    scene.pours = vec![Pour { place: Vec2::new(-100.0, -400.0), speed: scenes::POUR_SPEED, ..Pour::default() }];
    let numbers = deep_numbers(12.0, 1);
    let mut a = scene.start(&gpu, numbers);
    a.run_to(1.0);
    let mut b = scene.start(&gpu, numbers);
    for _ in 0..31 {
        b.frame();
    }
    assert!(b.water.deep_frames() > 0, "the deep part is running");
    let mut c = b.copy();
    b.run_to(1.0);
    c.run_to(1.0);
    let first = a.water.read();
    assert_eq!(probe::print_of(&first), probe::print_of(&b.water.read()), "twice from the start");
    assert_eq!(probe::print_of(&first), probe::print_of(&c.water.read()), "from a copy made at frame 31");
}

/// A pool on the course's open bottom edge: beyond the course is not full, so nothing holds the
/// pool up there; it falls out of the course, its top never rising.
#[test]
fn an_open_edge_is_not_deep() {
    let Some(gpu) = card() else { return };
    let layout = Layout::cover(Vec2::new(-200.0, -200.0), Vec2::new(200.0, 200.0), 12.0);
    let field = SolidField::build(layout, &[]);
    let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, deep_numbers(12.0, 1), &[Feel::default()]);
    water.set_edges_shut(false);
    // (Drops stop two cells in from the grid's edge: the pool reaches down to there.)
    let lo = layout.to_world(Vec2::new(2.2, 0.0));
    let hi = layout.to_world(Vec2::new(layout.cols as f32 - 3.2, layout.rows as f32 - 3.2));
    let pool = bpe_water::fill_stacked(&layout, &field, Vec2::new(lo.x, 0.0), hi, STOPS, 0.25);
    assert!(water.add(&pool) > 500);
    let mut highest = Stats::of(&water.read(), &layout).highest;
    for _ in 0..60 {
        water.frame();
        let drops = water.read();
        if drops.is_empty() {
            continue;
        }
        let now = Stats::of(&drops, &layout).highest;
        assert!(now >= highest - 0.05 * layout.cell, "the pool's top rose: {now} from {highest}");
        highest = now;
    }
    assert_eq!(water.count(), 0, "the pool has left the course");
    // (It was deep until it drained, and the deep part stopped with it.)
    let ran = water.deep_frames();
    assert!(ran > 2 && ran < 40, "the deep part ran {ran} frames");
    for _ in 0..10 {
        water.frame();
    }
    assert_eq!(water.deep_frames(), ran, "and runs no more");
}

/// A narrow deep column between walls (its pressure's ladder short) is held up from below as well
/// as a wide one: the walls count as full, so the water beside them is deep.
#[test]
fn a_small_course_still_solves() {
    let Some(gpu) = card() else { return };
    let half = Vec2::new(36.0, 75.0);
    let layout = Layout::cover(-half - Vec2::splat(12.0), half + Vec2::splat(12.0), 12.0);
    let field = SolidField::build_least(layout, &[Solid::rect(-half, half, false, 3.0)], LEAST_WALL * 12.0);
    let (inner, floor) = (half.x - 6.0, half.y - 6.0);
    let kept = |deep: u32| {
        let numbers = deep_numbers(12.0, deep);
        let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, numbers, &[Feel::default()]);
        let (lo, hi) = (Vec2::new(-inner, floor - 130.0), Vec2::new(inner, floor));
        assert!(water.add(&bpe_water::fill_stacked(&layout, &field, lo, hi, STOPS, numbers.wall_off)) > 150);
        let top0 = Stats::of(&water.read(), &layout).top;
        for _ in 0..60 {
            water.frame();
        }
        assert_eq!(water.deep_frames(), if deep == 1 { 60 } else { 0 }, "the column is deep every frame");
        (floor - Stats::of(&water.read(), &layout).top) / (floor - top0)
    };
    let (with, without) = (kept(1), kept(0));
    assert!(with >= without - 0.005, "the water keeps its height: {with} (without the deep part {without})");
}

/// A fill starts at rest: snug on its floor and against its walls, its weight held from the first
/// step (the deep part primes its pressure before anything moves).
#[test]
fn a_fill_starts_at_rest() {
    let Some(gpu) = card() else { return };
    let numbers = Numbers::for_cell(12.0);
    let mut run = scenes::still_box(12.0, 0.3).start(&gpu, numbers);
    let layout = run.water.layout();
    let drops = run.water.read();
    let off = |d: &Drop| run.field.at(layout.to_world(Vec2::from(d.x))) / layout.cell;
    let lowest = drops.iter().map(|d| d.x[1]).fold(f32::MIN, f32::max);
    for d in drops.iter().filter(|d| (d.x[1] - lowest).abs() < 0.1) {
        assert!((off(d) - numbers.wall_off).abs() < 1e-3, "the lowest row rests {} cell off the floor", off(d));
    }
    let mut fastest_rms: f32 = 0.0;
    while run.water.frames() < 15 {
        run.frame();
        fastest_rms = fastest_rms.max(Stats::of(&run.water.read(), &layout).rms);
    }
    assert!(fastest_rms < 20.0, "the pool stays still: {fastest_rms} px/s");
}

/// A box whose walls fall a third of a cell off the lattice: no drop by a side wall is thrown up it.
#[test]
fn no_drop_climbs_a_side_wall() {
    let Some(gpu) = card() else { return };
    let cell = 6.0;
    let mut scene = scenes::still_box(cell, 0.6);
    scene.layout.origin.x += cell / 3.0;
    let mut run = scene.start(&gpu, Numbers::for_cell(cell));
    let layout = run.water.layout();
    let mut fastest_up: f32 = 0.0;
    while run.water.frames() < 30 {
        run.frame();
        for d in run.water.read() {
            let p = layout.to_world(Vec2::from(d.x));
            if p.x.abs() > 291.0 - 2.0 * cell {
                fastest_up = fastest_up.max(-d.v[1] * cell);
            }
        }
    }
    // (C1's fill left a gap of up to two thirds of a cell at a wall, and its first drop went up the
    // wall at 605 px/s. Snug, and held from below, the pool's top layer settles by itself in its
    // first frames: a drop by the wall may hop a few pixels once.)
    assert!(fastest_up < 200.0, "a drop by a side wall went up at {fastest_up} px/s");
}

/// The rms speed of the drops between world x `from` and `to` more than `deep` pixels below their
/// top (the y above which 2 % of them are).
fn deep_rms(drops: &[Drop], layout: &Layout, (from, to): (f32, f32), deep: f32) -> f32 {
    let here: Vec<Drop> =
        drops.iter().filter(|d| (from..to).contains(&layout.to_world(Vec2::from(d.x)).x)).copied().collect();
    let top = Stats::of(&here, layout).top;
    let (mut sum, mut n) = (0.0f64, 0usize);
    for d in &here {
        if layout.to_world(Vec2::from(d.x)).y > top + deep {
            let v = Vec2::from(d.v) * layout.cell;
            sum += f64::from(v.length_squared());
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { (sum / n as f64).sqrt() as f32 }
}

/// A pool filled by a pour turns deep part way, and the push takes hold without a jolt: in the
/// frames after it starts, the drops deep in the half of the box away from where the pour lands move
/// as they do without the deep part, give or take 30 px/s (the two runs are the same to the bit
/// before it). (How well a pool holds its height is `a_deep_pool_keeps_its_height`'s: a poured pool
/// this small still sloshes at a few hundred px/s seconds after its pour stops.)
#[test]
fn a_pool_filled_by_a_pour_gets_its_push_without_a_jolt() {
    let Some(gpu) = card() else { return };
    let cell = 12.0;
    let numbers = deep_numbers(cell, 1);
    let below = numbers.deep_d + numbers.deep_b;
    let mut with = scenes::fill_pool(cell).start(&gpu, numbers);
    let mut without = scenes::fill_pool(cell).start(&gpu, deep_numbers(cell, 0));
    let layout = with.water.layout();
    let side = (30.0, 140.0);
    while with.water.deep_frames() == 0 {
        assert!(with.water.frames() < 120, "the pool turns deep");
        with.frame();
        without.frame();
    }
    let first = with.water.frames();
    println!("the deep part starts at frame {first}");
    assert!(first > 10, "not at once: frame {first}");
    for _ in 0..10 {
        let (a, b) =
            (deep_rms(&with.water.read(), &layout, side, below), deep_rms(&without.water.read(), &layout, side, below));
        assert!(
            (a - b).abs() < 30.0,
            "a jolt at frame {}: {a} px/s with the deep part, {b} without",
            with.water.frames()
        );
        with.frame();
        without.frame();
    }
}

/// Thin water never runs the deep part; a deep pool does every frame.
#[test]
fn deep_frames_follow_the_water() {
    let Some(gpu) = card() else { return };
    let mut thin = scenes::thin_pour(12.0, 2.0).start(&gpu, Numbers::for_cell(12.0));
    thin.run_to(0.5);
    assert_eq!(thin.water.deep_frames(), 0, "a thin pour runs it");
    let mut deep = scenes::still_box(12.0, 0.6).start(&gpu, Numbers::for_cell(12.0));
    deep.run_to(0.5);
    assert_eq!(deep.water.deep_frames(), deep.water.frames(), "a deep pool runs it every frame");
}

/// A hard landing at Fine lands as it does at Extreme: the weird shot's stream, at a weak skin, off
/// its block and over its disc, with no more of it in small pieces than at 3 px (C1's water had 38 %
/// of it in pieces under two draft cells across at 6 px against 30 % at 3 px; drops that meet now
/// keep together, 23 % and 27 %).
#[test]
fn a_hard_landing_stays_whole() {
    let Some(gpu) = card() else { return };
    let mut scene = scenes::weird(6.0);
    for feel in &mut scene.feels {
        feel.surface_pull = 0.2;
    }
    let mut run = scene.start(&gpu, Numbers::for_cell(6.0));
    let layout = run.water.layout();
    let above_pool = (Vec2::new(-420.0, -840.0), Vec2::new(420.0, 600.0));
    let mut shares = Vec::new();
    for t in [1.0, 1.25, 1.5] {
        run.run_to(t);
        shares.push(probe::pieces(&run.water.read(), &layout, 1.6, 16.0, above_pool).small_share);
    }
    let mean = shares.iter().sum::<f32>() / shares.len() as f32;
    assert!(mean < 0.30, "{:.1} % of the water in small pieces ({shares:?})", 100.0 * mean);
}

/// The mean speed of `drops` across, in pixels a second (every drop has the same mass).
fn mean_across(drops: &[Drop], cell: f32) -> f32 {
    drops.iter().map(|d| d.v[0]).sum::<f32>() / drops.len().max(1) as f32 * cell
}

/// Drops that meet lose their closing speed alike, so water hit by a drop, or two bodies that
/// meet, keep their momentum: with no gravity and no solids near, the mean speed stays (each of a
/// pair once took a share by its own neighbours' weight, and a surface drop, with fewer, took
/// more: a drop fired into a resting blob at 300 px/s drove it back the other way at 23 px/s).
#[test]
fn water_that_meets_keeps_its_momentum() {
    let Some(gpu) = card() else { return };
    let cell = 6.0;
    let layout = Layout::cover(Vec2::new(-400.0, -400.0), Vec2::new(400.0, 400.0), cell);
    let field = SolidField::build(layout, &[]);
    let blob = |lo: Vec2, hi: Vec2, speed: f32, first: u32| {
        let mut drops = bpe_water::fill(&layout, &field, lo, hi, STOPS, first);
        for d in &mut drops {
            d.v = [speed / cell, 0.0];
        }
        drops
    };
    // (A drop at 300 px/s into a resting blob.)
    let mut drops = blob(Vec2::new(-15.0, -15.0), Vec2::new(15.0, 15.0), 0.0, 0);
    let n = drops.len() as u32;
    assert!(n > 40, "{n}");
    let middle = drops.iter().map(|d| d.x[1]).sum::<f32>() / n as f32;
    let from = Vec2::new(layout.to_grid(Vec2::new(-70.0, 0.0)).x, middle);
    drops.push(Drop::new(from, Vec2::new(300.0 / cell, 0.0), STOPS, n, WATER));
    let drop_in = 300.0 / (n + 1) as f32;
    // (A big blob at 50 px/s meeting a small one coming the other way: no momentum in all.)
    let mut two = blob(Vec2::new(-115.0, 185.0), Vec2::new(-85.0, 215.0), 50.0, 100);
    let small = blob(Vec2::new(-8.0, 192.0), Vec2::new(8.0, 208.0), 0.0, 200);
    let back = -50.0 * two.len() as f32 / small.len() as f32;
    two.extend(small.into_iter().map(|mut d| {
        d.v = [back / cell, 0.0];
        d
    }));
    for (name, start, want) in [("a drop into a blob", drops, drop_in), ("two blobs", two, 0.0)] {
        let mut water = Water::new(gpu.clone(), &field, Vec2::ZERO, Numbers::for_cell(cell), &[Feel::default()]);
        water.add(&start);
        assert!((mean_across(&water.read(), cell) - want).abs() < 1e-3);
        for frame in 1..=60 {
            water.frame();
            if frame % 10 == 0 {
                let mean = mean_across(&water.read(), cell);
                assert!((mean - want).abs() < 0.25, "{name}: {mean:.2} px/s at frame {frame}, was {want:.2}");
            }
        }
    }
}
