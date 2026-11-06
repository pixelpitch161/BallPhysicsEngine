//! Part 5's blocks doing what the plan says (`docs/superpowers/plans/rules/part5.md`), with exact
//! steps where it matters.

use bpe_core::{BlockId, Registry, RuleId, Scene, Settings, SlotId, ThingId, Value, Who, resolve};
use bpe_runtime::rules::{MAX_BALLS, rule_item};
use bpe_runtime::{EventKind, Log, Playback, World, state};
use glam::Vec2;

use crate::testing::{BALLS, SHAPE, scene_with_balls};
use crate::things::emitter::sim::emitter_state;
use crate::{BounceState, Kit, ShapeSettings, add_thing, kit, new_scene};

fn lane(scene: &mut Scene, reg: &Registry, blocks: &[(&str, Vec<(&str, Value)>)]) -> (RuleId, Vec<BlockId>) {
    let rule = scene.add_rule("Test");
    let mut ids = Vec::new();
    for (kind, params) in blocks {
        let id = scene.add_block(reg, rule, kind, ids.last().copied()).unwrap();
        for (k, v) in params {
            scene.set_block_param(reg, rule, id, k, v.clone()).unwrap_or_else(|e| panic!("{kind} {k}: {e}"));
        }
        ids.push(id);
    }
    (rule, ids)
}

fn build(scene: &Scene, kit: &Kit) -> World {
    let resolved = resolve(scene, &kit.registry);
    let world = World::build(scene, &resolved, &kit.sims);
    if let Some(rules) = world.rules() {
        for r in &scene.rules.list {
            assert_eq!(rules.problem(r.id), None, "{}", r.name);
        }
    }
    world
}

fn run(scene: &Scene, kit: &Kit, seconds: f64) -> (World, Log) {
    let mut world = build(scene, kit);
    let mut log = Log::default();
    world.advance_to(seconds, &kit.sims, &mut log);
    (world, log)
}

fn fired(log: &Log, rule: RuleId, block: BlockId) -> Vec<u64> {
    log.events()
        .iter()
        .filter(|e| e.kind == EventKind::Rule && e.b.is_some_and(|b| b.item == rule_item(rule, block)))
        .map(|e| e.step)
        .collect()
}

fn steps_of(log: &Log, kind: EventKind) -> Vec<u64> {
    log.events().iter().filter(|e| e.kind == kind).map(|e| e.step).collect()
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn choice(c: &str) -> Value {
    Value::Choice(c.into())
}

fn text(t: &str) -> Value {
    Value::Text(t.into())
}

fn who(w: Who) -> Value {
    Value::Who(w)
}

fn thing(t: ThingId) -> Who {
    Who::Thing { thing: t, ball: None }
}

fn time(at: f64, every: Option<f64>) -> (&'static str, Vec<(&'static str, Value)>) {
    let mut p = vec![("at", n(at))];
    if let Some(e) = every {
        p.push(("repeat", Value::Bool(true)));
        p.push(("every", n(e)));
    }
    ("when.time", p)
}

fn starter() -> (Scene, Kit) {
    let kit = kit();
    let scene = new_scene(&kit.registry);
    (scene, kit)
}

/// The starting scene with its ball dropped straight down from (0, -200) and a zone (a circle of
/// 100 px balls pass through) at (0, 150).
fn zone_scene() -> (Scene, Kit, ThingId) {
    let (mut scene, kit) = starter();
    scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
    let zone = add_thing(&mut scene, &kit.registry, "shape", ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&zone).unwrap();
    t.params = ShapeSettings { passes: true, ..ShapeSettings::circle(100.0, 3.0) }.to_params();
    t.transform.position = Vec2::new(0.0, 150.0);
    (scene, kit, zone)
}

/// The steps a ball's centre came into (true) or left (false) the zone of `zone_scene`.
fn crossings(scene: &Scene, kit: &Kit, seconds: f64) -> Vec<(u64, bool)> {
    let mut world = build(scene, kit);
    let mut log = Log::default();
    let inside = |w: &World| (w.bodies[&BALLS].position[0] - Vec2::new(0.0, 150.0)).length() < 100.0;
    let mut was = inside(&world);
    let mut out = Vec::new();
    while world.time() < seconds - 1e-9 {
        world.step(&kit.sims, &mut log);
        let now = inside(&world);
        if now != was {
            out.push((world.step, now));
        }
        was = now;
    }
    out
}

