//! Health (rules part 4): the Health card, its damage, its blocks, Follow health and the Health
//! spread, with exact steps where the steps matter.

use bpe_core::{Address, Registry, RuleId, Scene, SlotId, Target, ThingId, Value, Who, resolve};
use bpe_gfx::{Frame, build_frame};
use bpe_runtime::rules::{WHOLE, rule_item};
use bpe_runtime::{Event, EventKind, Log, Playback, World, fingerprint, state};

use crate::testing::{BALLS, SHAPE, scene_with_balls};
use crate::{BounceState, Kit, fill_slot, kit, new_scene};

fn lane(scene: &mut Scene, reg: &Registry, blocks: &[(&str, Vec<(&str, Value)>)]) -> (RuleId, Vec<bpe_core::BlockId>) {
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
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
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

fn fired(log: &Log, rule: RuleId, block: bpe_core::BlockId) -> Vec<u64> {
    log.events()
        .iter()
        .filter(|e| e.kind == EventKind::Rule && e.b.is_some_and(|b| b.item == rule_item(rule, block)))
        .map(|e| e.step)
        .collect()
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn choice(c: &str) -> Value {
    Value::Choice(c.into())
}

fn who(w: Who) -> Value {
    Value::Who(w)
}

/// Gives `thing` a Health card with `set` on it.
fn health(scene: &mut Scene, reg: &Registry, thing: ThingId, set: &[(&str, Value)]) {
    fill_slot(scene, reg, thing, SlotId::Health).unwrap();
    let card = scene.card_mut(thing, SlotId::Health).unwrap();
    assert_eq!(card.kind, crate::modules::health::HEALTH);
    for (k, v) in set {
        card.params.set(k, v.clone());
    }
}

fn hits(log: &Log, of: ThingId) -> impl Iterator<Item = &Event> {
    log.events()
        .iter()
        .filter(move |e| e.kind == EventKind::Hit && (e.a.thing == of || e.b.is_some_and(|b| b.thing == of)))
}

#[test]
fn a_shape_with_health_breaks_the_step_after_its_third_hit() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    health(&mut scene, &kit.registry, SHAPE, &[]);
    let (world, log) = run(&scene, &kit, 8.0);
    let steps: Vec<u64> = hits(&log, SHAPE).map(|e| e.step).collect();
    assert_eq!(steps.len(), 3, "no more hits once it broke: {steps:?}");
    let broke: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Break).collect();
    assert_eq!(broke.len(), 1);
    assert_eq!((broke[0].a.thing, broke[0].a.item, broke[0].step), (SHAPE, WHOLE, steps[2] + 1));
    assert!(world.broken(SHAPE));
    // No rules at all: health alone keeps a rules state.
    assert!(scene.rules.list.is_empty() && world.rules().is_some());
    assert_eq!(world.health(SHAPE, None).map(|h| h.now), Some(0.0));
}

#[test]
fn nothing_at_zero_leaves_it_there_at_zero() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    health(&mut scene, &kit.registry, SHAPE, &[("at_zero", choice("nothing")), ("start", n(2.0))]);
    let (world, log) = run(&scene, &kit, 8.0);
    assert!(hits(&log, SHAPE).count() > 3);
    assert!(!world.broken(SHAPE) && log.events().iter().all(|e| e.kind != EventKind::Break));
    let hp = world.health(SHAPE, None).unwrap();
    assert_eq!((hp.now, hp.start, hp.share()), (0.0, 2.0, 0.0));
}

