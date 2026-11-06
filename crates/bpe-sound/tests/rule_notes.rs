//! Rules part 6: Play a note sounds on its shape's voice, on its sample, in the MIDI log, and
//! again after a seek.

use bpe_core::{Registry, Scene, SlotId, ThingId, Value, Who};
use bpe_kit::sound::starter_voice;
use bpe_kit::{new_scene, resolve_scene};
use bpe_sound::{BLOCK, Engine};

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

/// The new scene with the starter sound on its shape and its ball switched off (no hits at all),
/// with a rule playing `set` on the shape at 1 s.
fn quiet_scene(reg: &Registry, set: &[(&str, Value)]) -> Scene {
    let mut scene = new_scene(reg);
    let shape = shape_of(&scene);
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, starter_voice(reg));
    let ball = scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap();
    scene.things.get_mut(&ball).unwrap().enabled = false;
    let rule = scene.add_rule("Note");
    let when = scene.add_block(reg, rule, "when.time", None).unwrap();
    scene.set_block_param(reg, rule, when, "at", Value::Number(1.0)).unwrap();
    let note = scene.add_block(reg, rule, "do.note", Some(when)).unwrap();
    scene.set_block_param(reg, rule, note, "who", Value::Who(Who::Thing { thing: shape, ball: None })).unwrap();
    for (k, v) in set {
        scene.set_block_param(reg, rule, note, k, v.clone()).unwrap();
    }
    scene
}

/// The loudest sample in each tenth of a second of `seconds` from the engine.
fn peaks(engine: &mut Engine, seconds: f64) -> Vec<f32> {
    let tenth = 4_800;
    let mut out = Vec::new();
    let mut block = [0.0f32; BLOCK * 2];
    let (mut peak, mut n) = (0.0f32, 0);
    for _ in 0..((seconds * 48_000.0) as usize).div_ceil(BLOCK) {
        engine.render(&mut block);
        peak = block.iter().fold(peak, |m, s| m.max(s.abs()));
        n += BLOCK;
        if n >= tenth {
            out.push(peak);
            (peak, n) = (0.0, 0);
        }
    }
    out
}

#[test]
fn a_rule_note_sounds_on_its_sample_and_in_the_midi_log() {
    let kit = bpe_sound::kit();
    let scene = quiet_scene(&kit.registry, &[("note", Value::Int(72)), ("length", Value::Number(0.3))]);
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut engine = Engine::new(&scene, &resolved, &kit);
    engine.record_notes(true);
    let p = peaks(&mut engine, 2.0);
    assert!(p[..10].iter().all(|x| *x == 0.0), "nothing before the rule: {p:?}");
    assert!(p[10..14].iter().any(|x| *x > 0.01), "the note after 1 s: {p:?}");
    let spans = engine.note_log().spans(96_000);
    assert_eq!(spans.len(), 1, "{spans:?}");
    let (thing, note, _, start, end) = spans[0];
    assert_eq!((thing, note), (shape_of(&scene), 72));
    // Its step (241) starts on sample 240 × 200.
    assert_eq!(start, 48_000);
    assert_eq!(end - start, 14_400, "its length, 0.3 s");
}

#[test]
fn the_shapes_next_note_picks_from_its_slot_and_a_seek_hears_it_again() {
    let kit = bpe_sound::kit();
    let scene = quiet_scene(&kit.registry, &[("pitch_from", Value::Choice("the_shapes_next_note".into()))]);
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut engine = Engine::new(&scene, &resolved, &kit);
    engine.record_notes(true);
    let straight = peaks(&mut engine, 1.5);
    assert!(straight[10..].iter().any(|x| *x > 0.01), "{straight:?}");
    assert_eq!(engine.note_log().spans(72_000).len(), 1);
    let mut again = Engine::new(&scene, &resolved, &kit);
    again.seek(1.05);
    let after = peaks(&mut again, 0.2);
    assert!(after.iter().any(|x| *x > 0.01), "the note rings on after a seek into it: {after:?}");
}