#[test]
fn part5_kinds_are_registered() {
    let kit = kit();
    for id in [
        "when.enter",
        "when.fired",
        "when.removed",
        "when.count",
        "when.level",
        "when.signal",
        "if.counter",
        "do.split",
        "do.spawn",
        "do.fire",
        "do.tag",
        "do.count",
        "do.signal",
    ] {
        let b = kit.registry.block(id).unwrap_or_else(|| panic!("{id}"));
        assert!(kit.sims.block(id).is_some_and(|s| s.section() == b.section()), "{id}");
        assert!(b.settings().iter().all(|d| d.group == "Rule"), "{id}");
    }
}

#[test]
fn balls_pass_through_a_shape_that_lets_them() {
    let (scene, kit, _) = zone_scene();
    let c = crossings(&scene, &kit, 3.0);
    assert!(c.len() >= 2, "{c:?}");
    assert_eq!((c[0].1, c[1].1), (true, false), "in, then out the far side");
    let (_, log) = run(&scene, &kit, 3.0);
    assert!(log.events().iter().filter(|e| e.kind == EventKind::Hit).all(|e| e.b.unwrap().thing == SHAPE));
}

#[test]
fn enters_and_leaves_fire_as_the_ball_crosses() {
    let (mut scene, kit, zone) = zone_scene();
    let (a, ai) = lane(&mut scene, &kit.registry, &[("when.enter", vec![("zone", who(thing(zone)))])]);
    let (b, bi) =
        lane(&mut scene, &kit.registry, &[("when.enter", vec![("zone", who(thing(zone))), ("way", choice("leaves"))])]);
    let (c, ci) = lane(&mut scene, &kit.registry, &[("when.enter", vec![("way", choice("either"))])]);
    let cross = crossings(&scene, &kit, 4.0);
    let (_, log) = run(&scene, &kit, 4.0);
    let ins: Vec<u64> = cross.iter().filter(|c| c.1).map(|c| c.0).collect();
    let outs: Vec<u64> = cross.iter().filter(|c| !c.1).map(|c| c.0).collect();
    assert_eq!(fired(&log, a, ai[0]), ins);
    assert_eq!(fired(&log, b, bi[0]), outs);
    assert_eq!(fired(&log, c, ci[0]), cross.iter().map(|c| c.0).collect::<Vec<_>>());
}

#[test]
fn a_ball_inside_at_the_start_never_enters() {
    let (mut scene, kit, zone) = zone_scene();
    scene.things.get_mut(&BALLS).unwrap().transform.position = Vec2::new(0.0, 150.0);
    let (a, ai) = lane(&mut scene, &kit.registry, &[("when.enter", vec![("zone", who(thing(zone)))])]);
    let cross = crossings(&scene, &kit, 1.0);
    let (_, log) = run(&scene, &kit, 1.0);
    assert_eq!(cross.len(), 1, "it falls out once: {cross:?}");
    assert!(fired(&log, a, ai[0]).is_empty());
}

/// The starting scene with an emitter at the middle firing `fire_when` rounds of 4.
fn emitter_scene(fire_when: &str) -> (Scene, Kit, ThingId) {
    let (mut scene, kit) = starter();
    let e = add_thing(&mut scene, &kit.registry, "emitter", ThingId::ROOT).unwrap();
    let p = &mut scene.things.get_mut(&e).unwrap().params;
    p.set("count", Value::Int(4));
    p.set("fire_when", choice(fire_when));
    (scene, kit, e)
}

#[test]
fn fired_hears_an_emitters_balls_and_rounds() {
    let (mut scene, kit, e) = emitter_scene("every");
    scene.things.get_mut(&e).unwrap().params.set("every", n(0.5));
    scene.things.get_mut(&e).unwrap().params.set("rounds", Value::Int(3));
    let (balls, b) = lane(&mut scene, &kit.registry, &[("when.fired", vec![])]);
    let (rounds, r) = lane(&mut scene, &kit.registry, &[("when.fired", vec![("each", choice("a_round"))])]);
    let (world, log) = run(&scene, &kit, 2.0);
    let mut spawns = steps_of(&log, EventKind::Spawn);
    spawns.dedup();
    assert_eq!(fired(&log, balls, b[0]), spawns);
    let st = emitter_state(&world, e).unwrap();
    let mut rounds_at: Vec<u64> = st.fired.iter().map(|f| f.step).filter(|&s| s > 0).collect();
    rounds_at.dedup();
    assert_eq!(fired(&log, rounds, r[0]), rounds_at);
    assert_eq!(rounds_at, [120, 240]);
}

