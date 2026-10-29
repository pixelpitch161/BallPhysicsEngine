//! Scenes for this crate's tests, built from the new scene.

use bpe_core::{Params, Resolved, Scene, Settings, SlotId, ThingId, Value, resolve};
use glam::Vec2;

use crate::{Kit, kit, new_scene};

pub const ARENA: ThingId = ThingId(1);
pub const BALLS: ThingId = ThingId(2);
/// The obstacle [`scene_with_obstacle`] and [`peg_board`] add.
pub const OBSTACLE: ThingId = ThingId(3);

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

/// The new scene with `f`'s changes to the shared motion module.
pub fn one_ball_with_motion(f: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let m = bpe_core::resolve_slot(&scene, &kit.registry, BALLS, SlotId::Motion, &mut Vec::new()).unwrap().module;
    f(&mut scene.library.modules.get_mut(&m).unwrap().params);
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, kit)
}

/// The new scene with an obstacle at the middle, with `f`'s changes, and the ball dropped from
/// above it with no sideways speed.
pub fn scene_with_obstacle(f: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let obstacle = scene.add_thing(&kit.registry, "obstacle", ThingId::ROOT).unwrap();
    assert_eq!(obstacle, OBSTACLE);
    f(&mut scene.things.get_mut(&OBSTACLE).unwrap().params);
    let ball = scene.things.get_mut(&BALLS).unwrap();
    ball.transform.position = Vec2::new(0.0, -300.0);
    ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, kit)
}

/// A box arena with a staggered `columns` x `rows` peg grid and a square block of
/// `balls_per_side`² balls dropped from the top.
pub fn peg_board(columns: u32, rows: u32, balls_per_side: u32) -> (Scene, Resolved, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let height = (rows + 6) as f32 * 90.0;
    let arena = scene.things.get_mut(&ARENA).unwrap();
    arena.params.set("shape", Value::Choice("box".into()));
    arena.params.set("width", Value::Number(((columns + 1) as f32 * 90.0) as f64));
    arena.params.set("height", Value::Number(height as f64));
    let pegs = scene.add_thing(&kit.registry, "obstacle", ThingId::ROOT).unwrap();
    assert_eq!(pegs, OBSTACLE);
    let p = &mut scene.things.get_mut(&OBSTACLE).unwrap().params;
    p.set("shape", Value::Choice("peg_grid".into()));
    p.set("columns", Value::Int(columns as i64));
    p.set("rows", Value::Int(rows as i64));
    p.set("radius", Value::Number(9.0));
    let ball = scene.things.get_mut(&BALLS).unwrap();
    ball.transform.position = Vec2::new(0.0, -height * 0.5 + 150.0);
    ball.params.set("arrangement", Value::Choice("block".into()));
    ball.params.set("columns", Value::Int(balls_per_side as i64));
    ball.params.set("rows", Value::Int(balls_per_side as i64));
    ball.params.set("spacing", Value::Number(20.0));
    ball.params.set("radius", Value::Number(8.0));
    ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
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