#[test]
fn hits_by_strength_take_more_for_a_harder_hit_and_soft_ones_nothing() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let set =
        [("takes", choice("by_strength")), ("factor", n(0.001)), ("start", n(100.0)), ("at_zero", choice("nothing"))];
    health(&mut scene, &kit.registry, BALLS, &set);
    let (_, log) = run(&scene, &kit, 3.0);
    let first = hits(&log, BALLS).next().expect("a hit").clone();
    let after = |t: f64| {
        let resolved = resolve(&scene, &kit.registry);
        let mut p = Playback::new(&scene, &resolved, kit.sims.clone());
        p.seek(t);
        p.world().health(BALLS, Some(0)).unwrap().now
    };
    let at = first.step as f64 / 240.0;
    assert_eq!(after(at - 1.0 / 240.0), 100.0);
    let taken = 100.0 - after(at);
    assert!((taken - first.speed as f64 * 0.001).abs() < 1e-9, "{taken} for a {} px/s hit", first.speed);
    // Hits softer than the least take nothing.
    let mut soft = scene.clone();
    soft.card_mut(BALLS, SlotId::Health).unwrap().params.set("least", n(1e8));
    let (world, _) = run(&soft, &kit, 3.0);
    assert_eq!(world.health(BALLS, Some(0)).unwrap().now, 100.0);
}

#[test]
fn only_hits_from_what_the_card_names_count() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    health(&mut scene, &kit.registry, BALLS, &[("from", Value::Target(Target::Tag("brick".into())))]);
    let (world, log) = run(&scene, &kit, 4.0);
    assert!(hits(&log, BALLS).count() > 3);
    assert_eq!(world.health(BALLS, Some(0)).unwrap().now, 3.0, "the ring isn't a brick");
    scene.things.get_mut(&SHAPE).unwrap().tags.insert("brick".into());
    let (world, _) = run(&scene, &kit, 4.0);
    assert!(!world.bodies[&BALLS].alive[0], "the ring is a brick now: the ball broke");
}

#[test]
fn each_ball_of_a_group_keeps_its_own_health() {
    let (mut scene, _, kit) = scene_with_balls("ring", |_| {});
    health(&mut scene, &kit.registry, BALLS, &[("start", n(2.0))]);
    let (world, log) = run(&scene, &kit, 6.0);
    let breaks: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Break).collect();
    assert!(!breaks.is_empty());
    for b in &breaks {
        assert_eq!(b.a.thing, BALLS);
        let before = log.events().iter().filter(|e| e.kind == EventKind::Hit && e.a == b.a && e.step < b.step);
        assert_eq!(before.count(), 2, "ball row {} broke on its second hit", b.a.item);
        assert!(!world.bodies[&BALLS].alive[b.a.item as usize]);
    }
    // A ball not hit twice yet is still there, with its own health.
    let bodies = &world.bodies[&BALLS];
    for row in (0..bodies.len()).filter(|&r| bodies.alive[r]) {
        let hp = world.health(BALLS, Some(row as u32)).unwrap();
        let hit =
            log.events().iter().filter(|e| e.kind == EventKind::Hit && e.a.thing == BALLS && e.a.item == row as u32);
        assert_eq!(hp.now, 2.0 - hit.count() as f64);
    }
}

#[test]
fn starting_health_can_vary_across_the_balls() {
    let (mut scene, _, kit) = scene_with_balls("ring", |_| {});
    let set = [("start", n(1.0)), ("start~to", n(5.0)), ("start~by", choice("number"))];
    health(&mut scene, &kit.registry, BALLS, &set);
    let (world, _) = run(&scene, &kit, 1.0 / 240.0);
    let bodies = &world.bodies[&BALLS];
    let last = bodies.len() - 1;
    let start_of = |number: u32| world.health(BALLS, Some(bodies.row_of(number).unwrap() as u32)).unwrap().start;
    assert_eq!(start_of(0), 1.0);
    assert_eq!(start_of(last as u32), 5.0);
    assert!(start_of(last as u32 / 2) > 2.0 && start_of(last as u32 / 2) < 4.0);
}