#[test]
fn fire_a_round_fires_when_a_rule_asks() {
    let (mut scene, kit, e) = emitter_scene("by_a_rule");
    scene.things.get_mut(&e).unwrap().params.set("endless", Value::Bool(true));
    lane(&mut scene, &kit.registry, &[time(0.5, Some(0.5)), ("do.fire", vec![("who", who(thing(e)))])]);
    let (world, log) = build_and(&scene, &kit, 0.4);
    assert!(world.bodies[&e].is_empty(), "nothing until a rule asks");
    assert!(steps_of(&log, EventKind::Spawn).is_empty());
    let (world, log) = run(&scene, &kit, 1.1);
    let st = emitter_state(&world, e).unwrap();
    assert_eq!(st.fired.iter().map(|f| f.step).collect::<Vec<_>>(), [121, 241]);
    assert_eq!(steps_of(&log, EventKind::Spawn).len(), 8);
    assert_eq!(world.bodies[&e].len(), 8);
}

fn build_and(scene: &Scene, kit: &Kit, seconds: f64) -> (World, Log) {
    run(scene, kit, seconds)
}

#[test]
fn removed_and_broken_fire_on_a_break() {
    let (mut scene, kit) = starter();
    lane(&mut scene, &kit.registry, &[("when.hit", vec![]), ("do.break", vec![])]);
    let (gone, g) = lane(&mut scene, &kit.registry, &[("when.removed", vec![])]);
    let (broke, b) = lane(&mut scene, &kit.registry, &[("when.removed", vec![("what", choice("broken"))])]);
    let (_, log) = run(&scene, &kit, 4.0);
    let at = steps_of(&log, EventKind::Break);
    assert_eq!(at.len(), 1);
    assert_eq!(steps_of(&log, EventKind::Remove), at);
    assert_eq!(fired(&log, gone, g[0]), at);
    assert_eq!(fired(&log, broke, b[0]), at);
}

#[test]
fn count_reached_watches_hits_balls_and_counters() {
    // The third hit.
    let (mut scene, kit) = starter();
    let (hits, h) =
        lane(&mut scene, &kit.registry, &[("when.count", vec![("who", who(thing(BALLS))), ("amount", n(3.0))])]);
    let (_, log) = run(&scene, &kit, 8.0);
    let all = steps_of(&log, EventKind::Hit);
    assert!(all.len() >= 4);
    assert_eq!(fired(&log, hits, h[0]), [all[2]]);

    // A group broken ball by ball falls to no balls once.
    let (mut scene, _, kit) = scene_with_balls("ring", |p| p.set("count", Value::Int(3)));
    lane(&mut scene, &kit.registry, &[("when.hit", vec![]), ("do.break", vec![])]);
    let (none, z) = lane(
        &mut scene,
        &kit.registry,
        &[("when.count", vec![("of", choice("ball_count")), ("reach", choice("falls_to")), ("amount", n(0.0))])],
    );
    let (world, log) = run(&scene, &kit, 10.0);
    assert_eq!(world.bodies[&BALLS].alive_count(), 0);
    assert_eq!(fired(&log, none, z[0]), [*steps_of(&log, EventKind::Remove).last().unwrap()]);

    // A counter counted up every quarter second: the change lands a step later.
    let (mut scene, kit) = starter();
    lane(&mut scene, &kit.registry, &[time(0.0, Some(0.25)), ("do.count", vec![])]);
    let (three, t) =
        lane(&mut scene, &kit.registry, &[("when.count", vec![("of", choice("a_counter")), ("amount", n(3.0))])]);
    let (over, o) = lane(
        &mut scene,
        &kit.registry,
        &[time(0.0, Some(0.25)), ("if.counter", vec![("test", choice("over")), ("amount", n(1.5))])],
    );
    let (world, log) = run(&scene, &kit, 1.0);
    assert_eq!(fired(&log, three, t[0]), [121]);
    assert_eq!(fired(&log, over, o[1]), [120, 180, 240]);
    // Counted on 0, 0.25, 0.5 and 0.75 s; 1 s's count lands on the next step.
    assert_eq!(world.rules().unwrap().counter("score"), 4.0);
}

