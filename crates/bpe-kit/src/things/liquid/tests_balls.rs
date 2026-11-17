//! Liquid part 2 on the software graphics card: the liquid pushes balls back, slows and carries
//! them, and tells the rules when they go in, come out and splash.

use std::sync::{Arc, OnceLock};

use bpe_core::{Scene, Settings, SlotId, ThingId, Value, resolve};
use bpe_gfx::swarm::{SwarmGpu, use_on_this_thread};
use bpe_runtime::{EventKind, Log, Playback, World, main_part};
use glam::Vec2;

use super::cards::{Floats, MEETS, MeetsSettings};
use super::feel::{ENTER, SPLASH};
use super::state::host_of;
use super::{KIND, LiquidSettings};
use crate::scenes::{add_liquid, liquid_scenes};
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

fn meets(scene: &mut Scene, id: ThingId, edit: impl FnOnce(&mut MeetsSettings)) {
    let card = scene.card_mut(id, SlotId::Bounce).expect("a Meets card");
    assert_eq!(card.kind, MEETS);
    let mut m = MeetsSettings::from_params(&card.params);
    edit(&mut m);
    card.params = m.to_params();
}

/// The world `scene` builds, stepped to `time`, and its log.
fn run(scene: &Scene, time: f64) -> (World, Log) {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(time, &kit.sims, &mut log);
    (world, log)
}

fn ball_at(world: &World, ball: ThingId) -> (Vec2, f32) {
    let b = &world.bodies[&ball];
    (b.position[0], b.radius[0])
}

/// Where the water's top is in the scene's ring (508 round its middle) filled to 0.4.
const SURFACE_Y: f32 = 508.0 - 2.0 * 508.0 * 0.4;

#[test]
fn a_liquid_that_pushes_balls_steps_with_the_physics() {
    let kit = kit();
    let mut scene = water();
    let id = liquid_of(&scene);
    let resolved = resolve(&scene, &kit.registry);
    assert!(main_part(&scene, &resolved, &kit.sims).things.contains_key(&id), "in the main world");
    meets(&mut scene, id, |m| m.pushes_balls = false);
    let resolved = resolve(&scene, &kit.registry);
    assert!(!main_part(&scene, &resolved, &kit.sims).things.contains_key(&id), "beside");
}

#[test]
fn one_liquid_pushing_balls_keeps_every_liquid_with_it() {
    let kit = kit();
    let reg = registry();
    let mut scene = water();
    let first = liquid_of(&scene);
    meets(&mut scene, first, |m| m.pushes_balls = false);
    let second = add_liquid(&mut scene, &reg, ThingId::ROOT, |f| f.level = 0.1, |_| {});
    let resolved = resolve(&scene, &kit.registry);
    let main = main_part(&scene, &resolved, &kit.sims);
    assert!(main.things.contains_key(&first) && main.things.contains_key(&second));
    meets(&mut scene, second, |m| m.pushes_balls = false);
    let resolved = resolve(&scene, &kit.registry);
    let main = main_part(&scene, &resolved, &kit.sims);
    assert!(!main.things.contains_key(&first) && !main.things.contains_key(&second));
}

#[test]
fn a_light_ball_floats_and_a_heavy_one_sinks() {
    gpu();
    let mut scene = water();
    let (id, ball) = (liquid_of(&scene), ball_of(&scene));
    meets(&mut scene, id, |m| m.ball_weight = 0.3);
    let (world, _) = run(&scene, 5.0);
    let (light, r) = ball_at(&world, ball);
    meets(&mut scene, id, |m| m.ball_weight = 3.0);
    let (world, _) = run(&scene, 5.0);
    let (heavy, _) = ball_at(&world, ball);
    eprintln!("light at {light}, heavy at {heavy}, radius {r}, surface {SURFACE_Y}");
    assert!((light.y - SURFACE_Y).abs() < 2.5 * r, "the light ball floats at the top: {light}");
    assert!(heavy.y > SURFACE_Y + 4.0 * r, "the heavy ball sinks: {heavy}");
    // Floats and Sinks do so whatever Ball weight says.
    meets(&mut scene, id, |m| m.floats = Floats::Floats);
    let (world, _) = run(&scene, 5.0);
    assert!(ball_at(&world, ball).0.y < SURFACE_Y + 2.5 * r);
}

