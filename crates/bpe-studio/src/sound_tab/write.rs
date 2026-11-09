//! The Sound tab's edits: a knob turned (a key at the playhead when the setting has keys), the
//! sound turned on and off, a whole sound copied and pasted or saved to and loaded from a file, a
//! sample loaded and a MIDI file's chords imported.

use super::sample_panel::LoadedFile;
use bpe_core::{Address, LayerId, ROUTES_KEY, Registry, Scene, Slot, SlotId, SoundClip, ThingId, Value};
use bpe_kit::sound::{VOICE, starter_voice};
use bpe_sound::music::MidiImport;

/// What a [`write`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The setting holds the new value.
    Set,
    /// The setting has keys: a key at the playhead holds it (the studio pauses, as for any keyed
    /// edit, so a drag doesn't key every frame).
    Keyed,
    /// Nothing changed (no such setting, or a value of the wrong kind).
    Failed,
}

/// Sets the setting at `a` to `v`, or keys it at `time` when it has keys.
pub fn write(scene: &mut Scene, reg: &Registry, a: &Address, v: Value, time: f64) -> WriteOutcome {
    if scene.keyed(a) {
        match scene.set_key(reg, a, time, v) {
            Ok(_) => WriteOutcome::Keyed,
            Err(_) => WriteOutcome::Failed,
        }
    } else {
        match scene.set_at(reg, a, v) {
            Ok(()) => WriteOutcome::Set,
            Err(_) => WriteOutcome::Failed,
        }
    }
}

/// The sound card's `enabled` switch.
fn enabled_at(id: ThingId) -> Address {
    Address::on(id, SlotId::Sound, "enabled")
}

/// Turns the shape's sound on: the starter patch the first time, else the card as it was left.
pub fn enable(scene: &mut Scene, reg: &Registry, id: ThingId) -> bool {
    if scene.card(id, SlotId::Sound).is_some_and(|c| c.kind == VOICE) {
        return scene.set_at(reg, &enabled_at(id), Value::Bool(true)).is_ok();
    }
    if scene.card(id, SlotId::Sound).is_some() {
        scene.remove_slot(id, SlotId::Sound);
    }
    if scene.add_slot(reg, id, SlotId::Sound, VOICE).is_err() {
        return false;
    }
    match scene.things.get_mut(&id) {
        Some(thing) => {
            thing.slots.insert(SlotId::Sound, starter_voice(reg));
            true
        }
        None => false,
    }
}

/// Turns the shape's sound off, keeping every setting.
pub fn disable(scene: &mut Scene, reg: &Registry, id: ThingId) -> bool {
    scene.card(id, SlotId::Sound).is_some() && scene.set_at(reg, &enabled_at(id), Value::Bool(false)).is_ok()
}

/// The shape's whole sound card, to paste on another shape.
pub fn copy(scene: &Scene, id: ThingId) -> Option<Slot> {
    scene.card(id, SlotId::Sound).filter(|c| c.kind == VOICE).cloned()
}

/// Puts a copied sound on `id` in place of its own (whose keys go with it). The layers get the
/// ids 0, 1, 2… in order and the routes and layered settings follow them, so the card is the
/// same sound whatever ids it was copied with.
pub fn paste(scene: &mut Scene, reg: &Registry, id: ThingId, copied: &Slot) -> bool {
    if copied.kind != VOICE || !scene.things.contains_key(&id) {
        return false;
    }
    let card = renumbered(copied);
    scene.remove_slot(id, SlotId::Sound);
    if scene.add_slot(reg, id, SlotId::Sound, VOICE).is_err() {
        return false;
    }
    match scene.things.get_mut(&id) {
        Some(thing) => {
            thing.slots.insert(SlotId::Sound, card);
            true
        }
        None => false,
    }
}

/// The shape's sound with the sound files it plays, to paste on another shape (in this scene or
/// another one).
pub fn copy_clip(scene: &Scene, id: ThingId) -> Option<SoundClip> {
    copy(scene, id).map(|card| scene.sound_clip(&card))
}