#[test]
fn count_can_set_and_reset() {
    let (mut scene, kit) = starter();
    lane(&mut scene, &kit.registry, &[time(0.1, None), ("do.count", vec![("how", choice("set")), ("amount", n(7.0))])]);
    lane(&mut scene, &kit.registry, &[time(0.2, None), ("do.count", vec![("amount", n(-2.0))])]);
    let (world, _) = run(&scene, &kit, 0.25);
    assert_eq!(world.rules().unwrap().counter("score"), 5.0);
    lane(&mut scene, &kit.registry, &[time(0.3, None), ("do.count", vec![("how", choice("reset"))])]);
    let (world, _) = run(&scene, &kit, 0.5);
    assert_eq!(world.rules().unwrap().counter("score"), 0.0);
}

#[test]
fn crosses_a_level_fires_when_the_speed_goes_over() {
    let (mut scene, kit) = starter();
    let (over, o) = lane(&mut scene, &kit.registry, &[("when.level", vec![("level", n(700.0))])]);
    let (either, e) =
        lane(&mut scene, &kit.registry, &[("when.level", vec![("level", n(700.0)), ("way", choice("either"))])]);
    let mut world = build(&scene, &kit);
    let mut log = Log::default();
    let mut last = world.bodies[&BALLS].velocity[0].length();
    let (mut ups, mut downs) = (Vec::new(), Vec::new());
    while world.time() < 4.0 {
        world.step(&kit.sims, &mut log);
        let now = world.bodies[&BALLS].velocity[0].length();
        if last <= 700.0 && now > 700.0 {
            ups.push(world.step);
        }
        if last >= 700.0 && now < 700.0 {
            downs.push(world.step);
        }
        last = now;
    }
    assert!(!ups.is_empty() && !downs.is_empty(), "the ball speeds up and slows down");
    assert_eq!(fired(&log, over, o[0]), ups);
    let mut both = [ups, downs].concat();
    both.sort();
    assert_eq!(fired(&log, either, e[0]), both);
}

#[test]
fn signals_start_other_rules_in_the_same_step() {
    let (mut scene, kit) = starter();
    lane(&mut scene, &kit.registry, &[time(0.5, None), ("do.signal", vec![])]);
    let (heard, h) = lane(
        &mut scene,
        &kit.registry,
        &[
            ("when.signal", vec![]),
            ("if.compare", vec![("value", text("time")), ("test", choice("equal")), ("amount", n(0.5))]),
            ("do.count", vec![]),
        ],
    );
    let (other, o) = lane(&mut scene, &kit.registry, &[("when.signal", vec![("name", text("other"))])]);
    let (world, log) = run(&scene, &kit, 1.0);
    assert_eq!(fired(&log, heard, h[0]), [120]);
    assert_eq!(fired(&log, heard, h[1]), [120], "the sender's values come along");
    assert!(fired(&log, other, o[0]).is_empty());
    assert_eq!(world.rules().unwrap().counter("score"), 1.0);
}

#[test]
fn a_signal_loop_goes_round_eight_times_a_step() {
    let (mut scene, kit) = starter();
    lane(&mut scene, &kit.registry, &[time(0.5, None), ("do.signal", vec![])]);
    let (echo, e) = lane(&mut scene, &kit.registry, &[("when.signal", vec![]), ("do.signal", vec![])]);
    // Also count, beside the send.
    let c = scene.add_block(&kit.registry, echo, "do.count", Some(e[0])).unwrap();
    let (world, log) = run(&scene, &kit, 130.0 / 240.0);
    let at = fired(&log, echo, e[0]);
    assert_eq!(at[..3], [120, 121, 122], "round on round, carried to the next step");
    assert_eq!(fired(&log, echo, c).len(), at.len());
    // Eight rounds a step: heard 8 times on step 120 (rounds 1 to 8), then 9 a step (the carried
    // signal in round 0 too); counts land at the next step's start, so step 130's are still to come.
    assert_eq!(world.rules().unwrap().counter("score"), (8 + 9 * (130 - 121)) as f64);
}

