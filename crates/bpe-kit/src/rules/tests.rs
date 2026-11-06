//! Every block kind doing what the plan's section 3 says, with exact steps.

use bpe_core::{BlockId, Registry, RuleId, Scene, Settings, SlotId, ThingId, Value, Who, resolve};
use bpe_gfx::{Frame, build_frame};
use bpe_runtime::rules::rule_item;
use bpe_runtime::{EventKind, Log, Playback, World, state};

use crate::testing::{BALLS, SHAPE, scene_with_balls};
use crate::{BounceState, Kit, kit, new_scene};

/// Adds a rule whose blocks follow one another, each with its settings.
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

fn run(scene: &Scene, kit: &Kit, seconds: f64) -> (World, Log) {
    let resolved = resolve(scene, &kit.registry);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(seconds, &kit.sims, &mut log);
    if let Some(rules) = world.rules() {
        for r in &scene.rules.list {
            assert_eq!(rules.problem(r.id), None, "{}", r.name);
        }
    }
    (world, log)
}

/// The steps block `block` of rule `rule` fired on.
fn fired(log: &Log, rule: RuleId, block: BlockId) -> Vec<u64> {
    log.events()
        .iter()
        .filter(|e| e.kind == EventKind::Rule && e.b.is_some_and(|b| b.item == rule_item(rule, block)))
        .map(|e| e.step)
        .collect()
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn i(x: i64) -> Value {
    Value::Int(x)
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

fn the_ball() -> Who {
    Who::Thing { thing: BALLS, ball: None }
}

/// Time at `at`, again every `every` (none: once).
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

#[test]
fn every_kind_is_registered_with_settings_under_rule() {
    let kit = kit();
    let ids: Vec<&str> = kit.registry.blocks().map(|b| b.id()).collect();
    assert_eq!(
        ids,
        [
            "when.hit",
            "when.time",
            "when.beat",
            "if.compare",
            "if.is",
            "if.chance",
            "if.every",
            "if.cooldown",
            "if.only",
            "if.between",
            "logic",
            "do.motion",
            "do.change",
            "do.break",
            "when.health",
            "if.health",
            "do.health"
        ]
    );
    for b in kit.registry.blocks() {
        assert!(kit.sims.block(b.id()).is_some_and(|s| s.section() == b.section()), "{}", b.id());
        assert!(b.settings().iter().all(|d| d.group == "Rule"), "{}", b.id());
    }
}

#[test]
fn time_fires_at_its_moments() {
    let (mut scene, kit) = starter();
    let (rule, ids) = lane(
        &mut scene,
        &kit.registry,
        &[(
            "when.time",
            vec![
                ("at", n(0.5)),
                ("repeat", Value::Bool(true)),
                ("every", n(0.25)),
                ("until", Value::Bool(true)),
                ("until_at", n(1.0)),
            ],
        )],
    );
    let (_, log) = run(&scene, &kit, 3.0);
    assert_eq!(fired(&log, rule, ids[0]), [120, 180, 240]);
    // At 0 fires in the first step.
    let (mut scene, kit) = starter();
    let (rule, ids) = lane(&mut scene, &kit.registry, &[time(0.0, None)]);
    let (_, log) = run(&scene, &kit, 1.0);
    assert_eq!(fired(&log, rule, ids[0]), [1]);
}

#[test]
fn beat_fires_on_the_tempo() {
    let (mut scene, kit) = starter();
    let (beats, b) = lane(&mut scene, &kit.registry, &[("when.beat", vec![])]);
    let (bars, c) = lane(&mut scene, &kit.registry, &[("when.beat", vec![("every", choice("bar"))])]);
    let (late, d) = lane(&mut scene, &kit.registry, &[("when.beat", vec![("offset", n(1.0))])]);
    let (_, log) = run(&scene, &kit, 2.1);
    // 120 a minute in 4/4.
    assert_eq!(fired(&log, beats, b[0]), [1, 120, 240, 360, 480]);
    assert_eq!(fired(&log, bars, c[0]), [1, 480]);
    assert_eq!(fired(&log, late, d[0]), [120, 240, 360, 480]);
}

#[test]
fn hit_fires_for_who_hits_what() {
    let (mut scene, kit) = starter();
    let (any, a) = lane(&mut scene, &kit.registry, &[("when.hit", vec![])]);
    let (named, b) = lane(
        &mut scene,
        &kit.registry,
        &[(
            "when.hit",
            vec![
                ("who", who(Who::Thing { thing: BALLS, ball: Some(1) })),
                ("whom", who(Who::Thing { thing: SHAPE, ball: None })),
            ],
        )],
    );
    let (wrong, c) =
        lane(&mut scene, &kit.registry, &[("when.hit", vec![("who", who(Who::Thing { thing: SHAPE, ball: None }))])]);
    let (hard, d) = lane(&mut scene, &kit.registry, &[("when.hit", vec![("min_strength", n(1e6))])]);
    let (_, log) = run(&scene, &kit, 4.0);
    let hits: Vec<u64> = log.events().iter().filter(|e| e.kind == EventKind::Hit).map(|e| e.step).collect();
    assert!(hits.len() >= 2);
    assert_eq!(fired(&log, any, a[0]), hits);
    assert_eq!(fired(&log, named, b[0]), hits);
    assert!(fired(&log, wrong, c[0]).is_empty());
    assert!(fired(&log, hard, d[0]).is_empty());
}

/// A Time every 0.125 s for 2 s (17 firings: steps 1, 30, 60, … 480) into `check`.
fn counted(check: (&'static str, Vec<(&'static str, Value)>)) -> Vec<u64> {
    let (mut scene, kit) = starter();
    let (rule, ids) = lane(&mut scene, &kit.registry, &[time(0.0, Some(0.125)), check]);
    let (_, log) = run(&scene, &kit, 2.0);
    assert_eq!(fired(&log, rule, ids[0]).len(), 17);
    fired(&log, rule, ids[1])
}

#[test]
fn counting_checks_let_the_right_firings_through() {
    assert_eq!(counted(("if.every", vec![("n", i(3))])), [60, 150, 240, 330, 420]);
    assert_eq!(counted(("if.only", vec![("n", i(2))])), [1, 30]);
    assert_eq!(counted(("if.cooldown", vec![("seconds", n(0.3))])), [1, 90, 180, 270, 360, 450]);
    assert_eq!(counted(("if.between", vec![("from", n(0.5)), ("to", n(1.0))])), [120, 150, 180, 210, 240]);
    assert_eq!(
        counted(("if.compare", vec![("value", text("count")), ("test", choice("equal")), ("amount", n(3.0))])),
        [60]
    );
    // count > count × 0.5 + 2: from the fifth on.
    let from = counted((
        "if.compare",
        vec![
            ("value", text("count")),
            ("amount", n(0.0)),
            ("amount~from", text("count")),
            ("amount~times", n(0.5)),
            ("amount~plus", n(2.0)),
        ],
    ));
    assert_eq!(from.len(), 13);
    assert_eq!(from[0], 120);
    assert!(counted(("if.compare", vec![("value", text("nothing"))])).is_empty());
}

#[test]
fn chance_is_steady_and_about_right() {
    let all = counted(("if.chance", vec![("percent", n(100.0))]));
    assert_eq!(all.len(), 17);
    assert!(counted(("if.chance", vec![("percent", n(0.0))])).is_empty());
    let (mut scene, kit) = starter();
    let (rule, ids) =
        lane(&mut scene, &kit.registry, &[time(0.0, Some(0.125)), ("if.chance", vec![("percent", n(30.0))])]);
    let (_, log) = run(&scene, &kit, 40.0);
    let (_, again) = run(&scene, &kit, 40.0);
    let some = fired(&log, rule, ids[1]);
    assert_eq!(some, fired(&again, rule, ids[1]));
    let share = some.len() as f64 / 321.0;
    assert!((0.22..0.38).contains(&share), "{share}");
}

#[test]
fn is_checks_either_side() {
    let (mut scene, kit) = starter();
    let shape = who(Who::Thing { thing: SHAPE, ball: None });
    let (on_shape, a) = lane(
        &mut scene,
        &kit.registry,
        &[("when.hit", vec![]), ("if.is", vec![("side", choice("the_thing_it_hit")), ("who", shape.clone())])],
    );
    let (on_ball, b) = lane(
        &mut scene,
        &kit.registry,
        &[("when.hit", vec![]), ("if.is", vec![("side", choice("the_thing_it_hit")), ("who", who(the_ball()))])],
    );
    let (first, c) = lane(
        &mut scene,
        &kit.registry,
        &[("when.hit", vec![]), ("if.is", vec![("test", choice("ball_number")), ("number", i(1))])],
    );
    let (second, d) = lane(
        &mut scene,
        &kit.registry,
        &[("when.hit", vec![]), ("if.is", vec![("test", choice("ball_number")), ("number", i(2))])],
    );
    let (_, log) = run(&scene, &kit, 3.0);
    assert!(!fired(&log, on_shape, a[1]).is_empty());
    assert_eq!(fired(&log, on_shape, a[1]), fired(&log, on_shape, a[0]));
    assert!(fired(&log, on_ball, b[1]).is_empty());
    assert_eq!(fired(&log, first, c[1]), fired(&log, first, c[0]));
    assert!(fired(&log, second, d[1]).is_empty());
}

/// Two (or three) Time inputs at these moments joined by a Logic block in `mode`; the steps it
/// fired on.
fn logic(mode: &str, at: &[f64], more: Vec<(&'static str, Value)>) -> Vec<u64> {
    let (mut scene, kit) = starter();
    let reg = &kit.registry;
    let rule = scene.add_rule("Logic");
    let whens: Vec<BlockId> = at
        .iter()
        .map(|&m| {
            let id = scene.add_when(reg, rule, "when.time").unwrap();
            scene.set_block_param(reg, rule, id, "at", n(m)).unwrap();
            id
        })
        .collect();
    let join = scene.add_block(reg, rule, "logic", Some(whens[0])).unwrap();
    for w in &whens[1..] {
        scene.link(reg, rule, *w, join).unwrap();
    }
    scene.set_block_param(reg, rule, join, "mode", choice(mode)).unwrap();
    scene.set_block_param(reg, rule, join, "within", n(0.2)).unwrap();
    for (k, v) in more {
        scene.set_block_param(reg, rule, join, k, v).unwrap();
    }
    let (_, log) = run(&scene, &kit, 3.0);
    fired(&log, rule, join)
}

#[test]
fn logic_joins_lanes_in_every_mode() {
    // Within 0.2 s = 48 steps; 0.5 s is step 120.
    assert_eq!(logic("and_within", &[0.5, 0.6], vec![]), [144]);
    assert_eq!(logic("and_within", &[0.6, 0.5], vec![]), [144]);
    assert!(logic("and_within", &[0.5, 0.8], vec![]).is_empty());
    assert_eq!(logic("and_since_last", &[0.5, 2.0], vec![]), [480]);
    assert_eq!(logic("then", &[0.5, 0.6], vec![]), [144]);
    assert!(logic("then", &[0.6, 0.5], vec![]).is_empty());
    assert!(logic("then", &[0.5, 0.8], vec![]).is_empty());
    assert_eq!(logic("or", &[0.5, 0.6], vec![]), [120, 144]);
    assert!(logic("not", &[0.5, 0.6], vec![]).is_empty());
    assert!(logic("not", &[0.5, 0.5], vec![]).is_empty(), "the same step closes it");
    assert_eq!(logic("not", &[0.5, 1.0], vec![]), [169]);
    assert_eq!(logic("not", &[0.6, 0.5], vec![]), [193], "only later inputs close it");
    assert_eq!(logic("any_n", &[0.5, 0.6, 2.0], vec![("n", i(2))]), [144]);
    assert_eq!(logic("any_n", &[0.5, 1.0, 2.0], vec![("n", i(2))]), Vec::<u64>::new());
    assert_eq!(logic("and_within", &[0.5, 0.6, 0.65], vec![]), [156]);
}

#[test]
fn change_motion_moves_the_ball() {
    let (mut scene, kit) = starter();
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.5, None),
            (
                "do.motion",
                vec![
                    ("who", who(the_ball())),
                    ("mode", choice("push")),
                    ("way", choice("angle")),
                    ("angle", n(90.0)),
                    ("amount", n(400.0)),
                ],
            ),
        ],
    );
    let (world, _) = run(&scene, &kit, 121.0 / 240.0);
    let v = world.bodies[&BALLS].velocity[0];
    let (plain, _) = run(&starter().0, &kit, 121.0 / 240.0);
    let d = v - plain.bodies[&BALLS].velocity[0];
    assert!((d.x - 400.0).abs() < 1e-3 && d.y.abs() < 1e-3, "{d}");
    let (mut scene, kit) = starter();
    lane(
        &mut scene,
        &kit.registry,
        &[time(0.5, None), ("do.motion", vec![("who", who(the_ball())), ("mode", choice("teleport"))])],
    );
    let (world, _) = run(&scene, &kit, 121.0 / 240.0);
    assert!(world.bodies[&BALLS].position[0].length() < 5.0, "to the firing's point, the middle, then one step on");
    let (mut scene, kit) = starter();
    lane(
        &mut scene,
        &kit.registry,
        &[time(0.5, None), ("do.motion", vec![("who", who(the_ball())), ("mode", choice("stop"))])],
    );
    let (world, _) = run(&scene, &kit, 121.0 / 240.0);
    assert!(world.bodies[&BALLS].velocity[0].length() < 3.0, "only one step's pull");
}

