//! Rules part 6: the spectacle actions doing what the plan says, with exact steps.

use bpe_core::{BlockId, Registry, RuleId, Scene, SlotId, Value, Who, resolve};
use bpe_gfx::{Frame, build_frame};
use bpe_runtime::rules::AS_IF_HIT;
use bpe_runtime::{EventKind, Log, Playback, World, step_at};

use crate::testing::{BALLS, SHAPE, scene_with_balls};
use crate::{Kit, kit, new_scene};

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

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn choice(c: &str) -> Value {
    Value::Choice(c.into())
}

fn who(w: Who) -> Value {
    Value::Who(w)
}

/// When the clock reaches `at` (fires on that step; its actions apply on the next).
fn at(t: f64) -> (&'static str, Vec<(&'static str, Value)>) {
    ("when.time", vec![("at", n(t))])
}

/// The step a Time block at `t` acts on: the one after it fires.
fn acts_on(t: f64) -> u64 {
    step_at(t, 240) + 1
}

// --- Camera -------------------------------------------------------------------------------

#[test]
fn a_shake_moves_the_shot_then_dies_away() {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    crate::ensure_camera(&mut scene, reg);
    lane(&mut scene, reg, &[at(0.5), ("do.camera", vec![("amount", n(50.0)), ("seconds", n(0.4))])]);
    let (_, log) = run(&scene, &kit, 1.5);
    let start = acts_on(0.5);
    let still = log.camera_at(start - 1);
    let moved: Vec<f32> = (start..start + 48).map(|s| (log.camera_at(s).center - still.center).length()).collect();
    assert!(moved.iter().any(|d| *d > 5.0), "it shook: {moved:?}");
    assert!(moved.iter().all(|d| *d <= 50.0 * 1.5), "never past its strength: {moved:?}");
    assert_eq!(log.camera_at(step_at(1.0, 240)), still, "gone after its time");
}

#[test]
fn a_zoom_punch_zooms_in_at_once_and_eases_back() {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    crate::ensure_camera(&mut scene, reg);
    let punch = vec![("mode", choice("zoom_punch")), ("punch", n(0.5)), ("seconds", n(1.0))];
    lane(&mut scene, reg, &[at(0.5), ("do.camera", punch)]);
    let (_, log) = run(&scene, &kit, 2.0);
    let start = acts_on(0.5);
    assert_eq!(log.camera_at(start - 1).zoom, 1.0);
    assert!((log.camera_at(start).zoom - 1.5).abs() < 1e-6, "{}", log.camera_at(start).zoom);
    let half = log.camera_at(start + 120).zoom;
    assert!((half - 1.125).abs() < 1e-3, "a quarter left half way: {half}");
    assert_eq!(log.camera_at(step_at(1.6, 240)).zoom, 1.0);
}

#[test]
fn a_camera_block_naming_a_ball_is_a_problem() {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    let ball = Who::Thing { thing: BALLS, ball: None };
    let (rule, _) = lane(&mut scene, reg, &[at(0.5), ("do.camera", vec![("who", who(ball))])]);
    let resolved = resolve(&scene, reg);
    let world = World::build(&scene, &resolved, &kit.sims);
    let problem = world.rules().unwrap().problem(rule).unwrap_or_default().to_owned();
    assert!(problem.contains("isn't a camera"), "{problem}");
}

// --- Slow motion --------------------------------------------------------------------------

