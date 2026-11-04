//! The Notes header: Notes, the key (Root and Scale), Chord presets, and the keyboard's range
//! with − and +.

use super::{NotesState, OCTAVES, choice_menu, key, octaves, presets_menu};
use crate::sound_tab::keys::note_name;
use crate::sound_tab::{TabCx, name_it};
use crate::style;
use bpe_core::Value;
use bpe_kit::sound::{Root, Scale};
use eframe::egui::{self, RichText, Ui};

/// Draws the header. Returns a new key the user picked (the panel decides what to do with
/// the notes).
pub fn show(ui: &mut Ui, cx: &mut TabCx, state: &mut NotesState) -> Option<(Root, Scale)> {
    let (root, scale) = key(cx);
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Notes").font(style::semibold(13.0)));
        ui.add_space(6.0);
        ui.label(RichText::new("Key").small().color(style::MUTED));
        if let Some(r) = choice_menu(ui, cx, "notes_root", "Root", &Root::ALL, Root::label) {
            picked = Some((r, scale));
        }
        if let Some(s) = choice_menu(ui, cx, "notes_scale", "Scale", &Scale::ALL, Scale::label) {
            picked = Some((root, s));
        }
        presets_menu::show(ui, cx, state);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (low, n) = octaves(cx);
            let top = low + n;
            let more = ui.add_enabled(n < 8 && top < 10, egui::Button::new("+")).on_hover_text("One octave more");
            name_it(ui, &more, "One octave more");
            if more.clicked() {
                cx.set(OCTAVES, Value::Int(n as i64 + 1));
            }
            let fewer = ui.add_enabled(n > 1, egui::Button::new("-")).on_hover_text("One octave fewer");
            name_it(ui, &fewer, "One octave fewer");
            if fewer.clicked() {
                cx.set(OCTAVES, Value::Int(n as i64 - 1));
            }
            let range = format!("{} to {}", note_name(12 * low), note_name(12 * top - 1));
            ui.label(RichText::new(range).small().color(style::MUTED));
        });
    });
    picked
}
