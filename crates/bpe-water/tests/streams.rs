//! Thin streams and films (Water mode parts 2a and 2a-2): a pour stays one narrow stream down a
//! whole screen, a sheet off an edge stays whole, a drop alone is left alone, drops on the move
//! keep their elbow room and a film leaves a block evenly.

use std::sync::Arc;

use bpe_water::grid::{Feel, Numbers, Water, WaterGpu};
use bpe_water::probe::{self, Spacing, Stream};
use bpe_water::scenes;
use bpe_water::{Drop, Layout, SolidField, WATER};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

/// A two-cell pour falling 1,700 px, measured in six bands of its fall.
fn pour_at(cell: f32) -> Option<Stream> {
    let gpu = card()?;
    let mut run = scenes::thin_pour(cell, 2.0).start_grid(&gpu, Numbers::default());
    run.run_to(1.5);
    let layout = run.water.layout();
    let (top, floor) = scenes::THIN_POUR;
    let (lo, hi) = (Vec2::new(-250.0, top + 40.0), Vec2::new(250.0, floor - 300.0));
    let air = probe::inside(&run.water.read(), &layout, lo, hi);
    let s = Stream::of(&air, &layout, (lo.y, hi.y), 1.5, 6);
    println!("a thin pour at {cell} px cells: {s:?}");
    Some(s)
}

#[test]
fn a_thin_pour_stays_one_stream() {
    let Some(s) = pour_at(8.0) else { return };
    assert!(s.drops > 300, "the pour is in the air: {}", s.drops);
    assert!(s.widths.iter().all(|w| *w > 0.0), "it reaches every band of the fall unbroken: {:?}", s.widths);
    assert!(s.strays < 0.08, "it sheds no rain of droplets: {} of its drops are strays", s.strays);
    assert!(s.gap < 200.0, "its biggest piece has no long gap: {} px", s.gap);
}

#[test]
fn a_stream_keeps_its_width() {
    let Some(s) = pour_at(8.0) else { return };
    // Born 16 px wide; falling faster it can only narrow.
    assert!(s.widths.iter().all(|w| *w < 40.0), "it never spreads to more than 40 px: {:?}", s.widths);
}

#[test]
fn a_thin_pour_is_clean_at_draft_detail() {
    let Some(s) = pour_at(12.0) else { return };
    assert!(s.widths.iter().all(|w| *w > 0.0 && *w < 60.0), "one stream at 12 px cells too: {:?}", s.widths);
    assert!(s.strays < 0.1, "{}", s.strays);
}

#[test]
fn a_sheet_stays_a_sheet() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::sheet(8.0).start_grid(&gpu, Numbers::default());
    run.run_to(2.0);
    let layout = run.water.layout();
    // The air past the shelf's edge, down to a little above the floor and short of the wall.
    let (edge, shelf, floor) = scenes::SHEET;
    let (lo, hi) = (Vec2::new(edge + 4.0, shelf - 40.0), Vec2::new(380.0, floor - 200.0));
    let air = probe::inside(&run.water.read(), &layout, lo, hi);
    let s = Stream::of(&air, &layout, (lo.y, hi.y), 1.5, 6);
    println!("the sheet: {s:?}");
    assert!(s.drops > 100, "liquid runs off the shelf: {}", s.drops);
    assert!(s.whole > 0.6, "most of what is in the air is one piece: {}", s.whole);
    assert!(s.strays < 0.1, "with no rain of droplets: {}", s.strays);
}

/// Water running up a tilted block leaves its top corner as one arc, not a fan: the solid neither
/// steers nor slows the drops nearest it as they leave (the owner's Water Weird layout).
#[test]
fn water_leaves_a_corner_as_one_arc() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::weird(6.0).start_grid(&gpu, Numbers::default());
    run.run_to(3.0);
    let layout = run.water.layout();
    // The air between the block's top right corner (210, -330) and the box's right wall (420).
    let (lo, hi) = (Vec2::new(230.0, -420.0), Vec2::new(400.0, -200.0));
    let air = probe::inside(&run.water.read(), &layout, lo, hi);
    let s = Stream::of(&air, &layout, (lo.y, hi.y), 1.5, 1);
    println!("the corner's arc: {} drops, whole {}", s.drops, s.whole);
    assert!(s.drops > 30, "liquid flies off the corner: {}", s.drops);
    assert!(s.whole > 0.8, "as one piece: {}", s.whole);
    // How thick the arc is: the drops' spread in height in each 20 px of its way across.
    let mut spans = [(f32::MAX, f32::MIN); 8];
    for d in &air {
        let p = layout.to_world(Vec2::from(d.x));
        let span = &mut spans[(((p.x - lo.x) / 20.0) as usize).min(7)];
        *span = (span.0.min(p.y), span.1.max(p.y));
    }
    let thickest = spans.iter().map(|(a, b)| (b - a).max(0.0)).fold(0.0, f32::max);
    println!("the corner's arc: {thickest} px at its thickest");
    assert!(thickest < 30.0, "a thin arc all the way across, not a fan: {thickest} px at its thickest");
}

