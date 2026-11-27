//! Several liquids (Water mode part 4.2, `docs/superpowers/plans/water-mode/part4-more-liquids.md`):
//! a Weight per liquid, so a heavy one sinks and a light one floats; Keeps apart, so oil keeps to
//! round drops in water; and colours that run together where liquids meet, by Mix speed, the same at
//! every detail.

use std::sync::Arc;

use bpe_water::drops::LEAST_WALL;
use bpe_water::probe;
use bpe_water::scenes;
use bpe_water::{Drop, Feel, Layout, MixWhen, Numbers, STOPS, SolidField, Water, WaterGpu, fill};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

/// An open course `w` x `h` px, its middle at nought (its edges are the walls), at `cell`-px cells.
fn course(w: f32, h: f32, cell: f32) -> (Layout, SolidField) {
    let layout = Layout::cover(Vec2::new(-w / 2.0, -h / 2.0), Vec2::new(w / 2.0, h / 2.0), cell);
    (layout, SolidField::build_least(layout, &[], LEAST_WALL * cell))
}

/// Drops of liquid `liquid` filling the world rectangle `lo` to `hi`.
fn block(layout: &Layout, field: &SolidField, lo: Vec2, hi: Vec2, liquid: u32) -> Vec<Drop> {
    fill(layout, field, lo, hi, STOPS | liquid, 0)
}

fn plays(water: &mut Water, frames: u32) {
    for _ in 0..frames {
        water.frame();
    }
}

/// Each drop's place and speed, as bits.
fn motion(drops: &[Drop]) -> Vec<[u32; 4]> {
    drops.iter().map(|d| [d.x[0].to_bits(), d.x[1].to_bits(), d.v[0].to_bits(), d.v[1].to_bits()]).collect()
}

/// The mean height (world px, down) of liquid `liquid`'s drops.
fn mean_y(water: &Water, liquid: usize) -> f32 {
    let layout = water.layout();
    let (sum, n) = probe::liquid(&water.read())
        .iter()
        .filter(|d| d.liquid() == liquid)
        .fold((0.0f64, 0u32), |(s, n), d| (s + f64::from(layout.to_world(Vec2::from(d.x)).y), n + 1));
    assert!(n > 0, "liquid {liquid} has drops");
    (sum / f64::from(n)) as f32
}

fn weighing(weight: f32) -> Feel {
    Feel { weight, ..Feel::default() }
}

/// A liquid alone moves the same at any Weight, to the bit: a pool with a block falling into it.
#[test]
fn a_liquid_alone_moves_the_same_at_any_weight() {
    let Some(gpu) = card() else { return };
    let (layout, field) = course(200.0, 300.0, 8.0);
    let mut drops = block(&layout, &field, Vec2::new(-100.0, 60.0), Vec2::new(100.0, 150.0), 0);
    drops.extend(block(&layout, &field, Vec2::new(-30.0, -120.0), Vec2::new(30.0, -60.0), 0));
    let run = |feel: Feel| {
        let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(8.0), &[feel]);
        water.add(&drops);
        plays(&mut water, 90);
        motion(&water.read())
    };
    let one = run(weighing(1.0));
    assert_eq!(one, run(weighing(4.0)), "Weight 4");
    assert_eq!(one, run(weighing(0.3)), "Weight 0.3");
}

/// Two liquids that weigh the same, keep apart by nothing and don't mix move as one liquid does, to
/// the bit.
#[test]
fn two_liquids_alike_move_as_one() {
    let Some(gpu) = card() else { return };
    let (layout, field) = course(200.0, 300.0, 8.0);
    let mut drops = block(&layout, &field, Vec2::new(-100.0, 60.0), Vec2::new(100.0, 150.0), 0);
    drops.extend(block(&layout, &field, Vec2::new(-30.0, -120.0), Vec2::new(30.0, -60.0), 0));
    let run = |drops: &[Drop], feels: &[Feel]| {
        let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(8.0), feels);
        water.add(drops);
        plays(&mut water, 90);
        motion(&water.read())
    };
    let one = run(&drops, &[Feel::default()]);
    let halves: Vec<Drop> =
        drops.iter().enumerate().map(|(k, d)| Drop { kind: d.kind | (k as u32 % 2), ..*d }).collect();
    assert_eq!(one, run(&halves, &[Feel::default(), Feel::default()]));
}

