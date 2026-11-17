//! Liquid part 4 on the software graphics card: rain, balls that burst and leak into liquid, a
//! Fill of a word or picture, Un-mix by a rule, balls a liquid dissolves, shrinks and grows,
//! shapes that paint it, reactions, and every new setting replaying the same on a seek.

use std::sync::{Arc, OnceLock};

use bpe_core::{Scene, Settings, SlotId, ThingId, Value, layered_key, resolve};
use bpe_gfx::liquid_sim::Drop;
use bpe_gfx::swarm::{SwarmGpu, use_on_this_thread};
use bpe_runtime::{EventKind, Log, Playback, World};
use glam::Vec2;

use super::cards::{
    ChangesBall, CurrentLayer, DRAIN, DrainLayer, FILL, FillSettings, FillWith, MAGNET, MEETS, MagnetLayer, Meeting,
    MeetsSettings, OTHER, OtherLiquid,
};
use super::state::host_of;
use super::{CURRENT, KIND, LiquidSettings};
use crate::scenes::{add_liquid, add_pour, liquid_scenes};
use crate::{kit, registry};

fn gpu() {
    static GPU: OnceLock<Arc<SwarmGpu>> = OnceLock::new();
    let gpu = GPU.get_or_init(|| {
        let (d, q) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
        SwarmGpu::for_device(&d, &q)
    });
    use_on_this_thread(Some(gpu.clone()));
}

fn water() -> Scene {
    liquid_scenes(&registry()).into_iter().find(|(n, _)| n == "Liquid water").map(|(_, s)| s).expect("water")
}

fn liquid_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|t| scene.things[t].kind == KIND).expect("a liquid")
}

fn ball_of(scene: &Scene) -> ThingId {
    scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball")
}

fn fill(scene: &mut Scene, id: ThingId, edit: impl FnOnce(&mut FillSettings)) {
    let card = scene.card_mut(id, SlotId::Motion).expect("a Fill card");
    assert_eq!(card.kind, FILL);
    let mut f = FillSettings::from_params(&card.params);
    edit(&mut f);
    card.params = f.to_params();
}

fn meets(scene: &mut Scene, id: ThingId, edit: impl FnOnce(&mut MeetsSettings)) {
    let card = scene.card_mut(id, SlotId::Bounce).expect("a Meets card");
    assert_eq!(card.kind, MEETS);
    let mut m = MeetsSettings::from_params(&card.params);
    edit(&mut m);
    card.params = m.to_params();
}

fn own(scene: &mut Scene, id: ThingId, edit: impl FnOnce(&mut LiquidSettings)) {
    let t = scene.things.get_mut(&id).unwrap();
    let mut s = LiquidSettings::from_params(&t.params);
    edit(&mut s);
    t.params = s.to_params();
}

/// Adds a layer of `kind` to the card in `slot`, with `params`.
fn layer(scene: &mut Scene, id: ThingId, slot: SlotId, kind: &str, params: bpe_core::Params) {
    let reg = registry();
    let l = scene.add_layer(&reg, id, slot, kind).expect("a layer");
    let card = scene.card_mut(id, slot).expect("a card");
    for (k, v) in params.iter() {
        card.params.set(&layered_key(l, k), v.clone());
    }
}

fn run(scene: &Scene, time: f64) -> (World, Log) {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    (world, log)
}

fn drops(world: &World) -> Vec<Drop> {
    host_of(world).and_then(|s| s.host.as_ref()).and_then(|h| h.body.as_ref()).expect("a body").read()
}

#[test]
fn rain_falls_into_the_liquid() {
    gpu();
    let mut scene = water();
    let id = liquid_of(&scene);
    let dry = drops(&run(&scene, 1.0).0).len();
    fill(&mut scene, id, |f| {
        f.rain = 40.0;
        f.rain_size = 12.0;
    });
    let wet = drops(&run(&scene, 1.0).0).len();
    assert!(wet > dry + 40, "rain added {} drops", wet - dry);
}

#[test]
fn a_ball_that_goes_bursts_into_liquid() {
    gpu();
    let kit = kit();
    let mut scene = water();
    let id = liquid_of(&scene);
    // The ball breaks as it goes in.
    let rule = scene.add_rule("Test");
    let when = scene.add_block(&kit.registry, rule, "when.liquid", None).unwrap();
    scene.add_block(&kit.registry, rule, "do.break", Some(when)).unwrap();
    let plain = drops(&run(&scene, 3.0).0).len();
    fill(&mut scene, id, |f| f.bursts = 3.0);
    let (world, _) = run(&scene, 3.0);
    assert!(!world.bodies[&ball_of(&scene)].alive[0]);
    let burst = drops(&world).len();
    assert!(burst > plain + 50, "the ball burst into {} drops", burst - plain);
}