/// Pastes a copied sound and the sound files it plays: a file the scene keeps under the same name
/// but different sound is brought in under a new name and the card follows it.
pub fn paste_clip(scene: &mut Scene, reg: &Registry, id: ThingId, clip: &SoundClip) -> bool {
    if clip.card.kind != VOICE || !scene.things.contains_key(&id) {
        return false;
    }
    let renamed = scene.bring_audio(&clip.audio);
    let mut card = clip.card.clone();
    bpe_core::rename_audio_in(&mut card.params, &renamed);
    paste(scene, reg, id, &card)
}

/// What a saved sound file is called after its dot.
pub const SOUND_EXTENSION: &str = "ballsound";

/// Saves the shape's sound (and the sound files it plays) to `path`.
pub fn save_sound(scene: &Scene, id: ThingId, path: &std::path::Path) -> Result<(), String> {
    let clip = copy_clip(scene, id).ok_or_else(|| "This thing has no sound to save.".to_string())?;
    let text = bpe_core::sound_file_text(&clip);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    crate::files::write_safely(path, &text, false).map_err(|e| format!("Couldn't save {name}: {e}"))
}

/// Loads a saved sound from `path` in place of the shape's own.
pub fn load_sound(scene: &mut Scene, reg: &Registry, id: ThingId, path: &std::path::Path) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let text = std::fs::read_to_string(path).map_err(|e| format!("Couldn't read {name}: {e}"))?;
    load_sound_text(scene, reg, id, &text)
}

/// Loads a saved sound's text in place of the shape's own.
pub fn load_sound_text(scene: &mut Scene, reg: &Registry, id: ThingId, text: &str) -> Result<(), String> {
    let clip = bpe_core::read_sound_file(text)?;
    if clip.card.kind != VOICE {
        return Err("That file isn't a saved sound.".into());
    }
    if paste_clip(scene, reg, id, &clip) { Ok(()) } else { Err("That sound couldn't go on this thing.".into()) }
}

/// Keeps a chosen sound file and points the sample setting `key` (`a_sample` or `b_sample`) at it,
/// with its Root from a note in the file's name (like "Piano C4.wav") when there is one.
pub fn load_sample(scene: &mut Scene, reg: &Registry, id: ThingId, key: &str, file: &LoadedFile, time: f64) -> bool {
    let name = scene.add_audio(&file.name(), file.bytes.clone());
    let at = Address::on(id, SlotId::Sound, key);
    let done = write(scene, reg, &at, Value::Text(name), time) != WriteOutcome::Failed;
    if done && let Some(root) = bpe_sound::audio::guess_root(&file.file) {
        write(scene, reg, &Address::on(id, SlotId::Sound, &format!("{key}_root")), Value::Int(root as i64), time);
    }
    done
}

