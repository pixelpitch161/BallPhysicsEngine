//! Tests of the Liquid thing on the software graphics card (liquid part 1).

use std::sync::{Arc, OnceLock};

use bpe_core::{Scene, Settings, SlotId, ThingId, Value, resolve};
use bpe_gfx::liquid_sim::Drop;
use bpe_gfx::swarm::{SwarmGpu, use_on_this_thread};
use bpe_runtime::{Log, World};
use glam::Vec2;

use super::state::{Host, LiquidState, host_of, state_of};
use super::{KIND, LiquidSettings};
use crate::scenes::{add_liquid, add_pour, liquid_scenes};
use crate::{kit, registry};

/// The software card, named on the calling thread.
fn gpu() {
    static GPU: OnceLock<Arc<SwarmGpu>> = OnceLock::new();
    let gpu = GPU.get_or_init(|| {
        let (d, q) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
        SwarmGpu::for_device(&d, &q)
    });
    use_on_this_thread(Some(gpu.clone()));
}

fn scene(name: &str) -> Scene {
    liquid_scenes(&registry()).into_iter().find(|(n, _)| n == name).map(|(_, s)| s).expect("a liquid scene")
}

fn build(scene: &Scene) -> World {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    World::build(scene, &resolved, &kit.sims)
}

fn run(scene: &Scene, time: f64) -> World {
    let kit = kit();
    let mut world = build(scene);
    world.advance_to(time, &kit.sims, &mut Log::default());
    world
}

fn host(world: &World) -> &Host {
    host_of(world).and_then(|s| s.host.as_ref()).expect("a host")
}

fn drops(world: &World) -> Vec<Drop> {
    host(world).body.as_ref().expect("a card").read()
}

/// `d`'s place in the world.
fn place(world: &World, d: &Drop) -> Vec2 {
    host(world).layout.to_world(Vec2::from(d.x))
}

fn liquid_id(scene: &Scene) -> ThingId {
    scene.things.values().find(|t| t.kind == KIND).map(|t| t.id).expect("a liquid")
}

fn set_own(scene: &mut Scene, id: ThingId, edit: impl FnOnce(&mut LiquidSettings)) {
    let t = scene.things.get_mut(&id).expect("a liquid");
    let mut s = LiquidSettings::from_params(&t.params);
    edit(&mut s);
    t.params = s.to_params();
}

#[test]
fn it_fills_the_ring_up_to_its_level_and_stays_in() {
    gpu();
    let scene = scene("Liquid water");
    let world = build(&scene);
    let first = drops(&world);
    assert!(first.len() > 1000, "{} drops", first.len());
    // The ring is 508 round its middle, 3 thick: every drop inside, below the level.
    let level_y = 508.0 - 2.0 * 508.0 * 0.4;
    for d in &first {
        let p = place(&world, d);
        assert!(p.length() < 508.0, "a drop outside the ring at {p}");
        assert!(p.y > level_y - 8.0, "a drop above the level at {p}");
    }
    let world = run(&scene, 1.5);
    let later = drops(&world);
    assert_eq!(later.len(), first.len(), "no drop is lost in a closed ring");
    for d in &later {
        assert!(d.x.iter().chain(&d.v).all(|v| v.is_finite()));
        let p = place(&world, d);
        assert!(p.length() < 508.0 + 2.0, "a drop through the ring at {p}");
    }
}

#[test]
fn two_runs_are_the_same_and_a_copy_runs_on_alike() {
    gpu();
    let scene = scene("Liquid paint mixing");
    let kit = kit();
    let a = run(&scene, 1.0);
    let mut b = run(&scene, 0.5);
    let mut copy = b.clone();
    b.advance_to(1.0, &kit.sims, &mut Log::default());
    copy.advance_to(1.0, &kit.sims, &mut Log::default());
    let ha = super::hash_of(host_of(&a).unwrap());
    assert_ne!(ha, 0);
    assert_eq!(ha, super::hash_of(host_of(&b).unwrap()), "two runs differ");
    assert_eq!(ha, super::hash_of(host_of(&copy).unwrap()), "a copy runs differently");
}

#[test]
fn blue_paint_poured_into_yellow_paint_makes_green() {
    gpu();
    let world = run(&scene("Liquid paint mixing"), 2.5);
    let all = drops(&world);
    let greens = all.iter().filter(|d| d.colour[1] > d.colour[0] && d.colour[1] > d.colour[2] * 1.05).count();
    assert!(greens > all.len() / 50, "{greens} green drops of {}", all.len());
    // Some poured in.
    let st = state_of(&world, liquid_id(&scene("Liquid paint mixing"))).unwrap();
    assert!(st.born as usize > all.len() / 2);
}

