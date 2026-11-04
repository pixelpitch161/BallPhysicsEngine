//! Saving and opening scenes.

use crate::{Params, Registry, Scene, SettingDef};
use std::fmt;

/// Why a scene file didn't open.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadError {
    /// It has no format number, so the old editor wrote it.
    OldEditor,
    /// A later version wrote it, in this format.
    Newer(u32),
    /// It isn't a readable scene. Holds the parser's message.
    Broken(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::OldEditor => write!(f, "This scene was made with the old Ball Studio"),
            LoadError::Newer(_) => write!(f, "This scene was made with a newer version of Ball Studio"),
            LoadError::Broken(e) => write!(f, "This scene file is damaged: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// The scene as pretty RON text.
pub fn save(scene: &Scene) -> String {
    ron::ser::to_string_pretty(scene, ron::ser::PrettyConfig::default()).expect("scenes always serialize")
}

/// Opens a saved scene. A hand-edited tree is mended (see [`Scene::mend`]), values of the wrong
/// type are dropped and numbers are pulled into range; settings and kinds the registry doesn't
/// know are kept for `resolve` to report.
pub fn load(text: &str, reg: &Registry) -> Result<Scene, LoadError> {
    match probe_format(text)? {
        None => return Err(LoadError::OldEditor),
        Some(v) if v < Scene::FORMAT => return Err(LoadError::OldEditor),
        Some(v) if v > Scene::FORMAT => return Err(LoadError::Newer(v)),
        Some(_) => {}
    }
    let mut scene: Scene = ron::from_str(text).map_err(|e| LoadError::Broken(e.to_string()))?;
    if !scene.mend() {
        return Err(LoadError::Broken("it has no scene settings".into()));
    }
    scene.give_colours();

    for thing in scene.things.values_mut() {
        if let Some(kind) = reg.thing(&thing.kind) {
            clean(&mut thing.params, kind.settings());
        }
        for card in thing.slots.values_mut() {
            if let Some(kind) = reg.module(&card.kind) {
                clean(&mut card.params, kind.settings());
            }
            clean_layers(card, reg);
        }
    }
    if let Some(last) = scene.things.keys().next_back() {
        scene.next_thing = scene.next_thing.max(last.0 + 1);
    }
    Ok(scene)
}

/// The file's `format` number, read without caring about anything else in it. Old files have none.
fn probe_format(text: &str) -> Result<Option<u32>, LoadError> {
    let probe: probe::Scene = ron::from_str(text).map_err(|e| LoadError::Broken(e.to_string()))?;
    Ok((probe.format > 0).then_some(probe.format))
}

mod probe {
    /// Only the format number; every other field is skipped. Named `Scene` so files the old editor
    /// wrote, which start with `Scene(`, still parse this far.
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    pub struct Scene {
        pub format: u32,
    }
}

/// Keeps one layer per id, then cleans each layer's settings like a card's: a layered key of a
/// layer that isn't in the card goes, and so does a value its layer kind can't hold or doesn't
/// know. Layers of unknown kinds keep everything, for `resolve` to report.
fn clean_layers(card: &mut crate::Slot, reg: &Registry) {
    let mut seen = std::collections::BTreeSet::new();
    card.layers.retain(|l| seen.insert(l.id));
    // A mask whose layer is gone goes too.
    let ids: std::collections::BTreeSet<crate::LayerId> =
        card.layers.iter().filter(|l| l.parent.is_none()).map(|l| l.id).collect();
    card.layers.retain(|l| l.parent.is_none_or(|p| ids.contains(&p)));
    let snapshot = card.params.clone();
    let kinds: std::collections::BTreeMap<crate::LayerId, Option<&dyn crate::LayerKind>> =
        card.layers.iter().map(|l| (l.id, reg.layer(&l.kind))).collect();
    let keys: Vec<String> = card.params.0.keys().filter(|k| crate::split_layered(k).is_some()).cloned().collect();
    for key in keys {
        let (id, plain) = crate::split_layered(&key).expect("filtered above");
        let v = card.params.remove(&key).expect("listed above");
        match kinds.get(&id) {
            None => {}
            Some(None) => card.params.set(&key, v),
            Some(Some(kind)) => {
                let get = |k: &str| snapshot.get(&crate::layered_key(id, k)).cloned();
                if let Some(def) = crate::layer_def(*kind, &get, plain)
                    && def.ty.accepts(&v)
                {
                    card.params.set(&key, def.clamp(v));
                }
            }
        }
    }
}

/// Drops values their setting can't hold and clamps the rest.
fn clean(params: &mut Params, defs: &[SettingDef]) {
    for def in defs {
        if let Some(v) = params.remove(def.key)
            && def.ty.accepts(&v)
        {
            params.set(def.key, def.clamp(v));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SlotId, ThingId, Value, testkit};

    #[test]
    fn scene_round_trips() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        testkit::push(&mut s, "balls", ThingId::ROOT);
        assert_eq!(load(&save(&s), &r).unwrap(), s);
    }

    #[test]
    fn old_editor_files_are_refused_with_a_clear_message() {
        let old =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/old-editor-scene.ron")).unwrap();
        let e = load(&old, &testkit::registry()).unwrap_err();
        assert_eq!(e.to_string(), "This scene was made with the old Ball Studio");
    }

    #[test]
    fn load_cleans_layer_settings_and_drops_those_of_missing_layers() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.stack").unwrap();
        s.add_layer(&r, b, SlotId::Look, "layer.base").unwrap();
        s.add_layer(&r, b, SlotId::Look, "layer.ring").unwrap();
        let card = s.card_mut(b, SlotId::Look).unwrap();
        card.layers.push(crate::Layer { id: crate::LayerId(4), kind: "layer.hologram".into(), ..Default::default() });
        card.layers.push(crate::Layer { id: crate::LayerId(1), kind: "layer.ring".into(), ..Default::default() });
        for (k, v) in [
            ("1/width", Value::Number(500.0)),
            ("1/wobble", Value::Number(1.0)),
            ("0/size", Value::Bool(true)),
            ("9/width", Value::Number(3.0)),
            ("4/shimmer", Value::Number(3.0)),
        ] {
            card.params.set(k, v);
        }
        let back = load(&save(&s), &r).unwrap();
        let card = &back.things[&b].slots[&SlotId::Look];
        assert_eq!(card.layers.len(), 3, "a repeated id is dropped");
        assert_eq!(card.params.get("1/width"), Some(&Value::Number(60.0)));
        for gone in ["1/wobble", "0/size", "9/width"] {
            assert_eq!(card.params.get(gone), None, "{gone}");
        }
        assert_eq!(card.params.get("4/shimmer"), Some(&Value::Number(3.0)), "unknown layer kinds keep theirs");
    }

    #[test]
    fn format_three_to_six_files_are_old() {
        for v in 3..=6 {
            assert_eq!(load(&format!("(format: {v})"), &testkit::registry()).unwrap_err(), LoadError::OldEditor);
        }
    }

    #[test]
    fn newer_files_are_refused() {
        let e = load("(format: 8)", &testkit::registry()).unwrap_err();
        assert_eq!(e.to_string(), "This scene was made with a newer version of Ball Studio");
    }

    #[test]
    fn lower_format_numbers_count_as_old() {
        assert_eq!(load("(format: 1)", &testkit::registry()).unwrap_err(), LoadError::OldEditor);
        assert_eq!(load("(format: 2)", &testkit::registry()).unwrap_err(), LoadError::OldEditor);
        assert!(matches!(load("(format: 7", &testkit::registry()).unwrap_err(), LoadError::Broken(_)));
    }

    #[test]
    fn load_clamps_out_of_range_values() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = testkit::push(&mut s, "balls", ThingId::ROOT);
        s.things.get_mut(&b).unwrap().params.set("radius", Value::Number(5000.0));
        let back = load(&save(&s), &r).unwrap();
        assert_eq!(back.things[&b].params.get("radius"), Some(&Value::Number(200.0)));
    }

    #[test]
    fn load_cleans_card_params_with_the_card_kinds_settings() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.add_slot(&r, b, SlotId::Look, "look.plain").unwrap();
        let card = s.things.get_mut(&b).unwrap().slots.get_mut(&SlotId::Look).unwrap();
        card.params.set("glow", Value::Number(99.0));
        card.params.set("color", Value::Bool(true));
        card.params.set("wobble", Value::Number(1.0));
        let back = load(&save(&s), &r).unwrap();
        let card = &back.things[&b].slots[&SlotId::Look];
        assert_eq!(card.params.get("glow"), Some(&Value::Number(2.0)));
        assert_eq!(card.params.get("color"), None);
        assert_eq!(card.params.get("wobble"), Some(&Value::Number(1.0)), "unknown keys are left for resolve");
    }

    #[test]
    fn load_mends_the_tree_and_refuses_a_file_without_defaults() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let a = s.add_thing(&r, "folder", ThingId::ROOT).unwrap();
        let b = s.add_thing(&r, "balls", a).unwrap();
        s.things.get_mut(&b).unwrap().id = ThingId(50);
        s.things.get_mut(&a).unwrap().parent = Some(ThingId(123));
        s.order = vec![b];
        s.next_thing = 0;
        let back = load(&save(&s), &r).unwrap();
        assert_eq!(back.order, vec![ThingId::ROOT, a, b]);
        assert_eq!(back.things[&a].parent, Some(ThingId::ROOT));
        assert_eq!(back.things[&b].id, b);
        assert_eq!(back.next_thing, b.0 + 1);
        let e = load("(format: 7)", &r).unwrap_err();
        assert_eq!(e.to_string(), "This scene file is damaged: it has no scene settings");
    }

    #[test]
    fn a_moulded_path_survives_save_and_load() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let mut path = crate::ShapePath::circle();
        path.add_point(1, 0.25);
        s.things.get_mut(&b).unwrap().params.set("path", Value::Path(path.clone()));
        let back = load(&save(&s), &r).unwrap();
        assert_eq!(back.things[&b].params.get("path"), Some(&Value::Path(path)));
    }

    #[test]
    fn load_drops_non_finite_numbers() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        s.things.get_mut(&b).unwrap().params.set("radius", Value::Number(f64::NAN));
        assert_eq!(load(&save(&s), &r).unwrap().things[&b].params.get("radius"), None);
    }

    #[test]
    fn load_drops_values_of_the_wrong_type() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = testkit::push(&mut s, "balls", ThingId::ROOT);
        s.things.get_mut(&b).unwrap().params.set("radius", Value::Text("big".into()));
        assert_eq!(load(&save(&s), &r).unwrap().things[&b].params.get("radius"), None);
    }
}