/// Puts a MIDI file's chords in the shape's slots, its tempo and time signature in the scene, and
/// its timing in Slots change (By tempo, or Jump to slot keys at each chord from First beat at).
/// Says what it did, for the tab's note ("Imported 8 chords into 4 slots…").
pub fn import_midi(scene: &mut Scene, reg: &Registry, id: ThingId, import: &MidiImport, time: f64) -> String {
    use bpe_sound::music::SlotTiming;
    let at = |key: &str| Address::on(id, SlotId::Sound, key);
    let root = |key: &str| Address::own(ThingId::ROOT, key);
    write(scene, reg, &at("notes_slots"), Value::Slots(import.slots.clone()), time);
    write(scene, reg, &at("notes_low_octave"), Value::Int(import.low_octave as i64), time);
    write(scene, reg, &at("notes_octaves"), Value::Int(import.octaves as i64), time);
    if let Some(bpm) = import.bpm {
        write(scene, reg, &root("tempo"), Value::Number(bpm), time);
    }
    if let Some((top, bottom)) = import.time_signature {
        write(scene, reg, &root("time_top"), Value::Int(top as i64), time);
        write(scene, reg, &root("time_bottom"), Value::Choice(bottom.to_string()), time);
    }
    match &import.timing {
        SlotTiming::ByTempo { every, bars } => {
            write(scene, reg, &at("notes_change"), Value::Choice("by_tempo".into()), time);
            let unit = if *bars { "bars" } else { "beats" };
            write(scene, reg, &at("notes_every_unit"), Value::Choice(unit.into()), time);
            write(scene, reg, &at("notes_every_beats"), Value::Int(*every as i64), time);
            write(scene, reg, &at("notes_order"), Value::Choice("forward".into()), time);
        }
        SlotTiming::Keys(keys) => {
            write(scene, reg, &at("notes_change"), Value::Choice("only_keyframes".into()), time);
            let jump = at("notes_jump");
            scene.clear_keys(reg, &jump, time);
            let first = match scene.value_at_time(reg, &root("first_beat"), 0.0) {
                Some(Value::Number(n)) => bpe_kit::things::scene::first_beat_seconds(n as f32),
                _ => 0.0,
            };
            for (seconds, slot) in keys {
                let _ = scene.set_key(reg, &jump, first + seconds, Value::Int(*slot as i64));
            }
        }
    }
    let n = import.slots.len();
    let mut said = format!(
        "Imported {} chord{} into {n} slot{}",
        import.chords,
        if import.chords == 1 { "" } else { "s" },
        if n == 1 { "" } else { "s" }
    );
    if import.left_out > 0 {
        said += &format!(" ({} more different chords left out: a sound has 16 slots)", import.left_out);
    }
    if import.tempo_changes {
        said += " (the tempo changes in the file; the scene keeps its first tempo)";
    }
    said
}