#[test]
fn a_moving_ball_leaks() {
    gpu();
    let mut scene = water();
    let id = liquid_of(&scene);
    let plain = drops(&run(&scene, 1.0).0).len();
    fill(&mut scene, id, |f| f.leaks = 200.0);
    let leaked = drops(&run(&scene, 1.0).0).len();
    assert!(leaked > plain + 20, "it leaked {} drops", leaked - plain);
}

#[test]
fn a_word_or_a_picture_fills_it() {
    gpu();
    let mut scene = water();
    let id = liquid_of(&scene);
    fill(&mut scene, id, |f| {
        f.fill_with = FillWith::AWord;
        f.text = "HI".into();
        f.letter_height = 200.0;
    });
    let word = drops(&run(&scene, 0.0).0);
    assert!(word.len() > 100, "{} drops in the word", word.len());
    // A red picture, 8 × 8 with its right half clear.
    let path = std::env::temp_dir().join(format!("liquid-picture-{}.png", std::process::id()));
    {
        let file = std::fs::File::create(&path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 8, 8);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        let px: Vec<u8> = (0..64).flat_map(|i| if i % 8 < 4 { [255, 0, 0, 255] } else { [0, 0, 0, 0] }).collect();
        w.write_image_data(&px).unwrap();
    }
    fill(&mut scene, id, |f| {
        f.fill_with = FillWith::APicture;
        f.picture = path.to_string_lossy().into_owned();
        f.picture_width = 200.0;
    });
    let picture = drops(&run(&scene, 0.0).0);
    let _ = std::fs::remove_file(&path);
    assert!(picture.len() > 100, "{} drops in the picture", picture.len());
    assert!(picture.iter().all(|d| d.colour[0] > 0.9 && d.colour[1] < 0.05), "in the picture's red");
    // Only its left half: the drops are left of the thing's place.
    let host = host_of(&run(&scene, 0.0).0).unwrap().host.as_ref().unwrap().layout;
    let place = host.to_grid(Vec2::ZERO);
    assert!(picture.iter().all(|d| d.x[0] < place.x + 1.0));
}

/// The drops' mean height (grid y).
fn mean_y(d: &[Drop]) -> f32 {
    d.iter().map(|d| d.x[1]).sum::<f32>() / d.len().max(1) as f32
}

#[test]
fn a_rule_un_mixes_the_liquid_back_to_where_it_was() {
    gpu();
    let kit = kit();
    let mut scene = water();
    let id = liquid_of(&scene);
    // A word high up falls; a second later a rule plays it back.
    fill(&mut scene, id, |f| {
        f.fill_with = FillWith::AWord;
        f.text = "UP".into();
    });
    own(&mut scene, id, |s| {
        s.remember = true;
        s.unmix_seconds = 1.0;
    });
    let start = mean_y(&drops(&run(&scene, 0.0).0));
    let fallen = mean_y(&drops(&run(&scene, 1.0).0));
    assert!(fallen > start + 5.0, "it fell ({start} to {fallen})");
    let rule = scene.add_rule("Un-mix");
    let when = scene.add_block(&kit.registry, rule, "when.time", None).unwrap();
    scene.set_block_param(&kit.registry, rule, when, "at", Value::Number(1.0)).unwrap();
    scene.add_block(&kit.registry, rule, "do.unmix", Some(when)).unwrap();
    let (world, _) = run(&scene, 1.95);
    let back = mean_y(&drops(&world));
    assert!((back - start).abs() < (fallen - start) * 0.25, "played back to {back} (from {start}, fell to {fallen})");
    // And it goes on from there.
    let later = mean_y(&drops(&run(&scene, 2.6).0));
    assert!(later > back + 1.0, "it falls again ({back} to {later})");
}

