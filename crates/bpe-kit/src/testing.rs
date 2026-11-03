//! Scenes for this crate's tests, built from the new scene.

use bpe_core::{Params, Resolved, Scene, SlotId, ThingId, Value, resolve};
use glam::Vec2;

use crate::{Kit, kit, new_scene};

pub const ARENA: ThingId = ThingId(1);
pub const BALLS: ThingId = ThingId(2);

/// The new scene with its balls at the origin, in `arrangement`, with `f`'s changes.
pub fn scene_with_balls(arrangement: &str, f: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let thing = scene.things.get_mut(&BALLS).unwrap();
    thing.transform.position = Vec2::ZERO;
    thing.params.set("arrangement", Value::Choice(arrangement.into()));
    f(&mut thing.params);
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