/// `card` with its layers numbered from 0 and everything that names a layer following.
fn renumbered(card: &Slot) -> Slot {
    let map: Vec<(LayerId, LayerId)> = card.layers.iter().enumerate().map(|(i, l)| (l.id, LayerId(i as u32))).collect();
    let new_id = |old: LayerId| map.iter().find(|(o, _)| *o == old).map(|(_, n)| *n);
    let mut out = card.clone();
    for layer in &mut out.layers {
        if let Some(n) = new_id(layer.id) {
            layer.id = n;
        }
        layer.parent = layer.parent.and_then(new_id);
    }
    let mut params = bpe_core::Params::default();
    for (key, value) in card.params.iter() {
        let value = match (key.as_str(), value) {
            (ROUTES_KEY, Value::Routes(routes)) => Value::Routes(
                routes
                    .iter()
                    .filter_map(|r| {
                        // A route to a layer's knob follows that layer too.
                        let target = match bpe_core::split_layered(&r.target) {
                            Some((layer, rest)) => bpe_core::layered_key(new_id(layer)?, rest),
                            None => r.target.clone(),
                        };
                        Some(bpe_core::ModRoute { source: new_id(r.source)?, target, ..r.clone() })
                    })
                    .collect(),
            ),
            // In range's Distance, named by its layer id.
            ("play_zone", Value::Text(t)) => match t.trim().parse::<u32>().ok().and_then(|n| new_id(LayerId(n))) {
                Some(n) => Value::Text(n.0.to_string()),
                None => Value::Text(String::new()),
            },
            _ => value.clone(),
        };
        let key = match bpe_core::split_layered(key) {
            Some((layer, rest)) => match new_id(layer) {
                Some(n) => bpe_core::layered_key(n, rest),
                None => continue,
            },
            None => key.clone(),
        };
        params.set(&key, value);
    }
    out.params = params;
    bpe_kit::sound::chain::retarget_sources(&mut out, &new_id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Layer, ModRoute, layered_key};

    #[test]
    fn renumbering_keeps_routes_and_layered_settings_on_their_layers() {
        let mut card = Slot::new(VOICE);
        let layer = |id: u32| Layer { id: LayerId(id), kind: bpe_kit::sound::ADSR.into(), ..Layer::default() };
        card.layers = vec![layer(7), layer(3)];
        card.params.set(&layered_key(LayerId(3), "attack"), Value::Number(40.0));
        card.params.set(
            ROUTES_KEY,
            Value::Routes(vec![ModRoute {
                source: LayerId(3),
                target: "a_gain".into(),
                amount: 0.5,
                bipolar: false,
                invert: false,
            }]),
        );
        let out = renumbered(&card);
        assert_eq!(out.layers.iter().map(|l| l.id.0).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(out.params.get(&layered_key(LayerId(1), "attack")), Some(&Value::Number(40.0)));
        match out.params.get(ROUTES_KEY) {
            Some(Value::Routes(r)) => assert_eq!(r[0].source, LayerId(1)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn renumbering_follows_modulation_sources_and_route_targets() {
        use bpe_kit::sound::{FILTER, MODULATION};
        let mut card = Slot::new(VOICE);
        let layer =
            |id: u32, kind: &str| Layer { id: LayerId(id), kind: kind.into(), lane: Some(0), ..Layer::default() };
        card.layers = vec![layer(7, bpe_kit::sound::ADSR), layer(5, FILTER), layer(9, MODULATION)];
        card.params.set(&layered_key(LayerId(9), "source"), Value::Text("5".into()));
        card.params.set(
            ROUTES_KEY,
            Value::Routes(vec![ModRoute {
                source: LayerId(7),
                target: layered_key(LayerId(5), "cutoff"),
                amount: 0.5,
                bipolar: false,
                invert: false,
            }]),
        );
        let out = renumbered(&card);
        assert_eq!(
            out.params.get(&layered_key(LayerId(2), "source")),
            Some(&Value::Text("1".into())),
            "after the filter"
        );
        match out.params.get(ROUTES_KEY) {
            Some(Value::Routes(r)) => assert_eq!(r[0].target, layered_key(LayerId(1), "cutoff")),
            other => panic!("{other:?}"),
        }
    }

    /// A scene with the starter sound on its shape playing a kept sample "Tone".
    fn sampled(bytes: std::sync::Arc<[u8]>) -> (Scene, Registry, ThingId) {
        let reg = bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(&reg);
        let shape = ThingId(1);
        assert!(enable(&mut scene, &reg, shape));
        let file = LoadedFile { file: "Tone A3.wav".into(), bytes };
        assert!(load_sample(&mut scene, &reg, shape, "a_sample", &file, 0.0));
        (scene, reg, shape)
    }

    #[test]
    fn a_loaded_sample_is_kept_and_its_root_comes_from_its_name() {
        let (scene, _, shape) = sampled(bpe_sound::audio::sine_wav(220.0, 0.1, 48_000, 1));
        let card = scene.card(shape, SlotId::Sound).unwrap();
        assert_eq!(card.params.get("a_sample"), Some(&Value::Text("Tone A3".into())));
        assert!(scene.audio.contains_key("Tone A3"));
        // A3 is note 45 counting C5 as 60.
        assert_eq!(card.params.get("a_sample_root"), Some(&Value::Int(45)));
    }

    #[test]
    fn a_saved_sound_loads_on_another_scene_with_its_sample() {
        let dir = std::env::temp_dir().join(format!("bpe-ballsound-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Pluck.ballsound");
        let (scene, reg, shape) = sampled(bpe_sound::audio::sine_wav(220.0, 0.1, 48_000, 1));
        save_sound(&scene, shape, &path).unwrap();
        // The other scene keeps a different sound under the same name: the loaded one is renamed.
        let mut other = bpe_kit::new_scene(&reg);
        other.add_audio("Tone A3", bpe_sound::audio::sine_wav(440.0, 0.1, 48_000, 1));
        load_sound(&mut other, &reg, shape, &path).unwrap();
        let card = other.card(shape, SlotId::Sound).unwrap();
        assert_eq!(card.params.get("a_sample"), Some(&Value::Text("Tone A3 (2)".into())));
        assert_eq!(other.audio["Tone A3 (2)"].bytes, scene.audio["Tone A3"].bytes);
        assert_eq!(card.layers.len(), scene.card(shape, SlotId::Sound).unwrap().layers.len());
        // A file that isn't a saved sound is refused, in plain words, and changes nothing.
        let before = other.clone();
        let err = load_sound_text(&mut other, &reg, shape, "(width: 3)").unwrap_err();
        assert_eq!(err, "That file isn't a saved sound.");
        assert_eq!(other, before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copied_sound_brings_its_sample_to_another_scene() {
        let (scene, reg, shape) = sampled(bpe_sound::audio::sine_wav(220.0, 0.1, 48_000, 1));
        let clip = copy_clip(&scene, shape).unwrap();
        assert_eq!(clip.audio.keys().collect::<Vec<_>>(), ["Tone A3"]);
        let mut other = bpe_kit::new_scene(&reg);
        assert!(paste_clip(&mut other, &reg, shape, &clip));
        assert!(other.audio.contains_key("Tone A3"));
    }

    #[test]
    fn imported_midi_fills_the_slots_tempo_and_jump_keys() {
        use bpe_sound::music::{MidiImport, SlotTiming};
        let reg = &bpe_kit::registry();
        let mut scene = bpe_kit::new_scene(reg);
        let shape = ThingId(1);
        assert!(enable(&mut scene, reg, shape));
        let slot = |notes: &[u8]| bpe_core::NoteSlot { notes: notes.to_vec(), order_clicked: vec![] };
        let mut import = MidiImport {
            slots: vec![slot(&[60, 64, 67]), slot(&[57, 60, 64])],
            chords: 4,
            left_out: 0,
            bpm: Some(96.0),
            time_signature: Some((3, 4)),
            tempo_changes: false,
            timing: SlotTiming::ByTempo { every: 2, bars: true },
            low_octave: 4,
            octaves: 2,
        };
        let said = import_midi(&mut scene, reg, shape, &import, 0.0);
        assert_eq!(said, "Imported 4 chords into 2 slots");
        let own = |s: &Scene, k: &str| s.value_at_time(reg, &Address::on(shape, SlotId::Sound, k), 0.0);
        let root = |s: &Scene, k: &str| s.value_at_time(reg, &Address::own(ThingId::ROOT, k), 0.0);
        assert_eq!(own(&scene, "notes_slots"), Some(Value::Slots(import.slots.clone())));
        assert_eq!(own(&scene, "notes_change"), Some(Value::Choice("by_tempo".into())));
        assert_eq!(own(&scene, "notes_every_beats"), Some(Value::Int(2)));
        assert_eq!(root(&scene, "tempo"), Some(Value::Number(96.0)));
        assert_eq!(root(&scene, "time_top"), Some(Value::Int(3)));
        // Uneven chords: a Jump to slot key at each, counted from First beat at.
        scene.set_at(reg, &Address::own(ThingId::ROOT, "first_beat"), Value::Number(0.5)).unwrap();
        import.timing = SlotTiming::Keys(vec![(0.0, 1), (1.5, 2), (2.0, 1)]);
        import.left_out = 3;
        let said = import_midi(&mut scene, reg, shape, &import, 0.0);
        assert!(said.contains("3 more different chords left out"), "{said}");
        assert_eq!(own(&scene, "notes_change"), Some(Value::Choice("only_keyframes".into())));
        let jump = Address::on(shape, SlotId::Sound, "notes_jump");
        let keys: Vec<(f64, Value)> = scene.tracks[&jump].keys.iter().map(|k| (k.time, k.value.clone())).collect();
        assert_eq!(keys, vec![(0.5, Value::Int(1)), (2.0, Value::Int(2)), (2.5, Value::Int(1))]);
    }
}
