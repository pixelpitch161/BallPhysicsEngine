//! The Rules stage with tiny test blocks, over the runtime's test scenes.

use std::sync::Arc;

use bpe_core::{Block, BlockId, BlockSection, Params, Rule, RuleId, Scene, ThingId, Value};
use glam::Vec2;

use super::*;
use crate::testing::{DOTS, WALL, dots_in_ring};
use crate::{EventKind, Log, Playback, Sims, ThingState, World, fingerprint, state};

/// Fires for every Hit of a ball: subject the ball, other what it hit, strength its speed.
struct Hits;
impl BlockSim for Hits {
    fn kind(&self) -> &'static str {
        "when.hits"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, _params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(()))
    }
    fn fire(&self, _state: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        for e in cx.events.iter().filter(|e| e.kind == EventKind::Hit) {
            let mut values = Values::default();
            values.set("strength", e.speed as f64);
            out.push(Firing {
                step: cx.step,
                subject: Some(e.a),
                other: e.b,
                point: e.point,
                normal: e.normal,
                values,
            });
        }
    }
}

/// Fires every `every` steps (an Int, default 1), about nothing.
struct Tick;
impl BlockSim for Tick {
    fn kind(&self) -> &'static str {
        "when.tick"
    }
    fn section(&self) -> BlockSection {
        BlockSection::When
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        match params.get("every") {
            Some(Value::Int(n)) if *n > 0 => Ok(Box::new(*n as u64)),
            Some(_) => Err("every must be a positive whole number".into()),
            None => Ok(Box::new(1u64)),
        }
    }
    fn fire(&self, state: &mut dyn ThingState, cx: &StageCx, out: &mut Vec<Firing>) {
        if cx.step.is_multiple_of(*crate::state::<u64>(state)) {
            out.push(Firing { step: cx.step, ..Firing::default() });
        }
    }
}

/// Lets odd steps through.
struct Odd;
impl BlockSim for Odd {
    fn kind(&self) -> &'static str {
        "if.odd"
    }
    fn section(&self) -> BlockSection {
        BlockSection::If
    }
    fn prepare(&self, _params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(()))
    }
    fn pass(&self, _state: &mut dyn ThingState, firing: &Firing, _cx: &StageCx) -> bool {
        firing.step % 2 == 1
    }
}

/// Passes on every firing, counting them.
struct Or;
impl BlockSim for Or {
    fn kind(&self) -> &'static str {
        "logic.or"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Logic
    }
    fn prepare(&self, _params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        Ok(Box::new(0u32))
    }
    fn join(
        &self,
        state: &mut dyn ThingState,
        _input: usize,
        _inputs: usize,
        firing: &Firing,
        _cx: &StageCx,
        out: &mut Vec<Firing>,
    ) {
        *crate::state_mut::<u32>(state) += 1;
        out.push(firing.clone());
    }
}

/// What a test Do block does, from its `does` Text.
#[derive(Clone)]
struct Does(String, f64);