#[test]
fn balls_push_it_aside_unless_told_not_to() {
    gpu();
    let mut scene = scene("Liquid water");
    let ball = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).unwrap();
    let id = liquid_id(&scene);
    let count_in_ball = |scene: &Scene| {
        let world = run(scene, 2.0);
        let b = &world.bodies[&ball];
        let (at, r) = (b.position[0], b.radius[0]);
        drops(&world).iter().filter(|d| (place(&world, d) - at).length() < r * 0.8).count()
    };
    assert_eq!(count_in_ball(&scene), 0, "drops inside the ball");
    let card = scene.card_mut(id, SlotId::Bounce).unwrap();
    card.params.set("balls_push", Value::Bool(false));
    // The ball falls into the water and sinks through it: some drops are where it is.
    assert!(count_in_ball(&scene) > 0, "the ball pushed the liquid though told not to");
}

#[test]
fn without_a_card_nothing_breaks() {
    use_on_this_thread(None);
    let scene = scene("Liquid water");
    let world = run(&scene, 0.5);
    let st = host_of(&world).expect("a host");
    assert!(st.host.as_ref().unwrap().body.is_none());
    assert_eq!(super::hash_of(st), 0);
}

#[test]
fn numbers_that_arent_numbers_give_a_still_liquid() {
    gpu();
    let mut scene = scene("Liquid water");
    let id = liquid_id(&scene);
    set_own(&mut scene, id, |s| {
        s.thickness = f32::NAN;
        s.weight = f32::INFINITY;
        s.gravity = f32::NAN;
        s.settles = f32::NEG_INFINITY;
        s.margin = f32::NAN;
    });
    let world = run(&scene, 0.5);
    for d in drops(&world) {
        assert!(d.x.iter().chain(&d.v).chain(&d.colour).all(|v| v.is_finite()), "{d:?}");
    }
}

#[test]
fn most_drops_keeps_the_newest_and_a_lower_level_cuts_it() {
    gpu();
    let reg = registry();
    let mut scene = crate::new_scene(&reg);
    let ring = scene.things.values().find(|t| t.kind == "shape").map(|t| t.id).unwrap();
    let id = add_liquid(
        &mut scene,
        &reg,
        ring,
        |f| {
            f.fill = false;
            f.most = 2000;
        },
        |_| {},
    );
    add_pour(&mut scene, &reg, id, |p| {
        p.width = 60.0;
        p.speed = 600.0;
    });
    let world = run(&scene, 2.0);
    let all = drops(&world);
    let st: &LiquidState = state_of(&world, id).unwrap();
    assert!(st.born > 2000, "it poured {}", st.born);
    assert!(all.len() <= 2000, "{} drops kept", all.len());
    assert!(all.iter().all(|d| d.serial >= st.born - 2000));
}

#[test]
fn a_lower_level_cuts_the_liquid_and_a_higher_one_adds_to_it() {
    gpu();
    let mut scene = scene("Liquid water");
    let id = liquid_id(&scene);
    let kit = kit();
    let mut world = run(&scene, 0.2);
    let before = drops(&world).len();
    // Lower: the world reads the new Level as a key would give it.
    let set_level = |scene: &mut Scene, level: f64| {
        scene.card_mut(id, SlotId::Motion).unwrap().params.set("level", Value::Number(level));
    };
    set_level(&mut scene, 0.2);
    let resolved = resolve(&scene, &kit.registry);
    kit.sims.thing(KIND).unwrap().settings_changed(id, &resolved.things[&id], &mut world);
    world.advance_to(0.4, &kit.sims, &mut Log::default());
    let lower = drops(&world).len();
    assert!(lower < before * 3 / 4, "{lower} of {before} left");
    set_level(&mut scene, 0.5);
    let resolved = resolve(&scene, &kit.registry);
    kit.sims.thing(KIND).unwrap().settings_changed(id, &resolved.things[&id], &mut world);
    world.advance_to(0.6, &kit.sims, &mut Log::default());
    assert!(drops(&world).len() > before, "a higher level adds drops");
}

#[test]
fn every_liquids_most_drops_together_stay_under_the_cards_room() {
    use super::state::{MOST_BORN_A_STEP, MOST_KEPT, share_most};
    let mut states: Vec<LiquidState> = (0..3)
        .map(|k| {
            let mut s = LiquidState::new(ThingId(10 + k), None);
            s.index = Some(k as usize);
            s.fill.most = 800_000;
            s
        })
        .collect();
    share_most(states.iter_mut());
    let kept: u32 = states.iter().map(|s| s.kept).sum();
    assert!(kept <= MOST_KEPT, "{kept}");
    assert!(states.iter().all(|s| s.kept == states[0].kept), "shared evenly");
    assert!(kept as usize + MOST_BORN_A_STEP < bpe_gfx::liquid_sim::MOST_DROPS as usize);
    // Under the limit, each keeps its own.
    states[0].fill.most = 1000;
    states[1].fill.most = 2000;
    states[2].fill.most = 3000;
    share_most(states.iter_mut());
    assert_eq!(states.iter().map(|s| s.kept).collect::<Vec<_>>(), [1000, 2000, 3000]);
}