#[test]
fn when_health_fires_on_drops_and_zero_with_its_values() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    health(&mut scene, &kit.registry, SHAPE, &[("at_zero", choice("nothing"))]);
    let (drops, d) = lane(
        &mut scene,
        &kit.registry,
        &[("when.health", vec![]), ("if.compare", vec![("value", Value::Text("damage".into())), ("amount", n(0.5))])],
    );
    let (zero, z) = lane(
        &mut scene,
        &kit.registry,
        &[
            (
                "when.health",
                vec![("event", choice("reaches_zero")), ("who", who(Who::Thing { thing: SHAPE, ball: None }))],
            ),
            ("do.health", vec![("who", who(Who::That)), ("how", choice("refill"))]),
        ],
    );
    let (world, log) = run(&scene, &kit, 10.0);
    let hit_steps: Vec<u64> = hits(&log, SHAPE).map(|e| e.step).collect();
    assert_eq!(fired(&log, drops, d[0]), hit_steps, "a drop on every hit (in that step's stage)");
    assert_eq!(fired(&log, drops, d[1]), hit_steps, "each drop was one");
    // Every third hit empties it; the refill comes at the next step's start, so the fourth hit
    // finds it full again.
    let zeros: Vec<u64> = hit_steps.iter().copied().skip(2).step_by(3).collect();
    assert_eq!(fired(&log, zero, z[0]), zeros);
    assert_eq!(fired(&log, zero, z[1]), zeros);
    let expected = 3.0 - (hit_steps.len() % 3) as f64;
    assert_eq!(world.health(SHAPE, None).unwrap().now, expected);
}

#[test]
fn health_is_lets_through_only_low_health() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    health(&mut scene, &kit.registry, BALLS, &[("at_zero", choice("nothing"))]);
    let (rule, ids) = lane(
        &mut scene,
        &kit.registry,
        &[("when.hit", vec![]), ("if.health", vec![("test", choice("below")), ("amount", n(2.0))])],
    );
    let (_, log) = run(&scene, &kit, 6.0);
    let steps: Vec<u64> = hits(&log, BALLS).map(|e| e.step).collect();
    assert!(steps.len() > 3);
    // After the first hit it has 2 (not below 2); from the second on, 1 and then 0.
    assert_eq!(fired(&log, rule, ids[1]), steps[1..].to_vec());
    // In percent, and At zero.
    let mut pct = new_scene(&kit.registry);
    health(&mut pct, &kit.registry, BALLS, &[("at_zero", choice("nothing"))]);
    let (r2, i2) =
        lane(&mut pct, &kit.registry, &[("when.hit", vec![]), ("if.health", vec![("test", choice("at_zero"))])]);
    let (_, log) = run(&pct, &kit, 6.0);
    assert_eq!(fired(&log, r2, i2[1]), steps[2..].to_vec());
}

#[test]
fn change_health_heals_and_damages_and_breaks_at_zero_at_once() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    health(&mut scene, &kit.registry, BALLS, &[("takes", choice("nothing"))]);
    let ball = who(Who::Thing { thing: BALLS, ball: None });
    lane(
        &mut scene,
        &kit.registry,
        &[
            ("when.time", vec![("at", n(0.5))]),
            ("do.health", vec![("who", ball.clone()), ("how", choice("heal")), ("amount", n(4.0))]),
        ],
    );
    lane(
        &mut scene,
        &kit.registry,
        &[
            ("when.time", vec![("at", n(1.0))]),
            ("do.health", vec![("who", ball), ("how", choice("damage")), ("amount", n(7.0))]),
        ],
    );
    let (world, _) = run(&scene, &kit, 0.75);
    assert_eq!(world.health(BALLS, Some(0)).unwrap().now, 7.0, "healing goes past the starting health");
    let (world, log) = run(&scene, &kit, 1.5);
    let broke = log.events().iter().find(|e| e.kind == EventKind::Break).expect("it broke");
    // The Time block fires in the stage of step 240; the damage and the Break both at 241's start.
    assert_eq!(broke.step, 241);
    assert!(!world.bodies[&BALLS].alive[0]);
}

#[test]
fn health_blocks_naming_a_thing_without_health_are_a_problem() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let (rule, _) = lane(
        &mut scene,
        &kit.registry,
        &[("when.time", vec![]), ("do.health", vec![("who", who(Who::Thing { thing: SHAPE, ball: None }))])],
    );
    let resolved = resolve(&scene, &kit.registry);
    let world = World::build(&scene, &resolved, &kit.sims);
    let problem = world.rules().unwrap().problem(rule).unwrap();
    assert!(problem.contains("has no Health card"), "{problem}");
    // That ball, without a Health card: nothing happens, and no problem.
    let mut quiet = new_scene(&kit.registry);
    lane(&mut quiet, &kit.registry, &[("when.hit", vec![]), ("do.health", vec![])]);
    let (world, log) = run(&quiet, &kit, 3.0);
    assert!(world.health(BALLS, Some(0)).is_none() && log.events().iter().all(|e| e.kind != EventKind::Break));
}

