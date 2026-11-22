//! How long liquids take to step, and a print of the drops they end with (liquid speed, plan
//! `docs/superpowers/plans/liquid/faster-liquid.md`). Ignored: run it by hand before and after a
//! change to the liquid's stepping,
//!
//! `cargo test --release -p bpe-kit --test liquid_speed -- --ignored --nocapture`
//!
//! and compare the lines. A change that is only meant to make the liquid faster must leave every
//! print as it was: the same drops, to the last bit, on the same machine and driver.
//!
//! - `LIQUID_DETAILS=normal,fine` picks the Detail levels (the default is those two).
//! - `LIQUID_SECONDS=3` is how long each scene plays.
//! - `LIQUID_SCENE=<a saved scene>` times that scene too (its first Liquid thing's Detail is set).
//! - `LIQUID_ONLY=pour,box` picks scenes by name.

use std::time::Instant;

use bpe_core::{Registry, Scene, Settings, SlotId, ThingId, resolve};
use bpe_gfx::liquid_sim::Drop;
use bpe_gfx::swarm::{SwarmGpu, use_for_now};
use bpe_kit::things::liquid::{CURRENT, DRAIN, Detail, HEAT, LiquidSettings, MAGNET, MeetsSettings, state_of};
use bpe_kit::{kit, scenes};
use bpe_runtime::{Log, World};

fn details() -> Vec<Detail> {
    let asked = std::env::var("LIQUID_DETAILS").unwrap_or_else(|_| "normal,fine".into());
    asked
        .split(',')
        .map(|d| match d.trim() {
            "rough" => Detail::Rough,
            "normal" => Detail::Normal,
            "fine" => Detail::Fine,
            "very_fine" => Detail::VeryFine,
            "extreme" => Detail::Extreme,
            other => panic!("no Detail called {other}"),
        })
        .collect()
}

fn liquid_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|t| scene.things[t].kind == "liquid").expect("a liquid")
}

fn at_detail(mut scene: Scene, detail: Detail) -> Scene {
    let id = liquid_of(&scene);
    let t = scene.things.get_mut(&id).expect("the liquid");
    let mut s = LiquidSettings::from_params(&t.params);
    s.detail = detail;
    t.params = s.to_params();
    scene
}

/// Every number of a drop, as bits.
fn words(d: &Drop) -> impl Iterator<Item = u32> + '_ {
    let floats = d.x.iter().chain(&d.v).chain(&d.c).chain(&d.colour).chain([&d.j, &d.kick]).chain(&d.mat);
    floats.chain(&d.spare).chain(&d.f).chain(&d.more).map(|f| f.to_bits()).chain([d.kind, d.serial])
}

/// The water scene (a ring of water, a ball dropping in) with the stranger settings on, so the
/// print covers what they step: ice and stretch, drying to stains, a ball that the liquid clings to,
/// trails after, takes ink and heat from, and one of each force.
fn strange(reg: &Registry) -> Scene {
    let (_, mut scene) = scenes::liquid_scenes(reg).into_iter().next().expect("the water scene");
    let id = liquid_of(&scene);
    let t = scene.things.get_mut(&id).expect("the liquid");
    let mut s = LiquidSettings::from_params(&t.params);
    s.stretch = 0.5;
    s.temperature = 0.45;
    s.cools = 1.5;
    s.warms = 1.0;
    s.cold_thickens = 0.5;
    s.freezes = true;
    s.freezes_below = 0.4;
    s.melts_above = 0.6;
    s.dries = 0.3;
    s.stains = true;
    s.spray = 0.8;
    t.params = s.to_params();
    let card = scene.card_mut(id, SlotId::Bounce).expect("a Meets card");
    let mut m = MeetsSettings::from_params(&card.params);
    m.sticks = 0.5;
    m.wakes = 0.5;
    m.ink = 0.5;
    m.balls_heat = 0.5;
    m.paint = 0.5;
    card.params = m.to_params();
    for kind in [HEAT, DRAIN, CURRENT, MAGNET] {
        scene.add_layer(reg, id, SlotId::Motion, kind).expect("a force");
    }
    scene
}

#[test]
#[ignore = "a timing: run it by hand"]
fn liquid_timings() {
    let (device, queue) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
    let _card = use_for_now(Some(SwarmGpu::for_device(&device, &queue)));
    let kit = kit();
    let reg = &kit.registry;
    let seconds: f64 = std::env::var("LIQUID_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let only = std::env::var("LIQUID_ONLY").ok();
    let mut list: Vec<(String, Scene)> = Vec::new();
    // A pour onto a shape in a box (few drops at first, more each second).
    list.push(("pour".into(), scenes::liquid_pour_on_a_shape(reg)));
    // A box filled from the start (every drop there from the first step).
    let (mut filled, shape) = scenes::liquid_box(reg);
    scenes::add_liquid(&mut filled, reg, shape, |f| f.level = 0.6, |_| {});
    list.push(("box".into(), filled));
    list.extend(scenes::liquid_scenes(reg).into_iter().map(|(n, s)| (n.to_lowercase().replace(' ', "-"), s)));
    list.push(("strange".into(), strange(reg)));
    if let Ok(path) = std::env::var("LIQUID_SCENE") {
        let text = std::fs::read_to_string(&path).expect("the scene file");
        list.push(("file".into(), bpe_core::load(&text, reg).expect("a scene")));
    }
    println!("# scene\tdetail\tgrid\tdrops\tseconds\twall s\twall s a second\tprint");
    for (name, scene) in list {
        if only.as_ref().is_some_and(|o| !o.split(',').any(|n| n.trim() == name)) {
            continue;
        }
        for detail in details() {
            let scene = at_detail(scene.clone(), detail);
            let resolved = resolve(&scene, reg);
            assert!(resolved.problems.is_empty(), "{name}: {:?}", resolved.problems);
            let id = liquid_of(&scene);
            let mut world = World::build(&scene, &resolved, &kit.sims);
            let start = Instant::now();
            world.advance_to(seconds, &kit.sims, &mut Log::default());
            let st = state_of(&world, id).expect("its state");
            let host = st.host.as_ref().expect("a host");
            let body = host.body.as_ref().expect("a body on the card");
            // Reading the drops waits for every queued step.
            let drops = body.read();
            let wall = start.elapsed().as_secs_f64();
            // Every number of every drop (the liquid's own hash leaves some out).
            let print = drops
                .iter()
                .flat_map(words)
                .fold(0xcbf2_9ce4_8422_2325u64, |h, w| (h ^ u64::from(w)).wrapping_mul(0x0000_0100_0000_01b3));
            let drops = drops.len();
            let (cols, rows) = body.size();
            println!(
                "{name}\t{detail:?}\t{cols}x{rows}\t{drops}\t{seconds}\t{wall:.2}\t{:.2}\t{:016x}",
                wall / seconds,
                print
            );
        }
    }
}
