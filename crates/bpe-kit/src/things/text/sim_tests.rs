use super::*;
use crate::testing::{BALLS, SHAPE};
use crate::{kit, new_scene};
use bpe_core::{Address, Value, resolve};
use bpe_runtime::{EventKind, Log};

/// The test scene (its ring round everything, its ball) with a Text of `words` in the world at the
/// middle, balls doing `balls` at it, and the ball at `ball`, still.
fn with_text(words: &str, balls: &str, ball: Vec2) -> (Scene, ThingId) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let id = crate::add_thing(&mut scene, &kit.registry, KIND, ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&id).unwrap();
    t.screen = Some(false);
    t.transform.position = Vec2::ZERO;
    t.params.set("words", Value::Text(words.into()));
    t.params.set("size", Value::Number(300.0));
    t.params.set("balls", Value::Choice(balls.into()));
    let b = scene.things.get_mut(&BALLS).unwrap();
    b.transform.position = ball;
    b.params.set("velocity", Value::Vec2([0.0, 0.0]));
    (scene, id)
}

fn run(scene: &Scene, seconds: f64) -> (World, Log) {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(seconds, &kit.sims, &mut log);
    (world, log)
}

fn hits_on(log: &Log, thing: ThingId) -> Vec<u32> {
    log.events()
        .iter()
        .filter(|e| e.kind == EventKind::Hit)
        .filter_map(|e| e.b.filter(|b| b.thing == thing))
        .map(|b| b.item)
        .collect()
}

#[test]
fn a_ball_bounces_off_a_solid_letter_and_the_hit_names_it() {
    // "II": the ball falls onto the second I.
    let (scene, id) = with_text("II", "bounce_off", Vec2::ZERO);
    let (w, _) = run(&scene, 0.0);
    let Some(Wall { outline: Outline::Letters { letters, .. }, solid: true, passes: false, .. }) = w.walls.get(&id)
    else {
        panic!("{:?}", w.walls.get(&id));
    };
    let second = &letters.list[1];
    let above = Vec2::new(0.5 * (second.lo.x + second.hi.x), second.lo.y - 200.0);
    let (scene, id) = with_text("II", "bounce_off", above);
    let (w, log) = run(&scene, 2.0);
    let hits = hits_on(&log, id);
    assert!(!hits.is_empty() && hits.iter().all(|&k| k == 1), "{hits:?}");
    let p = w.bodies[&BALLS].position[0];
    assert!(p.y < second.lo.y, "it stays on top of the letter: {p}");
    assert_eq!(w.parts[&id].alive, [true, true]);
    assert_eq!(letter_char(&w, id, 1), Some('I'));
}

#[test]
fn a_ball_inside_a_hollow_letter_stays_inside_it() {
    let (scene, id) = with_text("O", "bounce_inside", Vec2::ZERO);
    let (w, _) = run(&scene, 0.0);
    let Outline::Letters { letters, thickness, .. } = &w.walls[&id].outline else { panic!() };
    assert_eq!(*thickness, 10.0);
    // A place in the O's stroke, on its left side halfway down.
    let o = &letters.list[0];
    let mid_y = 0.5 * (o.lo.y + o.hi.y);
    let inside = (0..200)
        .map(|k| Vec2::new(o.lo.x + k as f32 * (o.hi.x - o.lo.x) / 200.0, mid_y))
        .filter(|p| o.inside(*p))
        .collect::<Vec<_>>();
    let start = inside[inside.len() / 8];
    let (mut scene, id) = with_text("O", "bounce_inside", start);
    scene.things.get_mut(&BALLS).unwrap().params.set("radius", Value::Number(4.0));
    let (w, log) = run(&scene, 3.0);
    let p = w.bodies[&BALLS].position[0];
    let Outline::Letters { letters, .. } = &w.walls[&id].outline else { panic!() };
    assert!(letters.list[0].inside(p), "the ball left the letter: {p}");
    assert!(!hits_on(&log, id).is_empty());
}

