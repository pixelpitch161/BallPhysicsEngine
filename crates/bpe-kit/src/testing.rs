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
    let m = bpe_core::resolve_slot(&scene, &kit.registry, BALLS, SlotId::Bounce, &mut Vec::new()).unwrap().module;
    f(&mut scene.library.modules.get_mut(&m).unwrap().params);
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, kit)
}

/// The new scene plus one more thing of `kind` at `position`, with `f`'s changes to its
/// settings. The ball is left where the new scene puts it. Problems are not checked, so tests
/// can point targets at missing things.
pub fn scene_with(
    kind: &str,
    name: &str,
    position: Vec2,
    f: impl FnOnce(&mut Params),
) -> (Scene, Resolved, Kit, ThingId) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let id = scene.add_thing(&kit.registry, kind, ThingId::ROOT).expect("kit kind");
    let thing = scene.things.get_mut(&id).unwrap();
    thing.name = name.into();
    thing.transform.position = position;
    f(&mut thing.params);
    let resolved = resolve(&scene, &kit.registry);
    (scene, resolved, kit, id)
}

/// Runs `scene` for `seconds` and gives back the world and its log.
pub fn run(scene: &Scene, resolved: &Resolved, kit: &Kit, seconds: f64) -> (bpe_runtime::World, bpe_runtime::Log) {
    let mut world = bpe_runtime::World::build(scene, resolved, &kit.sims);
    let mut log = bpe_runtime::Log::default();
    world.advance_to(seconds, &kit.sims, &mut log);
    (world, log)
}
