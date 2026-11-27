//! Wetting (part 2c, `docs/superpowers/plans/water-mode/part2c-air-and-wetting.md`): a wetted face
//! is liquid to the liquid's skin, as much as it is wet. Water beads up on a waxy face and spreads
//! into a film on a soaked one; water away from the faces does not feel it.

use std::sync::Arc;

use bpe_core::maths::Maths;
use bpe_water::probe;
use bpe_water::scenes::{self, BLOB};
use bpe_water::{AIR, Drop, Layout, Numbers, STOPS, Solid, SolidField, Water, WaterGpu, fill_stacked};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

/// A blob's width and height on the floor (pixels), away from the box's sides.
fn size(drops: &[Drop], layout: &Layout) -> (f32, f32) {
    let (floor, half) = BLOB;
    probe::puddle(drops, layout, floor, (-half + 2.0 * layout.cell, half - 2.0 * layout.cell))
}

/// A 24 px blob settles wider and lower the wetter the floor: standing up on a waxy one (as the
/// skin of C2's water holds it, its edge near square to the floor), a film on a soaked one.
#[test]
fn a_blob_spreads_on_a_wet_floor_and_stands_up_on_a_waxy_one() {
    let Some(gpu) = card() else { return };
    let cell = 6.0;
    let settle = |wetting: f32| {
        let mut run = scenes::blob(cell, 24.0, wetting).start(&gpu, Numbers::for_cell(cell));
        let water = run.water.read().len();
        run.run_to(3.0);
        let drops = run.water.read();
        assert_eq!(drops.len(), water);
        let (w, h) = size(&drops, &run.water.layout());
        eprintln!("Wetting {wetting}: {w:.1} px wide, {h:.1} px tall");
        (w, h)
    };
    let sizes: Vec<(f32, f32)> = [0.0, 0.5, 1.0].into_iter().map(settle).collect();
    let spacing = 0.5 * cell;
    for pair in sizes.windows(2) {
        assert!(pair[1].0 > pair[0].0 + spacing, "wider as it wets: {sizes:?}");
        assert!(pair[1].1 < pair[0].1, "lower as it wets: {sizes:?}");
    }
    assert!(sizes[0].1 > 0.4 * sizes[0].0, "standing up on a waxy floor: {:?}", sizes[0]);
    assert!(sizes[2].1 < 2.0 * spacing, "a film on a soaked one: {:?}", sizes[2]);
}

/// The edge's own angle on a face (a light cap, its weight too small to flatten it) is the same at
/// 6 and 3 px: Wetting reaches as far at every detail (part 3, item 0).
#[test]
fn the_edge_angle_is_the_same_at_every_detail() {
    let Some(gpu) = card() else { return };
    let angle = |cell: f32| {
        let mut run = scenes::cap(cell, 36.0, 0.75).start(&gpu, Numbers::for_cell(cell));
        run.run_to(2.0);
        let (w, h) = size(&run.water.read(), &run.water.layout());
        let angle = 2.0 * (2.0 * h / w).det_atan().to_degrees();
        eprintln!("{cell} px: {w:.1} x {h:.1} px, {angle:.0} degrees");
        angle
    };
    let (fine, finer) = (angle(6.0), angle(3.0));
    assert!((fine - finer).abs() <= 8.0, "{fine:.0} at 6 px, {finer:.0} at 3 px");
    for a in [fine, finer] {
        assert!((45.0..80.0).contains(&a), "Wetting 0.75 makes an edge of about 60 degrees, not {a:.0}");
    }
}

/// On a soaked floor (Wetting 1) a 24 px blob spreads into a film at 6 and at 3 px (part 2c's 3 px
/// one stood as a cap 12.6 px tall, its edge held up by the skin's bend: part 3, item 0).
#[test]
fn a_soaked_floor_spreads_a_blob_into_a_film_at_every_detail() {
    let Some(gpu) = card() else { return };
    for cell in [6.0, 3.0] {
        let mut run = scenes::blob(cell, 24.0, 1.0).start(&gpu, Numbers::for_cell(cell));
        run.run_to(3.0);
        let (w, h) = size(&run.water.read(), &run.water.layout());
        eprintln!("{cell} px: {w:.1} x {h:.1} px");
        assert!(h < cell, "a film at {cell} px is under two spacings deep, not {h:.1} px");
    }
}

/// Water further than a reach from every face plays the same to the bit however wet the faces are.
#[test]
fn wetting_reaches_only_water_near_a_face() {
    let Some(gpu) = card() else { return };
    let cell = 6.0;
    let play = |wetting: f32| {
        let half = Vec2::new(300.0, 400.0);
        let layout = Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), cell);
        let field = SolidField::build(layout, &[Solid::rect(-half, half, false, 6.0)]).with_wetting(wetting);
        let mut water = Water::new(
            gpu.clone(),
            &field,
            scenes::GRAVITY,
            Numbers::for_cell(cell),
            &scenes::blob(cell, 1.0, 0.0).feels,
        );
        water.add(&fill_stacked(&layout, &field, Vec2::new(-40.0, -300.0), Vec2::new(40.0, -220.0), STOPS, 0.25));
        for _ in 0..10 {
            water.frame();
        }
        water.read()
    };
    let (dry, wet) = (play(0.0), play(1.0));
    assert!(dry.len() > 500);
    assert_eq!(probe::print_of(&dry), probe::print_of(&wet));
}

/// Air takes no part in Wetting: a block of air drops set on a floor with no water round it (out of
/// the deep water, so it bursts at once) is gone after its first frame however wet the floor is; a
/// wet floor holds no air.
#[test]
fn air_on_a_wet_floor_bursts_as_on_a_dry_one() {
    let Some(gpu) = card() else { return };
    let cell = 6.0;
    let play = |wetting: f32| {
        let scene = scenes::blob(cell, 120.0, wetting);
        let field = SolidField::build_least(scene.layout, &scene.solids, bpe_water::drops::LEAST_WALL * cell)
            .with_wetting(wetting);
        let mut water = Water::new(gpu.clone(), &field, scene.gravity, Numbers::for_cell(cell), &scene.feels);
        let (lo, hi) = scene.fills[0];
        let added = water.add(&fill_stacked(&scene.layout, &field, lo, hi, AIR | STOPS, 0.25));
        water.frame();
        (added, water.read())
    };
    for wetting in [0.0, 1.0] {
        let (added, left) = play(wetting);
        assert!(added > 100, "{added}");
        assert!(left.is_empty(), "at Wetting {wetting}, {} of {added} air drops are left", left.len());
    }
}
