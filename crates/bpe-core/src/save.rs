//! Saving and opening scenes.

use crate::{Params, Registry, Scene, SettingDef, SlotLink};
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
        return Err(LoadError::Broken("it has no Scene defaults".into()));
    }

    for module in scene.library.modules.values_mut() {
        if let Some(kind) = reg.module(&module.kind) {
            module.slot = kind.slot();
            clean(&mut module.params, kind.settings());
        }
    }
    for thing in scene.things.values_mut() {
        if let Some(kind) = reg.thing(&thing.kind) {
            clean(&mut thing.params, kind.settings());
        }
        for link in thing.slots.values_mut() {
            if let SlotLink::Linked { module, overrides } = link
                && let Some(kind) = scene.library.modules.get(module).and_then(|m| reg.module(&m.kind))
            {
                clean(overrides, kind.settings());
            }
        }
    }
    if let Some(last) = scene.things.keys().next_back() {
        scene.next_thing = scene.next_thing.max(last.0 + 1);
    }
    if let Some(last) = scene.library.modules.keys().next_back() {
        scene.next_module = scene.next_module.max(last.0 + 1);
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
    use crate::{Color, SlotId, ThingId, Value, testkit};

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
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenes/reference-reel.ron")).unwrap();
        let e = load(&old, &testkit::registry()).unwrap_err();
        assert_eq!(e.to_string(), "This scene was made with the old Ball Studio");
    }

    #[test]
    fn newer_files_are_refused() {
        let e = load("(format: 3)", &testkit::registry()).unwrap_err();
        assert_eq!(e.to_string(), "This scene was made with a newer version of Ball Studio");
    }

    #[test]
    fn lower_format_numbers_count_as_old() {
        assert_eq!(load("(format: 1)", &testkit::registry()).unwrap_err(), LoadError::OldEditor);
        assert!(matches!(load("(format: 2", &testkit::registry()).unwrap_err(), LoadError::Broken(_)));
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
    fn load_cleans_module_params_and_overrides_with_the_module_kinds_settings() {
        let r = testkit::registry();
        let mut s = Scene::empty(&r);
        let b = s.add_thing(&r, "balls", ThingId::ROOT).unwrap();
        let m = s.add_module(&r, "look.plain", "Plain").unwrap();
        s.link(&r, b, SlotId::Look, m).unwrap();
        let module = s.library.modules.get_mut(&m).unwrap();
        module.params.set("glow", Value::Text("bright".into()));
        module.params.set("color", Value::Color(Color::rgb(1, 2, 3)));
        module.slot = SlotId::Sound; // a hand edit the registry corrects
        if let Some(SlotLink::Linked { overrides, .. }) = s.things.get_mut(&b).unwrap().slots.get_mut(&SlotId::Look) {
            overrides.set("glow", Value::Number(99.0));
            overrides.set("color", Value::Bool(true));
            overrides.set("wobble", Value::Number(1.0));
        }
        let back = load(&save(&s), &r).unwrap();
        let module = &back.library.modules[&m];
        assert_eq!(module.params.get("glow"), None);
        assert_eq!(module.params.get("color"), Some(&Value::Color(Color::rgb(1, 2, 3))));
        assert_eq!(module.slot, SlotId::Look);
        let SlotLink::Linked { overrides, .. } = &back.things[&b].slots[&SlotId::Look] else { panic!("still linked") };
        assert_eq!(overrides.get("glow"), Some(&Value::Number(2.0)));
        assert_eq!(overrides.get("color"), None);
        assert_eq!(overrides.get("wobble"), Some(&Value::Number(1.0)), "unknown keys are left for resolve");
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
        let e = load("(format: 2)", &r).unwrap_err();
        assert_eq!(e.to_string(), "This scene file is damaged: it has no Scene defaults");
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