fn bounciness(world: &World) -> f32 {
    state::<BounceState>(world.card_for(BALLS, SlotId::Bounce, 0).unwrap()).settings.bounciness
}

#[test]
fn a_physics_setting_can_follow_health() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    health(&mut scene, &kit.registry, SHAPE, &[("start", n(4.0)), ("at_zero", choice("nothing"))]);
    scene.card_mut(BALLS, SlotId::Bounce).unwrap().params.set("bounciness", n(0.5));
    let a = Address::on(BALLS, SlotId::Bounce, "bounciness");
    scene.set_pulse(&kit.registry, &a, bpe_core::PulseSource::Health { thing: SHAPE }, n(0.4), 0.25).unwrap();
    let (_, log) = run(&scene, &kit, 3.0);
    let first = hits(&log, SHAPE).next().unwrap().step;
    let at = |step: u64| run(&scene, &kit, step as f64 / 240.0).0;
    let built = World::build(&scene, &resolve(&scene, &kit.registry), &kit.sims);
    assert!((bounciness(&built) - 0.9).abs() < 1e-6, "built at full health");
    assert!((bounciness(&at(first)) - 0.9).abs() < 1e-6, "full health: all of it");
    let w = at(first + 1);
    assert_eq!(w.health_levels().unwrap().share(SHAPE), 0.75);
    assert!((bounciness(&w) - 0.8).abs() < 1e-6, "{}", bounciness(&w));
    // A test flight's copy keeps the health as it is, and hits there take none.
    let mut lone = w.lone(&|id| id != BALLS);
    assert_eq!(lone.health_levels().unwrap().share(SHAPE), 0.75);
    lone.advance_to(3.0, &kit.sims, &mut Log::default());
    assert_eq!(lone.health_levels().unwrap().share(SHAPE), 0.75);
    // Taking the Health card away takes Follow health of it too.
    let mut without = scene.clone();
    assert!(without.remove_slot(SHAPE, SlotId::Health));
    assert!(without.pulses.get(&a).is_none());
    // It changes the run, so it changes the fingerprint; so does a Health card.
    let resolved = resolve(&scene, &kit.registry);
    let plain = new_scene(&kit.registry);
    assert_ne!(
        fingerprint(&scene, &resolved, &kit.sims),
        fingerprint(&plain, &resolve(&plain, &kit.registry), &kit.sims)
    );
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
fn a_look_setting_can_follow_health() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    health(&mut scene, &kit.registry, SHAPE, &[("start", n(4.0)), ("at_zero", choice("nothing"))]);
    let (_, log) = run(&scene, &kit, 3.0);
    let first = hits(&log, SHAPE).next().unwrap().step as f64 / 240.0;
    let (before, after) = (first - 0.1, first + 0.1);
    let plain = (frame_at(&scene, &kit, before).look_layers, frame_at(&scene, &kit, after).look_layers);
    assert_eq!(plain.0, plain.1, "the look stays put by itself");
    let a = Address::on(BALLS, SlotId::Look, "0/opacity");
    scene.set_pulse(&kit.registry, &a, bpe_core::PulseSource::Health { thing: SHAPE }, n(-0.5), 0.25).unwrap();
    let followed = (frame_at(&scene, &kit, before).look_layers, frame_at(&scene, &kit, after).look_layers);
    assert_ne!(followed.0, followed.1, "the hit took health, so the look moved");
}

