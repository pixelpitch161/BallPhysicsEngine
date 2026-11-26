//! Trapped air (part 2c, `docs/superpowers/plans/water-mode/part2c-air-and-wetting.md`): air drops
//! rise, keep a pocket's room and burst at the top; the card finds air trapped under the water and
//! fills it with them; where there is none, nothing changes.

use std::sync::Arc;

use bpe_water::probe::{self, Stats};
use bpe_water::scenes;
use bpe_water::{Drop, Layout, Numbers, WaterGpu};
use glam::Vec2;

fn card() -> Option<Arc<WaterGpu>> {
    let (device, queue) = bpe_gfx::test_device().ok()?;
    Some(WaterGpu::new(&device, &queue))
}

/// The air drops' world places.
fn air_at(drops: &[Drop], layout: &Layout) -> Vec<Vec2> {
    probe::air(drops).iter().map(|d| layout.to_world(Vec2::from(d.x))).collect()
}

fn mean(at: &[Vec2]) -> Vec2 {
    at.iter().copied().sum::<Vec2>() / at.len().max(1) as f32
}

/// The mean height of the liquid's drops (world y, down), and how many there are.
fn liquid_middle(drops: &[Drop], layout: &Layout) -> (f32, usize) {
    let water = probe::liquid(drops);
    let sum: f64 = water.iter().map(|d| f64::from(layout.to_world(Vec2::from(d.x)).y)).sum();
    ((sum / water.len().max(1) as f64) as f32, water.len())
}

/// A pocket of air in a still pool rises every quarter second and is gone by three seconds; the
/// water is kept, and comes down into the pocket's room.
#[test]
fn air_rises_and_bursts() {
    let Some(gpu) = card() else { return };
    let scene = scenes::pool_with_hole(12.0, true);
    // (Nothing is found and born: only the air put there.)
    let mut run = scene.start(&gpu, Numbers { air: 0, ..Numbers::for_cell(12.0) });
    let layout = run.water.layout();
    let first = run.water.read();
    let (middle, water) = liquid_middle(&first, &layout);
    let mut was = air_at(&first, &layout);
    assert!(was.len() > 80, "the pocket holds air: {}", was.len());
    // (Once the air has gone and the pool is level again, the liquid's middle is where the liquid
    // and the air's was, less half the air's room spread over the pool's width.)
    let air = was.len() as f32;
    let width = 2.0 * (300.0 - 6.0);
    let all = (middle * water as f32 + mean(&was).y * air) / (water as f32 + air);
    let level = all + air * 0.25 * 144.0 / (2.0 * width);
    let mut gone = None;
    for k in 1..=12 {
        run.run_to(0.25 * k as f32);
        let drops = run.water.read();
        assert_eq!(probe::liquid(&drops).len(), water, "the water is kept at {} s", 0.25 * k as f32);
        let air = air_at(&drops, &layout);
        if air.is_empty() {
            gone = Some(k);
            break;
        }
        if air.len() * 2 > was.len() {
            assert!(
                mean(&air).y < mean(&was).y - 2.0,
                "the air rises at {} s: {} then {}",
                0.25 * k as f32,
                mean(&was),
                mean(&air)
            );
        }
        was = air;
    }
    assert!(gone.is_some(), "the air is gone by 3 s: {} drops left", was.len());
    run.run_to(3.0);
    let (after, _) = liquid_middle(&run.water.read(), &layout);
    eprintln!(
        "gone at {} s; the liquid's middle {middle}, at 3 s {after}, level {level}",
        0.25 * gone.unwrap_or(0) as f32
    );
    assert!(
        (after - level).abs() < 0.4 * (level - middle),
        "the water comes down into the pocket's room: its middle from {middle} to {after} ({level} level)"
    );
}

/// The liquid's skin leaves air out, so a pocket's wall is a surface: a stronger skin holds a bubble
/// whole for longer as it rises (real bubbles hold together by their water's skin).
#[test]
fn a_stronger_skin_holds_a_bubble_whole_longer() {
    let Some(gpu) = card() else { return };
    let whole_at = |surface_pull: f32| {
        let mut scene = scenes::pool_with_hole(12.0, true);
        scene.feels[0].surface_pull = surface_pull;
        let mut run = scene.start(&gpu, Numbers { air: 0, ..Numbers::for_cell(12.0) });
        let start = probe::pockets(&run.water.read(), &run.field).biggest;
        run.run_to(0.5);
        let p = probe::pockets(&run.water.read(), &run.field);
        eprintln!("Surface pull {surface_pull}: the pocket {start} px2, at 0.5 s {p:?}");
        p.biggest / start
    };
    let (weak, strong) = (whole_at(0.2), whole_at(1.0));
    assert!(strong > weak + 0.2, "a stronger skin keeps more of the pocket whole: {weak} and {strong}");
}

#[test]
#[ignore]
fn trace_air() {
    let Some(gpu) = card() else { return };
    let cell: f32 = std::env::var("WATER_CELL").ok().and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let mut scene = scenes::pool_with_hole(cell, true);
    if let Some(sp) = std::env::var("WATER_PULL").ok().and_then(|s| s.parse().ok()) {
        scene.feels[0].surface_pull = sp;
    }
    let mut run = scene.start(&gpu, Numbers { air: 0, ..Numbers::from_env_for(cell) });
    let layout = run.water.layout();
    for k in 0..=24 {
        run.run_to(0.125 * k as f32);
        let drops = run.water.read();
        let air = air_at(&drops, &layout);
        let m = mean(&air);
        let spread = |f: fn(Vec2) -> f32| {
            (air.iter()
                .map(|p| {
                    let d = f(*p - m);
                    d * d
                })
                .sum::<f32>()
                / air.len().max(1) as f32)
                .sqrt()
        };
        let s = Stats::of(&drops, &layout);
        let p = probe::pockets(&drops, &run.field);
        eprintln!(
            "{:5.3} s air {:5} mid ({:7.1} {:7.1}) spread {:5.1} x {:5.1} | pockets {:6.0} px2, biggest {:6.0} at {:7.1} | water {} top {:.1} rms {:.0}",
            0.125 * k as f32,
            air.len(),
            m.x,
            m.y,
            spread(|p| p.x),
            spread(|p| p.y),
            p.area,
            p.biggest,
            p.middle.y,
            probe::liquid(&drops).len(),
            s.top,
            s.rms
        );
    }
}