#[test]
fn balls_pass_through_but_the_letters_are_still_a_wall() {
    let (scene, id) = with_text("I", "pass_through", Vec2::new(0.0, -400.0));
    let (w, log) = run(&scene, 2.0);
    assert!(w.walls[&id].passes);
    assert!(hits_on(&log, id).is_empty());
}

#[test]
fn a_screen_text_or_a_riding_text_has_no_wall() {
    let (mut scene, id) = with_text("I", "bounce_off", Vec2::ZERO);
    scene.things.get_mut(&id).unwrap().screen = Some(true);
    let (w, _) = run(&scene, 0.1);
    assert!(!w.walls.contains_key(&id) && !w.parts.contains_key(&id));
    let (mut scene, id) = with_text("I", "bounce_off", Vec2::ZERO);
    scene.set_parent(id, BALLS, bpe_core::Attach::Group).unwrap();
    let (w, _) = run(&scene, 0.1);
    assert!(!w.walls.contains_key(&id));
    let _ = SHAPE;
}

#[test]
fn spin_turns_the_wall_and_a_words_key_changes_it() {
    let (mut scene, id) = with_text("I", "bounce_off", Vec2::new(0.0, -2000.0));
    scene.things.get_mut(&id).unwrap().params.set("spin", Value::Number(90.0));
    let (w, _) = run(&scene, 1.0);
    let a = w.walls[&id].outline.angle();
    assert!((a - std::f32::consts::FRAC_PI_2).abs() < 0.02, "{a}");
    let reg = kit().registry;
    let (mut scene, id) = with_text("I", "bounce_off", Vec2::new(0.0, -2000.0));
    let words = Address { thing: id, slot: None, key: "words".into() };
    scene.set_key(&reg, &words, 0.0, Value::Text("I".into())).unwrap();
    scene.set_key(&reg, &words, 0.5, Value::Text("III".into())).unwrap();
    let count = |t: f64| {
        let (w, _) = run(&scene, t);
        match &w.walls[&id].outline {
            Outline::Letters { letters, .. } => (letters.list.len(), w.parts[&id].len()),
            _ => panic!(),
        }
    };
    assert_eq!(count(0.2), (1, 1));
    assert_eq!(count(0.8), (3, 3));
}

#[test]
fn only_layout_path_and_balls_settings_are_physics() {
    for key in ["words", "font", "size", "path", "radius", "balls", "thickness", "spin"] {
        assert!(TextSim.affects_physics(key), "{key}");
    }
    for key in ["show", "appear", "resort", "rides_on", "fast"] {
        assert!(!TextSim.affects_physics(key), "{key}");
    }
}

/// Adds a rule of `blocks` (kind, settings) to `scene`, each after the one before.
fn rule(scene: &mut Scene, blocks: &[(&str, Vec<(&str, Value)>)]) {
    let reg = kit().registry;
    let rule = scene.add_rule("Test");
    let mut last = None;
    for (kind, params) in blocks {
        let id = scene.add_block(&reg, rule, kind, last).unwrap();
        for (k, v) in params {
            scene.set_block_param(&reg, rule, id, k, v.clone()).unwrap_or_else(|e| panic!("{kind} {k}: {e}"));
        }
        last = Some(id);
    }
}

/// "II" with the ball falling onto the second I from 200 px above it.
fn falling_on_the_second(balls: &str) -> (Scene, ThingId) {
    let (scene, _) = with_text("II", balls, Vec2::ZERO);
    let (w, _) = run(&scene, 0.0);
    let id = *w.parts.keys().next().unwrap();
    let Outline::Letters { letters, .. } = &w.walls[&id].outline else { panic!() };
    let second = &letters.list[1];
    with_text("II", balls, Vec2::new(0.5 * (second.lo.x + second.hi.x), second.lo.y - 200.0))
}

