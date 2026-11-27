//! Forces on the water (Water mode part 4b, `docs/superpowers/plans/water-mode/part4b-forces-and-ferrofluid.md`):
//! wind, whirlpools and magnets with spikes, in the engine.

use std::sync::Arc;

use bpe_core::maths::Maths;

use bpe_water::drops::LEAST_WALL;
use bpe_water::drops::forces::{GRAVITY_UNIT, PSI_SLOPE_MOST};
use bpe_water::probe;
use bpe_water::{Drop, Feel, Force, Layout, Numbers, Push, STOPS, Solid, SolidField, WATER, Water, WaterGpu};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

const GRAVITY: Vec2 = Vec2::new(0.0, 4000.0);

/// A 600 x 900 box (inner faces at x ±300, y ±450), at `cell`-pixel cells.
fn open_box(cell: f32) -> (Layout, SolidField) {
    let half = Vec2::new(300.0, 450.0);
    let layout = Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), cell);
    let field = SolidField::build_least(layout, &[Solid::rect(-half, half, false, 3.0)], LEAST_WALL * cell);
    (layout, field)
}

/// Drops at rest spacing filling a disc of `radius` pixels round `centre`.
fn blob(layout: &Layout, centre: Vec2, radius: f32, kind: u32, first: u32) -> Vec<Drop> {
    let step = layout.cell / 2.0;
    let n = (radius / step).ceil() as i32;
    let mut out = Vec::new();
    for j in -n..=n {
        for i in -n..=n {
            let p = centre + Vec2::new(i as f32 + 0.5, j as f32 + 0.5) * step;
            if p.distance(centre) <= radius {
                out.push(Drop::new(layout.to_grid(p), Vec2::ZERO, kind | STOPS, first + out.len() as u32, WATER));
            }
        }
    }
    out
}

fn magnet(place: Vec2, strength: f32, spikes: u32, spread_deg: f32, liquids: u32) -> Force {
    Force {
        push: Push::Magnet {
            pull: strength * GRAVITY_UNIT / PSI_SLOPE_MOST,
            spikes,
            depth: 0.45,
            aim: -std::f32::consts::FRAC_PI_2,
            spread: spread_deg.to_radians(),
        },
        place,
        reach: 300.0,
        liquids,
    }
}

fn whirlpool(place: Vec2, speed: f32, rate: f32) -> Force {
    Force { push: Push::Whirlpool { speed, rate }, place, reach: 200.0, liquids: u32::MAX }
}

fn wind(velocity: Vec2, rate: f32) -> Force {
    Force { push: Push::Wind { velocity, rate }, place: Vec2::ZERO, reach: 0.0, liquids: u32::MAX }
}

fn play(water: &mut Water, frames: u32) {
    for _ in 0..frames {
        water.frame();
    }
}

/// Forces of no strength leave the water as it is, to the bit.
#[test]
fn no_force_at_strength_zero() {
    let Some(gpu) = card() else { return };
    let run = |forces: &[Force]| {
        let (layout, field) = open_box(12.0);
        let mut water = Water::new(gpu.clone(), &field, GRAVITY, Numbers::for_cell(12.0), &[Feel::default()]);
        water.add(&blob(&layout, Vec2::new(20.0, -200.0), 90.0, 0, 0));
        water.set_forces(forces);
        play(&mut water, 60);
        water.read()
    };
    let none = run(&[]);
    let zero = run(&[
        magnet(Vec2::new(0.0, 300.0), 0.0, 9, 180.0, u32::MAX),
        whirlpool(Vec2::new(0.0, 200.0), 0.0, 0.0),
        wind(Vec2::ZERO, 0.0),
    ]);
    assert!(none.len() > 500);
    assert_eq!(probe::print_of(&none), probe::print_of(&zero));
}

/// A lone drop in still air takes the wind's speed.
#[test]
fn wind_blows_a_lone_drop_its_way() {
    let Some(gpu) = card() else { return };
    let (layout, field) = open_box(8.0);
    let mut water = Water::new(gpu.clone(), &field, Vec2::ZERO, Numbers::for_cell(8.0), &[Feel::default()]);
    water.add(&[Drop::new(layout.to_grid(Vec2::new(-280.0, 0.0)), Vec2::ZERO, STOPS, 0, WATER)]);
    water.set_forces(&[wind(Vec2::new(1000.0, 0.0), 3.0)]);
    play(&mut water, 30);
    let d = water.read()[0];
    let v = Vec2::from(d.v) * layout.cell;
    // (1000 (1 - e^-1.5) = 777 px/s after half a second, 240 px on.)
    assert!((740.0..=810.0).contains(&v.x) && v.y.abs() < 5.0, "after half a second: {v}");
    let x = layout.to_world(Vec2::from(d.x)).x;
    assert!((-60.0..=-20.0).contains(&x), "{x}");
}

