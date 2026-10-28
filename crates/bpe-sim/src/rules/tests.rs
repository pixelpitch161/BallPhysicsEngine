use super::*;
use crate::{Playback, Simulation, collect_wall_hits};
use bpe_graph::Link;
use bpe_scene::Scene;

fn preset(name: &str) -> Scene {
    Scene::presets().into_iter().find(|p| p.name == name).expect("preset").scene
}

fn run(scene: &Scene) -> Vec<Event> {
    collect_wall_hits(scene)
}

fn count(events: &[Event], pick: impl Fn(&EventKind) -> bool) -> usize {
    events.iter().filter(|e| pick(&e.kind)).count()
}

fn recipe(name: &str) -> Graph {
    recipes().into_iter().find(|r| r.name == name).expect("recipe").graph
}

#[test]
fn every_catalog_block_has_a_kind_and_every_recipe_is_valid() {
    for spec in catalog().all() {
        assert!(kind_of(&spec.kind).is_some(), "{} has no kind", spec.kind);
    }
    for recipe in recipes() {
        let problems = bpe_graph::validate(&recipe.graph, catalog());
        assert!(problems.is_empty(), "{}: {problems:?}", recipe.name);
    }
}

#[test]
fn a_scene_without_rules_is_unchanged() {
    let scene = preset("Plinko");
    assert!(RuleState::new(&scene).is_empty());
}

#[test]
fn escape_and_multiply_spawns_and_flashes_on_every_escape() {
    let mut scene = preset("Escape and multiply");
    scene.rules.spawn_per_escape = 0;
    scene.logic = vec![recipe("Escape and multiply")];
    let events = run(&scene);
    let escapes = count(&events, |k| matches!(k, EventKind::Escape(_)));
    let spawns = count(&events, |k| *k == EventKind::Spawn);
    assert!(escapes >= 3, "only {escapes} escapes");
    let actions: Vec<&Event> = events.iter().filter(|e| matches!(e.kind, EventKind::Action { .. })).collect();
    assert_eq!(actions.len(), escapes, "one flash per escape");
    assert!(spawns >= escapes * 2 - 2 && spawns <= escapes * 2, "{spawns} spawns for {escapes} escapes");
    let state = RuleState::new(&scene);
    assert!(matches!(state.output(actions[0]), Some(RuleOutput::Flash { .. })));
}

#[test]
fn rules_are_deterministic_and_seeking_agrees() {
    let mut scene = preset("Escape and multiply");
    scene.rules.spawn_per_escape = 0;
    scene.logic = vec![recipe("Escape and multiply"), recipe("Last ball standing")];
    let a = run(&scene);
    let b = run(&scene);
    assert_eq!(a, b);
    let mut playback = Playback::new(&scene);
    let end = scene.video.duration as f64;
    playback.seek(end * 0.9);
    playback.seek(end * 0.2);
    playback.seek(end);
    let straight: Vec<Event> = a.iter().copied().filter(|e| e.time <= playback.simulation().time() + 1e-9).collect();
    assert_eq!(playback.events(), &straight[..]);
}

#[test]
fn removed_balls_stop_and_count_down() {
    let mut scene = preset("Multiplying balls");
    let mut graph = chain("Remove", &[("wall", &[]), ("remove", &[])]);
    graph.name = "Remove on hit".into();
    scene.logic = vec![graph];
    let mut sim = Simulation::new(&scene);
    let mut events = Vec::new();
    sim.advance_to(5.0, &mut events);
    let removes = count(&events, |k| *k == EventKind::Remove);
    assert!(removes > 0);
    assert_eq!(sim.alive_balls() + removes, sim.balls().len());
    assert!(sim.balls().iter().filter(|b| !b.alive).all(|b| b.velocity == Vec2::ZERO));
}

#[test]
fn a_rule_that_triggers_itself_stays_within_the_budget_and_the_ball_cap() {
    let mut scene = preset("Growing ball");
    scene.rules.max_balls = 5000;
    scene.logic = vec![chain("Loop", &[("appear", &[]), ("spawn", &[("count", Param::Number(1.0))])])];
    let mut sim = Simulation::new(&scene);
    let mut events = Vec::new();
    for _ in 0..20 {
        let before = events.len();
        sim.step(&mut events);
        let spawns = events[before..].iter().filter(|e| e.kind == EventKind::Spawn).count();
        assert!(spawns as u32 <= ACTION_BUDGET + 1, "{spawns} spawns in a step");
    }
    assert!(sim.alive_balls() <= 5000);
    scene.rules.max_balls = 10;
    let mut sim = Simulation::new(&scene);
    sim.advance_to(2.0, &mut Vec::new());
    assert!(sim.alive_balls() <= 10);
}