/// A Do block whose `does` says what: "push" (adds `amount` speed to the subject), "spam" (5000
/// pushes), "gravity" (sets the scene's gravity up for `amount` seconds), "break wall", "break
/// ball", "show wall" and "show ball" (breaks whose look lasts `amount` seconds), "freeze" (`amount`
/// seconds), "tint" (adds to the dots' look-only Tint).
struct Act;
impl BlockSim for Act {
    fn kind(&self) -> &'static str {
        "do.test"
    }
    fn section(&self) -> BlockSection {
        BlockSection::Do
    }
    fn prepare(&self, params: &Params, _cx: &PrepareCx) -> Result<Box<dyn ThingState>, String> {
        let does = match params.get("does") {
            Some(Value::Text(t)) => t.clone(),
            _ => "push".into(),
        };
        let amount = match params.get("amount") {
            Some(Value::Number(x)) => *x,
            _ => 100.0,
        };
        Ok(Box::new(Does(does, amount)))
    }
    fn act(&self, state: &mut dyn ThingState, firing: &Firing, _cx: &StageCx, out: &mut Vec<Action>) {
        let Does(does, amount) = crate::state::<Does>(state).clone();
        let ball = firing.subject.filter(|p| p.thing == DOTS);
        let change = |how, value, lasts| Change {
            how,
            value,
            start: 0.0,
            glide: 0.0,
            ease: bpe_core::Ease::Linear,
            lasts,
            range: None,
        };
        match does.as_str() {
            "push" => out.push(Action::Motion {
                thing: DOTS,
                row: Some(ball.map_or(0, |p| p.item)),
                motion: MotionChange::AddSpeed(amount as f32),
            }),
            "spam" => {
                for _ in 0..5000 {
                    out.push(Action::Motion { thing: DOTS, row: Some(0), motion: MotionChange::AddSpeed(0.0) });
                }
            }
            "gravity" => out.push(Action::Change {
                key: ChangeKey { thing: ThingId::ROOT, slot: None, row: None, key: "gravity".into() },
                change: Box::new(change(How::Set, Value::Vec2([0.0, -500.0]), Some(amount))),
            }),
            "tint" => out.push(Action::Change {
                key: ChangeKey { thing: DOTS, slot: None, row: None, key: "tint".into() },
                change: Box::new(change(How::Add, Value::Number(amount), None)),
            }),
            "break wall" | "show wall" => out.push(Action::Break {
                thing: WALL,
                row: None,
                point: firing.point,
                normal: firing.normal,
                strength: 1.0,
                show: shown(&does, amount),
            }),
            "break ball" | "show ball" => out.push(Action::Break {
                thing: DOTS,
                row: Some(ball.map_or(0, |p| p.item)),
                point: firing.point,
                normal: firing.normal,
                strength: 1.0,
                show: shown(&does, amount),
            }),
            "freeze" => {
                out.push(Action::Motion { thing: DOTS, row: Some(0), motion: MotionChange::Freeze(amount as f32) })
            }
            _ => {}
        }
    }
    fn changes_settings(&self, state: &dyn ThingState) -> bool {
        matches!(crate::state::<Does>(state).0.as_str(), "gravity" | "tint")
    }
}

/// A "show …" break's look: `lasts` seconds; a "break …" one vanishes.
fn shown(does: &str, lasts: f64) -> Arc<BreakShow> {
    let lasts = if does.starts_with("show") { lasts } else { 0.0 };
    let mut look = Params::default();
    look.set("look", Value::Text("test".into()));
    Arc::new(BreakShow { look, lasts })
}

fn sims() -> Sims {
    let mut s = crate::testing::sims(true);
    s.add_block(Hits);
    s.add_block(Tick);
    s.add_block(Odd);
    s.add_block(Or);
    s.add_block(Act);
    s
}

fn block(id: u32, kind: &str, inputs: &[u32], params: &[(&str, Value)]) -> Block {
    let mut p = Params::default();
    for (k, v) in params {
        p.set(k, v.clone());
    }
    Block { id: BlockId(id), kind: kind.into(), inputs: inputs.iter().map(|&i| BlockId(i)).collect(), params: p }
}

fn rule(id: u32, blocks: Vec<Block>) -> Rule {
    Rule { id: RuleId(id), name: format!("Rule {id}"), enabled: true, next_block: blocks.len() as u32, blocks }
}

/// One dot in the ring, with these rules.
fn scene_with(rules: Vec<Rule>) -> (Scene, bpe_core::Resolved, Sims) {
    let (mut scene, resolved, _) = dots_in_ring(1);
    scene.rules.next_rule = rules.len() as u32;
    scene.rules.list = rules;
    (scene, resolved, sims())
}

fn push_on_hit(amount: f64) -> Rule {
    rule(
        0,
        vec![
            block(0, "when.hits", &[], &[]),
            block(1, "do.test", &[0], &[("does", Value::Text("push".into())), ("amount", Value::Number(amount))]),
        ],
    )
}

fn run(scene: &Scene, resolved: &bpe_core::Resolved, sims: &Sims, seconds: f64) -> (World, Log) {
    let mut world = World::build(scene, resolved, sims);
    let mut log = Log::default();
    world.advance_to(seconds, sims, &mut log);
    (world, log)
}

fn rule_events(log: &Log) -> Vec<&crate::Event> {
    log.events().iter().filter(|e| e.kind == EventKind::Rule).collect()
}

#[test]
fn a_scene_without_enabled_rules_runs_and_hashes_as_before() {
    let (scene, resolved, sims) = scene_with(Vec::new());
    let (plain_world, plain) = run(&scene, &resolved, &sims, 3.0);
    assert!(plain_world.rules().is_none());
    let mut off = push_on_hit(500.0);
    off.enabled = false;
    let (scene_off, _, _) = scene_with(vec![off]);
    let (world, log) = run(&scene_off, &resolved, &sims, 3.0);
    assert!(world.rules().is_none());
    assert_eq!(log.hash(), plain.hash());
    assert_eq!(fingerprint(&scene_off, &resolved, &sims), fingerprint(&scene, &resolved, &sims));
    let (scene_on, _, _) = scene_with(vec![push_on_hit(500.0)]);
    assert_ne!(fingerprint(&scene_on, &resolved, &sims), fingerprint(&scene, &resolved, &sims));
}

