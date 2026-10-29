//! Scenes for this crate's tests, built from the new scene.

use bpe_core::{Params, Resolved, Scene, Settings, SlotId, ThingId, Value, resolve};
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

/// The module `thing` gets for `slot`.
pub fn module_of(scene: &Scene, reg: &bpe_core::Registry, thing: ThingId, slot: SlotId) -> bpe_core::ModuleId {
    bpe_core::resolve_slot(scene, reg, thing, slot, &mut Vec::new()).unwrap().module
}

/// Stores the module's template, bound to its settings, as a custom graph: the same graph,
/// forced through the compiled path.
pub fn with_custom_template(scene: &mut Scene, module: bpe_core::ModuleId, reg: &bpe_core::Registry) {
    let m = scene.library.modules.get_mut(&module).unwrap();
    let kind = reg.module(&m.kind).unwrap();
    let mut g = crate::graphs::module_graph(kind, m).expect("the kind has a template");
    g.custom = true;
    m.graph = Some(g);
}

/// Runs the scene to its end: the log's hash and length, and the soundtrack's samples.
pub fn log_hash_and_samples(scene: &Scene, kit: &Kit) -> (u64, usize, Vec<f32>) {
    let resolved = crate::resolve_scene(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut playback = bpe_runtime::Playback::new(scene, &resolved, kit.sims.clone());
    let root = crate::SceneSettings::from_params(&resolved.things[&ThingId::ROOT].params);
    playback.seek(root.duration as f64);
    let log = playback.log();
    (log.hash(), log.len(), crate::mix_scene(scene, &resolved, log, kit).samples)
}