/// Every drop's speed in pixels a second.
fn speeds(world: &World) -> Vec<f32> {
    let cell = host(world).layout.cell;
    drops(world).iter().map(|d| (Vec2::from(d.v) * cell).length()).collect()
}

/// How many drops are on exactly the same spot as another.
fn stacked(all: &[Drop]) -> usize {
    let mut at: std::collections::HashMap<(u32, u32), usize> = Default::default();
    for d in all {
        *at.entry((d.x[0].to_bits(), d.x[1].to_bits())).or_default() += 1;
    }
    at.values().filter(|n| **n > 1).sum()
}

#[test]
fn still_water_in_a_box_comes_to_rest_without_bursting_or_packing_against_the_floor() {
    gpu();
    let (mut scene, shape) = crate::scenes::liquid_box(&registry());
    add_liquid(&mut scene, &registry(), shape, |f| f.level = 0.3, |_| {});
    let kit = kit();
    let mut world = build(&scene);
    let top = |world: &World| drops(world).iter().map(|d| place(world, d).y).fold(f32::MAX, f32::min);
    let level = top(&world);
    let mut highest = level;
    for quarter in 1..=14 {
        world.advance_to(quarter as f64 * 0.25, &kit.sims, &mut Log::default());
        highest = highest.min(top(&world));
    }
    let all = drops(&world);
    assert!(all.len() > 1000, "{} drops", all.len());
    let v = speeds(&world);
    let rms = (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
    // Drops in each half-cell layer up from the floor's inner face (it is at 448.5), over the
    // middle 400 px, clear of the side walls.
    let cell = host(&world).layout.cell;
    let mut layers = [0usize; 16];
    for p in all.iter().map(|d| place(&world, d)).filter(|p| p.x.abs() < 200.0) {
        let layer = ((448.5 - p.y) / cell * 2.0).floor();
        if (0.0..16.0).contains(&layer) {
            layers[layer as usize] += 1;
        }
    }
    let fullest = *layers[..4].iter().max().expect("four layers") as f32;
    let above = layers[8..].iter().sum::<usize>() as f32 / 8.0;
    eprintln!(
        "still water: threw {:.0} px up, rms {rms:.1} px/s, {} of {} stacked, fullest layer on the floor {:.2} of one above",
        level - highest,
        stacked(&all),
        all.len(),
        fullest / above
    );
    // It once burst upwards by itself after three seconds, throwing drops 236 px.
    assert!(level - highest < 80.0, "still water threw a drop {:.0} px up", level - highest);
    // And simmered at 45 to 50 px/s for good.
    assert!(rms < 25.0, "still water still moves at {rms:.1} px/s");
    // A twentieth of its drops sat on exactly the same spots along the walls.
    assert!(stacked(&all) * 200 < all.len(), "{} of {} drops share a spot", stacked(&all), all.len());
    // And it packed against the floor: the drop line there held over three times the drops of a
    // layer higher up (two rows can share a layer, so 2 is no packing at all).
    assert!(fullest < above * 2.6, "the floor's fullest layer holds {fullest} drops, one higher up {above}");
}

#[test]
fn water_poured_onto_a_shape_is_never_thrown_faster_than_it_could_fall() {
    gpu();
    let scene = crate::scenes::liquid_pour_on_a_shape(&registry());
    let kit = kit();
    let mut world = build(&scene);
    // The fastest a drop can fairly go: its speed out of the spout (300 px/s × √Size), then
    // falling all the way to the box's floor (the scene's gravity times the liquid's Size).
    let (mut fastest, mut limit) = (0.0f32, 0.0f32);
    for frame in 1..=150 {
        world.advance_to(frame as f64 / 60.0, &kit.sims, &mut Log::default());
        let fall = 2.0 * world.gravity.length() * host(&world).size * (450.0 + 380.0);
        limit = (300.0f32 * 300.0 * host(&world).size + fall).sqrt();
        fastest = speeds(&world).into_iter().fold(fastest, f32::max);
    }
    eprintln!("pour: fastest {fastest:.0} px/s, a fall gives {limit:.0}");
    assert!(drops(&world).len() > 1000);
    // It once threw drops at 18,000 px/s.
    assert!(fastest < limit * 1.5, "a drop was thrown at {fastest:.0} px/s; falling gives at most {limit:.0}");
}

/// The middle of all the drops, across (pixels).
fn middle_x(world: &World) -> f32 {
    let all = drops(world);
    all.iter().map(|d| place(world, d).x).sum::<f32>() / all.len().max(1) as f32
}

#[test]
fn a_slosh_in_a_box_keeps_swinging() {
    gpu();
    let (mut scene, shape) = crate::scenes::liquid_box(&registry());
    add_liquid(&mut scene, &registry(), shape, |f| f.level = 0.3, |_| {});
    let kit = kit();
    let mut world = build(&scene);
    let down = world.gravity;
    // Pulled sideways for 0.4 s, then left alone: where its middle is every twentieth of a second.
    let mut swing = Vec::new();
    for k in 1..=90 {
        let t = k as f64 * 0.05;
        world.gravity = if t <= 0.4 { down + Vec2::new(250.0, 0.0) } else { down };
        world.advance_to(t, &kit.sims, &mut Log::default());
        swing.push((t as f32, middle_x(&world)));
    }
    // Each turn of the swing (a furthest point either side), once the pull has stopped.
    let mut turns = Vec::new();
    for w in swing.windows(3) {
        let (a, (t, b), c) = (w[0].1, w[1], w[2].1);
        if t > 0.45 && b.abs() > 0.8 && ((b > a && b >= c) || (b < a && b <= c)) {
            turns.push((t, b));
        }
    }
    eprintln!("slosh turns: {turns:?}");
    assert!(turns.len() >= 5, "it swung {} times and stopped: {turns:?}", turns.len());
    // Water this deep in a box this wide swings once every 1.46 s (a half swing every 0.73).
    let half = (turns[3].0 - turns[0].0) / 3.0;
    assert!((0.6..0.86).contains(&half), "a half swing takes {half:.2} s");
    // Each swing is nearly as wide as the one before (the first loses most, to ripples). It once
    // kept two thirds, then 0.59, then under a half, and was dead by the fifth: a rounding error
    // in the grid's sums braked slow liquid like friction.
    for pair in turns[..5].windows(2) {
        let kept = (pair[1].1 / pair[0].1).abs();
        assert!(kept > 0.7, "a swing kept only {kept:.2} of the one before: {turns:?}");
    }
    let left = (turns[4].1 / turns[0].1).abs();
    assert!(left > 0.4, "by its fifth turn the swing was down to {left:.2} of its first: {turns:?}");
}

#[test]
fn still_water_rests_wherever_the_walls_sit_on_the_grid() {
    gpu();
    // The box's width and its walls' thickness: each puts the walls' faces somewhere else between
    // the grid's nodes. Without the friction of the grid's old rounding, still water simmered or
    // threw drops at some of these and rested at others, so each Detail level (which moves the
    // grid) had its own; and "holes close" stirred it where it had no holes to close.
    for (width, wall) in [(594.0, 3.0), (604.0, 3.0), (600.0, 36.0)] {
        let (mut scene, shape) = crate::scenes::liquid_box(&registry());
        scene.things.get_mut(&shape).expect("the box").params =
            crate::ShapeSettings::rectangle(width, 900.0, wall).to_params();
        add_liquid(&mut scene, &registry(), shape, |f| f.level = 0.3, |_| {});
        let kit = kit();
        let mut world = build(&scene);
        let top = |world: &World| drops(world).iter().map(|d| place(world, d).y).fold(f32::MAX, f32::min);
        let level = top(&world);
        let mut highest = level;
        for quarter in 1..=20 {
            world.advance_to(quarter as f64 * 0.25, &kit.sims, &mut Log::default());
            highest = highest.min(top(&world));
        }
        let all = drops(&world);
        let v = speeds(&world);
        let rms = (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
        eprintln!(
            "still water, box {width} wide, walls {wall}: threw {:.0} px up, rms {rms:.1} px/s, {} of {} stacked",
            level - highest,
            stacked(&all),
            all.len()
        );
        assert!(all.len() > 1000, "{} drops", all.len());
        // (A Fill starts with a small jolt, as its first rows are not laid to fit the walls: a
        // drop or two rise 30 to 70 px in the first second. A burst threw them 236.)
        assert!(level - highest < 120.0, "box {width}, walls {wall}: threw a drop {:.0} px up", level - highest);
        assert!(rms < 25.0, "box {width}, walls {wall}: still water moves at {rms:.1} px/s");
        assert!(stacked(&all) * 200 < all.len(), "box {width}, walls {wall}: {} drops share a spot", stacked(&all));
    }
}