/// A heavy blob laid on a light pool sinks to the bottom, the light liquid rising round it, in thin
/// water at two details.
#[test]
fn a_heavy_liquid_sinks_through_a_light_one() {
    let Some(gpu) = card() else { return };
    for cell in [6.0, 3.0] {
        let (layout, field) = course(160.0, 240.0, cell);
        let mut drops = block(&layout, &field, Vec2::new(-80.0, 40.0), Vec2::new(80.0, 110.0), 0);
        drops.extend(block(&layout, &field, Vec2::new(-30.0, -20.0), Vec2::new(30.0, 38.0), 1));
        let feels = [weighing(1.0), weighing(3.0)];
        let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(cell), &feels);
        water.add(&drops);
        let (light, heavy) = (mean_y(&water, 0), mean_y(&water, 1));
        assert!(heavy < light - 40.0, "it starts on top: {heavy} over {light}");
        plays(&mut water, 180);
        let (light, heavy) = (mean_y(&water, 0), mean_y(&water, 1));
        assert!(heavy > light + 10.0, "{cell} px: the heavy liquid's mean height {heavy} is under the light's {light}");
    }
}

/// The same in a pool deep enough for the deep water to hold it up: the deep push shares by mass.
#[test]
fn a_deep_pool_settles_by_weight() {
    let Some(gpu) = card() else { return };
    let cell = 6.0;
    let (layout, field) = course(160.0, 420.0, cell);
    let mut drops = block(&layout, &field, Vec2::new(-80.0, -90.0), Vec2::new(80.0, 200.0), 0);
    drops.extend(block(&layout, &field, Vec2::new(-30.0, -160.0), Vec2::new(30.0, -92.0), 1));
    let feels = [weighing(1.0), weighing(3.0)];
    let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(cell), &feels);
    water.add(&drops);
    plays(&mut water, 240);
    assert!(water.deep_frames() > 100, "the deep part held the pool up ({} frames)", water.deep_frames());
    let (light, heavy) = (mean_y(&water, 0), mean_y(&water, 1));
    assert!(heavy > light + 40.0, "the heavy liquid's mean height {heavy} is under the light's {light}");
}

/// Oil (a little lighter) under a pool of water rises to the top.
#[test]
fn oil_floats() {
    let Some(gpu) = card() else { return };
    let cell = 6.0;
    let (layout, field) = course(160.0, 240.0, cell);
    let mut drops = block(&layout, &field, Vec2::new(-80.0, -20.0), Vec2::new(80.0, 110.0), 0);
    drops.retain(|d| {
        let p = layout.to_world(Vec2::from(d.x));
        !(p.x.abs() < 30.0 && p.y > 50.0)
    });
    drops.extend(block(&layout, &field, Vec2::new(-30.0, 50.0), Vec2::new(30.0, 110.0), 1));
    let feels = [weighing(1.0), weighing(0.8)];
    let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(cell), &feels);
    water.add(&drops);
    plays(&mut water, 240);
    let (water_y, oil) = (mean_y(&water, 0), mean_y(&water, 1));
    assert!(oil < water_y - 20.0, "the oil's mean height {oil} is over the water's {water_y}");
}

/// The furthest liquid `liquid`'s drops are from their middle (world px).
fn spread(water: &Water, liquid: usize) -> f32 {
    let layout = water.layout();
    let drops = water.read();
    let at: Vec<Vec2> =
        drops.iter().filter(|d| d.liquid() == liquid).map(|d| layout.to_world(Vec2::from(d.x))).collect();
    let middle = at.iter().copied().sum::<Vec2>() / at.len() as f32;
    at.iter().map(|p| p.distance(middle)).fold(0.0, f32::max)
}