/// The new scene with its ball falling freely (no shape), and `rule` added when given.
fn falling(slow: Option<Vec<(&'static str, Value)>>) -> (Scene, Kit) {
    let (mut scene, _, kit) = scene_with_balls("single", |p| p.set("velocity", Value::Vec2([0.0, 0.0])));
    scene.things.get_mut(&SHAPE).unwrap().enabled = false;
    if let Some(set) = slow {
        lane(&mut scene, &kit.registry, &[at(0.0), ("do.slow", set)]);
    }
    (scene, kit)
}

#[test]
fn slow_motion_slows_the_balls_while_the_clock_runs_on() {
    let (plain, kit) = falling(None);
    let (slowed, _) = falling(Some(vec![("speed", n(0.5)), ("glide", n(0.0)), ("lasts", choice("for_good"))]));
    let (w0, _) = run(&plain, &kit, 1.0);
    let (w1, _) = run(&slowed, &kit, 1.0);
    let fell = |w: &World| w.bodies[&BALLS].position[0].y;
    let ratio = fell(&w1) / fell(&w0);
    // Half the speed gained over half the time each step: a quarter of the fall.
    assert!((ratio - 0.25).abs() < 0.01, "{ratio}");
    assert_eq!(w1.step, w0.step, "the video clock runs on");
    assert_eq!(w1.pace(), 0.5);
    assert_eq!(w0.pace(), 1.0);
    let lag = w1.rules().unwrap().lag();
    assert!((lag - 0.5 * (1.0 - 1.0 / 240.0)).abs() < 1e-6, "{lag}");
    assert!((w1.sim_time() - (1.0 - lag)).abs() < 1e-9);
}

#[test]
fn slow_motion_for_a_time_glides_back() {
    let set = vec![("speed", n(0.25)), ("glide", n(0.1)), ("lasts", choice("for_a_time")), ("seconds", n(0.5))];
    let (scene, kit) = falling(Some(set));
    let (w, _) = run(&scene, &kit, 0.3);
    assert!((w.pace() - 0.25).abs() < 1e-6, "in by now: {}", w.pace());
    let (w, _) = run(&scene, &kit, 1.0);
    assert_eq!(w.pace(), 1.0, "back after 0.5 s and the glide");
    assert!(w.rules().unwrap().lag() > 0.3);
}

#[test]
fn slow_motion_slows_a_spinning_shape_too() {
    let spin = |slow: bool| {
        let (mut scene, _, kit) = scene_with_balls("single", |_| {});
        scene.things.get_mut(&SHAPE).unwrap().params.set("spin", n(90.0));
        if slow {
            let set = vec![("speed", n(0.5)), ("glide", n(0.0)), ("lasts", choice("for_good"))];
            lane(&mut scene, &kit.registry, &[at(0.0), ("do.slow", set)]);
        }
        let (w, _) = run(&scene, &kit, 2.0);
        w.poses[&SHAPE].angle
    };
    let ratio = spin(true) / spin(false);
    assert!((ratio - 0.5).abs() < 0.01, "{ratio}");
}

// --- Flash --------------------------------------------------------------------------------

#[test]
fn a_flash_fires_a_balls_hit_layers_without_a_hit() {
    use crate::modules::look::layers::{OUTLINE, SUB_FLASH};
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    let outline = scene.add_layer(reg, BALLS, SlotId::Look, OUTLINE).unwrap();
    scene.add_sub_layer(reg, BALLS, SlotId::Look, outline, SUB_FLASH).unwrap();
    let ball = Who::Thing { thing: BALLS, ball: None };
    lane(&mut scene, reg, &[at(0.25), ("do.flash", vec![("who", who(ball)), ("strength", n(0.5))])]);
    let resolved = resolve(&scene, reg);
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let t = acts_on(0.25) as f64 / 240.0 + 0.004;
    playback.seek(t);
    let world = playback.world();
    let flashes: Vec<_> = playback.log().events().iter().filter(|e| e.kind == EventKind::Flash).collect();
    assert_eq!(flashes.len(), 1);
    assert_eq!((flashes[0].step, flashes[0].a.thing, flashes[0].a.item), (acts_on(0.25), BALLS, 0));
    assert_eq!(world.rules().unwrap().flashes(BALLS, 0), 1);
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, world, playback.log(), t, &kit.draws, &mut frame);
    let hit = frame.layered[0].hit;
    assert!(hit[0] < 0.01, "just flashed: {hit:?}");
    assert!((hit[1] - 0.5).abs() < 1e-4, "at its strength: {hit:?}");
    assert_eq!(hit[2], world.bodies[&BALLS].hits[0] as f32 + 1.0, "a hit more for Colour on hit");
    assert_eq!(
        playback.log().events().iter().filter(|e| e.kind == EventKind::Hit && e.step >= acts_on(0.25)).count(),
        0
    );
}

#[test]
fn a_flash_fires_a_shapes_hit_layers() {
    use crate::modules::look::shape as look;
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    let paint = scene.add_layer(reg, SHAPE, SlotId::Look, look::PAINT).unwrap();
    scene.add_sub_layer(reg, SHAPE, SlotId::Look, paint, look::SUB_HUE).unwrap();
    let shape = Who::Thing { thing: SHAPE, ball: None };
    lane(&mut scene, reg, &[at(0.25), ("do.flash", vec![("who", who(shape))])]);
    let resolved = resolve(&scene, reg);
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let t = acts_on(0.25) as f64 / 240.0 + 0.004;
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
    let hit = frame.paths[0].hit;
    let hits = playback
        .log()
        .events()
        .iter()
        .filter(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == SHAPE))
        .count();
    assert!(hit[0] < 0.01, "just flashed: {hit:?}");
    assert_eq!(hit[2], hits as f32 + 1.0);
}

// --- Lighting scene -----------------------------------------------------------------------

