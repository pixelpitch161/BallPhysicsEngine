//! Scenes for this crate's tests, built from the new scene.

use bpe_core::{Params, Resolved, Scene, SlotId, ThingId, Value, resolve};
use glam::Vec2;

use crate::{Kit, kit, new_scene};

pub const SHAPE: ThingId = ThingId(1);
pub const BALLS: ThingId = ThingId(2);

/// The new scene with its balls at the origin, in the old `arrangement` ("single", "block" or
/// "ring"), with `f`'s changes. A block's `columns`, `rows` and `spacing` (from one ball's middle
/// to the next; defaults 10, 10 and 19) become the grid that lays out the same.
pub fn scene_with_balls(arrangement: &str, f: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let thing = scene.things.get_mut(&BALLS).unwrap();
    thing.transform.position = Vec2::ZERO;
    let p = &mut thing.params;
    match arrangement {
        "single" => {}
        "block" => {
            p.set("group", Value::Bool(true));
            p.set("shape", Value::Choice("grid".into()));
            p.set("columns", Value::Int(10));
            p.set("rows", Value::Int(10));
            p.set("spacing", Value::Number(19.0));
        }
        "ring" => {
            p.set("group", Value::Bool(true));
            p.set("shape", Value::Choice("ring".into()));
            p.set("count", Value::Int(24));
        }
        other => panic!("no arrangement {other}"),
    }
    f(p);
    if let (Some(Value::Int(rows)), Some(Value::Number(spacing))) = (p.remove("rows"), p.remove("spacing")) {
        let columns = match p.get("columns") {
            Some(Value::Int(c)) => *c,
            _ => 10,
        };
        let radius = match p.get("radius") {
            Some(Value::Number(r)) => *r,
            _ => 24.0,
        };
        p.set("count", Value::Int(columns * rows));
        p.set("space", Value::Number(spacing - 2.0 * radius));
    }
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, kit)
}

/// The new scene with `f`'s changes to the shared bounce module.
pub fn one_ball_with_bounce(f: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let m = (BALLS, SlotId::Bounce);
    f(&mut scene.card_mut(m.0, m.1).unwrap().params);
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, kit)
}

/// Runs `scene` for `seconds` and gives back the world and its log.
pub fn run(scene: &Scene, resolved: &Resolved, kit: &Kit, seconds: f64) -> (bpe_runtime::World, bpe_runtime::Log) {
    let mut world = bpe_runtime::World::build(scene, resolved, &kit.sims);
    let mut log = bpe_runtime::Log::default();
    world.advance_to(seconds, &kit.sims, &mut log);
    (world, log)
}

/// The new scene with `f`'s changes to the shared motion module.
pub fn one_ball_with_motion(f: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let m = (BALLS, SlotId::Motion);
    f(&mut scene.card_mut(m.0, m.1).unwrap().params);
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, kit)
}

/// Sets `key` of the first `kind` layer on `thing`'s Look to `v`, adding the layer when the look
/// has none. Gives back the layer's id.
pub fn set_layer(scene: &mut Scene, thing: ThingId, kind: &str, key: &str, v: Value) -> bpe_core::LayerId {
    let reg = crate::registry();
    let found = scene.card_mut(thing, SlotId::Look).unwrap().layers.iter().find(|l| l.kind == kind).map(|l| l.id);
    let id = found.unwrap_or_else(|| scene.add_layer(&reg, thing, SlotId::Look, kind).unwrap());
    scene.card_mut(thing, SlotId::Look).unwrap().params.set(&bpe_core::layered_key(id, key), v);
    id
}

/// Adds an effect layer of `kind` to the scene's Effects card, adding the card when it has none.
pub fn add_effect(scene: &mut Scene, reg: &bpe_core::Registry, kind: &str) -> bpe_core::LayerId {
    let has = scene.things[&ThingId::ROOT].slots.get(&SlotId::Shader).is_some_and(|c| c.kind == "effects");
    if !has {
        scene.add_slot(reg, ThingId::ROOT, SlotId::Shader, "effects").expect("the scene takes Effects");
    }
    scene.add_layer(reg, ThingId::ROOT, SlotId::Shader, kind).expect("an effect layer")
}
