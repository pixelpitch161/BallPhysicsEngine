//! Sound files saved inside a scene (W11): a sample picked for an oscillator, or the song loaded
//! as the Mixer's backing track, is kept as the file's bytes in [`Scene::audio`], written as
//! base64 in the saved file, so moving or deleting the file never loses it. The setting holds the
//! sound's name. A scene with no sound files saves exactly as before (the field is left out), and
//! a sound no setting names any more is left out of the file. [`SoundClip`] is a Sound card with
//! the sound files it names: what Copy sound copies and what a `.ballsound` file holds.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::scene_pictures::is_key;
use crate::{Params, Scene, Slot, Value};

/// The setting keys that name a sound file: the oscillators' Sample and the Mixer's Backing track.
pub const AUDIO_KEYS: [&str; 3] = ["a_sample", "b_sample", "backing"];

/// Whether `key` (plain or layered) names a sound file.
pub fn is_audio_key(key: &str) -> bool {
    AUDIO_KEYS.iter().any(|name| is_key(key, name))
}

/// A sound file's bytes, as the editor read them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneAudio {
    pub bytes: Arc<[u8]>,
}

impl SceneAudio {
    pub fn new(bytes: Arc<[u8]>) -> Self {
        Self { bytes }
    }
}

impl Serialize for SceneAudio {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(&self.bytes))
    }
}

impl<'de> Deserialize<'de> for SceneAudio {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        // A damaged sound leaves the rest of the scene to open: it reads as an empty file, which
        // plays nothing and says it can't be read.
        let bytes = base64::engine::general_purpose::STANDARD.decode(text.as_bytes()).unwrap_or_default();
        Ok(Self { bytes: bytes.into() })
    }
}

/// A Sound card and the sound files it names, by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SoundClip {
    pub card: Slot,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub audio: BTreeMap<String, SceneAudio>,
}

/// The format a `.ballsound` file is written in.
pub const SOUND_FILE_FORMAT: u32 = 1;

/// A `.ballsound` file: a [`SoundClip`] with its format number.
#[derive(Serialize, Deserialize)]
struct SoundFile {
    format: u32,
    #[serde(default)]
    card: Slot,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    audio: BTreeMap<String, SceneAudio>,
}

/// `clip` as the text of a `.ballsound` file.
pub fn sound_file_text(clip: &SoundClip) -> String {
    let file = SoundFile { format: SOUND_FILE_FORMAT, card: clip.card.clone(), audio: clip.audio.clone() };
    ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).expect("sounds always serialize")
}

/// The sound a `.ballsound` file's `text` holds, or why it can't be read, in plain words.
pub fn read_sound_file(text: &str) -> Result<SoundClip, String> {
    let file: SoundFile = ron::from_str(text).map_err(|_| "That file isn't a saved sound.".to_string())?;
    if file.format > SOUND_FILE_FORMAT {
        return Err("That sound was saved by a newer version of Ball Studio.".into());
    }
    Ok(SoundClip { card: file.card, audio: file.audio })
}