#[test]
fn counters_add_up_and_watches_fire_once() {
    let mut scene = preset("Box arena");
    let mut graph = chain("Score", &[("wall", &[]), ("counter_add", &[("amount", Param::Number(2.0))])]);
    let reach = graph.add(catalog().spec("counter_reaches").unwrap(), [0.0, 300.0]);
    graph.node_mut(reach).unwrap().params.insert("value".into(), Param::Number(20.0));
    let flash = graph.add(catalog().spec("flash").unwrap(), [260.0, 300.0]);
    graph.connect(Link { from: (reach, 0), to: (flash, 0) }, catalog());
    scene.logic = vec![graph];
    let mut playback = Playback::new(&scene);
    playback.seek(6.0);
    let walls = count(playback.events(), |k| *k == EventKind::Wall);
    let counts = playback.counts_at(6.0);
    assert_eq!(counts.counters, vec![walls as f64 * 2.0]);
    assert_eq!(playback.simulation().rules().counters()[0], walls as f64 * 2.0);
    assert_eq!(playback.simulation().rules().counter_names(), ["score"]);
    let state = playback.simulation().rules();
    let flashes =
        playback.events().iter().filter(|e| matches!(state.output(e), Some(RuleOutput::Flash { .. }))).count();
    assert_eq!(flashes, 1);
}

#[test]
fn timers_and_moments_fire_on_time() {
    let mut scene = preset("Growing ball");
    scene.video.duration = 10.0;
    let mut graph = chain("Tick", &[("timer", &[("every", Param::Number(1.0))]), ("flash", &[])]);
    let at = graph.add(catalog().spec("at").unwrap(), [0.0, 300.0]);
    graph.node_mut(at).unwrap().params.insert("time".into(), Param::Number(4.5));
    let caption = graph.add(catalog().spec("caption").unwrap(), [260.0, 300.0]);
    graph.connect(Link { from: (at, 0), to: (caption, 0) }, catalog());
    scene.logic = vec![graph];
    let events = run(&scene);
    let state = RuleState::new(&scene);
    let flashes: Vec<f64> =
        events.iter().filter(|e| matches!(state.output(e), Some(RuleOutput::Flash { .. }))).map(|e| e.time).collect();
    // 0 s through 10 s, both ends included.
    assert_eq!(flashes.len(), 11, "{flashes:?}");
    assert!(flashes[0] < 0.01 && (flashes[3] - 3.0).abs() < 0.01, "{flashes:?}");
    let captions: Vec<f64> =
        events.iter().filter(|e| matches!(state.output(e), Some(RuleOutput::Caption { .. }))).map(|e| e.time).collect();
    assert_eq!(captions.len(), 1);
    assert!((captions[0] - 4.5).abs() < 0.01);
}

#[test]
fn chance_is_repeatable_and_roughly_right() {
    let mut scene = preset("Plinko");
    scene.logic = vec![chain("Coin", &[("peg", &[]), ("chance", &[("percent", Param::Number(25.0))]), ("note", &[])])];
    let a = run(&scene);
    assert_eq!(a, run(&scene));
    let pegs = count(&a, |k| matches!(k, EventKind::Obstacle(_)));
    let notes = count(&a, |k| matches!(k, EventKind::Action { .. }));
    let ratio = notes as f64 / pegs as f64;
    assert!((0.2..0.3).contains(&ratio), "{notes} of {pegs}");
}

#[test]
fn compare_goes_the_other_way_when_false() {
    let mut scene = preset("Growing ball");
    let mut graph = Graph::new("Compare");
    let c = catalog();
    let wall = graph.add(c.spec("wall").unwrap(), [0.0, 0.0]);
    let compare = graph.add(c.spec("compare").unwrap(), [260.0, 0.0]);
    let speed = graph.add(c.spec("ball_speed").unwrap(), [0.0, 200.0]);
    let fast = graph.add(c.spec("flash").unwrap(), [520.0, 0.0]);
    let slow = graph.add(c.spec("caption").unwrap(), [520.0, 200.0]);
    graph.node_mut(compare).unwrap().params.insert("than".into(), Param::Number(1e9));
    graph.connect(Link { from: (wall, 0), to: (compare, 0) }, c);
    graph.connect(Link { from: (speed, 0), to: (compare, 1) }, c);
    graph.connect(Link { from: (wall, 1), to: (speed, 0) }, c);
    graph.connect(Link { from: (compare, 0), to: (fast, 0) }, c);
    graph.connect(Link { from: (compare, 1), to: (slow, 0) }, c);
    assert!(bpe_graph::validate(&graph, c).is_empty(), "{:?}", bpe_graph::validate(&graph, c));
    scene.logic = vec![graph];
    let events = run(&scene);
    let state = RuleState::new(&scene);
    let walls = count(&events, |k| *k == EventKind::Wall);
    let captions = events.iter().filter(|e| matches!(state.output(e), Some(RuleOutput::Caption { .. }))).count();
    let flashes = events.iter().filter(|e| matches!(state.output(e), Some(RuleOutput::Flash { .. }))).count();
    assert_eq!((captions, flashes), (walls, 0));
}