/// A drop by itself flies as if nothing else were there: the pull and the spacing act on nothing.
/// Checked at every step of its flight (the spacing's push on a drop's own share would come and
/// go with where it is in its cell), unstretched and stretched.
#[test]
fn a_lone_drop_pulls_on_nothing() {
    let Some(gpu) = card() else { return };
    let layout = Layout::cover(Vec2::splat(-200.0), Vec2::splat(200.0), 8.0);
    let field = SolidField::build(layout, &[]);
    let gravity = Vec2::new(0.0, 4000.0);
    // (Stretched by a half: the most a pull could act on. That one is held to the fixed-point
    // sums' own rounding of the pull, the unstretched one to next to nothing.)
    for (stretch, most) in [(0.0, 0.02), (0.5, 0.1)] {
        let feels = [Feel { surface_pull: 1.0, ..Feel::default() }];
        let mut water = Water::new(gpu.clone(), &field, gravity, Numbers::default(), &feels);
        let (x0, v0) = (Vec2::new(-150.3, -120.7), Vec2::new(420.0, -300.0));
        let mut lone = Drop::new(layout.to_grid(x0), v0 / layout.cell, 0, 0, WATER);
        lone.e = stretch;
        water.add(&[lone]);
        let dt = water.numbers().dt();
        // What the step does to one drop alone: speed first, then place.
        let (mut x, mut v) = (x0, v0);
        let (mut off, mut off_speed) = (0.0f32, 0.0f32);
        for _ in 0..48 {
            water.step();
            v += gravity * dt;
            x += v * dt;
            let d = water.read()[0];
            off = off.max((layout.to_world(Vec2::from(d.x)) - x).length());
            off_speed = off_speed.max((Vec2::from(d.v) * layout.cell - v).length());
        }
        println!("a lone drop stretched {stretch}: at most {off} px and {off_speed} px/s off a free fall");
        assert!(off < most, "stretched {stretch}, it is where a free fall puts it all the way: {off} px off");
        assert!(off_speed < 4.0 * most, "at a free fall's speed: {off_speed} px/s off");
    }
}

/// Drops on the move keep their elbow room: in the films and the thread of the Water Weird layout
/// few fast drops have another right beside them. (Without it they sit where chance put them:
/// most have another within a quarter of a cell.)
#[test]
fn drops_on_the_move_keep_apart() {
    let Some(gpu) = card() else { return };
    let fast = |numbers: Numbers| {
        let mut run = scenes::weird(8.0).start_grid(&gpu, numbers);
        run.run_to(1.2);
        let drops: Vec<Drop> =
            run.water.read().into_iter().filter(|d| Vec2::from(d.v).length() * 8.0 > 300.0).collect();
        (drops.len(), Spacing::of(&drops))
    };
    let (n, with) = fast(Numbers::default());
    let (_, without) = fast(Numbers { apart: 0.0, ..Numbers::default() });
    println!("{n} fast drops: {with:?}; with no elbow room {without:?}");
    assert!(n > 500, "the liquid is on the move: {n}");
    assert!(with.close < 0.15, "few have another within a quarter of a cell: {}", with.close);
    assert!(with.median > 0.3, "the usual gap is over 0.3 cell: {}", with.median);
    assert!(without.close > 2.0 * with.close, "and it is the elbow room that does it: {without:?}");
}

/// Two drops born on the very same point part, and neither is kicked: each still falls freely.
#[test]
fn two_drops_on_one_spot_part_without_a_kick() {
    let Some(gpu) = card() else { return };
    let layout = Layout::cover(Vec2::new(-200.0, -400.0), Vec2::new(200.0, 400.0), 8.0);
    let field = SolidField::build(layout, &[]);
    let gravity = Vec2::new(0.0, 4000.0);
    let mut water = Water::new(gpu.clone(), &field, gravity, Numbers::default(), &[Feel::default()]);
    let v0 = Vec2::new(0.0, 600.0);
    let at = layout.to_grid(Vec2::new(-3.3, -350.7));
    water.add(&[Drop::new(at, v0 / 8.0, 0, 0, WATER), Drop::new(at, v0 / 8.0, 0, 1, WATER)]);
    let steps = 36;
    for _ in 0..steps {
        water.step();
    }
    let drops = water.read();
    let gap = (Vec2::from(drops[0].x) - Vec2::from(drops[1].x)).length();
    let free = v0 + gravity * steps as f32 * water.numbers().dt();
    let off = drops.iter().map(|d| (Vec2::from(d.v) * 8.0 - free).length()).fold(0.0, f32::max);
    println!("two drops born on one point: {gap} cells apart, at most {off} px/s off a free fall of {} px/s", free.y);
    assert!(gap > 0.3, "they have parted: {gap} cells");
    // (The grid's own share of this is about 10 px/s on the cloud's card; a kick from being
    // parted would be the 400 px/s the parting moves them at.)
    assert!(off < 0.04 * free.length(), "and fall freely still: {off} px/s off {}", free.length());
}

/// A film leaves the block's corner evenly (the Water Weird layout): along the first stretch of
/// its flight no part of it is much thinner than another. (The elbow room, the one-stream pour and
/// the steps by detail together make it so; the plan has what each was measured to do.)
#[test]
fn a_film_leaves_a_block_even() {
    let Some(gpu) = card() else { return };
    let mut run = scenes::weird(6.0).start_grid(&gpu, Numbers::for_cell(6.0));
    // The block's top edge runs from its right corner (210, -330) down to its left one (0, -240);
    // the film runs down it and flies on along the same line.
    let (corner, way) = (Vec2::new(0.0, -240.0), Vec2::new(-210.0, 90.0).normalize());
    let mut evenness = 0.0;
    for k in 0..10 {
        run.run_to(1.0 + 0.1 * k as f32);
        let bins = probe::along(&run.water.read(), &run.water.layout(), corner, corner + way * 78.0, 30.0, 13);
        let (least, most) = (*bins.iter().min().expect("bins"), *bins.iter().max().expect("bins"));
        assert!(most > 0, "the film flies off the corner");
        evenness += least as f32 / most as f32 / 10.0;
    }
    println!("the film off the block: its thinnest stretch is {evenness} of its thickest, on average");
    assert!(evenness > 0.3, "the film is even along its flight: thinnest {evenness} of thickest");
}