fn bounciness(world: &World, row: usize) -> f32 {
    state::<BounceState>(world.card_for(BALLS, SlotId::Bounce, row).unwrap()).settings.bounciness
}

#[test]
fn change_setting_changes_a_physics_setting_for_a_while() {
    let (mut scene, kit) = starter();
    let base = bounciness(&run(&scene, &kit, 0.0).0, 0);
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.5, None),
            (
                "do.change",
                vec![
                    ("who", who(the_ball())),
                    ("card", choice("bounce")),
                    ("setting", text("bounciness")),
                    ("to_number", n(0.25)),
                    ("glide", n(0.5)),
                    ("ease", choice("linear")),
                    ("lasts", choice("for_a_time")),
                    ("seconds", n(1.0)),
                ],
            ),
        ],
    );
    let at = |t: f64| bounciness(&run(&scene, &kit, t).0, 0);
    assert_eq!(at(0.5), base);
    let half = (base + 0.25) / 2.0;
    // Applied at step 121 (0.504 s): half way there 0.25 s later.
    assert!((at(121.0 / 240.0 + 0.25) - half).abs() < 1e-4);
    assert!((at(1.4) - 0.25).abs() < 1e-6);
    assert!((at(121.0 / 240.0 + 1.25) - half).abs() < 1e-4, "half way back");
    assert_eq!(at(2.5), base, "and back");
}