#[test]
fn grow_on_every_hit_grows_up_to_the_limit() {
    let mut scene = preset("Growing ball");
    scene.rules.grow = 0.0;
    scene.logic = vec![recipe("Grow on every hit")];
    let start = Simulation::new(&scene).balls()[0].radius;
    let mut sim = Simulation::new(&scene);
    sim.advance_to(scene.video.duration as f64, &mut Vec::new());
    let radius = sim.balls()[0].radius;
    assert!(radius > start && radius <= 80.0, "{start} -> {radius}");
}

#[test]
fn switched_off_rule_sets_and_blocks_do_nothing() {
    let mut scene = preset("Escape and multiply");
    let plain = run(&scene);
    let mut graph = recipe("Escape and multiply");
    graph.enabled = false;
    scene.logic = vec![graph.clone()];
    assert_eq!(run(&scene), plain);
    graph.enabled = true;
    graph.nodes.iter_mut().for_each(|n| n.enabled = false);
    scene.logic = vec![graph];
    assert_eq!(run(&scene), plain);
}

#[test]
fn beats_fire_on_every_nth_and_downbeats() {
    let mut scene = preset("Growing ball");
    scene.video.duration = 8.0;
    scene.logic = vec![chain("Beat", &[("beat", &[("every", Param::Number(2.0))]), ("flash", &[])])];
    let beats: Vec<f64> = (0..16).map(|i| i as f64 * 0.5).collect();
    let inputs = crate::SimInputs { beats, music: None };
    let events = crate::collect_events(&scene, &inputs);
    assert_eq!(count(&events, |k| matches!(k, EventKind::Action { .. })), 8);
    scene.logic = vec![chain("Bar", &[("beat", &[("downbeats", Param::Bool(true))]), ("flash", &[])])];
    let events = crate::collect_events(&scene, &inputs);
    assert_eq!(count(&events, |k| matches!(k, EventKind::Action { .. })), 4);
}

/// Twenty rules on a busy scene must cost under 0.1 ms a step on top of the physics.
#[test]
#[cfg_attr(debug_assertions, ignore)]
fn twenty_rules_are_cheap() {
    let plain = preset("Plinko");
    let mut scene = plain.clone();
    let names = ["Escape and multiply", "Grow on every hit", "Beat confetti", "Chaos caption at 100 balls"];
    for i in 0..20 {
        let mut graph = recipe(names[i % names.len()]);
        if i % 4 == 1 {
            graph = chain("Pegs", &[("peg", &[]), ("chance", &[]), ("counter_add", &[])]);
        }
        scene.logic.push(graph);
    }
    scene.rules.max_balls = plain.rules.max_balls;
    let time = |scene: &Scene| {
        let mut best = f64::MAX;
        for _ in 0..3 {
            let mut sim = Simulation::new(scene);
            let mut events = Vec::new();
            let started = std::time::Instant::now();
            sim.advance_to(8.0, &mut events);
            best = best.min(started.elapsed().as_secs_f64());
        }
        best
    };
    let steps = 8.0 * plain.physics.steps_per_second as f64;
    let extra_ms = (time(&scene) - time(&plain)) / steps * 1000.0;
    assert!(extra_ms < 0.1, "rules cost {extra_ms:.4} ms a step");
}

#[test]
fn the_rules_preset_matches_its_recipe_and_multiplies() {
    let scene = preset("Escape and multiply (rules)");
    assert_eq!(scene.logic, vec![recipe("Escape and multiply")]);
    let events = run(&scene);
    let escapes = count(&events, |k| matches!(k, EventKind::Escape(_)));
    assert!(escapes > 3 && count(&events, |k| *k == EventKind::Spawn) > escapes);
}