/// A square of a liquid that keeps apart, in a still pool of another, pulls into a round drop; one
/// that doesn't stays as it is.
#[test]
fn kept_apart_liquids_pull_into_round_drops() {
    let Some(gpu) = card() else { return };
    let cell = 6.0;
    let (layout, field) = course(240.0, 240.0, cell);
    let mut drops = block(&layout, &field, Vec2::new(-116.0, -116.0), Vec2::new(116.0, 116.0), 0);
    drops.retain(|d| {
        let p = layout.to_world(Vec2::from(d.x));
        !(p.x.abs() < 30.0 && p.y.abs() < 30.0)
    });
    drops.extend(block(&layout, &field, Vec2::new(-30.0, -30.0), Vec2::new(30.0, 30.0), 1));
    let run = |keep: f32| {
        let pull = |keeps_apart| Feel { surface_pull: 1.0, keeps_apart, ..Feel::default() };
        let feels = [pull(0.0), pull(keep)];
        let mut water = Water::new(gpu.clone(), &field, Vec2::ZERO, Numbers::for_cell(cell), &feels);
        water.add(&drops);
        let before = spread(&water, 1);
        plays(&mut water, 60);
        (before, spread(&water, 1))
    };
    let (before, mixed) = run(0.0);
    let (_, kept) = run(1.0);
    assert!(kept < before * 0.88, "kept apart, the square's corners pull in: {before} to {kept}");
    assert!(mixed > before * 0.95, "not kept apart, it stays: {before} to {mixed}");
}

fn mixing(mix_speed: f32, keeps_apart: f32, mixes_when: MixWhen) -> Feel {
    Feel { mix_speed, keeps_apart, mixes_when, ..Feel::default() }
}

/// Two still liquids side by side (no gravity) at `cell`-px cells, each with `feel`, played 1 s.
fn side_by_side(gpu: &Arc<WaterGpu>, cell: f32, feel: Feel) -> Water {
    let (layout, field) = course(400.0, 120.0, cell);
    let mut drops = block(&layout, &field, Vec2::new(-196.0, -56.0), Vec2::new(0.0, 56.0), 0);
    drops.extend(block(&layout, &field, Vec2::new(0.0, -56.0), Vec2::new(196.0, 56.0), 1));
    let mut water = Water::new(gpu.clone(), &field, Vec2::ZERO, Numbers::for_cell(cell), &[feel, feel]);
    water.add(&drops);
    plays(&mut water, 60);
    water
}

/// How much of each liquid the drops hold, their own and what they carry.
fn held(drops: &[Drop]) -> [f64; 2] {
    let mut out = [0.0f64; 2];
    for d in drops {
        let s = f64::from(d.mix_share());
        out[d.liquid()] += 1.0 - s;
        if s > 0.0 {
            out[d.mix_other()] += s;
        }
    }
    out
}

/// The blend's width (world px): from the left-most drop carrying more than 0.1 of the other to the
/// right-most, among drops in the middle third of the height.
fn blend(water: &Water) -> f32 {
    let layout = water.layout();
    let xs: Vec<f32> = water
        .read()
        .iter()
        .filter(|d| d.mix_share() > 0.1)
        .map(|d| layout.to_world(Vec2::from(d.x)))
        .filter(|p| p.y.abs() < 20.0)
        .map(|p| p.x)
        .collect();
    xs.iter().copied().fold(f32::MIN, f32::max) - xs.iter().copied().fold(f32::MAX, f32::min)
}

/// Where two liquids meet their colours run together, the far parts stay their own, and each
/// liquid's amount (own and carried) is kept.
#[test]
fn colours_run_together_where_liquids_meet() {
    let Some(gpu) = card() else { return };
    let water = side_by_side(&gpu, 6.0, mixing(1.0, 0.0, MixWhen::Always));
    let layout = water.layout();
    let drops = water.read();
    let (mut near, mut far, mut near_mixed, mut far_mixed) = (0, 0, 0, 0);
    for d in &drops {
        let x = layout.to_world(Vec2::from(d.x)).x;
        if x.abs() < 10.0 {
            near += 1;
            near_mixed += usize::from((0.15..0.85).contains(&d.mix_share()));
        } else if x.abs() > 80.0 {
            far += 1;
            far_mixed += usize::from(d.mix_share() >= 0.02);
        }
    }
    assert!(near_mixed * 10 >= near * 8, "{near_mixed} of {near} drops by the line carry some of the other");
    assert!(far_mixed * 100 <= far, "{far_mixed} of {far} drops far off carry some of the other");
    let amounts = held(&drops);
    let half = drops.len() as f64 / 2.0;
    for a in amounts {
        assert!((a - half).abs() < half * 0.01, "each liquid's amount is kept: {amounts:?} of {}", drops.len());
    }
}