#[test]
fn a_change_adds_to_what_the_keyframes_give() {
    let (mut scene, kit) = starter();
    let a = bpe_core::Address::on(BALLS, SlotId::Bounce, "bounciness");
    scene.set_key(&kit.registry, &a, 0.0, n(0.5)).unwrap();
    scene.set_key(&kit.registry, &a, 2.0, n(0.9)).unwrap();
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.0, None),
            (
                "do.change",
                vec![
                    ("who", who(the_ball())),
                    ("card", choice("bounce")),
                    ("setting", text("bounciness")),
                    ("how", choice("add")),
                    ("to_number", n(0.05)),
                ],
            ),
        ],
    );
    let (keyed, _) = run(&scene, &kit, 1.0);
    let k = bounciness(&keyed, 0);
    scene.rules = Default::default();
    let (plain, _) = run(&scene, &kit, 1.0);
    assert!((k - bounciness(&plain, 0) - 0.05).abs() < 1e-5, "{k}");
}

#[test]
fn change_setting_for_one_ball_leaves_the_others() {
    let (mut scene, _, kit) = scene_with_balls("ring", |_| {});
    let third = who(Who::Thing { thing: BALLS, ball: Some(3) });
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.25, None),
            (
                "do.change",
                vec![
                    ("who", third.clone()),
                    ("setting", text("radius")),
                    ("to_number", n(40.0)),
                    ("lasts", choice("for_a_time")),
                    ("seconds", n(0.5)),
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
                "do.change",
                vec![
                    ("who", third),
                    ("card", choice("bounce")),
                    ("setting", text("bounciness")),
                    ("to_number", n(0.1)),
                ],
            ),
        ],
    );
    let (world, _) = run(&scene, &kit, 0.5);
    let b = &world.bodies[&BALLS];
    let row = b.row_of(2).unwrap();
    let base = b.radius[(row + 1) % b.len()];
    assert_eq!(b.radius[row], 40.0);
    assert!(b.radius.iter().enumerate().all(|(r, &x)| r == row || x == base));
    assert!((bounciness(&world, row) - 0.1).abs() < 1e-6);
    assert!(bounciness(&world, (row + 1) % b.len()) > 0.5);
    let (world, _) = run(&scene, &kit, 1.0);
    assert_eq!(world.bodies[&BALLS].radius[row], base, "back after half a second");
}