#[test]
fn a_liquid_dissolves_shrinks_and_grows_balls() {
    gpu();
    let mut scene = water();
    let (id, ball) = (liquid_of(&scene), ball_of(&scene));
    let start = run(&scene, 0.0).0.bodies[&ball].radius[0];
    let with = |scene: &mut Scene, changes: ChangesBall, speed: f32| {
        meets(scene, id, |m| {
            m.ball_weight = 3.0;
            m.changes = changes;
            m.change_speed = speed;
        });
        run(scene, 4.0)
    };
    let (world, _) = with(&mut scene, ChangesBall::Shrinks, 1.0);
    let r = world.bodies[&ball].radius[0];
    assert!(r < start * 0.8 && r >= start * 0.29, "shrank to {r} from {start}");
    let (world, _) = with(&mut scene, ChangesBall::Grows, 1.0);
    let r = world.bodies[&ball].radius[0];
    assert!(r > start * 1.2 && r <= start * 3.01, "grew to {r} from {start}");
    let (world, log) = with(&mut scene, ChangesBall::Dissolves, 4.0);
    assert!(!world.bodies[&ball].alive[0], "it dissolved");
    assert!(log.events().iter().any(|e| e.kind == EventKind::Remove && e.a.thing == ball));
}

#[test]
fn shapes_paint_the_liquid_they_touch() {
    gpu();
    let mut scene = water();
    let id = liquid_of(&scene);
    meets(&mut scene, id, |m| {
        m.paint = 1.0;
        m.paint_colour = bpe_core::Color::rgb(255, 0, 0);
    });
    let d = drops(&run(&scene, 1.0).0);
    let red = d.iter().filter(|d| d.colour[0] > d.colour[2]).count();
    assert!(red > 50, "{red} drops took the ring's paint");
    assert!(red < d.len(), "the middle stays water");
}

#[test]
fn two_liquids_react_into_a_third() {
    gpu();
    let reg = registry();
    let mut scene = water();
    let first = liquid_of(&scene);
    let ring = scene.things[&first].parent.unwrap();
    let second = add_liquid(&mut scene, &reg, ring, |f| f.fill = false, |_| {});
    add_pour(&mut scene, &reg, second, |p| {
        p.place = Vec2::new(0.0, -300.0);
        p.colour = crate::paint::Paint::solid(bpe_core::Color::rgb(255, 0, 0));
    });
    let third = add_liquid(
        &mut scene,
        &reg,
        ring,
        |f| {
            f.fill = false;
            f.colour = crate::paint::Paint::solid(bpe_core::Color::rgb(0, 255, 0));
        },
        |_| {},
    );
    let rule = OtherLiquid {
        liquid: bpe_core::Target::Thing(second),
        meeting: Meeting::React,
        makes: bpe_core::Target::Thing(third),
        react_speed: 1.0,
        heat: 0.5,
        fizz: 0.3,
        ..OtherLiquid::default()
    };
    layer(&mut scene, first, SlotId::Bounce, OTHER, rule.to_params());
    let (world, _) = run(&scene, 2.0);
    let made = drops(&world).iter().filter(|d| d.liquid() == 2).count();
    assert!(made > 20, "{made} drops reacted into the third liquid");
}

#[test]
fn every_new_setting_plays_the_same_after_a_seek() {
    gpu();
    let kit = kit();
    let mut scene = water();
    let id = liquid_of(&scene);
    own(&mut scene, id, |s| {
        s.stretch = 0.3;
        s.cools = 0.5;
        s.cold_thickens = 0.5;
        s.freezes = true;
        s.dries = 0.2;
        s.stains = true;
        s.remember = true;
    });
    fill(&mut scene, id, |f| {
        f.rain = 20.0;
        f.leaks = 30.0;
    });
    meets(&mut scene, id, |m| {
        m.changes = ChangesBall::Shrinks;
        m.balls_heat = 1.0;
        m.paint = 0.5;
    });
    layer(&mut scene, id, SlotId::Motion, MAGNET, MagnetLayer { spikes: 5, ..MagnetLayer::default() }.to_params());
    layer(&mut scene, id, SlotId::Motion, DRAIN, DrainLayer::default().to_params());
    layer(&mut scene, id, SlotId::Motion, CURRENT, CurrentLayer::default().to_params());
    let resolved = resolve(&scene, &kit.registry);
    let hash = |p: &Playback| super::hash_of(host_of(p.world()).unwrap());
    let mut a = Playback::new(&scene, &resolved, kit.sims.clone());
    a.seek(2.0);
    let mut b = Playback::new(&scene, &resolved, kit.sims.clone());
    for t in [1.5, 0.5, 2.0] {
        b.seek(t);
    }
    assert_eq!(hash(&a), hash(&b), "a seek gives the same drops");
    assert_eq!(a.world().bodies, b.world().bodies);
    assert!(drops(a.world()).iter().all(|d| d.x.iter().all(|x| x.is_finite())));
}
