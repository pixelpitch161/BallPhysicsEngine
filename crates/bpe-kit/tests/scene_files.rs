//! Every test scene, saved as a file, opens cleanly: the loader keeps settings it doesn't know
//! for `resolve` and quietly drops values of the wrong type, so this compares the file's own keys
//! with the registry instead of trusting a load that "worked".

use bpe_core::{Params, Scene, SettingDef};
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
fn every_saved_test_scene_loads_with_known_keys() {
    let reg = registry();
    let all = bpe_kit::scenes::all(&reg);
    assert!(all.len() >= 15, "found {} test scenes", all.len());
    for (name, scene) in all {
        let text = bpe_core::save(&scene);
        let loaded = bpe_core::load(&text, &reg).unwrap_or_else(|e| panic!("{name}: {e}"));
        let problems = bpe_kit::resolve_scene(&loaded, &reg).problems;
        assert!(problems.is_empty(), "{name}: {problems:?}");

        // The file as written, before the loader cleans anything.
        let raw: Scene = ron::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut bad = Vec::new();
        for thing in raw.things.values() {
            let kind = reg.thing(&thing.kind).unwrap_or_else(|| panic!("{name}: thing kind {:?}", thing.kind));
            bad.extend(bad_keys(&thing.params, kind.settings(), &format!("thing {}", thing.kind)));
            for card in thing.slots.values() {
                let kind = reg.module(&card.kind).unwrap_or_else(|| panic!("{name}: card kind {:?}", card.kind));
                // A layer's settings are kept under "<layer id>/<key>" and checked by its kind.
                let (layered, plain): (Vec<_>, Vec<_>) =
                    card.params.iter().partition(|(k, _)| bpe_core::split_layered(k).is_some());
                let plain = Params(plain.into_iter().map(|(k, v)| (k.clone(), v.clone())).collect());
                bad.extend(bad_keys(&plain, kind.settings(), &format!("card {}", card.kind)));
                for layer in &card.layers {
                    let lk = reg.layer(&layer.kind).unwrap_or_else(|| panic!("{name}: layer kind {:?}", layer.kind));
                    let own = Params(
                        layered
                            .iter()
                            .filter_map(|(k, v)| {
                                let (id, key) = bpe_core::split_layered(k)?;
                                (id == layer.id).then(|| (key.to_owned(), (*v).clone()))
                            })
                            .collect(),
                    );
                    bad.extend(bad_keys(&own, lk.settings(), &format!("layer {} of {}", layer.kind, card.kind)));
                }
                for (k, _) in &layered {
                    let (id, _) = bpe_core::split_layered(k).unwrap();
                    if card.layer(id).is_none() {
                        bad.push(format!("card {}: `{k}` belongs to no layer", card.kind));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "{name}:\n{}", bad.join("\n"));
    }
}
