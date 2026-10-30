//! Every scene file under `scenes/next/` opens cleanly: the loader keeps settings it doesn't know
//! for `resolve` and quietly drops values of the wrong type, so this compares the file's own keys
//! with the registry instead of trusting a load that "worked".

use bpe_core::{Params, Scene, SettingDef, SlotLink};
use bpe_kit::registry;

/// What is wrong with one set of saved settings: keys no setting has, values the setting can't
/// hold, and numbers out of range.
fn bad_keys(params: &Params, defs: &[SettingDef], whose: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (key, value) in params.iter() {
        match defs.iter().find(|d| d.key == key) {
            None => out.push(format!("{whose}: unknown key `{key}`")),
            Some(d) if !d.ty.accepts(value) => out.push(format!("{whose}: `{key}` can't hold {value:?}")),
            Some(d) if d.clamp(value.clone()) != *value => out.push(format!("{whose}: `{key}` is out of range")),
            Some(_) => {}
        }
    }
    out
}

#[test]
fn every_bundled_scene_loads_with_known_keys() {
    let reg = registry();
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenes/next");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "ron"))
        .collect();
    files.sort();
    assert!(files.len() >= 30, "found {} scene files", files.len());
    for file in files {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&file).unwrap();
        let loaded = bpe_core::load(&text, &reg).unwrap_or_else(|e| panic!("{name}: {e}"));
        let problems = bpe_kit::resolve_scene(&loaded, &reg).problems;
        assert!(problems.is_empty(), "{name}: {problems:?}");

        // The file as written, before the loader cleans anything.
        let raw: Scene = ron::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut bad = Vec::new();
        for module in raw.library.modules.values() {
            let kind = reg.module(&module.kind).unwrap_or_else(|| panic!("{name}: module kind {:?}", module.kind));
            bad.extend(bad_keys(&module.params, kind.settings(), &format!("module {}", module.kind)));
        }
        for thing in raw.things.values() {
            let kind = reg.thing(&thing.kind).unwrap_or_else(|| panic!("{name}: thing kind {:?}", thing.kind));
            bad.extend(bad_keys(&thing.params, kind.settings(), &format!("thing {}", thing.kind)));
            for link in thing.slots.values() {
                if let SlotLink::Linked { module, overrides } = link {
                    let kind = &raw.library.modules[module].kind;
                    let defs = reg.module(kind).unwrap().settings();
                    bad.extend(bad_keys(overrides, defs, &format!("override on {kind}")));
                }
            }
        }
        assert!(bad.is_empty(), "{name}:\n{}", bad.join("\n"));
    }
}