#[test]
fn a_change_naming_a_thing_without_the_setting_is_a_problem() {
    let (mut scene, kit) = starter();
    let (rule, _) = lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.0, None),
            (
                "do.change",
                vec![
                    ("who", who(Who::Thing { thing: SHAPE, ball: None })),
                    ("card", choice("motion")),
                    ("setting", text("bounciness")),
                ],
            ),
        ],
    );
    let resolved = resolve(&scene, &kit.registry);
    let world = World::build(&scene, &resolved, &kit.sims);
    assert!(world.rules().unwrap().problem(rule).unwrap().contains("no setting"));
}

#[test]
fn break_takes_a_ball_out() {
    let (mut scene, kit) = starter();
    lane(&mut scene, &kit.registry, &[("when.hit", vec![]), ("do.break", vec![])]);
    let (world, log) = run(&scene, &kit, 4.0);
    let kinds: Vec<EventKind> =
        log.events().iter().filter(|e| e.kind != EventKind::Rule).map(|e| e.kind.clone()).collect();
    assert_eq!(kinds, [EventKind::Hit, EventKind::Break, EventKind::Remove]);
    assert!(!world.bodies[&BALLS].alive[0]);
}

fn frame_at(scene: &Scene, kit: &Kit, time: f64) -> Frame {
    let resolved = resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    frame
}