fn bounciness(world: &World, row: usize) -> f32 {
    state::<BounceState>(world.card_for(BALLS, SlotId::Bounce, row).unwrap()).settings.bounciness
}

#[test]
fn split_fans_a_ball_into_smaller_ones() {
    let (mut scene, kit) = starter();
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.0, None),
            (
                "do.change",
                vec![
                    ("who", who(Who::Thing { thing: BALLS, ball: Some(1) })),
                    ("card", choice("bounce")),
                    ("setting", text("bounciness")),
                    ("to_number", n(0.3)),
                ],
            ),
        ],
    );
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.25, None),
            (
                "do.split",
                vec![
                    ("who", who(Who::Thing { thing: BALLS, ball: Some(1) })),
                    ("into", Value::Int(3)),
                    ("spread", n(90.0)),
                ],
            ),
        ],
    );
    let (before, _) = run(&scene, &kit, 0.25);
    let (world, _) = run(&scene, &kit, 61.0 / 240.0);
    let b = &world.bodies[&BALLS];
    assert_eq!(b.len(), 3);
    assert_eq!(b.alive_count(), 3);
    let r0 = before.bodies[&BALLS].radius[0];
    let area: f32 = b.radius.iter().map(|r| r * r).sum();
    assert!((area - r0 * r0).abs() < 1e-2, "{area} against {}", r0 * r0);
    assert_eq!(b.number, [0, 0, 0], "each keeps the ball's number");
    let speeds: Vec<f32> = b.velocity.iter().map(|v| v.length()).collect();
    // The same speed, give or take the step of gravity since.
    assert!(speeds.iter().all(|s| (s - speeds[0]).abs() < 5.0), "{speeds:?}");
    let angle = |i: usize| b.velocity[i].angle_to(b.velocity[0]).to_degrees().abs();
    let mut turns = [angle(1), angle(2)];
    turns.sort_by(f32::total_cmp);
    assert!((turns[0] - 45.0).abs() < 1.0 && (turns[1] - 45.0).abs() < 1.0, "{turns:?}");
    for row in 0..3 {
        assert!((bounciness(&world, row) - 0.3).abs() < 1e-6, "row {row} keeps its own bounciness");
    }
}

#[test]
fn a_split_stops_at_the_ball_cap() {
    let (mut scene, _, kit) = scene_with_balls("block", |p| {
        p.set("columns", Value::Int(64));
        p.set("rows", Value::Int(64));
        p.set("spacing", Value::Number(4.0));
        p.set("radius", Value::Number(1.0));
        p.set("velocity", Value::Vec2([0.0, 0.0]));
    });
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.0, None),
            ("do.split", vec![("who", who(Who::Thing { thing: BALLS, ball: Some(1) })), ("into", Value::Int(4))]),
        ],
    );
    let (world, _) = run(&scene, &kit, 3.0 / 240.0);
    assert_eq!(world.bodies[&BALLS].alive_count(), MAX_BALLS);
    assert_eq!(world.bodies[&BALLS].len(), MAX_BALLS);
}

#[test]
fn spawn_adds_balls_round_a_point() {
    let (mut scene, kit) = starter();
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.25, None),
            (
                "do.spawn",
                vec![
                    ("into", who(thing(BALLS))),
                    ("count", Value::Int(4)),
                    ("way", choice("all_round")),
                    ("speed", n(100.0)),
                    ("size", choice("set_size")),
                    ("radius", n(10.0)),
                ],
            ),
        ],
    );
    let (world, _) = run(&scene, &kit, 61.0 / 240.0);
    let b = &world.bodies[&BALLS];
    assert_eq!(b.len(), 5);
    assert_eq!(&b.number[1..], [1, 2, 3, 4]);
    for i in 1..5 {
        assert_eq!(b.radius[i], 10.0);
        // Moved one step since: 100 px/s plus a step of gravity.
        assert!((b.velocity[i].length() - 100.0).abs() < 10.0, "{}", b.velocity[i]);
        assert!((b.spawn[i].length() - 10.0).abs() < 1e-3, "{}", b.spawn[i]);
    }
    let up = b.spawn[1].normalize();
    assert!((up - Vec2::NEG_Y).length() < 1e-4);
    assert!((b.spawn[2].normalize() - Vec2::X).length() < 1e-4, "a quarter turn clockwise: {}", b.spawn[2]);
}

