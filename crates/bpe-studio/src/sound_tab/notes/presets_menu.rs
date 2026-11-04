//! Chord presets: 40 progressions in 8 moods, each shown as numerals and as chords in the
//! shape's key. Choosing one fills the slots (one chord each, voiced to move little inside the
//! keyboard's keys) and sets the slots to change By tempo every bar.

use super::{NotesState, SLOTS, key, octaves};
use crate::sound_tab::{TabCx, TabEdit, name_it};
use crate::style;
use bpe_core::Value;
use bpe_kit::sound::{BeatsOrBars, Change};
use bpe_sound::music::{Mood, PRESETS, Preset, Voicing, chord_labels, realise};
use eframe::egui::{self, RichText, Ui};

/// What a preset's menu entry reads: "I–V–vi–IV   C G Am F".
pub fn entry(p: &Preset, cx: &TabCx) -> String {
    let (root, scale) = key(cx);
    format!("{}   {}", p.numerals, chord_labels(p, root, scale).join(" "))
}

/// The edits choosing `p` makes: the slots, and By tempo every 1 bar.
pub fn choose(cx: &mut TabCx, p: &Preset, voicing: Voicing, add_bass: bool) {
    let (root, scale) = key(cx);
    let (low, n) = octaves(cx);
    let range = 12 * low..=(12 * (low as u16 + n as u16) - 1).min(127) as u8;
    let slots = realise(p, root, scale, range, voicing, add_bass);
    cx.edits.push(TabEdit::OwnStep);
    cx.set(SLOTS, Value::Slots(slots));
    cx.set("notes_change", Value::Choice(bpe_core::Choice::name(Change::ByTempo).into()));
    cx.set("notes_every_unit", Value::Choice(bpe_core::Choice::name(BeatsOrBars::Bars).into()));
    cx.set("notes_every_beats", Value::Int(1));
}

/// The Chord presets button and its menu.
pub fn show(ui: &mut Ui, cx: &mut TabCx, state: &mut NotesState) {
    let mut chosen = None;
    // The options stay open while they are set; a preset closes the menu.
    let stay = egui::containers::menu::MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let menu = egui::containers::menu::MenuButton::new("Chord presets").config(stay).ui(ui, |ui| {
        ui.set_min_width(300.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for v in Voicing::ALL {
                if ui.add(style::toggle_button(v.label(), state.voicing == v)).clicked() {
                    state.voicing = v;
                }
            }
            ui.add_space(8.0);
            let r = style::switch(ui, &mut state.add_bass);
            name_it(ui, &r, "Add bass");
            ui.label(RichText::new("Add bass").small().color(style::MUTED));
        });
        ui.label(RichText::new("Fills the slots, one chord each, changing every bar.").small().color(style::MUTED));
        ui.separator();
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            for mood in Mood::ALL {
                ui.label(RichText::new(mood.label()).font(style::semibold(12.0)));
                for p in PRESETS.iter().filter(|p| p.mood == mood) {
                    let text = entry(p, cx);
                    let r = ui.button(RichText::new(&text).font(style::mono(11.0))).on_hover_text(p.name);
                    name_it(ui, &r, &format!("{} {}", p.name, text));
                    if r.clicked() {
                        chosen = Some(*p);
                        ui.close();
                    }
                }
            }
        });
    });
    name_it(ui, &menu.0, "Chord presets");
    if let Some(p) = chosen {
        choose(cx, &p, state.voicing, state.add_bass);
    }
}