#[test]
fn a_broken_shape_is_gone_from_the_physics_and_the_picture() {
    let (mut scene, kit) = starter();
    assert!(!frame_at(&scene, &kit, 2.0).paths.is_empty());
    lane(
        &mut scene,
        &kit.registry,
        &[time(0.1, None), ("do.break", vec![("who", who(Who::Thing { thing: SHAPE, ball: None }))])],
    );
    let (world, log) = run(&scene, &kit, 4.0);
    assert!(log.events().iter().all(|e| e.kind != EventKind::Hit));
    assert!(world.broken(SHAPE));
    assert!(frame_at(&scene, &kit, 2.0).paths.is_empty());
}

#[test]
fn one_balls_look_changes_alone() {
    let (mut scene, _, kit) = scene_with_balls("ring", |_| {});
    let plain = frame_at(&scene, &kit, 0.5);
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.25, None),
            (
                "do.change",
                vec![
                    ("who", who(Who::Thing { thing: BALLS, ball: Some(3) })),
                    ("card", choice("look")),
                    ("setting", text("0/opacity")),
                    ("to_number", n(0.25)),
                ],
            ),
        ],
    );
    let ruled = frame_at(&scene, &kit, 0.5);
    let before = frame_at(&scene, &kit, 0.2);
    assert_eq!(before.look_layers.len(), plain.look_layers.len());
    assert!(ruled.look_layers.len() > plain.look_layers.len(), "a stack of its own");
    assert_eq!(ruled.layered.len(), plain.layered.len());
}