#[test]
fn spawn_into_an_emitter_numbers_on() {
    let (mut scene, kit, e) = emitter_scene("at_start");
    lane(
        &mut scene,
        &kit.registry,
        &[time(0.25, None), ("do.spawn", vec![("into", who(thing(e))), ("count", Value::Int(2))])],
    );
    let (world, _) = run(&scene, &kit, 0.3);
    assert_eq!(world.bodies[&e].len(), 6);
    let st = emitter_state(&world, e).unwrap();
    assert_eq!(st.rows.numbers, [0, 1, 2, 3, 4, 5]);
    assert_eq!(st.next_number, 6);
    assert_eq!(world.bodies[&e].radius[5], st.settings.radius);
}

#[test]
fn a_balls_own_tag_picks_it_out() {
    let (mut scene, kit) = starter();
    lane(&mut scene, &kit.registry, &[("when.hit", vec![]), ("do.tag", vec![("tag", text("hot"))])]);
    let (hot, h) = lane(&mut scene, &kit.registry, &[("when.hit", vec![("who", who(Who::Tag("hot".into())))])]);
    let (world, log) = run(&scene, &kit, 8.0);
    let hits = steps_of(&log, EventKind::Hit);
    assert!(hits.len() >= 3);
    assert_eq!(fired(&log, hot, h[0]), hits[1..]);
    assert!(world.rules().unwrap().ball_tags(BALLS, 0).unwrap().contains("hot"));
    assert!(!world.tags.get(&BALLS).is_some_and(|t| t.contains("hot")), "the ball, not its thing");
}

#[test]
fn a_things_tag_is_added_and_taken_away() {
    let (mut scene, kit) = starter();
    lane(
        &mut scene,
        &kit.registry,
        &[time(0.1, None), ("do.tag", vec![("who", who(thing(SHAPE))), ("tag", text("wall"))])],
    );
    let (world, _) = run(&scene, &kit, 0.2);
    assert!(world.tags[&SHAPE].contains("wall"));
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.15, None),
            ("do.tag", vec![("who", who(thing(SHAPE))), ("tag", text("wall")), ("how", choice("remove"))]),
        ],
    );
    let (world, _) = run(&scene, &kit, 0.2);
    assert!(!world.tags.get(&SHAPE).is_some_and(|t| t.contains("wall")));
}

#[test]
fn part5_rules_replay_the_same_through_seeks() {
    let (mut scene, kit, zone) = zone_scene();
    lane(
        &mut scene,
        &kit.registry,
        &[("when.hit", vec![]), ("if.only", vec![("n", Value::Int(3))]), ("do.split", vec![("into", Value::Int(2))])],
    );
    lane(&mut scene, &kit.registry, &[("when.enter", vec![("zone", who(thing(zone)))]), ("do.signal", vec![])]);
    lane(&mut scene, &kit.registry, &[("when.signal", vec![]), ("do.count", vec![])]);
    lane(&mut scene, &kit.registry, &[("when.signal", vec![]), ("do.tag", vec![])]);
    lane(
        &mut scene,
        &kit.registry,
        &[
            ("when.count", vec![("of", choice("a_counter")), ("amount", n(2.0))]),
            ("do.spawn", vec![("into", who(thing(BALLS))), ("count", Value::Int(3)), ("way", choice("random"))]),
        ],
    );
    let resolved = resolve(&scene, &kit.registry);
    let mut straight = Playback::new(&scene, &resolved, kit.sims.clone());
    straight.seek(6.0);
    let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
    for t in [2.2, 0.7, 4.1, 1.0, 6.0] {
        jumpy.seek(t);
    }
    assert_eq!(straight.log().hash(), jumpy.log().hash());
    assert_eq!(straight.world().bodies[&BALLS].position, jumpy.world().bodies[&BALLS].position);
    assert!(straight.world().bodies[&BALLS].len() > 3, "the balls split and spawned");
}
