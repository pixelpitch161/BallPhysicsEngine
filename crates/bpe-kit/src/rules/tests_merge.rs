//! Merge balls (built-in scenes part 2, plan `docs/superpowers/plans/built-in-scenes/part2.md`):
//! two touching balls become one.

use bpe_core::{Registry, Scene, Settings, ThingId, Value, Who, resolve};
use bpe_runtime::{EventKind, Log, World};
use glam::Vec2;

use crate::testing::{BALLS, SHAPE};
use crate::{Kit, ShapeSettings, add_thing, kit, new_scene};

fn lane(scene: &mut Scene, reg: &Registry, blocks: &[(&str, Vec<(&str, Value)>)]) {
    let rule = scene.add_rule("Merge");
    let mut last = None;
    for (kind, params) in blocks {
        let id = scene.add_block(reg, rule, kind, last).unwrap();
        for (k, v) in params {
            scene.set_block_param(reg, rule, id, k, v.clone()).unwrap_or_else(|e| panic!("{kind} {k}: {e}"));
        }
        last = Some(id);
    }
}

fn run(scene: &Scene, kit: &Kit, seconds: f64) -> (World, Log) {
    let mut world = World::build(scene, &resolve(scene, &kit.registry), &kit.sims);
    let mut log = Log::default();
    world.advance_to(seconds, &kit.sims, &mut log);
    if let Some(rules) = world.rules() {
        for r in &scene.rules.list {
            assert_eq!(rules.problem(r.id), None, "{}", r.name);
        }
    }
    (world, log)
}

/// No gravity, a big ring, and the starting ball (radius `ra`) at (-200, 0) going right at 300 px/s
/// toward a second Balls thing's ball (radius `rb`) at (200, 0) going left as fast. A rule merges
/// any ball that hits a ball, with `merge` its settings.
fn meeting(ra: f64, rb: f64, merge: Vec<(&'static str, Value)>) -> (Scene, Kit, ThingId) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
    scene.things.get_mut(&SHAPE).unwrap().params = ShapeSettings::circle(900.0, 4.0).to_params();
    let mid = scene.things[&SHAPE].transform.position;
    let a = scene.things.get_mut(&BALLS).unwrap();
    a.transform.position = mid + Vec2::new(-200.0, 0.0);
    a.params.set("velocity", Value::Vec2([300.0, 0.0]));
    a.params.set("radius", Value::Number(ra));
    let b = add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&b).unwrap();
    t.transform.position = mid + Vec2::new(200.0, 0.0);
    t.params.set("velocity", Value::Vec2([-300.0, 0.0]));
    t.params.set("radius", Value::Number(rb));
    for t in [BALLS, b] {
        scene.card_mut(t, bpe_core::SlotId::Bounce).unwrap().params.set("hit_balls", Value::Bool(true));
    }
    let balls = Value::Who(Who::Kind("balls".into()));
    lane(
        &mut scene,
        &kit.registry,
        &[("when.hit", vec![("who", balls.clone()), ("whom", balls)]), ("do.merge", merge)],
    );
    (scene, kit, b)
}

fn live(w: &World, thing: ThingId) -> Vec<usize> {
    w.bodies[&thing].live_rows().filter(|&i| w.bodies[&thing].alive[i]).collect()
}

#[test]
fn two_touching_balls_merge_into_one_keeping_their_momentum() {
    let (scene, kit, b) = meeting(30.0, 30.0, vec![]);
    let (w, log) = run(&scene, &kit, 1.5);
    let (a_live, b_live) = (live(&w, BALLS), live(&w, b));
    assert_eq!(a_live.len() + b_live.len(), 1, "one ball is left");
    let (thing, row) = if a_live.is_empty() { (b, b_live[0]) } else { (BALLS, a_live[0]) };
    let r = w.bodies[&thing].radius[row];
    assert!((r - 30.0 * 2f32.sqrt()).abs() < 1e-3, "two areas: {r}");
    // Equal and opposite: the merged ball is all but still (the bounce off each other happened first,
    // equal and opposite too).
    assert!(w.bodies[&thing].velocity[row].length() < 1.0, "{}", w.bodies[&thing].velocity[row]);
    let removes = log.events().iter().filter(|e| e.kind == EventKind::Remove).count();
    assert_eq!(removes, 1, "the other ball was removed once");
}