#[test]
fn a_whole_look_change_shows() {
    let (mut scene, kit) = starter();
    let plain = frame_at(&scene, &kit, 0.5);
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.25, None),
            (
                "do.change",
                vec![
                    ("who", who(the_ball())),
                    ("card", choice("look")),
                    ("setting", text("0/opacity")),
                    ("to_number", n(0.25)),
                ],
            ),
        ],
    );
    assert_eq!(frame_at(&scene, &kit, 0.2).look_layers, plain.look_layers);
    assert_ne!(frame_at(&scene, &kit, 0.5).look_layers, plain.look_layers);
}

#[test]
fn rules_replay_the_same_through_seeks() {
    let (mut scene, _, kit) = scene_with_balls("ring", |_| {});
    lane(
        &mut scene,
        &kit.registry,
        &[("when.hit", vec![]), ("if.chance", vec![("percent", n(20.0))]), ("do.break", vec![])],
    );
    lane(
        &mut scene,
        &kit.registry,
        &[
            time(0.5, Some(0.5)),
            (
                "do.change",
                vec![
                    ("who", who(the_ball())),
                    ("card", choice("bounce")),
                    ("setting", text("bounciness")),
                    ("how", choice("multiply")),
                    ("times", n(0.9)),
                    ("glide", n(0.2)),
                ],
            ),
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
    assert!(straight.log().events().iter().any(|e| e.kind == EventKind::Break));
}

#[test]
fn blocks_settings_round_trip_through_their_structs() {
    use super::change::ChangeSettingSettings;
    let s = ChangeSettingSettings { setting: "radius".into(), ..Default::default() };
    assert_eq!(ChangeSettingSettings::from_params(&s.to_params()), s);
    assert_eq!(ThingId::ROOT, ThingId(0));
}

#[test]
fn a_ball_s_own_glide_gets_all_the_way_there() {
    for glide in [0.001, 0.33] {
        let (mut scene, _, kit) = scene_with_balls("ring", |_| {});
        lane(
            &mut scene,
            &kit.registry,
            &[
                time(0.25, None),
                (
                    "do.change",
                    vec![
                        ("who", who(Who::Thing { thing: BALLS, ball: Some(3) })),
                        ("card", choice("bounce")),
                        ("setting", text("bounciness")),
                        ("to_number", n(0.1)),
                        ("glide", n(glide)),
                    ],
                ),
            ],
        );
        let (world, _) = run(&scene, &kit, 1.0);
        let row = world.bodies[&BALLS].row_of(2).unwrap();
        assert!((bounciness(&world, row) - 0.1).abs() < 1e-6, "glide {glide}: {}", bounciness(&world, row));
    }
}

#[test]
fn breaking_the_group_breaks_its_balls() {
    let (mut scene, _, kit) = scene_with_balls("ring", |_| {});
    lane(&mut scene, &kit.registry, &[("when.hit", vec![]), ("do.break", vec![("who", who(Who::Group))])]);
    let (world, log) = run(&scene, &kit, 4.0);
    assert!(world.bodies[&BALLS].alive.iter().all(|a| !a));
    assert!(!world.broken(BALLS), "the thing stays");
    assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Break).count(), 24);
}

#[test]
fn a_broken_thing_is_no_target_any_more() {
    let (mut scene, kit) = starter();
    lane(
        &mut scene,
        &kit.registry,
        &[time(0.1, None), ("do.break", vec![("who", who(Who::Thing { thing: SHAPE, ball: None }))])],
    );
    let (rule, ids) = lane(
        &mut scene,
        &kit.registry,
        &[time(0.5, None), ("do.break", vec![("who", who(Who::Kind("shape".into())))])],
    );
    let (_, log) = run(&scene, &kit, 1.0);
    assert_eq!(fired(&log, rule, ids[1]), [120], "it fired");
    assert_eq!(log.events().iter().filter(|e| e.kind == EventKind::Break).count(), 1, "but broke nothing more");
}

#[test]
fn compare_reads_a_thing_too() {
    // The ball's hits so far, at each tick.
    let hits = |over: f64| {
        counted((
            "if.compare",
            vec![("value", text("hits")), ("of", choice("a_thing")), ("who", who(the_ball())), ("amount", n(over))],
        ))
    };
    assert!(hits(-1.0).len() == 17, "every tick: the ball has hits");
    assert!(hits(1e6).is_empty());
}
