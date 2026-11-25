//! Thin streams and films (Water mode part 2a): a pour stays one narrow stream down a whole
//! screen, a sheet off an edge stays whole, and a drop alone is left alone.

use std::sync::Arc;

use bpe_water::probe::{self, Stream};
use bpe_water::scenes;
use bpe_water::{Drop, Feel, Layout, Numbers, SolidField, WATER, Water, WaterGpu};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

/// A two-cell pour falling 1,700 px, measured in six bands of its fall.
fn pour_at(cell: f32) -> Option<Stream> {
    let gpu = card()?;
    let mut run = scenes::thin_pour(cell, 2.0).start(&gpu, Numbers::default());
    run.run_to(1.5);
    let layout = run.water.layout();
    let (top, floor) = scenes::THIN_POUR;
    let (lo, hi) = (Vec2::new(-250.0, top + 40.0), Vec2::new(250.0, floor - 300.0));
    let air = probe::inside(&run.water.read(), &layout, lo, hi);
    Some(Stream::of(&air, &layout, (lo.y, hi.y), 1.5, 6))
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
    let mut run = scenes::sheet(8.0).start(&gpu, Numbers::default());
    run.run_to(2.0);
    let layout = run.water.layout();
    // The air past the shelf's edge, down to a little above the floor and short of the wall.
    let (edge, shelf, floor) = scenes::SHEET;
    let (lo, hi) = (Vec2::new(edge + 4.0, shelf - 40.0), Vec2::new(380.0, floor - 200.0));
    let air = probe::inside(&run.water.read(), &layout, lo, hi);
    let s = Stream::of(&air, &layout, (lo.y, hi.y), 1.5, 6);
    assert!(s.drops > 100, "liquid runs off the shelf: {}", s.drops);
    assert!(s.whole > 0.6, "most of what is in the air is one piece: {}", s.whole);
    assert!(s.strays < 0.1, "with no rain of droplets: {}", s.strays);
}

/// A drop by itself flies as if nothing else were there: the pull and the spacing act on nothing.
#[test]
fn a_lone_drop_pulls_on_nothing() {
    let Some(gpu) = card() else { return };
    let layout = Layout::cover(Vec2::splat(-200.0), Vec2::splat(200.0), 8.0);
    let field = SolidField::build(layout, &[]);
    let gravity = Vec2::new(0.0, 4000.0);
    let mut water =
        Water::new(gpu, &field, gravity, Numbers::default(), &[Feel { surface_pull: 1.0, ..Feel::default() }]);
    let (x0, v0) = (Vec2::new(-150.3, -120.7), Vec2::new(900.0, -300.0));
    let mut lone = Drop::new(layout.to_grid(x0), v0 / layout.cell, 0, 0, WATER);
    // (Stretched: the most a pull could act on.)
    lone.e = 0.5;
    water.add(&[lone]);
    let steps = 48u32;
    for _ in 0..steps {
        water.step();
    }
    let dt = water.numbers().dt();
    // What the step does to one drop alone: speed first, then place.
    let (mut x, mut v) = (x0, v0);
    for _ in 0..steps {
        v += gravity * dt;
        x += v * dt;
    }
    let d = water.read()[0];
    let at = layout.to_world(Vec2::from(d.x));
    assert!((at - x).length() < 0.5, "it is where a free fall puts it: {at} against {x}");
    assert!((Vec2::from(d.v) * layout.cell - v).length() < 2.0, "at a free fall's speed");
}
