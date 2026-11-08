//! Files dropped onto the window (editor workflow, W5): a scene opens (asking to save first), a
//! font is kept in the scene and the selected Texts are set in it, and a PNG is kept in the
//! scene as a Particles picture (the selected Particles things form it, or a new one does).
//! Anything else is reported in the status line.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bpe_core::{Address, ThingId, Value};
use eframe::egui;

use crate::app::{Pending, ProblemKind, Studio};

/// What a dropped file is, by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropped {
    Scene,
    Font,
    Picture,
    Other,
}

/// What the file at `name` is, by its extension.
pub fn what(name: &str) -> Dropped {
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase());
    match ext.as_deref() {
        Some("ron") => Dropped::Scene,
        Some("ttf" | "otf") => Dropped::Font,
        Some("png") => Dropped::Picture,
        _ => Dropped::Other,
    }
}

/// The status line's words for a file Ball Studio can't take.
pub fn cant_open(name: &str) -> String {
    format!("Ball Studio can't open {name}: drop a scene (.ron), a font (.ttf, .otf) or a PNG picture.")
}

impl Studio {
    /// Takes the files dropped on the window this frame.
    pub(crate) fn take_drops(&mut self, ctx: &egui::Context) {
        // (Not called while a dialog is open: files dropped then are let go.)
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        // One scene opens: the first.
        let mut opened = false;
        for file in dropped {
            let path = file.path().to_path_buf();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let bytes = || -> Arc<[u8]> { file.bytes().unwrap_or_default().into() };
            match what(&name) {
                Dropped::Scene if !opened => {
                    opened = true;
                    self.drop_scene(path, ctx);
                }
                Dropped::Scene => {}
                Dropped::Font => self.drop_font(&name, bytes()),
                Dropped::Picture => self.drop_picture(&name, bytes()),
                Dropped::Other => self.report(ProblemKind::File, cant_open(&name)),
            }
        }
    }

    fn drop_scene(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.guard(Pending::OpenPath(path), ctx);
    }

    /// Keeps the font in the scene and sets the selected Texts in it.
    pub(crate) fn drop_font(&mut self, name: &str, bytes: Arc<[u8]>) {
        if !bpe_kit::text::fonts::is_font(&bytes) {
            self.report(ProblemKind::File, format!("{name} isn't a font the program can read (.ttf or .otf)."));
            return;
        }
        let stem = crate::picture_picker::name_of(Path::new(name));
        let family = bpe_kit::text::fonts::family_name(&bytes).unwrap_or(stem);
        let family = self.scene.add_font(&family, bytes);
        let texts = self.selected_of(bpe_kit::things::text::KIND);
        for &t in &texts {
            self.set_now(Address::own(t, "font"), Value::Text(family.clone()));
        }
        self.status = match texts.len() {
            0 => format!("Added the font {family}: pick it in a Text's Font."),
            _ => format!("Added the font {family} and set the selected Text in it."),
        };
        self.separate_edit = true;
    }

    /// Keeps the picture in the scene and has the selected Particles things form it, or a new
    /// Particles thing when none is selected.
    pub(crate) fn drop_picture(&mut self, name: &str, bytes: Arc<[u8]>) {
        if !crate::picture_picker::is_png(&bytes) {
            self.report(ProblemKind::File, format!("{name} isn't a PNG picture the program can read."));
            return;
        }
        let picture = self.scene.add_picture(&crate::picture_picker::name_of(Path::new(name)), bytes);
        let mut things = self.selected_of("particles");
        if things.is_empty() {
            let kit = self.kit.clone();
            let parent = crate::things::paste_place(&self.scene, self.selection.primary).0;
            match bpe_kit::add_thing(&mut self.scene, &kit.registry, "particles", parent) {
                Ok(id) => {
                    self.place_added(id);
                    self.selection.set(id);
                    things.push(id);
                }
                Err(_) => return,
            }
        }
        for &t in &things {
            self.set_now(Address::own(t, "from"), Value::Choice("a_word_or_picture".into()));
            self.set_now(Address::own(t, "word_from"), Value::Choice("picture".into()));
            self.set_now(Address::own(t, bpe_core::PICTURE_KEY), Value::Text(picture.clone()));
        }
        self.status = format!("Added the picture {picture}: the particles form it.");
        self.separate_edit = true;
    }

    /// The selected things of `kind`.
    fn selected_of(&self, kind: &str) -> Vec<ThingId> {
        let all = self.selection.all(&self.scene);
        all.into_iter().filter(|id| self.scene.things.get(id).is_some_and(|t| t.kind == kind)).collect()
    }

    /// Sets the setting at `a`: as a key at the playhead when it has keys.
    fn set_now(&mut self, a: Address, value: Value) {
        let kit = self.kit.clone();
        let mut keyed = false;
        crate::inspector::set_or_key(&mut self.scene, &kit.registry, &a, value, self.time, &mut keyed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dropped_file_is_known_by_its_extension() {
        assert_eq!(what("C:\\Scenes\\Bounce.RON"), Dropped::Scene);
        assert_eq!(what("Anton.ttf"), Dropped::Font);
        assert_eq!(what("Font.OTF"), Dropped::Font);
        assert_eq!(what("logo.png"), Dropped::Picture);
        assert_eq!(what("song.wav"), Dropped::Other);
        assert_eq!(what("no extension"), Dropped::Other);
        assert!(cant_open("song.wav").contains("song.wav"));
    }
}