#[test]
fn only_same_size_leaves_different_sizes_alone() {
    let (scene, kit, b) = meeting(30.0, 40.0, vec![]);
    let (w, _) = run(&scene, &kit, 1.5);
    assert_eq!(live(&w, BALLS).len() + live(&w, b).len(), 2);
    // Within the size match they merge.
    let (scene, kit, b) = meeting(30.0, 40.0, vec![("size_match", Value::Number(30.0))]);
    let (w, _) = run(&scene, &kit, 1.5);
    assert_eq!(live(&w, BALLS).len() + live(&w, b).len(), 1);
    // And with Only same size off.
    let (scene, kit, b) = meeting(30.0, 40.0, vec![("same_size", Value::Bool(false))]);
    let (w, _) = run(&scene, &kit, 1.5);
    assert_eq!(live(&w, BALLS).len() + live(&w, b).len(), 1);
}

#[test]
fn keep_size_times_an_amount_grows_that() {
    let grow = vec![("grow", Value::Choice("keep_size".into())), ("times", Value::Number(1.5))];
    let (scene, kit, b) = meeting(30.0, 30.0, grow);
    let (w, _) = run(&scene, &kit, 1.5);
    let sizes: Vec<f32> = [BALLS, b]
        .iter()
        .flat_map(|t| live(&w, *t).into_iter().map(|i| w.bodies[t].radius[i]).collect::<Vec<_>>())
        .collect();
    assert_eq!(sizes.len(), 1);
    assert!((sizes[0] - 45.0).abs() < 1e-3, "{sizes:?}");
}

/// Who and With the other way round: the firing about B hitting A keeps B.
#[test]
fn the_ball_that_stays_is_who() {
    let (scene, kit, b) = meeting(30.0, 30.0, vec![]);
    let (w, log) = run(&scene, &kit, 1.5);
    // The hit's first ball is the one the first firing names, and it stays in its own thing.
    let hit = log
        .events()
        .iter()
        .find(|e| e.kind == EventKind::Hit && e.b.is_some_and(|p| p.thing == b || p.thing == BALLS))
        .unwrap();
    let stayed = hit.a.thing;
    assert_eq!(live(&w, stayed).len(), 1, "the first firing's That stays");
}

#[test]
fn a_merge_never_takes_in_a_ball_twice_or_itself() {
    // Three balls in a row meeting at once can't merge one ball into two.
    let (mut scene, kit, _) = meeting(30.0, 30.0, vec![]);
    let c = add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
    let mid = scene.things[&SHAPE].transform.position;
    let t = scene.things.get_mut(&c).unwrap();
    t.transform.position = mid + Vec2::new(0.0, -200.0);
    t.params.set("velocity", Value::Vec2([0.0, 300.0]));
    t.params.set("radius", Value::Number(30.0));
    scene.card_mut(c, bpe_core::SlotId::Bounce).unwrap().params.set("hit_balls", Value::Bool(true));
    let (w, _) = run(&scene, &kit, 2.0);
    let total: usize = w.bodies.keys().map(|t| live(&w, *t).len()).sum();
    assert!((1..3).contains(&total), "some balls merged: {total} left");
    let area: f32 =
        w.bodies.values().flat_map(|b| b.live_rows().filter(|&i| b.alive[i]).map(|i| b.radius[i].powi(2))).sum();
    assert!((area - 3.0 * 900.0).abs() < 1.0, "the balls' area is kept: {area}");
}