#[test]
fn the_liquid_slows_a_ball_and_a_thick_one_slows_it_more() {
    gpu();
    let mut scene = water();
    let (id, ball) = (liquid_of(&scene), ball_of(&scene));
    meets(&mut scene, id, |m| m.ball_weight = 3.0);
    let speed = |scene: &Scene| {
        let (world, _) = run(scene, 3.0);
        world.bodies[&ball].velocity[0].length()
    };
    let thin = speed(&scene);
    let t = scene.things.get_mut(&id).unwrap();
    let mut s = LiquidSettings::from_params(&t.params);
    s.thickness = 0.8;
    t.params = s.to_params();
    let thick = speed(&scene);
    eprintln!("sinking at {thin} in water, {thick} in thick liquid");
    assert!(thick < thin, "{thick} not slower than {thin}");
}

#[test]
fn a_dropped_ball_goes_in_and_splashes() {
    gpu();
    let scene = water();
    let (id, ball) = (liquid_of(&scene), ball_of(&scene));
    let (_, log) = run(&scene, 3.0);
    let ins: Vec<_> = log.events().iter().filter(|e| e.kind == EventKind::Liquid).collect();
    eprintln!("{ins:?}");
    let went_in = ins.iter().find(|e| e.b.is_some_and(|b| b.thing == id && b.item == ENTER)).expect("it went in");
    assert_eq!(went_in.a.thing, ball);
    assert!(went_in.speed > 0.0);
    assert!(ins.iter().any(|e| e.b.is_some_and(|b| b.item == SPLASH)), "it fell in fast enough to splash");
}

#[test]
fn pushes_balls_off_leaves_the_balls_as_without_the_liquid() {
    gpu();
    let kit = kit();
    let mut scene = water();
    let id = liquid_of(&scene);
    meets(&mut scene, id, |m| m.pushes_balls = false);
    let resolved = resolve(&scene, &kit.registry);
    let mut whole = Playback::new(&scene, &resolved, kit.sims.clone());
    whole.seek(3.0);
    let mut alone = Playback::new(&scene, &main_part(&scene, &resolved, &kit.sims), kit.sims.clone());
    alone.seek(3.0);
    assert_eq!(alone.log().hash(), whole.log().hash());
    // Pushing, it moves them.
    meets(&mut scene, id, |m| m.pushes_balls = true);
    let resolved = resolve(&scene, &kit.registry);
    let mut pushed = Playback::new(&scene, &resolved, kit.sims.clone());
    pushed.seek(3.0);
    assert_ne!(pushed.world().bodies, alone.world().bodies, "the liquid didn't move the ball");
}

#[test]
fn two_runs_and_a_seek_move_the_balls_alike() {
    gpu();
    let kit = kit();
    let scene = water();
    let resolved = resolve(&scene, &kit.registry);
    let mut a = Playback::new(&scene, &resolved, kit.sims.clone());
    a.seek(3.0);
    let mut b = Playback::new(&scene, &resolved, kit.sims.clone());
    for t in [2.0, 0.5, 3.0] {
        b.seek(t);
    }
    assert_eq!(a.world().bodies, b.world().bodies);
    assert_eq!(a.log().hash(), b.log().hash());
}

#[test]
fn balls_take_its_colour_and_drop_ink() {
    gpu();
    let mut scene = water();
    let (id, ball) = (liquid_of(&scene), ball_of(&scene));
    meets(&mut scene, id, |m| m.take = 1.0);
    let (world, _) = run(&scene, 3.0);
    let taken = super::taken_colours(&world, ball).expect("the ball took a colour");
    let c = taken[&0];
    assert!(c[3] > 0.2 && c[2] > c[0] * 2.0, "it took the water's blue: {c:?}");
    // With red ink, the drops it touched turn red (and so does what it takes).
    meets(&mut scene, id, |m| {
        m.ink = 1.0;
        m.ink_colour = bpe_core::Color::rgb(255, 0, 0);
    });
    let (world, _) = run(&scene, 3.0);
    let host = host_of(&world).and_then(|s| s.host.as_ref()).unwrap();
    let drops = host.body.as_ref().unwrap().read();
    let red = drops.iter().filter(|d| d.colour[0] > d.colour[2]).count();
    assert!(red > 10, "{red} drops took the ball's ink");
    let c = super::taken_colours(&world, ball).unwrap()[&0];
    assert!(c[0] > c[2], "it took the ink back: {c:?}");
}

#[test]
fn a_rule_hears_a_ball_go_in() {
    gpu();
    let kit = kit();
    let mut scene = water();
    let ball = ball_of(&scene);
    let rule = scene.add_rule("Test");
    let when = scene.add_block(&kit.registry, rule, "when.liquid", None).unwrap();
    scene.set_block_param(&kit.registry, rule, when, "way", Value::Choice("goes_in".into())).unwrap();
    scene.add_block(&kit.registry, rule, "do.break", Some(when)).unwrap();
    let (world, _) = run(&scene, 3.0);
    assert!(!world.bodies[&ball].alive[0], "the ball broke going in");
}