#[test]
fn the_balls_size_can_vary_by_health() {
    let (mut scene, _, kit) = scene_with_balls("ring", |p| {
        p.set("radius~to", n(6.0));
        p.set("radius~by", choice("health"));
    });
    scene.card_mut(BALLS, SlotId::Bounce).unwrap().params.set("grow", n(0.0));
    health(&mut scene, &kit.registry, BALLS, &[("start", n(4.0)), ("at_zero", choice("nothing"))]);
    let (world, log) = run(&scene, &kit, 3.0);
    let bodies = &world.bodies[&BALLS];
    let mut hit_rows = 0;
    for row in 0..bodies.len() {
        let hurt = world.hurt(BALLS, row);
        let hit = |e: &&Event| e.kind == EventKind::Hit && e.a.thing == BALLS && e.a.item == row as u32;
        assert_eq!(hurt, (log.events().iter().filter(hit).count().min(4) as f32) / 4.0);
        // Sizes follow health as the step before left it (the stage comes after the moves).
        let before = log.events().iter().filter(hit).filter(|e| e.step < world.step).count();
        let want = 24.0 + (6.0 - 24.0) * (before.min(4) as f32 / 4.0);
        assert!((bodies.radius[row] - want).abs() < 1e-3, "row {row}: {} for {want}", bodies.radius[row]);
        hit_rows += (hurt > 0.0) as usize;
    }
    assert!(hit_rows > 0, "some ball was hit");
}

#[test]
fn a_card_setting_can_vary_by_health() {
    let (mut scene, _, kit) = scene_with_balls("ring", |_| {});
    {
        let p = &mut scene.card_mut(BALLS, SlotId::Bounce).unwrap().params;
        p.set("bounciness", n(1.0));
        p.set("bounciness~to", n(0.2));
        p.set("bounciness~by", choice("health"));
    }
    health(&mut scene, &kit.registry, BALLS, &[("start", n(2.0)), ("at_zero", choice("nothing"))]);
    let (world, _) = run(&scene, &kit, 3.0);
    let bodies = &world.bodies[&BALLS];
    let mut seen = std::collections::BTreeSet::new();
    for row in 0..bodies.len() {
        let hurt = world.hurt(BALLS, row);
        let b = state::<BounceState>(world.card_for(BALLS, SlotId::Bounce, row).unwrap()).settings.bounciness;
        assert!((b - (1.0 - 0.8 * hurt)).abs() < 2e-3, "row {row}: {b} at hurt {hurt}");
        seen.insert((hurt * 2.0) as u32);
    }
    assert!(seen.len() > 1, "balls at different health: {seen:?}");
}

#[test]
fn health_replays_the_same_through_seeks() {
    let (mut scene, _, kit) = scene_with_balls("ring", |_| {});
    health(&mut scene, &kit.registry, BALLS, &[("start", n(3.0))]);
    lane(
        &mut scene,
        &kit.registry,
        &[("when.health", vec![]), ("do.motion", vec![("mode", choice("add_speed")), ("amount", n(50.0))])],
    );
    let resolved = resolve(&scene, &kit.registry);
    let mut straight = Playback::new(&scene, &resolved, kit.sims.clone());
    straight.seek(8.0);
    let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
    for t in [5.0, 1.0, 6.5, 8.0] {
        jumpy.seek(t);
    }
    assert_eq!(straight.log().hash(), jumpy.log().hash());
    assert!(straight.log().events().iter().any(|e| e.kind == EventKind::Break));
}

#[test]
fn an_emitters_balls_have_health_each() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let emitter = crate::add_thing(&mut scene, &kit.registry, "emitter", ThingId::ROOT).unwrap();
    assert!(!scene.things[&emitter].slots.contains_key(&SlotId::Health), "no health until it is added");
    health(&mut scene, &kit.registry, emitter, &[("start", n(1.0))]);
    let (world, log) = run(&scene, &kit, 6.0);
    let fired = world.bodies.get(&emitter).map_or(0, |b| b.len());
    assert!(fired > 0);
    let breaks: Vec<&Event> = log.events().iter().filter(|e| e.kind == EventKind::Break).collect();
    assert!(!breaks.is_empty(), "a fired ball hit the ring and broke");
    for b in breaks {
        assert_eq!(b.a.thing, emitter);
        let first_hit = log.events().iter().find(|e| e.kind == EventKind::Hit && e.a == b.a).expect("hit first");
        assert_eq!(b.step, first_hit.step + 1);
    }
}