#[test]
fn a_rule_shows_lighting_scenes_and_hands_back_to_the_card() {
    use crate::modules::light_scenes;
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = crate::scenes::lit_room(reg);
    let light = scene.things.values().find(|t| t.kind == "light").unwrap().id;
    let strength = bpe_core::Address::own(light, "strength");
    for s in [1.0, 3.0] {
        scene.set_at(reg, &strength, n(s)).unwrap();
        light_scenes::add_scene(&mut scene, reg, 0.0).unwrap();
    }
    scene.set_at(reg, &strength, n(2.0)).unwrap();
    let own = |fade: f64| vec![("crossfade", choice("its_own")), ("fade", n(fade))];
    let mut two = own(1.0);
    two.push(("scene", Value::Int(2)));
    lane(&mut scene, reg, &[at(1.0), ("do.light_scene", two)]);
    let mut back = own(0.0);
    back.push(("show", choice("back_to_the_card")));
    lane(&mut scene, reg, &[at(3.0), ("do.light_scene", back)]);
    let mut next = own(0.0);
    next.push(("show", choice("next_scene")));
    lane(&mut scene, reg, &[at(4.0), ("do.light_scene", next)]);
    let resolved = resolve(&scene, reg);
    let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut seen = |t: f64| {
        playback.seek(t);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
        frame.light.expect("lit").lights[0].p1.iter().copied().fold(0.0, f32::max)
    };
    let s: Vec<f32> = [0.5, 1.5, 2.5, 3.5, 4.5].into_iter().map(&mut seen).collect();
    assert!((s[0] - 2.0).abs() < 0.05, "its own before: {s:?}");
    assert!((s[1] - 2.5).abs() < 0.05, "half way to scene 2: {s:?}");
    assert!((s[2] - 3.0).abs() < 0.05, "scene 2: {s:?}");
    assert!((s[3] - 2.0).abs() < 0.05, "the card shows nothing: its own again: {s:?}");
    assert!((s[4] - 1.0).abs() < 0.05, "next after none is scene 1: {s:?}");
}

// --- Play a note --------------------------------------------------------------------------

#[test]
fn play_a_note_logs_it_on_the_shape() {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    let shape = Who::Thing { thing: SHAPE, ball: None };
    let set = vec![("who", who(shape.clone())), ("note", Value::Int(64)), ("velocity", n(0.5)), ("length", n(0.25))];
    lane(&mut scene, reg, &[at(0.5), ("do.note", set)]);
    let next = vec![("who", who(shape)), ("pitch_from", choice("the_shapes_next_note"))];
    lane(&mut scene, reg, &[at(0.75), ("do.note", next)]);
    let (_, log) = run(&scene, &kit, 1.0);
    let notes: Vec<_> = log.events().iter().filter(|e| e.kind == EventKind::Note).collect();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].step, acts_on(0.5));
    assert_eq!(notes[0].b.map(|b| (b.thing, b.item)), Some((SHAPE, 64)));
    assert_eq!((notes[0].speed, notes[0].normal.x), (0.5, 0.25));
    assert_eq!(notes[1].b.map(|b| b.item), Some(AS_IF_HIT));
}

#[test]
fn play_a_note_on_a_ball_is_a_problem() {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    let ball = Who::Thing { thing: BALLS, ball: None };
    let (rule, _) = lane(&mut scene, reg, &[at(0.5), ("do.note", vec![("who", who(ball))])]);
    let world = World::build(&scene, &resolve(&scene, reg), &kit.sims);
    assert!(world.rules().unwrap().problem(rule).is_some_and(|p| p.contains("isn't a shape")));
}

#[test]
fn spectacle_runs_replay_the_same_after_a_seek() {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_scene(reg);
    crate::ensure_camera(&mut scene, reg);
    let slow = vec![("speed", n(0.3)), ("lasts", choice("for_a_time")), ("seconds", n(0.6))];
    let shape = Who::Thing { thing: SHAPE, ball: None };
    let rule = scene.add_rule("On hits");
    let when = scene.add_block(reg, rule, "when.hit", None).unwrap();
    for (kind, set) in
        [("do.slow", slow), ("do.camera", vec![]), ("do.flash", vec![]), ("do.note", vec![("who", who(shape))])]
    {
        let id = scene.add_block(reg, rule, kind, Some(when)).unwrap();
        for (k, v) in set {
            scene.set_block_param(reg, rule, id, k, v).unwrap();
        }
    }
    let resolved = resolve(&scene, reg);
    let mut straight = Playback::new(&scene, &resolved, kit.sims.clone());
    straight.seek(6.0);
    let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
    for t in [2.0, 0.5, 4.5, 6.0] {
        jumpy.seek(t);
    }
    assert_eq!(straight.log().hash(), jumpy.log().hash());
    assert!(straight.log().events().iter().any(|e| e.kind == EventKind::Note));
    assert!(straight.log().events().iter().any(|e| e.kind == EventKind::Flash));
}