/// The blend is as wide in pixels at every detail.
#[test]
fn mixing_is_the_same_at_every_detail() {
    let Some(gpu) = card() else { return };
    let coarse = blend(&side_by_side(&gpu, 6.0, mixing(0.6, 0.0, MixWhen::Always)));
    let fine = blend(&side_by_side(&gpu, 3.0, mixing(0.6, 0.0, MixWhen::Always)));
    assert!(coarse > 10.0, "the colours ran together: {coarse} px");
    assert!((fine - coarse).abs() < 0.3 * coarse, "{coarse} px at 6 px cells, {fine} px at 3 px");
}

/// No Mix speed, keeping apart, or a stir that never comes: no colour crosses.
#[test]
fn colours_cross_only_when_they_may() {
    let Some(gpu) = card() else { return };
    for (what, feel) in
        [("no Mix speed", mixing(0.0, 0.0, MixWhen::Always)), ("kept apart", mixing(1.0, 1.0, MixWhen::Always))]
    {
        let water = side_by_side(&gpu, 6.0, feel);
        let mixed = water.read().iter().filter(|d| d.mix != 0).count();
        assert_eq!(mixed, 0, "{what}: {mixed} drops carry another's colour");
    }
}

/// Two liquids resting side by side on the floor of a 400 x 200 course (6 px cells), settled 2 s with
/// no Mix speed, then given `feel` and played on 1 s; with `lump`, a lump of liquid 1 falls into
/// liquid 0 as they play on.
fn rest_then(gpu: &Arc<WaterGpu>, feel: Feel, lump: bool) -> Water {
    let cell = 6.0;
    let (layout, field) = course(400.0, 200.0, cell);
    let mut drops = block(&layout, &field, Vec2::new(-196.0, 0.0), Vec2::new(0.0, 96.0), 0);
    drops.extend(block(&layout, &field, Vec2::new(0.0, 0.0), Vec2::new(196.0, 96.0), 1));
    let still = Feel { mix_speed: 0.0, ..feel };
    let mut water = Water::new(gpu.clone(), &field, scenes::GRAVITY, Numbers::for_cell(cell), &[still, still]);
    water.add(&drops);
    plays(&mut water, 120);
    water.set_feels(&[feel, feel]);
    if lump {
        water.add(&block(&layout, &field, Vec2::new(-130.0, -90.0), Vec2::new(-70.0, -50.0), 1));
    }
    plays(&mut water, 60);
    water
}

/// Only when stirred and Only on hits: liquids resting side by side keep their colours (a resting
/// pool's jiggle is no stir); a lump falling into one stirs and hits it, and the colours run
/// together there.
#[test]
fn colours_wait_for_a_stir_or_a_hit() {
    let Some(gpu) = card() else { return };
    for (what, when) in [("only when stirred", MixWhen::Stirred), ("only on hits", MixWhen::Hits)] {
        let feel = mixing(1.0, 0.0, when);
        let still = rest_then(&gpu, feel, false).read().iter().filter(|d| d.mix != 0).count();
        assert_eq!(still, 0, "{what}, resting: {still} drops carry another's colour");
        let water = rest_then(&gpu, feel, true);
        let layout = water.layout();
        let hit = water.read().iter().filter(|d| d.mix != 0 && layout.to_world(Vec2::from(d.x)).x < -40.0).count();
        assert!(hit > 20, "{what}: the lump's fall mixed {hit} drops where it landed");
    }
}

/// A restart point and a copy carry the mixes: played on, they give the same drops and colours.
#[test]
fn a_restart_point_keeps_the_mixes() {
    let Some(gpu) = card() else { return };
    let feel = mixing(1.0, 0.0, MixWhen::Always);
    let mut a = side_by_side(&gpu, 6.0, feel);
    assert!(a.read().iter().any(|d| d.mix != 0), "some colour crossed");
    let state = a.state();
    let back = bpe_water::WaterState::from_bytes(&state.to_bytes()).expect("the state reads back");
    let (_, field) = course(400.0, 120.0, 6.0);
    let mut b = Water::from_state(gpu.clone(), &field, Vec2::ZERO, Numbers::for_cell(6.0), &[feel, feel], &back)
        .expect("a body from the state");
    let mut c = a.copy();
    plays(&mut a, 30);
    plays(&mut b, 30);
    plays(&mut c, 30);
    let first = probe::print_of(&a.read());
    assert_eq!(first, probe::print_of(&b.read()), "from the restart point");
    assert_eq!(first, probe::print_of(&c.read()), "from a copy");
}
