//! Undo, redo, and copying, pasting, duplicating and deleting things.

use super::*;

impl Studio {
    /// Copies `ids` (with everything under them) to the clipboard. False when there was nothing
    /// that can be copied (the scene settings, the camera).
    pub fn copy_things(&mut self, ids: &[ThingId]) -> bool {
        let Some(clip) = self.scene.copy_things(&self.kit.registry, ids) else {
            if !ids.is_empty() {
                self.status = "The scene settings and the camera can't be copied.".into();
            }
            return false;
        };
        let clipboard = crate::things::Clipboard::Things(clip);
        self.status = format!("Copied {}.", clipboard.things_words().unwrap_or_default());
        self.copied_note = Some(format!("Ball Studio: {}", self.status));
        self.clipboard = clipboard;
        true
    }

    /// Pastes the copied things beside `at` (into it when it is a folder; with `None`, beside the
    /// selection) and selects them, as an undo step of its own. False with no things copied.
    pub fn paste_things(&mut self, at: Option<ThingId>) -> bool {
        let crate::things::Clipboard::Things(clip) = &self.clipboard else { return false };
        let clip = clip.clone();
        let (parent, after) = crate::things::paste_place(&self.scene, at.or(self.selection.primary));
        match self.scene.paste_things(&self.kit.registry, &clip, parent, after) {
            Ok(pasted) if !pasted.tops.is_empty() => {
                tree::select_all(&mut self.selection, &pasted.tops);
                self.status = crate::things::pasted_words(&clip, &pasted);
                self.separate_edit = true;
                true
            }
            _ => {
                self.status = "Nothing to paste: this scene has its camera already.".into();
                false
            }
        }
    }

    /// Duplicates the selected things with everything under them, each copy right after its
    /// original, and selects the copies, as an undo step of its own.
    pub fn duplicate_selection(&mut self) -> bool {
        let ids = self.selection.all(&self.scene);
        let copies = crate::things::duplicate(&mut self.scene, &self.kit.registry, &ids);
        if copies.is_empty() {
            return false;
        }
        tree::select_all(&mut self.selection, &copies);
        self.separate_edit = true;
        true
    }

    /// Deletes the selected things (never the scene settings or the camera), as an undo step of
    /// its own.
    pub fn delete_selection(&mut self) -> bool {
        let ids = self.selection.all(&self.scene);
        let reg = &self.kit.registry;
        let gone = tree::delete_things(&mut self.panels.tree, &mut self.scene, reg, &mut self.selection, ids);
        if gone.is_some() {
            self.separate_edit = true;
        }
        gone.is_some()
    }

    pub fn undo(&mut self) -> bool {
        if !self.history.undo(&mut self.scene) {
            return false;
        }
        self.status = "Undone.".into();
        self.panels.timeline.selected.clear();
        self.recorded = true;
        self.snapshot = None;
        self.scene_changed();
        true
    }

    pub fn redo(&mut self) -> bool {
        if !self.history.redo(&mut self.scene) {
            return false;
        }
        self.status = "Redone.".into();
        self.panels.timeline.selected.clear();
        self.recorded = true;
        self.snapshot = None;
        self.scene_changed();
        true
    }
}