#[test]
fn actions_apply_at_the_next_steps_start() {
    let (scene, resolved, sims) = scene_with(vec![push_on_hit(300.0)]);
    let mut world = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    // Step until the first hit.
    while !log.events().iter().any(|e| e.kind == EventKind::Hit) {
        world.step(&sims, &mut log);
        assert!(world.step < 2400, "no hit in 10 s");
    }
    let hit_step = world.step;
    assert_eq!(world.rules().unwrap().pending().len(), 1, "queued, not applied");
    let speed = world.bodies[&DOTS].velocity[0].length();
    // The next step adds it before anything moves (then gravity pulls a step's worth).
    world.step(&sims, &mut log);
    let after = world.bodies[&DOTS].velocity[0].length();
    assert!(after > speed + 250.0, "{speed} then {after}");
    assert!(world.rules().unwrap().pending().is_empty());
    // One Rule event per block that fired, on the hit's step.
    let fired = rule_events(&log);
    assert_eq!(fired.len(), 2, "{fired:?}");
    assert!(fired.iter().all(|e| e.step == hit_step && e.a.thing == DOTS));
    assert_eq!(fired.iter().map(|e| e.b.unwrap().item).collect::<Vec<_>>(), [0, 1]);
}

#[test]
fn rule_events_name_the_rule_and_block_once_a_step() {
    let r = rule(
        3,
        vec![
            block(0, "when.tick", &[], &[]),
            block(1, "when.tick", &[], &[]),
            block(2, "logic.or", &[0, 1], &[]),
            block(5, "if.odd", &[2], &[]),
            block(6, "do.test", &[5], &[("does", Value::Text("nothing".into()))]),
        ],
    );
    let (scene, resolved, sims) = scene_with(vec![r]);
    let (world, log) = run(&scene, &resolved, &sims, 4.0 / 240.0);
    let fired: Vec<(u64, u32)> = rule_events(&log).iter().map(|e| (e.step, e.b.unwrap().item)).collect();
    let id = |b: u32| rule_item(RuleId(3), BlockId(b));
    assert_eq!(
        fired,
        [
            (1, id(0)),
            (1, id(1)),
            (1, id(2)),
            (1, id(5)),
            (1, id(6)),
            (2, id(0)),
            (2, id(1)),
            (2, id(2)),
            (3, id(0)),
            (3, id(1)),
            (3, id(2)),
            (3, id(5)),
            (3, id(6)),
            (4, id(0)),
            (4, id(1)),
            (4, id(2)),
        ]
    );
    assert!(rule_events(&log).iter().all(|e| e.a == crate::Party { thing: ThingId::ROOT, item: 0 }));
    // The Logic block saw both ticks of every step.
    let rules = world.rules().unwrap();
    assert_eq!(*state::<u32>(rules.rules[0].blocks[2].state.as_ref()), 8);
}

#[test]
fn a_busy_rule_stops_for_the_step_and_others_run_on() {
    let spam = rule(
        0,
        vec![block(0, "when.tick", &[], &[]), block(1, "do.test", &[0], &[("does", Value::Text("spam".into()))])],
    );
    let mut other = push_on_hit(10.0);
    other.id = RuleId(1);
    other.blocks[0] = block(0, "when.tick", &[], &[]);
    let (scene, resolved, sims) = scene_with(vec![spam, other]);
    let mut world = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    world.step(&sims, &mut log);
    let rules = world.rules().unwrap();
    assert_eq!(rules.busy_at(RuleId(0)), Some(1));
    assert_eq!(rules.busy_at(RuleId(1)), None);
    assert_eq!(rules.pending().len(), MOST_ACTIONS + 1, "the other rule's push is queued too");
}

