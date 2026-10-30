//! The user's saved palettes: a small RON file beside the app (or in the settings folder when
//! that place is read-only). Scenes copy palette colours in; they never link to a palette.

use bpe_core::Gradient;
use bpe_kit::paint::palettes::Palette;
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const FILE: &str = "palettes.ron";

pub struct PaletteStore {
    path: PathBuf,
    pub saved: Vec<Palette>,
    broken: bool,
}

impl PaletteStore {
    /// Reads `path`. A missing file is an empty list; an unreadable or corrupt one is an empty
    /// list that is left alone on disk until the first save.
    pub fn load_from(path: PathBuf) -> PaletteStore {
        let (saved, broken) = match std::fs::read_to_string(&path) {
            Ok(text) => match ron::from_str::<Vec<Palette>>(&text) {
                Ok(list) => (list, false),
                Err(_) => (Vec::new(), true),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), false),
            Err(_) => (Vec::new(), true),
        };
        PaletteStore { path, saved, broken }
    }

    /// `palettes.ron` beside the executable when that folder is writable, else in the settings
    /// folder.
    pub fn open() -> PaletteStore {
        let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.to_path_buf()));
        let dir =
            beside.filter(|d| writable(d)).or_else(crate::settings::settings_dir).unwrap_or_else(|| PathBuf::from("."));
        PaletteStore::load_from(dir.join(FILE))
    }

    /// Saves `colours` under `name` (trimmed; empty becomes "My palette"), replacing a palette
    /// of the same name in place.
    pub fn save_as(&mut self, name: &str, colours: &Gradient) -> std::io::Result<()> {
        let name = name.trim();
        let name = if name.is_empty() { "My palette" } else { name };
        let palette = Palette { name: name.to_string(), colours: colours.clone() };
        match self.saved.iter_mut().find(|p| p.name == name) {
            Some(slot) => *slot = palette,
            None => self.saved.push(palette),
        }
        self.write()
    }

    pub fn remove(&mut self, name: &str) -> std::io::Result<()> {
        self.saved.retain(|p| p.name != name);
        self.write()
    }

    fn write(&mut self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if self.broken {
            if self.path.exists() {
                std::fs::copy(&self.path, sibling(&self.path, ".bak"))?;
            }
            self.broken = false;
        }
        let text = ron::ser::to_string_pretty(&self.saved, Default::default()).map_err(std::io::Error::other)?;
        let tmp = sibling(&self.path, ".tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)
    }
}

fn sibling(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

fn writable(dir: &std::path::Path) -> bool {
    let probe = dir.join(format!(".palettes-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

fn store_id() -> egui::Id {
    egui::Id::unique("palette store")
}

/// The saved palettes, if the app installed a store in this context.
#[allow(dead_code)]
pub fn store(ctx: &egui::Context) -> Option<Arc<Mutex<PaletteStore>>> {
    ctx.data(|d| d.get_temp::<Arc<Mutex<PaletteStore>>>(store_id()))
}

pub fn install(ctx: &egui::Context, store: PaletteStore) {
    ctx.data_mut(|d| d.insert_temp(store_id(), Arc::new(Mutex::new(store))));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Color;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ball-studio-palettes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(FILE)
    }

    fn grad(hexes: &[&str]) -> Gradient {
        let c: Vec<Color> = hexes.iter().map(|h| Color::from_hex(h).unwrap()).collect();
        Gradient::even(&c)
    }

    #[test]
    fn missing_file_loads_empty() {
        let s = PaletteStore::load_from(temp("missing"));
        assert!(s.saved.is_empty());
        assert!(!s.broken);
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = temp("round");
        let mut s = PaletteStore::load_from(path.clone());
        s.save_as("Mine", &grad(&["#ff0000", "#0000ff"])).unwrap();
        let back = PaletteStore::load_from(path);
        assert_eq!(back.saved, s.saved);
        assert_eq!(back.saved[0].name, "Mine");
        assert_eq!(back.saved[0].colours, grad(&["#ff0000", "#0000ff"]));
    }

    #[test]
    fn saving_an_existing_name_replaces_it() {
        let mut s = PaletteStore::load_from(temp("replace"));
        s.save_as("A", &grad(&["#ff0000", "#00ff00"])).unwrap();
        s.save_as("B", &grad(&["#0000ff", "#00ff00"])).unwrap();
        s.save_as("A", &grad(&["#111111", "#222222"])).unwrap();
        assert_eq!(s.saved.len(), 2);
        assert_eq!(s.saved[0].name, "A");
        assert_eq!(s.saved[0].colours, grad(&["#111111", "#222222"]));
    }

    #[test]
    fn a_corrupt_file_is_kept_as_bak_on_first_save() {
        let path = temp("corrupt");
        std::fs::write(&path, "nonsense(").unwrap();
        let mut s = PaletteStore::load_from(path.clone());
        assert!(s.saved.is_empty());
        assert!(s.broken);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "nonsense(", "not touched until a save");
        s.save_as("Fresh", &grad(&["#ff0000", "#0000ff"])).unwrap();
        assert_eq!(std::fs::read_to_string(path.with_extension("ron.bak")).unwrap(), "nonsense(");
        assert!(!s.broken);
        assert_eq!(PaletteStore::load_from(path).saved.len(), 1);
    }

    #[test]
    fn empty_name_becomes_my_palette() {
        let mut s = PaletteStore::load_from(temp("empty"));
        s.save_as("   ", &grad(&["#ff0000", "#0000ff"])).unwrap();
        assert_eq!(s.saved[0].name, "My palette");
    }

    #[test]
    fn remove_deletes_by_name() {
        let path = temp("remove");
        let mut s = PaletteStore::load_from(path.clone());
        s.save_as("A", &grad(&["#ff0000", "#0000ff"])).unwrap();
        s.save_as("B", &grad(&["#00ff00", "#0000ff"])).unwrap();
        s.remove("A").unwrap();
        assert_eq!(s.saved.len(), 1);
        assert_eq!(PaletteStore::load_from(path).saved[0].name, "B");
    }
}
