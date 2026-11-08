//! Speed checks for the physics, playback and export work (workstream W7): not tests of
//! correctness, timings to read before and after a change. Run them in release:
//!
//! `cargo test --release -p bpe-kit --test speed_check -- --ignored --nocapture --test-threads=1`
//!
//! Each prints how long its stretch took and what it holds, so two runs can be compared.

use std::time::{Duration, Instant};

use bpe_core::{Address, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_kit::things::balls::BallsSettings;
use bpe_kit::things::emitter::{Earlier, EmitterSettings, FireWhen};
use bpe_kit::{kit, new_scene, resolve_scene, scenes};
use bpe_runtime::{Log, Playback, World};
use glam::Vec2;

fn the_ball(scene: &Scene) -> ThingId {
    scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball")
}

/// Turns on "Hit other balls" on `thing`'s Bounce card.
fn hit_balls(scene: &mut Scene, thing: ThingId) {
    scene.card_mut(thing, SlotId::Bounce).expect("a bounce card").params.set("hit_balls", Value::Bool(true));
}

fn timed(label: &str, f: impl FnOnce()) -> Duration {
    let started = Instant::now();
    f();
    let took = started.elapsed();
    println!("  {label}: {:.1} ms", took.as_secs_f64() * 1000.0);
    took
}

/// A ring of `count` balls in the test scene, each with a trail (a Line that stays when
/// `forever`).
fn trails_scene(count: u32, forever: bool) -> Scene {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    let ball = the_ball(&scene);
    let mut balls = BallsSettings::from_params(&scene.things[&ball].params);
    balls.radius = 6.0;
    balls.ring(count, 300.0);
    balls.velocity = Vec2::new(300.0, -200.0);
    scene.things.get_mut(&ball).unwrap().params = balls.to_params();
    hit_balls(&mut scene, ball);
    let trail = scene.add_layer(reg, ball, SlotId::Look, "layer.trail").unwrap();
    let set = |s: &mut Scene, k: &str, v| s.set_at(reg, &Address::on(ball, SlotId::Look, &layered_key(trail, k)), v);
    if forever {
        set(&mut scene, "kind", Value::Choice("line".into())).unwrap();
        set(&mut scene, "forever", Value::Bool(true)).unwrap();
    } else {
        set(&mut scene, "seconds", Value::Number(2.0)).unwrap();
    }
    scene
}

/// S10: a long playback with trails, then jumps back. The jump points copy the paths.
#[test]
#[ignore]
fn jumping_back_with_trails() {
    let kit = kit();
    for forever in [false, true] {
        let scene = trails_scene(120, forever);
        let resolved = resolve_scene(&scene, &kit.registry);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        println!("trails (Line that stays: {forever}), 120 balls:");
        timed("play 120 s", || playback.seek(120.0));
        println!("  memory {:.1} MB, {} jump points", playback.memory() as f64 / 1e6, playback.checkpoint_count());
        for to in [119.0, 90.0, 30.0] {
            let mut copy = playback.clone();
            timed(&format!("jump back to {to} s"), || copy.seek(to));
        }
    }
}

/// S11: the sound engine's run of a scene with trails and particle layers.
#[test]
#[ignore]
fn the_sound_engines_physics() {
    let kit = kit();
    let scene = trails_scene(120, true);
    let resolved = resolve_scene(&scene, &kit.registry);
    let heard = bpe_runtime::heard_part(&scene, &resolved, &kit.sims);
    let mut playback = Playback::new(&scene, &heard, kit.sims.clone());
    println!("the sound engine's physics, 120 balls with a Line that stays:");
    timed("play 60 s", || playback.seek(60.0));
    println!("  memory {:.1} MB", playback.memory() as f64 / 1e6);
}

/// S14: an endless emitter whose earlier rounds go: how a second of physics costs as the run
/// gets older.
#[test]
#[ignore]
fn an_endless_emitter() {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    scene.remove_thing(the_ball(&scene)).unwrap();
    let emitter = bpe_kit::add_thing(&mut scene, reg, "emitter", ThingId::ROOT).unwrap();
    let mut s = EmitterSettings::default();
    s.count = 20;
    s.fire_when = FireWhen::Every;
    s.every = 0.1;
    s.endless = true;
    s.earlier = Earlier::Remove;
    s.most_balls = 100_000;
    s.speed = 500.0;
    scene.things.get_mut(&emitter).unwrap().params = s.to_params();
    scene.add_layer(reg, emitter, SlotId::Look, "layer.trail").unwrap();
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    println!("an endless emitter, 200 balls a second, earlier rounds removed:");
    for until in [10.0, 60.0, 120.0] {
        world.advance_to(until - 1.0, &kit.sims, &mut log);
        timed(&format!("the second up to {until} s"), || world.advance_to(until, &kit.sims, &mut log));
        let rows: usize = world.bodies.values().map(|b| b.len()).sum();
        let alive: usize = world.bodies.values().map(|b| b.alive_count()).sum();
        println!("    rows {rows}, alive {alive}, world {:.1} MB", world.memory() as f64 / 1e6);
    }
}

/// S15: a second of fine air.
#[test]
#[ignore]
fn fine_air() {
    let kit = kit();
    let reg = &kit.registry;
    let (_, mut scene) =
        scenes::particle_scenes(reg).into_iter().find(|(n, _)| n == "Dust in the air").expect("the scene");
    bpe_kit::things::particles::air::ensure_air_card(&mut scene, reg).unwrap();
    let card = scene.card_mut(ThingId::ROOT, SlotId::Air).expect("an air card");
    card.params.set("resolution", Value::Choice("fine".into()));
    let resolved = resolve_scene(&scene, reg);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    println!("dust in fine air:");
    world.advance_to(0.5, &kit.sims, &mut log);
    let air = bpe_kit::air::air_of(&world).map(|a| a.size());
    println!("  air grid {air:?}");
    timed("two seconds", || world.advance_to(2.5, &kit.sims, &mut log));
}

/// S16: many small balls and one very big one.
#[test]
#[ignore]
fn one_big_ball_among_many() {
    let kit = kit();
    let reg = &kit.registry;
    for big in [false, true] {
        let mut scene = new_scene(reg);
        let ball = the_ball(&scene);
        let mut balls = BallsSettings::from_params(&scene.things[&ball].params);
        balls.radius = 5.0;
        balls.block(40, 40, 12.0);
        scene.things.get_mut(&ball).unwrap().params = balls.to_params();
        hit_balls(&mut scene, ball);
        if big {
            let giant = bpe_kit::add_thing(&mut scene, reg, "balls", ThingId::ROOT).unwrap();
            let mut g = BallsSettings::from_params(&scene.things[&giant].params);
            g.radius = 200.0;
            scene.things.get_mut(&giant).unwrap().params = g.to_params();
            scene.things.get_mut(&giant).unwrap().transform.position = Vec2::new(0.0, 250.0);
            for slot in [SlotId::Motion, SlotId::Bounce] {
                let card = scene.things[&ball].slots[&slot].clone();
                scene.things.get_mut(&giant).unwrap().slots.insert(slot, card);
            }
        }
        let resolved = resolve_scene(&scene, reg);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        println!("1600 small balls, a big one too: {big}");
        timed("three seconds", || world.advance_to(3.0, &kit.sims, &mut log));
        println!("    {} events", log.len());
    }
}

/// S17: moths, whose pull reads the light probe every step.
#[test]
#[ignore]
fn moths() {
    let kit = kit();
    let reg = &kit.registry;
    let scene = scenes::moths(reg);
    let resolved = resolve_scene(&scene, reg);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    println!("moths:");
    timed("five seconds", || world.advance_to(5.0, &kit.sims, &mut log));
    let scene = scenes::light_pressure(reg);
    let resolved = resolve_scene(&scene, reg);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    println!("light pressure:");
    timed("five seconds", || world.advance_to(5.0, &kit.sims, &mut log));
}