/// Every sound name the settings in `params` hold.
pub fn audio_names_in(params: &Params) -> BTreeSet<String> {
    params
        .iter()
        .filter(|(k, _)| is_audio_key(k))
        .filter_map(|(_, v)| match v {
            Value::Text(t) if !t.is_empty() => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// Renames the sound names in `params` as `renamed` says (old name to new).
pub fn rename_audio_in(params: &mut Params, renamed: &BTreeMap<String, String>) {
    if renamed.is_empty() {
        return;
    }
    let changes: Vec<(String, String)> = params
        .iter()
        .filter(|(k, _)| is_audio_key(k))
        .filter_map(|(k, v)| match v {
            Value::Text(t) => renamed.get(t).map(|new| (k.clone(), new.clone())),
            _ => None,
        })
        .collect();
    for (k, new) in changes {
        params.set(&k, Value::Text(new));
    }
}

impl Scene {
    /// Keeps the sound file `bytes` in the scene under `name`, or `"name (2)"`, `"name (3)"` …
    /// when that is taken, giving the name used. The same bytes added again give the name they
    /// already have, with no second copy.
    pub fn add_audio(&mut self, name: &str, bytes: Arc<[u8]>) -> String {
        if let Some((have, _)) = self.audio.iter().find(|(_, a)| *a.bytes == *bytes) {
            return have.clone();
        }
        let name = match name.trim() {
            "" => "Sound",
            n => n,
        };
        let mut used = name.to_string();
        let mut n = 2;
        while self.audio.contains_key(&used) {
            used = format!("{name} ({n})");
            n += 1;
        }
        self.audio.insert(used.clone(), SceneAudio::new(bytes));
        used
    }

    /// Brings `audio` (from a copy or a file) into the scene, giving what each name became where
    /// it was taken by other bytes (or the same bytes had another name here).
    pub fn bring_audio(&mut self, audio: &BTreeMap<String, SceneAudio>) -> BTreeMap<String, String> {
        let mut renamed = BTreeMap::new();
        for (name, a) in audio {
            if self.audio.get(name) == Some(a) {
                continue;
            }
            let used = self.add_audio(name, a.bytes.clone());
            if used != *name {
                renamed.insert(name.clone(), used);
            }
        }
        renamed
    }

    /// The Sound card `card` with the sound files it names that the scene keeps.
    pub fn sound_clip(&self, card: &Slot) -> SoundClip {
        let audio = audio_names_in(&card.params)
            .into_iter()
            .filter_map(|n| Some((n.clone(), self.audio.get(&n)?.clone())))
            .collect();
        SoundClip { card: card.clone(), audio }
    }

    /// The scene as it is saved: sound files no setting names any more are left out.
    pub(crate) fn without_unused_audio(&self) -> Option<Scene> {
        if self.audio.is_empty() {
            return None;
        }
        let mut used = BTreeSet::new();
        for name in AUDIO_KEYS {
            used.extend(self.texts_named_by(name));
        }
        if self.audio.keys().all(|name| used.contains(name)) {
            return None;
        }
        let mut scene = self.clone();
        scene.audio.retain(|name, _| used.contains(name));
        Some(scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Address, ThingId, testkit};

    fn bytes(b: &[u8]) -> Arc<[u8]> {
        b.to_vec().into()
    }

    #[test]
    fn a_scene_with_a_sound_file_saves_and_loads_it() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let data: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let name = scene.add_audio("Kick", data.clone().into());
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("backing", Value::Text(name));
        let text = crate::save(&scene);
        assert!(text.contains("audio"), "{text}");
        let back = crate::load(&text, &reg).unwrap();
        assert_eq!(&*back.audio["Kick"].bytes, &data[..]);
    }

    #[test]
    fn a_scene_without_sound_files_saves_as_before() {
        let reg = testkit::registry();
        assert!(!crate::save(&Scene::empty(&reg)).contains("audio"));
    }

    #[test]
    fn sound_files_nothing_names_are_left_out_of_the_file() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let kept = scene.add_audio("Kept", bytes(b"one"));
        scene.add_audio("Dropped", bytes(b"two"));
        let keyed = scene.add_audio("Keyed", bytes(b"three"));
        let id = testkit::push(&mut scene, "balls", ThingId::ROOT);
        scene.things.get_mut(&id).unwrap().params.set("b_sample", Value::Text(kept));
        let a = Address::own(id, "a_sample");
        scene.tracks.entry(a).or_default().set_key(1.0, Value::Text(keyed));
        let back = crate::load(&crate::save(&scene), &reg).unwrap();
        let names: Vec<&str> = back.audio.keys().map(String::as_str).collect();
        assert_eq!(names, ["Kept", "Keyed"]);
        assert_eq!(scene.audio.len(), 3, "the scene in memory keeps them (undo may need them)");
    }

    #[test]
    fn adding_a_sound_twice_names_it_once_and_a_new_one_gets_a_number() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        assert_eq!(scene.add_audio("Kick", bytes(b"one")), "Kick");
        assert_eq!(scene.add_audio("Other", bytes(b"one")), "Kick", "the same bytes again");
        assert_eq!(scene.add_audio("Kick", bytes(b"two")), "Kick (2)");
        assert_eq!(scene.add_audio(" ", bytes(b"three")), "Sound");
    }

    #[test]
    fn a_sound_clip_takes_its_samples_and_brings_them_in_renamed_on_a_clash() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let kick = scene.add_audio("Kick", bytes(b"kick"));
        scene.add_audio("Unused", bytes(b"other"));
        let mut card = Slot::new("sound.voice");
        card.params.set("a_sample", Value::Text(kick));
        card.params.set("b_sample", Value::Text("Gone".into()));
        let clip = scene.sound_clip(&card);
        assert_eq!(clip.audio.keys().collect::<Vec<_>>(), ["Kick"]);
        let text = ron::to_string(&clip).unwrap();
        let back: SoundClip = ron::from_str(&text).unwrap();
        assert_eq!(back, clip);

        let mut other = Scene::empty(&reg);
        other.add_audio("Kick", bytes(b"a different kick"));
        let renamed = other.bring_audio(&clip.audio);
        assert_eq!(renamed.get("Kick").map(String::as_str), Some("Kick (2)"));
        let mut params = clip.card.params.clone();
        rename_audio_in(&mut params, &renamed);
        assert_eq!(params.get("a_sample"), Some(&Value::Text("Kick (2)".into())));
        assert_eq!(&*other.audio["Kick (2)"].bytes, b"kick");
        assert!(other.bring_audio(&clip.audio).contains_key("Kick"), "the same bytes keep their name here");
        assert_eq!(other.audio.len(), 2);
    }

    #[test]
    fn a_ballsound_file_round_trips_and_refuses_what_it_isnt() {
        let mut card = Slot::new("sound.voice");
        card.params.set("a_sample", Value::Text("Kick".into()));
        let clip = SoundClip { card, audio: [("Kick".to_string(), SceneAudio::new(bytes(b"kick")))].into() };
        let text = sound_file_text(&clip);
        assert!(text.contains("format: 1"), "{text}");
        assert_eq!(read_sound_file(&text).unwrap(), clip);
        assert!(read_sound_file("hello").unwrap_err().contains("isn't a saved sound"));
        assert!(read_sound_file("(format: 9)").unwrap_err().contains("newer"));
    }

    #[test]
    fn a_damaged_sound_file_leaves_the_scene_to_open() {
        let audio: SceneAudio = ron::from_str("\"not base64 !!\"").unwrap();
        assert!(audio.bytes.is_empty());
    }
}