/// Wind blows spray far more than the water of a pool, which it hardly reaches under the top.
#[test]
fn wind_moves_spray_more_than_a_pool() {
    let Some(gpu) = card() else { return };
    let (layout, field) = open_box(12.0);
    let mut water = Water::new(gpu.clone(), &field, GRAVITY, Numbers::for_cell(12.0), &[Feel::default()]);
    let pool = bpe_water::fill_stacked(&layout, &field, Vec2::new(-298.0, 300.0), Vec2::new(298.0, 448.0), STOPS, 0.25);
    let n = pool.len();
    water.add(&pool);
    play(&mut water, 30);
    water.add(&[Drop::new(layout.to_grid(Vec2::new(-200.0, -400.0)), Vec2::ZERO, STOPS, 0, WATER)]);
    water.set_forces(&[wind(Vec2::new(1000.0, 0.0), 3.0)]);
    play(&mut water, 15);
    let drops = probe::liquid(&water.read()).into_owned();
    let world = |d: &Drop| layout.to_world(Vec2::from(d.x));
    let lone = drops.iter().find(|d| world(d).y < 0.0).expect("the lone drop");
    let lone_vx = lone.v[0] * layout.cell;
    let deep: Vec<f32> = drops.iter().filter(|d| world(d).y > 360.0).map(|d| d.v[0] * layout.cell).collect();
    assert!(deep.len() * 3 > n, "{} deep of {n}", deep.len());
    let deep_vx = deep.iter().sum::<f32>() / deep.len() as f32;
    assert!(lone_vx > 300.0, "the spray is blown: {lone_vx}");
    assert!(deep_vx.abs() < 20.0 && lone_vx > 5.0 * deep_vx.abs(), "the deep water hardly: {deep_vx}");
}

/// The water round a whirlpool turns its way.
#[test]
fn a_whirlpool_turns_the_water_round_it() {
    let Some(gpu) = card() else { return };
    let turning = |speed: f32| {
        let (layout, field) = open_box(8.0);
        let mut water = Water::new(gpu.clone(), &field, Vec2::ZERO, Numbers::for_cell(8.0), &[Feel::default()]);
        let middle = Vec2::new(10.0, -20.0);
        water.add(&blob(&layout, middle, 70.0, 0, 0));
        water.set_forces(&[whirlpool(middle, speed, 4.0)]);
        play(&mut water, 30);
        let drops = water.read();
        let spin: f32 = drops
            .iter()
            .map(|d| {
                let r = layout.to_world(Vec2::from(d.x)) - middle;
                let v = Vec2::from(d.v) * layout.cell;
                r.perp_dot(v)
            })
            .sum::<f32>()
            / drops.len() as f32;
        spin
    };
    let (one, other) = (turning(600.0), turning(-600.0));
    assert!(one > 2000.0, "a whirlpool turns the water: {one} px²/s a drop");
    assert!(other < -2000.0, "and the other way below 0: {other}");
}

/// A magnet pulls the liquid it names in, and leaves the others.
#[test]
fn a_magnet_pulls_only_its_liquid() {
    let Some(gpu) = card() else { return };
    let (layout, field) = open_box(8.0);
    let feels = [Feel::default(), Feel::default()];
    let mut water = Water::new(gpu.clone(), &field, Vec2::ZERO, Numbers::for_cell(8.0), &feels);
    // (Liquid 1 swings through the magnet's middle and out the far side: liquid 0 is out of its way.)
    let a = blob(&layout, Vec2::new(0.0, -250.0), 30.0, 0, 0);
    let b = blob(&layout, Vec2::new(160.0, 0.0), 30.0, 1, a.len() as u32);
    water.add(&a);
    water.add(&b);
    water.set_forces(&[magnet(Vec2::ZERO, 2.0, 0, 180.0, 0b10)]);
    // (Each liquid's middle, and its drops' mean distance from the magnet.)
    let mean = |water: &Water, kind: u32| {
        let drops: Vec<Vec2> =
            water.read().iter().filter(|d| d.kind & 15 == kind).map(|d| layout.to_world(Vec2::from(d.x))).collect();
        let n = drops.len() as f32;
        (drops.iter().sum::<Vec2>() / n, drops.iter().map(|p| p.length()).sum::<f32>() / n, drops.len())
    };
    let (mid0, _, n0) = mean(&water, 0);
    let (_, far1, n1) = mean(&water, 1);
    play(&mut water, 90);
    let (now0, _, m0) = mean(&water, 0);
    let (_, near1, m1) = mean(&water, 1);
    assert_eq!((n0, n1), (m0, m1));
    assert!(near1 < far1 * 2.0 / 3.0, "the magnet's liquid comes in: {far1} to {near1} px");
    assert!(mid0.distance(now0) < 2.0, "the other stays where it is: {mid0} to {now0}");
}

