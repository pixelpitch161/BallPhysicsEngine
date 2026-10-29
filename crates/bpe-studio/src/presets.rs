//! My presets: modules saved as small files in the user's settings folder, listed in the
//! Library to add to any scene.

use bpe_core::{Module, ModuleId, Scene, SlotId};
use std::path::{Path, PathBuf};

/// A module saved on its own, ready to add to a scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub path: PathBuf,
    pub module: Module,
}

/// Every readable preset in `dir`, by name. A missing folder holds none.
pub fn list_in(dir: &Path) -> Vec<Preset> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut presets: Vec<Preset> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ron"))
        .filter_map(|path| {
            let module: Module = ron::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
            Some(Preset { path, module })
        })
        .collect();
    presets.sort_by(|a, b| a.module.name.cmp(&b.module.name).then_with(|| a.path.cmp(&b.path)));
    presets
}

/// Saves `module` in `dir` as `<slot>-<name>.ron`, with "-2", "-3"… when that file is taken.
/// The preset belongs to no scene, so its id is written as 0.
pub fn save_in(dir: &Path, module: &Module) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stem = format!("{}-{}", slug(module.slot.name()), slug(&module.name));
    let path = (1..)
        .map(|n| if n == 1 { dir.join(format!("{stem}.ron")) } else { dir.join(format!("{stem}-{n}.ron")) })
        .find(|p| !p.exists())
        .expect("some number is free");
    let preset = Module { id: ModuleId(0), ..module.clone() };
    let text = ron::ser::to_string_pretty(&preset, ron::ser::PrettyConfig::default())?;
    std::fs::write(&path, text)?;
    Ok(path)
}

pub fn delete(preset: &Preset) -> std::io::Result<()> {
    std::fs::remove_file(&preset.path)
}

/// Adds a copy of `module` to the scene under a fresh id. When a module of that slot already has
/// its name, the copy is called "Name (2)", then "Name (3)"…
pub fn add_to_scene(scene: &mut Scene, module: &Module) -> ModuleId {
    let id = ModuleId(scene.next_module);
    scene.next_module += 1;
    let name = free_name(scene, module.slot, &module.name);
    scene.library.modules.insert(id, Module { id, name, ..module.clone() });
    id
}

fn free_name(scene: &Scene, slot: SlotId, name: &str) -> String {
    let taken = |candidate: &str| scene.library.modules.values().any(|m| m.slot == slot && m.name == candidate);
    if !taken(name) {
        return name.to_owned();
    }
    (2..).map(|n| format!("{name} ({n})")).find(|c| !taken(c)).expect("some number is free")
}

/// A name as a file name: lower case, letters and digits, dashes between words.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() { "preset".to_owned() } else { out.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Address, Owner, Value};
    use bpe_kit::{kit, new_scene};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bpe-presets-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The new scene's ball look, renamed "Neon" with a glow.
    fn neon() -> (Scene, Module) {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let id = scene.library.modules.values().find(|m| m.name == "Ball look").expect("the ball look").id;
        scene.rename_module(id, "Neon").unwrap();
        let glow = Address { owner: Owner::Module(id), slot: None, key: "glow".into() };
        scene.set_at(&kit.registry, &glow, Value::Number(0.8)).unwrap();
        let module = scene.library.modules[&id].clone();
        (scene, module)
    }

    #[test]
    fn presets_save_list_and_delete_in_a_folder() {
        let dir = temp_dir("save");
        assert!(list_in(&dir).is_empty(), "no folder yet means no presets");
        let (_, module) = neon();
        let first = save_in(&dir, &module).unwrap();
        assert_eq!(first.file_name().unwrap(), "look-neon.ron");
        let second = save_in(&dir, &module).unwrap();
        assert_eq!(second.file_name().unwrap(), "look-neon-2.ron");
        let listed = list_in(&dir);
        assert_eq!(listed.len(), 2);
        for preset in &listed {
            assert_eq!(preset.module.name, "Neon");
            assert_eq!(preset.module.id, ModuleId(0), "a preset belongs to no scene");
            assert_eq!(preset.module.params, module.params);
            assert_eq!(preset.module.slot, SlotId::Look);
        }
        delete(&listed[0]).unwrap();
        assert_eq!(list_in(&dir).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn adding_a_preset_makes_a_fresh_module_with_a_free_name() {
        let (mut scene, module) = neon();
        let preset = Module { id: ModuleId(0), ..module.clone() };
        let before = scene.library.modules.len();
        let id = add_to_scene(&mut scene, &preset);
        assert_ne!(id, module.id);
        assert_eq!(scene.library.modules.len(), before + 1);
        let added = &scene.library.modules[&id];
        assert_eq!(added.name, "Neon (2)");
        assert_eq!(added.params, module.params);
        assert_eq!(added.slot, SlotId::Look);
        let again = add_to_scene(&mut scene, &preset);
        assert_eq!(scene.library.modules[&again].name, "Neon (3)");
    }

    #[test]
    fn slugs_are_file_safe() {
        assert_eq!(slug("Neon glow!"), "neon-glow");
        assert_eq!(slug("  "), "preset");
        assert_eq!(slug("Ünïcode/name"), "ünïcode-name");
    }
}