fn breaks(log: &Log, thing: ThingId) -> Vec<u32> {
    log.events().iter().filter(|e| e.kind == EventKind::Break && e.a.thing == thing).map(|e| e.a.item).collect()
}

#[test]
fn a_rule_breaks_only_the_letter_that_was_hit() {
    let (mut scene, id) = falling_on_the_second("bounce_off");
    let text = Value::Who(bpe_core::Who::Thing { thing: id, ball: None });
    rule(
        &mut scene,
        &[
            ("when.hit", vec![("whom", text)]),
            (
                "do.break",
                vec![
                    ("who", Value::Who(bpe_core::Who::Other)),
                    ("look", Value::Choice("fade_out".into())),
                    ("fade_time", Value::Number(100.0)),
                ],
            ),
        ],
    );
    let (w, log) = run(&scene, 3.0);
    assert_eq!(breaks(&log, id), [1]);
    assert_eq!(w.parts[&id].alive, [true, false]);
    let Outline::Letters { letters, .. } = &w.walls[&id].outline else { panic!() };
    assert_eq!(letters.list.iter().map(|l| l.part).collect::<Vec<_>>(), [0], "the broken letter left the wall");
    let broke = log.events().iter().find(|e| e.kind == EventKind::Break).unwrap().step;
    let after = log.events().iter().filter(|e| e.kind == EventKind::Hit && e.step > broke);
    assert!(after.filter_map(|e| e.b).all(|b| !(b.thing == id && b.item == 1)), "nothing hits a broken letter");
    assert!(w.rules().unwrap().remains_of(id, Some(1)).is_some(), "its look plays out");
}

#[test]
fn each_letter_has_its_own_health() {
    let (mut scene, id) = falling_on_the_second("bounce_off");
    let reg = kit().registry;
    crate::fill_slot(&mut scene, &reg, id, bpe_core::SlotId::Health).unwrap();
    scene.card_mut(id, bpe_core::SlotId::Health).unwrap().params.set("start", Value::Number(2.0));
    let (w, log) = run(&scene, 6.0);
    let hp = |k| w.rules().unwrap().health().of(id, Some(k)).unwrap().now;
    let hits = hits_on(&log, id);
    let on = |k: u32| hits.iter().filter(|&&h| h == k).count() as f64;
    assert_eq!(&hits[..2], [1, 1], "{hits:?}");
    assert_eq!(hp(0), (2.0 - on(0)).max(0.0), "the first letter takes only its own hits");
    assert_eq!(hp(1), 0.0);
    assert_eq!(breaks(&log, id)[0], 1, "at zero the second letter breaks");
    assert_eq!(w.parts[&id].alive, [true, false]);
}

#[test]
fn a_rule_names_a_letter_by_number_and_flashes_one() {
    let (mut scene, id) = with_text("ABC", "bounce_off", Vec2::new(0.0, -3000.0));
    let second = Value::Who(bpe_core::Who::Thing { thing: id, ball: Some(2) });
    rule(&mut scene, &[("when.time", vec![("at", Value::Number(0.1))]), ("do.break", vec![("who", second.clone())])]);
    rule(&mut scene, &[("when.time", vec![("at", Value::Number(0.2))]), ("do.flash", vec![("who", second)])]);
    let text = Value::Who(bpe_core::Who::Thing { thing: id, ball: None });
    rule(&mut scene, &[("when.time", vec![("at", Value::Number(0.3))]), ("do.flash", vec![("who", text)])]);
    let (w, log) = run(&scene, 0.5);
    assert_eq!(breaks(&log, id), [1]);
    let flashes: Vec<u32> =
        log.events().iter().filter(|e| e.kind == EventKind::Flash && e.a.thing == id).map(|e| e.a.item).collect();
    assert_eq!(flashes, [0, 2], "a broken letter doesn't flash; the whole Text flashes each live letter");
    assert_eq!(w.parts[&id].alive, [true, false, true]);
}
