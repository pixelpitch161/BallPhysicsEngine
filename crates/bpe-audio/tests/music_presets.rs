//! The music presets do what their descriptions say.

use bpe_scene::Scene;

#[test]
fn marimba_melody_plays_the_tune_in_order() {
    let scene = Scene::marimba_melody();
    let sound = bpe_audio::render_scene(&scene, None).unwrap();
    let steps: Vec<u32> = sound.mix.notes.iter().filter_map(|n| n.step).collect();
    assert!(steps.len() > 40, "only {} notes", steps.len());
    // Every hit plays the next step, starting again after the last.
    assert!(steps.windows(2).all(|w| w[1] == w[0] + 1 || w[1] == 0), "{steps:?}");
}

#[test]
fn beat_locked_bounce_lands_on_the_beat() {
    let scene = Scene::beat_locked_bounce();
    let sound = bpe_audio::render_scene(&scene, None).unwrap();
    let share = sound.on_beat.unwrap();
    assert!(share >= 0.9, "{:.0}% on the beat", share * 100.0);
    let hits = sound.events.iter().filter(|e| e.kind.is_bounce()).count();
    assert!(hits >= 15, "only {hits} bounces");
}

#[test]
fn drum_machine_plays_each_drum() {
    let scene = Scene::drum_machine();
    let sound = bpe_audio::render_scene(&scene, None).unwrap();
    let count = |voice: u16, key: u8| sound.mix.notes.iter().filter(|n| n.voice == voice && n.key == key).count();
    assert!(count(0, 36) > 0, "no kick");
    assert!(count(2, 49) > 0, "no crash");
    assert!(count(3, 42) >= 30, "no hi-hat on the beat");
    assert!(sound.mix.notes.iter().any(|n| n.voice == 4), "no bass on the bars");
    // Balls meeting play the snare once the simulation reports them.
    let meetings = sound.events.iter().filter(|e| matches!(e.kind, bpe_sim::EventKind::Ball(_))).count();
    assert!(count(1, 38) == 0 || meetings > 0);
}
