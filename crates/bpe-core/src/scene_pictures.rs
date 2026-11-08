//! Pictures saved inside a scene (editor workflow, W5): a PNG picked for a Particles thing's
//! Picture is kept as its bytes in [`Scene::pictures`], written as base64 in the saved file, so
//! moving or deleting the PNG never loses it. The setting holds the picture's name. A scene with no
//! pictures saves exactly as before (the field is left out), and a picture no setting names any
//! more is left out of the file.

use std::collections::BTreeSet;
use std::sync::Arc;

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Scene, Value};

/// The setting key that names a picture (a Particles thing's Picture).
pub const PICTURE_KEY: &str = "picture";

/// A picture file's bytes, as the editor read them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenePicture {
    pub bytes: Arc<[u8]>,
}

impl ScenePicture {
    pub fn new(bytes: Arc<[u8]>) -> Self {
        Self { bytes }
    }
}

impl Serialize for ScenePicture {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(&self.bytes))
    }
}

impl<'de> Deserialize<'de> for ScenePicture {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        // A damaged picture leaves the rest of the scene to open: it reads as no picture.
        let bytes = base64::engine::general_purpose::STANDARD.decode(text.as_bytes()).unwrap_or_default();
        Ok(Self { bytes: bytes.into() })
    }
}

/// Whether `key` (a thing's own key, a card's or a layered `"<layer>/<key>"`) is `name`'s key.
pub(crate) fn is_key(key: &str, name: &str) -> bool {
    key == name || key.rsplit_once('/').is_some_and(|(_, k)| k == name)
}

impl Scene {
    /// Keeps the picture `bytes` in the scene under `name`, or `"name (2)"`, `"name (3)"` … when
    /// that is taken, giving the name used. The same bytes added again give the name they already
    /// have, with no second copy.
    pub fn add_picture(&mut self, name: &str, bytes: Arc<[u8]>) -> String {
        if let Some((have, _)) = self.pictures.iter().find(|(_, p)| *p.bytes == *bytes) {
            return have.clone();
        }
        let name = match name.trim() {
            "" => "Picture",
            n => n,
        };
        let mut used = name.to_string();
        let mut n = 2;
        while self.pictures.contains_key(&used) {
            used = format!("{name} ({n})");
            n += 1;
        }
        self.pictures.insert(used.clone(), ScenePicture::new(bytes));
        used
    }

    /// Every name some `key` setting holds: in a thing's own settings, its cards (layers too),
    /// what its cards keep for other kinds, and the keys of its tracks.
    pub fn texts_named_by(&self, key: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut take = |k: &str, v: &Value| {
            if is_key(k, key)
                && let Value::Text(t) = v
                && !t.is_empty()
            {
                out.insert(t.clone());
            }
        };
        for thing in self.things.values() {
            for (k, v) in thing.params.iter() {
                take(k, v);
            }
            for slot in thing.slots.values() {
                for (k, v) in slot.params.iter() {
                    take(k, v);
                }
                for stash in slot.stash.values() {
                    for (k, v) in stash.params.iter() {
                        take(k, v);
                    }
                }
            }
        }
        for (a, track) in &self.tracks {
            for k in &track.keys {
                take(&a.key, &k.value);
            }
        }
        out
    }

    /// The scene as it is saved: pictures no Picture setting names any more are left out.
    pub(crate) fn without_unused_pictures(&self) -> Option<Scene> {
        if self.pictures.is_empty() {
            return None;
        }
        let used = self.texts_named_by(PICTURE_KEY);
        if self.pictures.keys().all(|name| used.contains(name)) {
            return None;
        }
        let mut scene = self.clone();
        scene.pictures.retain(|name, _| used.contains(name));
        Some(scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Address, testkit};

    fn bytes(b: &[u8]) -> Arc<[u8]> {
        b.to_vec().into()
    }

    #[test]
    fn a_scene_with_a_picture_saves_and_loads_it() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let data: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
        let name = scene.add_picture("Logo", data.clone().into());
        scene.things.get_mut(&crate::ThingId::ROOT).unwrap().params.set(PICTURE_KEY, Value::Text(name));
        let text = crate::save(&scene);
        assert!(text.contains("pictures"), "{text}");
        let back = crate::load(&text, &reg).unwrap();
        assert_eq!(&*back.pictures["Logo"].bytes, &data[..]);
    }

    #[test]
    fn a_scene_without_pictures_saves_as_before() {
        let reg = testkit::registry();
        assert!(!crate::save(&Scene::empty(&reg)).contains("pictures"));
    }

    #[test]
    fn pictures_nothing_names_are_left_out_of_the_file() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let kept = scene.add_picture("Kept", bytes(b"one"));
        scene.add_picture("Dropped", bytes(b"two"));
        let keyed = scene.add_picture("Keyed", bytes(b"three"));
        let id = testkit::push(&mut scene, "balls", crate::ThingId::ROOT);
        scene.things.get_mut(&id).unwrap().params.set("layer 2/picture", Value::Text(kept));
        let a = Address::own(id, PICTURE_KEY);
        scene.tracks.entry(a).or_default().set_key(1.0, Value::Text(keyed));
        let back = crate::load(&crate::save(&scene), &reg).unwrap();
        let names: Vec<&str> = back.pictures.keys().map(String::as_str).collect();
        assert_eq!(names, ["Kept", "Keyed"]);
        assert_eq!(scene.pictures.len(), 3, "the scene in memory keeps them (undo may need them)");
    }

    #[test]
    fn adding_a_picture_twice_names_it_once_and_a_new_one_gets_a_number() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        assert_eq!(scene.add_picture("Logo", bytes(b"one")), "Logo");
        assert_eq!(scene.add_picture("Other", bytes(b"one")), "Logo", "the same bytes again");
        assert_eq!(scene.add_picture("Logo", bytes(b"two")), "Logo (2)");
        assert_eq!(scene.add_picture(" ", bytes(b"three")), "Picture");
    }

    #[test]
    fn a_damaged_picture_leaves_the_scene_to_open() {
        let picture: ScenePicture = ron::from_str("\"not base64 !!\"").unwrap();
        assert!(picture.bytes.is_empty());
    }
}
