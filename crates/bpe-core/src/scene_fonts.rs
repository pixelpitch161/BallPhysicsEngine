//! Fonts saved inside a scene (Text part 1): a font file loaded with Load font is kept as its
//! bytes in [`Scene::fonts`], written as base64 in the saved file, so the scene opens the same on
//! any PC. A scene with no loaded fonts saves exactly as before (the field is left out).

use std::sync::Arc;

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::Scene;

/// A font file's bytes, as Load font read them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneFont {
    pub bytes: Arc<[u8]>,
}

impl SceneFont {
    pub fn new(bytes: Arc<[u8]>) -> Self {
        Self { bytes }
    }
}

impl Serialize for SceneFont {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(&self.bytes))
    }
}

impl<'de> Deserialize<'de> for SceneFont {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        // A damaged font leaves the rest of the scene to open: it reads as no font (the texts set
        // in it show the fallback and say so).
        let bytes = base64::engine::general_purpose::STANDARD.decode(text.as_bytes()).unwrap_or_default();
        Ok(Self { bytes: bytes.into() })
    }
}

impl Scene {
    /// Keeps the font `bytes` in the scene under `name`, or `"name (2)"`, `"name (3)"` … when
    /// that is taken, giving the name used. The same bytes added again give the name they already
    /// have, with no second copy.
    pub fn add_font(&mut self, name: &str, bytes: Arc<[u8]>) -> String {
        if let Some((have, _)) = self.fonts.iter().find(|(_, f)| *f.bytes == *bytes) {
            return have.clone();
        }
        let name = match name.trim() {
            "" => "Loaded font",
            n => n,
        };
        let mut used = name.to_string();
        let mut n = 2;
        while self.fonts.contains_key(&used) {
            used = format!("{name} ({n})");
            n += 1;
        }
        self.fonts.insert(used.clone(), SceneFont::new(bytes));
        used
    }

    /// Takes the loaded font `name` out of the scene; whether it was there. Texts set in it draw
    /// in the fallback font.
    pub fn remove_font(&mut self, name: &str) -> bool {
        self.fonts.remove(name).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit;

    fn bytes(b: &[u8]) -> Arc<[u8]> {
        b.to_vec().into()
    }

    #[test]
    fn a_scene_with_a_font_saves_and_loads_it() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        let data: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
        scene.add_font("Mine", data.clone().into());
        let text = crate::save(&scene);
        assert!(text.contains("fonts"), "{text}");
        assert!(!text.contains("0, 1, 2, 3"), "kept as text, not a list of numbers");
        let back = crate::load(&text, &reg).unwrap();
        assert_eq!(&*back.fonts["Mine"].bytes, &data[..]);
        assert_eq!(back.fonts, scene.fonts);
    }

    #[test]
    fn a_scene_without_fonts_saves_as_before() {
        let reg = testkit::registry();
        let scene = Scene::empty(&reg);
        assert!(!crate::save(&scene).contains("fonts"));
    }

    #[test]
    fn adding_a_font_twice_names_it_once_and_a_new_one_gets_a_number() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        assert_eq!(scene.add_font("Mine", bytes(b"one")), "Mine");
        assert_eq!(scene.add_font("Mine", bytes(b"one")), "Mine", "the same bytes again");
        assert_eq!(scene.add_font("Mine", bytes(b"two")), "Mine (2)");
        assert_eq!(scene.add_font("Mine", bytes(b"three")), "Mine (3)");
        assert_eq!(scene.add_font("  ", bytes(b"four")), "Loaded font");
        assert_eq!(scene.fonts.len(), 4);
    }

    #[test]
    fn a_damaged_font_leaves_the_scene_to_open() {
        let font: SceneFont = ron::from_str("\"not base64 !!\"").unwrap();
        assert!(font.bytes.is_empty());
    }

    #[test]
    fn remove_font_says_whether_it_was_there() {
        let reg = testkit::registry();
        let mut scene = Scene::empty(&reg);
        scene.add_font("Mine", bytes(b"one"));
        assert!(scene.remove_font("Mine"));
        assert!(!scene.remove_font("Mine"));
        assert!(scene.fonts.is_empty());
    }
}