#[test]
fn a_rule_with_a_problem_is_skipped_alone() {
    let broken = rule(
        0,
        vec![block(0, "when.tick", &[], &[]), block(1, "do.test", &[], &[("does", Value::Text("spam".into()))])],
    );
    let unknown = rule(1, vec![block(0, "when.mystery", &[], &[])]);
    let bad_setting = rule(2, vec![block(0, "when.tick", &[], &[("every", Value::Int(-3))])]);
    let circle =
        rule(3, vec![block(0, "when.tick", &[], &[]), block(1, "if.odd", &[2], &[]), block(2, "if.odd", &[1], &[])]);
    let gone_thing = rule(
        4,
        vec![block(
            0,
            "when.tick",
            &[],
            &[("who", Value::Who(bpe_core::Who::Thing { thing: ThingId(99), ball: None }))],
        )],
    );
    let mut fine = push_on_hit(10.0);
    fine.id = RuleId(5);
    let (scene, resolved, sims) = scene_with(vec![broken, unknown, bad_setting, circle, gone_thing, fine]);
    let (world, log) = run(&scene, &resolved, &sims, 3.0);
    let rules = world.rules().unwrap();
    for id in 0..5 {
        assert!(rules.problem(RuleId(id)).is_some(), "rule {id}");
    }
    assert_eq!(rules.problem(RuleId(2)), Some("every must be a positive whole number"));
    assert_eq!(rules.problem(RuleId(5)), None);
    assert!(!rule_events(&log).is_empty());
    assert!(rule_events(&log).iter().all(|e| e.b.unwrap().item >> 16 == 5), "only the good rule fires");
}

#[test]
fn a_change_moves_a_physics_setting_for_a_while_then_back() {
    // Every 240 steps, gravity points up for half a second.
    let r = rule(
        0,
        vec![
            block(0, "when.tick", &[], &[("every", Value::Int(240))]),
            block(1, "do.test", &[0], &[("does", Value::Text("gravity".into())), ("amount", Value::Number(0.5))]),
        ],
    );
    let (scene, resolved, sims) = scene_with(vec![r]);
    let mut world = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    let down = world.gravity;
    world.advance_to(1.0, &sims, &mut log);
    assert_eq!(world.gravity, down, "queued at step 240");
    world.step(&sims, &mut log);
    assert_eq!(world.gravity, Vec2::new(0.0, -500.0), "applied at step 241");
    world.advance_to(1.0 + 0.5, &sims, &mut log);
    assert_eq!(world.gravity, Vec2::new(0.0, -500.0));
    world.advance_to(1.0 + 0.5 + 2.0 / 240.0, &sims, &mut log);
    assert_eq!(world.gravity, down, "and back");
}

#[test]
fn a_look_change_waits_for_the_picture() {
    let r = rule(
        0,
        vec![
            block(0, "when.tick", &[], &[("every", Value::Int(10))]),
            block(1, "do.test", &[0], &[("does", Value::Text("tint".into())), ("amount", Value::Number(1.0))]),
        ],
    );
    let (scene, resolved, sims) = scene_with(vec![r]);
    let (world, _) = run(&scene, &resolved, &sims, 1.0);
    let rules = world.rules().unwrap();
    let key = ChangeKey { thing: DOTS, slot: None, row: None, key: "tint".into() };
    assert_eq!(rules.changes_on(&key).len(), 23, "every tenth step but the last, which waits");
    let mut copy = resolved.clone();
    rules.apply_whole(&mut copy, world.time(), |_| true);
    assert_eq!(copy.things[&DOTS].params.get("tint"), Some(&Value::Number(23.0)), "they add up");
    assert!(rules.changes_picture());
}

#[test]
fn breaking_the_wall_lets_the_ball_fall_through() {
    let r = rule(
        0,
        vec![block(0, "when.hits", &[], &[]), block(1, "do.test", &[0], &[("does", Value::Text("break wall".into()))])],
    );
    let (scene, resolved, sims) = scene_with(vec![r]);
    let (world, log) = run(&scene, &resolved, &sims, 4.0);
    let hits = log.events().iter().filter(|e| e.kind == EventKind::Hit).count();
    assert_eq!(hits, 1, "the first hit breaks it");
    let breaks: Vec<_> = log.events().iter().filter(|e| e.kind == EventKind::Break).collect();
    assert_eq!(breaks.len(), 1);
    assert_eq!(breaks[0].a, crate::Party { thing: WALL, item: WHOLE });
    assert!(world.broken(WALL) && !world.walls.contains_key(&WALL));
    assert!(world.bodies[&DOTS].position[0].y > 200.0, "fell out");
}