/// A magnet under a pool's floor stands its liquid up in a crown of spikes, one for each of its teeth.
#[test]
fn a_magnet_stands_spikes_up() {
    let Some(gpu) = card() else { return };
    let (layout, field) = open_box(6.0);
    let mut water = Water::new(gpu.clone(), &field, GRAVITY, Numbers::for_cell(6.0), &[Feel::default()]);
    let pool = bpe_water::fill_stacked(&layout, &field, Vec2::new(-150.0, 390.0), Vec2::new(150.0, 448.0), STOPS, 0.25);
    water.add(&pool);
    let middle = Vec2::new(0.0, 490.0);
    water.set_forces(&[magnet(middle, 3.0, 5, 120.0, u32::MAX)]);
    play(&mut water, 180);
    let drops = probe::liquid(&water.read()).into_owned();
    // The furthest drop from the magnet at each degree of the spread.
    let mut far = [0.0f32; 120];
    for d in &drops {
        let r = layout.to_world(Vec2::from(d.x)) - middle;
        let deg = r.x.det_atan2(-r.y).to_degrees();
        if deg.abs() < 60.0 {
            let k = ((deg + 60.0) as usize).min(119);
            far[k] = far[k].max(r.length());
        }
    }
    let tallest = far.iter().cloned().fold(0.0, f32::max);
    let mut peaks = Vec::new();
    for k in 2..118 {
        let here = far[k];
        // (A spike stands near the tallest: the valleys between are far lower.)
        if here > 0.8 * tallest && here >= far[k - 1] && here >= far[k + 1] && here > far[k - 2] && here > far[k + 2] {
            // (Tall enough over the valleys either side, within a tooth's span: 24°.)
            let left = far[k.saturating_sub(14)..k].iter().cloned().fold(f32::MAX, f32::min);
            let right = far[k + 1..(k + 15).min(120)].iter().cloned().fold(f32::MAX, f32::min);
            if here - left > 12.0 && here - right > 12.0 {
                peaks.push((k as f32 - 60.0, here));
            }
        }
    }
    assert_eq!(peaks.len(), 5, "five spikes: {peaks:?}\n{far:?}");
}

/// A magnet's mound, spikes and all, comes to rest: inside a magnet's reach the water holds itself up
/// (the deep water's pressure, left on there, kept it churning at about 100 px/s).
#[test]
fn a_magnet_holds_its_mound_still() {
    let Some(gpu) = card() else { return };
    let (layout, field) = open_box(6.0);
    let mut water = Water::new(gpu.clone(), &field, GRAVITY, Numbers::for_cell(6.0), &[Feel::default()]);
    let pool = bpe_water::fill_stacked(&layout, &field, Vec2::new(-200.0, 380.0), Vec2::new(200.0, 448.0), STOPS, 0.25);
    water.add(&pool);
    let full = magnet(Vec2::new(0.0, 490.0), 3.0, 9, 160.0, u32::MAX);
    for frame in 0..150 {
        // (Its strength rises over the first second.)
        let mut f = full;
        if let Push::Magnet { pull, .. } = &mut f.push {
            *pull *= (frame as f32 / 60.0).min(1.0);
        }
        water.set_forces(&[f]);
        water.frame();
    }
    let drops = probe::liquid(&water.read()).into_owned();
    let still = (drops.iter().map(|d| (Vec2::from(d.v) * layout.cell).length_squared()).sum::<f32>()
        / drops.len() as f32)
        .sqrt();
    assert!(still < 50.0, "the mound stands still: {still:.0} px/s");
}

/// Forces play the same twice and from a copy.
#[test]
fn forces_play_the_same_twice() {
    let Some(gpu) = card() else { return };
    let forces = [
        magnet(Vec2::new(0.0, 300.0), 2.0, 7, 180.0, u32::MAX),
        whirlpool(Vec2::new(-100.0, 100.0), 500.0, 4.0),
        wind(Vec2::new(-700.0, 0.0), 3.0),
    ];
    let start = || {
        let (layout, field) = open_box(12.0);
        let mut water = Water::new(gpu.clone(), &field, GRAVITY, Numbers::for_cell(12.0), &[Feel::default()]);
        water.add(&blob(&layout, Vec2::new(20.0, -100.0), 120.0, 0, 0));
        water.set_forces(&forces);
        water
    };
    let mut a = start();
    play(&mut a, 40);
    let mut b = start();
    play(&mut b, 20);
    let mut c = b.copy();
    assert_eq!(c.forces(), b.forces());
    play(&mut b, 20);
    play(&mut c, 20);
    let first = a.read();
    assert_eq!(probe::print_of(&first), probe::print_of(&b.read()), "twice");
    assert_eq!(probe::print_of(&first), probe::print_of(&c.read()), "from a copy");
}
