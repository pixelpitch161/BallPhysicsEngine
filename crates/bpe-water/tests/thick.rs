//! Thick liquids (Water mode part 4.3b, `docs/superpowers/plans/water-mode/part4-more-liquids.md`):
//! past a Thickness of 0.1 a liquid's speeds are evened out by stages of a prescribed velocity
//! gradient after each step, so honey spreads slowly, the same at 6 and 3 px, keeps a blob's spin
//! and lets go of its shear.

use std::sync::Arc;

use bpe_water::drops::LEAST_WALL;
use bpe_water::probe;
use bpe_water::scenes;
use bpe_water::{Drop, Feel, Layout, Numbers, STOPS, Solid, SolidField, Water, WaterGpu, fill};
use glam::Vec2;

fn card() -> Arc<WaterGpu> {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    WaterGpu::new(&device, &queue)
}

/// An open course `w` x `h` px, its middle at nought (its edges are the walls), at `cell`-px cells.
fn course(w: f32, h: f32, cell: f32) -> (Layout, SolidField) {
    let layout = Layout::cover(Vec2::new(-w / 2.0, -h / 2.0), Vec2::new(w / 2.0, h / 2.0), cell);
    (layout, SolidField::build_least(layout, &[], LEAST_WALL * cell))
}

fn thick(thickness: f32) -> Feel {
    Feel { thickness, ..Feel::default() }
}

fn plays(water: &mut Water, frames: u32) {
    for _ in 0..frames {
        water.frame();
    }
}

/// How wide the liquid lies (world px): from its 2nd to its 98th hundredth across, so a splash or
/// two doesn't count.
fn width(water: &Water) -> f32 {
    let layout = water.layout();
    let mut xs: Vec<f32> = probe::liquid(&water.read()).iter().map(|d| layout.to_world(Vec2::from(d.x)).x).collect();
    xs.sort_by(f32::total_cmp);
    let at = |share: f32| xs[((xs.len() - 1) as f32 * share) as usize];
    at(0.98) - at(0.02)
}

/// A 120 px square lump of a liquid of `thickness` standing on the floor of a course 1000 px wide,
/// at `cell`-px cells, after a quarter of a second: how wide it lies.
fn spread(gpu: &Arc<WaterGpu>, thickness: f32, cell: f32) -> f32 {
    let (layout, field) = course(1000.0, 300.0, cell);
    let lump = fill(&layout, &field, Vec2::new(-60.0, 30.0), Vec2::new(60.0, 150.0), STOPS, 0);
    let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(cell), &[thick(thickness)]);
    water.add(&lump);
    plays(&mut water, 15);
    width(&water)
}

#[test]
fn thicker_liquids_spread_slower() {
    let gpu = card();
    // (Across the Thickness the stages start from too: 0.1 has the plain evening, 0.11 the stages.)
    let widths: Vec<f32> = [0.02, 0.1, 0.11, 0.35, 0.6, 1.0].iter().map(|t| spread(&gpu, *t, 6.0)).collect();
    eprintln!("widths at 0.25 s: {widths:?}");
    for pair in widths.windows(2) {
        assert!(pair[1] < pair[0], "a thicker liquid lies narrower: {widths:?}");
    }
    let (water, honey) = (widths[0] - 120.0, widths[4] - 120.0);
    assert!(honey < 0.6 * water, "honey spreads {honey} px, water {water} px");
}

#[test]
fn thickness_spreads_alike_at_6_and_3_px() {
    let gpu = card();
    let (coarse, fine) = (spread(&gpu, 0.6, 6.0), spread(&gpu, 0.6, 3.0));
    eprintln!("honey's width at 0.25 s: {coarse} px at 6 px, {fine} px at 3 px");
    assert!((coarse - fine).abs() < 0.15 * fine, "{coarse} at 6 px, {fine} at 3 px");
}

/// A disc of liquid `feel` 120 px across in a course with no gravity at 6 px, each drop moving at
/// `v(offset from the middle)` (px a second), after `frames`: the drops' places off the middle and
/// speeds (px, px a second), at the start and at the end.
fn disc(gpu: &Arc<WaterGpu>, feel: Feel, v: impl Fn(Vec2) -> Vec2, frames: u32) -> [Vec<(Vec2, Vec2)>; 2] {
    let cell = 6.0;
    let (layout, field) = course(600.0, 600.0, cell);
    let mut drops: Vec<Drop> = fill(&layout, &field, Vec2::splat(-60.0), Vec2::splat(60.0), STOPS, 0)
        .into_iter()
        .filter(|d| layout.to_world(Vec2::from(d.x)).length() < 60.0)
        .collect();
    for d in &mut drops {
        d.v = (v(layout.to_world(Vec2::from(d.x))) / cell).to_array();
    }
    let mut water = Water::new(gpu.clone(), &field, Vec2::ZERO, Numbers::for_cell(cell), &[feel]);
    water.add(&drops);
    let at = |water: &Water| -> Vec<(Vec2, Vec2)> {
        let all = water.read();
        let drops = probe::liquid(&all);
        let mid = drops.iter().map(|d| layout.to_world(Vec2::from(d.x))).sum::<Vec2>() / drops.len() as f32;
        drops.iter().map(|d| (layout.to_world(Vec2::from(d.x)) - mid, Vec2::from(d.v) * cell)).collect()
    };
    let start = at(&water);
    plays(&mut water, frames);
    [start, at(&water)]
}

