//! The code box of a Shader layer's own code: monospace, several lines, kept on clicking away
//! or Ctrl+Enter, with why the code can't run under it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use eframe::egui::{self, Key, Modifiers, RichText, Ui};

use crate::style;

/// How many lines the box shows before it scrolls.
const ROWS: usize = 12;

/// Draws the box for `code` and returns the new code when an edit is kept.
pub fn show(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug, code: &str) -> Option<String> {
    let id = ui.make_persistent_id(("code box", salt));
    let edit_id = id.with("edit");
    let editing = ui.memory(|m| m.has_focus(edit_id));
    // While the box isn't being typed in, it shows the code as saved (undo, paste, another ball).
    let mut text = if editing { ui.data(|d| d.get_temp::<String>(id)) } else { None }.unwrap_or_else(|| code.into());
    let label = ui.label(RichText::new("Code").color(style::MUTED));
    // Ctrl+Enter keeps the code rather than starting a new line.
    let keep = editing && ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
    let response = egui::ScrollArea::vertical()
        .id_salt(id.with("scroll"))
        .max_height(ROWS as f32 * 16.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .id(edit_id)
                    .font(style::mono(12.0))
                    .code_editor()
                    .desired_rows(ROWS)
                    .desired_width(f32::INFINITY)
                    .lock_focus(true),
            )
            .labelled_by(label.id)
            .on_hover_text("Click away or press Ctrl+Enter to use the code")
        })
        .inner;
    let kept = (keep || response.lost_focus()) && text != code;
    ui.data_mut(|d| d.insert_temp(id, text.clone()));
    let shown = if kept { text.as_str() } else { code };
    match problem(shown) {
        Some(why) => {
            ui.label(RichText::new(format!("Doesn't run: {why}")).small().color(style::TEXT));
        }
        None => {
            ui.label(RichText::new("Runs").small().color(style::FAINT));
        }
    }
    kept.then_some(text)
}

/// Why `code` can't run inside a ball, worked out once per distinct code.
pub fn problem(code: &str) -> Option<String> {
    static SEEN: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let mut seen = SEEN.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = seen.get(code) {
        return p.clone();
    }
    if seen.len() > 64 {
        seen.clear();
    }
    let p = bpe_gfx::in_ball_problem(code);
    seen.insert(code.to_owned(), p.clone());
    p
}
