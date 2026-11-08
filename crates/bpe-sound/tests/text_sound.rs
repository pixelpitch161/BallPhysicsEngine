//! Text part 3: a Text's Sound card plays when balls hit its letters, By letter picking the
//! slot's note by the letter hit and Spell notes playing the hit letter's own note name.

use bpe_core::{NoteSlot, Registry, Scene, SlotId, ThingId, Value};
use bpe_kit::sound::starter_voice;
use bpe_kit::{new_scene, resolve_scene};
use bpe_runtime::{Outline, World};
use bpe_sound::{BLOCK, Engine};
use glam::Vec2;

/// The new scene's ball still at `ball` and a Text of `words` in the world at the middle that
/// balls bounce off, with the starter sound playing `plays` from the slot `notes`.
fn scene_of(reg: &Registry, words: &str, plays: &str, notes: &[u8], ball: Vec2) -> (Scene, ThingId) {
    let mut scene = new_scene(reg);
    let balls = scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap();
    let b = scene.things.get_mut(&balls).unwrap();
    b.transform.position = ball;
    b.params.set("velocity", Value::Vec2([0.0, 0.0]));
    let id = bpe_kit::add_thing(&mut scene, reg, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&id).unwrap();
    t.screen = Some(false);
    t.transform.position = Vec2::ZERO;
    t.params.set("words", Value::Text(words.into()));
    t.params.set("size", Value::Number(300.0));
    t.params.set("balls", Value::Choice("bounce_off".into()));
    let mut card = starter_voice(reg);
    card.params.set("notes_plays", Value::Choice(plays.into()));
    card.params.set(
        bpe_core::SLOTS_KEY,
        Value::Slots(vec![NoteSlot { notes: notes.to_vec(), order_clicked: notes.to_vec() }]),
    );
    t.slots.insert(SlotId::Sound, card);
    (scene, id)
}

/// The scene with its ball falling onto letter `k` from 200 px above it.
fn falling_on(reg: &Registry, words: &str, k: usize, plays: &str, notes: &[u8]) -> (Scene, ThingId) {
    let kit = bpe_sound::kit();
    let (scene, id) = scene_of(reg, words, plays, notes, Vec2::ZERO);
    let resolved = resolve_scene(&scene, reg);
    let world = World::build(&scene, &resolved, &kit.sims);
    let Outline::Letters { letters, .. } = &world.walls[&id].outline else { panic!("no letters") };
    let l = letters.list.iter().find(|l| l.part == k as u32).unwrap();
    scene_of(reg, words, plays, notes, Vec2::new(0.5 * (l.lo.x + l.hi.x), l.lo.y - 200.0))
}

/// The notes Text `id` plays in the first two seconds.
fn notes_played(scene: &Scene, id: ThingId) -> Vec<u8> {
    let kit = bpe_sound::kit();
    let resolved = resolve_scene(scene, &kit.registry);
    let mut engine = Engine::new(scene, &resolved, &kit);
    engine.record_notes(true);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..(2 * 48_000usize).div_ceil(BLOCK) {
        engine.render(&mut block);
    }
    engine.note_log().spans(96_000).into_iter().filter(|s| s.0 == id).map(|s| s.1).collect()
}

#[test]
fn by_letter_plays_the_letters_place_in_the_slot() {
    let kit = bpe_sound::kit();
    let (scene, id) = falling_on(&kit.registry, "II", 1, "by_letter", &[60, 64, 67]);
    let notes = notes_played(&scene, id);
    assert!(!notes.is_empty() && notes.iter().all(|&n| n == 64), "the second letter, the second note: {notes:?}");
}

#[test]
fn spell_notes_play_the_letters_own_names() {
    let kit = bpe_sound::kit();
    // A G, in the octave of the slot's lowest note (D3 = 50: the octave from C3 = 48).
    let (scene, id) = falling_on(&kit.registry, "GX", 0, "spell_notes", &[50, 70]);
    let notes = notes_played(&scene, id);
    assert!(!notes.is_empty() && notes.iter().all(|&n| n == 55), "{notes:?}");
    // An empty slot spells from C5; a lowercase letter plays too.
    let (scene, id) = falling_on(&kit.registry, "eX", 0, "spell_notes", &[]);
    let notes = notes_played(&scene, id);
    assert!(!notes.is_empty() && notes.iter().all(|&n| n == 64), "{notes:?}");
    // Any other letter plays nothing.
    let (scene, id) = falling_on(&kit.registry, "GX", 1, "spell_notes", &[60]);
    assert!(notes_played(&scene, id).is_empty());
}