#[test]
fn breaking_a_ball_logs_break_then_remove() {
    let r = rule(
        0,
        vec![block(0, "when.hits", &[], &[]), block(1, "do.test", &[0], &[("does", Value::Text("break ball".into()))])],
    );
    let (scene, resolved, sims) = scene_with(vec![r]);
    let (world, log) = run(&scene, &resolved, &sims, 4.0);
    let kinds: Vec<&EventKind> = log.events().iter().filter(|e| e.kind != EventKind::Rule).map(|e| &e.kind).collect();
    assert_eq!(kinds, [&EventKind::Hit, &EventKind::Break, &EventKind::Remove]);
    assert!(!world.bodies[&DOTS].alive[0]);
    assert!(!world.broken(DOTS), "one ball, not the thing");
}

#[test]
fn a_break_with_a_look_keeps_its_remains_until_the_look_is_over() {
    let shows = |does: &str| {
        rule(
            0,
            vec![
                block(0, "when.hits", &[], &[]),
                block(1, "do.test", &[0], &[("does", Value::Text(does.into())), ("amount", Value::Number(0.5))]),
            ],
        )
    };
    // The wall: its remains keep the wall as it stood, while the physics has none.
    let (scene, resolved, sims) = scene_with(vec![shows("show wall")]);
    let sims = Arc::new(sims);
    let mut playback = Playback::new(&scene, &resolved, sims.clone());
    playback.seek(4.0);
    let broke = playback.log().events().iter().find(|e| e.kind == EventKind::Break).expect("a break").time(240);
    let mut p = Playback::new(&scene, &resolved, sims.clone());
    p.seek(broke + 0.1);
    let rules = p.world().rules().unwrap();
    let r = rules.remains_of(WALL, None).expect("kept");
    assert!(r.wall.is_some() && r.ball.is_none() && r.show.lasts == 0.5);
    assert!((r.time - broke).abs() < 1e-9);
    assert!(rules.showing(WALL, broke + 0.1) && !rules.showing(WALL, broke + 0.6));
    assert!(!p.world().walls.contains_key(&WALL), "gone from the physics at once");
    p.seek(broke + 0.6);
    assert!(p.world().rules().unwrap().remains_of(WALL, None).is_none(), "forgotten once the look is over");
    // Back before the break: none; forward again: the same remains.
    p.seek(broke - 0.2);
    assert!(p.world().rules().unwrap().remains().next().is_none());
    p.seek(broke + 0.1);
    let mut again = Playback::new(&scene, &resolved, sims.clone());
    again.seek(broke + 0.1);
    assert_eq!(
        p.world().rules().unwrap().remains_of(WALL, None),
        again.world().rules().unwrap().remains_of(WALL, None)
    );
    // A ball: its place and size as it broke.
    let (scene, resolved, sims) = scene_with(vec![shows("show ball")]);
    let (world, log) = run(&scene, &resolved, &sims, 4.0);
    let e = log.events().iter().find(|e| e.kind == EventKind::Break).expect("a break");
    let (w, _) = run(&scene, &resolved, &sims, e.time(240) + 0.01);
    let r = w.rules().unwrap().remains_of(DOTS, Some(0)).expect("kept");
    let b = r.ball.expect("a ball");
    assert!(b.radius > 0.0 && (b.position - e.point).length() < 2.0 * b.radius, "{b:?} {:?}", e.point);
    assert!(!w.bodies[&DOTS].alive[0]);
    assert!(world.rules().unwrap().remains().next().is_none(), "over by 4 s");
    // Vanish keeps nothing.
    let (scene, resolved, sims) = scene_with(vec![shows("break wall")]);
    let (w, _) = run(&scene, &resolved, &sims, 4.0);
    assert!(w.broken(WALL) && w.rules().unwrap().remains().next().is_none());
}

#[test]
fn a_frozen_ball_waits_then_goes_on_as_it_was() {
    let r = rule(
        0,
        vec![
            block(0, "when.tick", &[], &[("every", Value::Int(100))]),
            block(1, "do.test", &[0], &[("does", Value::Text("freeze".into())), ("amount", Value::Number(0.25))]),
        ],
    );
    let (scene, resolved, sims) = scene_with(vec![r]);
    let mut world = World::build(&scene, &resolved, &sims);
    let mut log = Log::default();
    world.advance_to(100.0 / 240.0, &sims, &mut log);
    let (at, going) = (world.bodies[&DOTS].position[0], world.bodies[&DOTS].velocity[0]);
    world.advance_to(150.0 / 240.0, &sims, &mut log);
    assert_eq!(world.bodies[&DOTS].position[0], at, "held");
    assert!(world.bodies[&DOTS].held[0]);
    world.advance_to(161.0 / 240.0, &sims, &mut log);
    assert!(!world.bodies[&DOTS].held[0], "let go after 60 steps");
    let v = world.bodies[&DOTS].velocity[0];
    assert!((v - going).length() < 30.0, "{going} then {v}");
}