/// How fast the drops turn about their middle: their angular momentum over their moment of inertia
/// (radians a second).
fn spin(drops: &[(Vec2, Vec2)]) -> f32 {
    let (l, i) = drops.iter().fold((0.0f32, 0.0f32), |(l, i), (x, v)| (l + x.perp_dot(*v), i + x.length_squared()));
    l / i
}

/// The shear of the straight-line field that fits the drops' speeds best (a second): half the sum
/// of how vx changes down and vy across.
fn shear(drops: &[(Vec2, Vec2)]) -> f32 {
    // Least squares: v = v0 + G x, the places' middle at nought.
    let (mut xx, mut xy, mut yy) = (0.0f32, 0.0f32, 0.0f32);
    let (mut vxx, mut vxy, mut vyx, mut vyy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for (x, v) in drops {
        (xx, xy, yy) = (xx + x.x * x.x, xy + x.x * x.y, yy + x.y * x.y);
        (vxx, vxy) = (vxx + v.x * x.x, vxy + v.x * x.y);
        (vyx, vyy) = (vyx + v.y * x.x, vyy + v.y * x.y);
    }
    let det = xx * yy - xy * xy;
    // Row a of G solves [xx xy; xy yy] g = [v_a x, v_a y].
    let row = |bx: f32, by: f32| ((bx * yy - by * xy) / det, (by * xx - bx * xy) / det);
    let (_, dvx_dy) = row(vxx, vxy);
    let (dvy_dx, _) = row(vyx, vyy);
    0.5 * (dvx_dy + dvy_dx)
}

#[test]
fn a_thick_blob_keeps_its_spin() {
    let gpu = card();
    let turning = |x: Vec2| 3.0 * x.perp();
    let [start, end] = disc(&gpu, thick(0.6), turning, 15);
    let (was, now) = (spin(&start), spin(&end));
    eprintln!("honey's spin: {was} to {now} radians a second in a quarter second");
    assert!((was - 3.0).abs() < 0.1, "{was}");
    assert!(now > 0.8 * was && now < 1.1 * was, "{was} to {now}");
}

#[test]
fn a_thick_blob_loses_its_shear() {
    let gpu = card();
    let sheared = |x: Vec2| Vec2::new(3.0 * x.y, 0.0);
    let [start, honey] = disc(&gpu, thick(0.6), sheared, 6);
    let [_, water] = disc(&gpu, thick(0.02), sheared, 6);
    let (was, honey, water) = (shear(&start), shear(&honey), shear(&water));
    eprintln!("shear {was}: honey {honey}, water {water} after a tenth of a second");
    assert!((was - 1.5).abs() < 0.05, "{was}");
    assert!(honey.abs() < 0.3 * was, "honey keeps {honey} of {was}");
    assert!(water.abs() > 0.7 * was, "water keeps {water} of {was}");
}

/// How far (px) a 60 x 120 px block of liquid of `thickness` standing against a wall (the floor far
/// below) falls in a quarter of a second, on average, at 6 px.
fn slide(gpu: &Arc<WaterGpu>, thickness: f32) -> f32 {
    let cell = 6.0;
    let layout = Layout::cover(Vec2::new(-200.0, -600.0), Vec2::new(200.0, 600.0), cell);
    let wall = Solid::rect(Vec2::new(-200.0, -600.0), Vec2::new(-150.0, 600.0), true, 0.0);
    let field = SolidField::build_least(layout, &[wall], LEAST_WALL * cell);
    // (Its first column within a reach of the face.)
    let lo = Vec2::new(-150.0 + 0.5 * cell, -500.0);
    let block = fill(&layout, &field, lo, lo + Vec2::new(60.0, 120.0), STOPS, 0);
    let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(cell), &[thick(thickness)]);
    water.add(&block);
    let mean = |water: &Water| {
        let all = water.read();
        let drops = probe::liquid(&all);
        drops.iter().map(|d| layout.to_world(Vec2::from(d.x)).y).sum::<f32>() / drops.len() as f32
    };
    let was = mean(&water);
    plays(&mut water, 15);
    mean(&water) - was
}

#[test]
fn a_thick_liquid_clings_to_a_wall() {
    let gpu = card();
    let (water, honey) = (slide(&gpu, 0.02), slide(&gpu, 0.6));
    eprintln!("down the wall in a quarter second: water {water} px, honey {honey} px");
    assert!(water > 100.0, "water falls freely: {water}");
    assert!(honey < 0.65 * water, "honey {honey}, water {water}");
}
