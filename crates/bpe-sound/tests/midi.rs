//! The export's MIDI file: the notes the shapes played, on their exact samples, with the tempo.

use bpe_core::SlotId;
use bpe_kit::sound::starter_voice;
use bpe_kit::{kit, new_scene, resolve_scene};
use bpe_sound::midi::{PPQ, tick_of};
use bpe_sound::{BLOCK, Engine, Extras, render_sound};
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

#[test]
fn midi_round_trips_the_played_notes() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, starter_voice(&kit.registry));
    let seconds = 4.0;
    let total = (seconds * 48_000.0) as u64;

    // The notes as the engine noted them.
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut e = Engine::new(&scene, &resolved, &kit);
    e.record_notes(true);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..(total as usize).div_ceil(BLOCK) {
        e.render(&mut block);
    }
    let spans = e.note_log().spans(total);
    assert!(spans.len() >= 2, "the ball hits the shape a few times: {}", spans.len());
    let tempo = *e.tempo();

    // The same render, saved.
    let dir = std::env::temp_dir().join(format!("bpe-midi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let midi = dir.join("clip.mid");
    let extras = Extras { midi: Some(midi.clone()), ..Extras::default() };
    render_sound(&scene, &kit, &dir.join("clip.wav"), seconds, &extras, |_| true).unwrap();
    let bytes = std::fs::read(&midi).unwrap();
    let smf = Smf::parse(&bytes).unwrap();
    assert_eq!(smf.header.timing, Timing::Metrical(PPQ.into()));
    assert_eq!(smf.tracks.len(), 2, "the tempo track and the shape's");
    let has_tempo = smf.tracks[0].iter().any(|t| matches!(t.kind, TrackEventKind::Meta(MetaMessage::Tempo(_))));
    assert!(has_tempo);
    let name = smf.tracks[1].iter().find_map(|t| match t.kind {
        TrackEventKind::Meta(MetaMessage::TrackName(n)) => Some(String::from_utf8_lossy(n).into_owned()),
        _ => None,
    });
    assert_eq!(name.as_deref(), Some(scene.things[&shape].name.as_str()));

    // Every note on, at its tick, with its key.
    let mut tick = 0u64;
    let mut ons = Vec::new();
    for t in &smf.tracks[1] {
        tick += t.delta.as_int() as u64;
        if let TrackEventKind::Midi { message: MidiMessage::NoteOn { key, vel }, .. } = t.kind
            && vel.as_int() > 0
        {
            ons.push((tick, key.as_int()));
        }
    }
    let expected: Vec<(u64, u8)> = spans.iter().map(|s| (tick_of(s.3, &tempo), s.1)).collect();
    assert_eq!(ons, expected);
    let _ = std::fs::remove_dir_all(&dir);
}