#[test]
fn rules_replay_exactly_through_seeks_and_copies() {
    let mut r2 = rule(
        1,
        vec![
            block(0, "when.tick", &[], &[("every", Value::Int(120))]),
            block(1, "do.test", &[0], &[("does", Value::Text("gravity".into())), ("amount", Value::Number(0.3))]),
        ],
    );
    r2.name = "Flip".into();
    let (scene, resolved, sims) = scene_with(vec![push_on_hit(150.0), r2]);
    let sims = Arc::new(sims);
    let mut straight = Playback::new(&scene, &resolved, sims.clone());
    straight.seek(8.0);
    let mut jumpy = Playback::new(&scene, &resolved, sims);
    for t in [3.3, 1.1, 6.0, 2.0, 8.0] {
        jumpy.seek(t);
    }
    assert_eq!(straight.log().hash(), jumpy.log().hash());
    assert_eq!(straight.world().bodies[&DOTS].position, jumpy.world().bodies[&DOTS].position);
    assert!(straight.log().events().iter().filter(|e| e.kind == EventKind::Rule).count() > 10);
    // A lone copy carries no rules.
    assert!(straight.world().lone(&|_| true).rules().is_none());
}

#[test]
fn dice_are_steady_and_spread() {
    let a = dice(7, RuleId(1), BlockId(2), 30, None, 0);
    assert_eq!(a, dice(7, RuleId(1), BlockId(2), 30, None, 0));
    let all: Vec<f32> = (0..2000).map(|s| dice(7, RuleId(1), BlockId(2), s, None, 0)).collect();
    assert!(all.iter().all(|x| (0.0..1.0).contains(x)));
    let mean = all.iter().sum::<f32>() / all.len() as f32;
    assert!((mean - 0.5).abs() < 0.03, "{mean}");
    assert_ne!(a, dice(7, RuleId(2), BlockId(2), 30, None, 0));
}

#[test]
fn a_setting_keeps_at_most_most_changes() {
    let (scene, resolved) = (scene_with(vec![push_on_hit(1.0)]).0, dots_in_ring(1).1);
    let world = World::build(&scene, &resolved, &sims());
    let mut rules = RulesState::build(&scene, &resolved, &sims(), &world).unwrap();
    let key = ChangeKey { thing: DOTS, slot: None, row: None, key: "tint".into() };
    let add = |start: f64| Change {
        how: How::Add,
        value: Value::Number(1.0),
        start,
        glide: 0.0,
        ease: bpe_core::Ease::Linear,
        lasts: None,
        range: None,
    };
    for k in 0..500 {
        rules.add_change(key.clone(), add(k as f64));
    }
    let list = rules.changes_on(&key);
    assert!(list.len() <= MOST_CHANGES);
    assert_eq!(fold(list, &Value::Number(0.0), 1000.0), Value::Number(500.0), "nothing lost");
}

/// A Set for good hides what came before it at once, and what it hides goes a little later: the
/// sound engine, a step behind its physics, still folds it there (W13).
#[test]
fn a_set_for_good_hides_what_came_before_and_it_goes_a_little_later() {
    let (scene, resolved) = (scene_with(vec![push_on_hit(1.0)]).0, dots_in_ring(1).1);
    let world = World::build(&scene, &resolved, &sims());
    let mut rules = RulesState::build(&scene, &resolved, &sims(), &world).unwrap();
    let key = ChangeKey { thing: DOTS, slot: None, row: None, key: "tint".into() };
    let set = |start: f64, to: f64| Change {
        how: How::Set,
        value: Value::Number(to),
        start,
        glide: 0.0,
        ease: bpe_core::Ease::Linear,
        lasts: None,
        range: None,
    };
    rules.add_change(key.clone(), set(1.0, -60.0));
    rules.add_change(key.clone(), set(2.0, -60.0));
    let list = rules.changes_on(&key).to_vec();
    assert_eq!(list.len(), 2);
    assert_eq!(fold(&list, &Value::Number(0.0), 1.99), Value::Number(-60.0), "just before the second, the first");
    assert_eq!(fold(&list, &Value::Number(0.0), 2.0), Value::Number(-60.0));
    rules.prune(2.05);
    assert_eq!(rules.changes_on(&key).len(), 2, "kept while the sound may be behind");
    rules.prune(2.2);
    assert_eq!(rules.changes_on(&key).len(), 1);
}
