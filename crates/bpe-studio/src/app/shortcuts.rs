//! Keyboard shortcuts.

use super::*;

impl Studio {
    pub(super) fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused();
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)));
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
            self.save(true);
        }
        if pressed(Modifiers::COMMAND, Key::S) {
            self.save(false);
        }
        if pressed(Modifiers::COMMAND, Key::O) {
            self.guard(Pending::Open, ctx);
        }
        if pressed(Modifiers::COMMAND, Key::N) {
            self.guard(Pending::New, ctx);
        }
        // F11: fully fullscreen (no title bar), and back to the window.
        if pressed(Modifiers::NONE, Key::F11) {
            let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
        }
        if typing {
            return;
        }
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            self.redo();
        }
        if pressed(Modifiers::COMMAND, Key::Z) {
            self.undo();
        }
        if ctx.input(|i| i.key_pressed(Key::Space)) {
            self.set_playing(!self.playing);
        }
        // Other keys belong to a slider or button that has the keyboard focus.
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::Home)) {
            self.seek(0.0);
        }
        if ctx.input(|i| i.key_pressed(Key::End)) {
            self.set_playing(false);
            self.seek(self.duration());
        }
        // Left and Right step one frame; with Shift, one beat. (Shift first: the plain shortcut
        // matches with Shift held too.)
        if pressed(Modifiers::SHIFT, Key::ArrowLeft) {
            self.step(-1.0, true);
        } else if pressed(Modifiers::NONE, Key::ArrowLeft) {
            self.step(-1.0, false);
        }
        if pressed(Modifiers::SHIFT, Key::ArrowRight) {
            self.step(1.0, true);
        } else if pressed(Modifiers::NONE, Key::ArrowRight) {
            self.step(1.0, false);
        }
    }

    /// The keys that act on what is selected, read after the panels so a panel that used one
    /// (the timeline's Delete on a selected key) keeps it: Ctrl+D, Ctrl+C, Ctrl+V, and Delete
    /// from the Lights or Mixer tab (the Things tab's tree reads Delete itself).
    pub(crate) fn late_shortcuts(&mut self, ctx: &egui::Context) {
        if let Some(note) = self.copied_note.take() {
            ctx.copy_text(note);
        }
        if ctx.text_edit_focused() || self.confirm.is_some() || self.offer.is_some() || self.ask_stop_export {
            return;
        }
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)));
        // egui-winit sends Ctrl+C and Ctrl+V as copy and paste events instead of keys, and Ctrl+V
        // only when the system clipboard holds text (or a picture): a copy here puts a note there.
        let event = |f: fn(&egui::Event) -> bool| ctx.input(|i| i.events.iter().any(f));
        let copy = pressed(Modifiers::COMMAND, Key::C) | event(|e| matches!(e, egui::Event::Copy));
        let paste = pressed(Modifiers::COMMAND, Key::V)
            | event(|e| matches!(e, egui::Event::Paste(_) | egui::Event::PasteImage(_)));
        if pressed(Modifiers::COMMAND, Key::D) {
            self.duplicate_selection();
        }
        if copy && !timeline::copy_keys(self) {
            let ids = self.selection.all(&self.scene);
            self.copy_things(&ids);
        }
        if paste {
            match &self.clipboard {
                crate::things::Clipboard::Keys(_) => _ = timeline::paste_keys(self),
                crate::things::Clipboard::Things(_) => _ = self.paste_things(None),
                crate::things::Clipboard::Empty => {}
            }
        }
        if self.left != LeftPanel::Things && pressed(Modifiers::NONE, Key::Delete) {
            self.delete_selection();
        }
    }
}
