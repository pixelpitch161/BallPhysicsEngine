//! The presets list: known progressions by mood, shown in the progression's own key.

use super::{ChordOutcome, ChordUi};
use crate::theme;
use bpe_core::{MusicKey, Progression};
use bpe_mix::{PRESETS, moods, numeral};
use eframe::egui::{self, RichText, Ui};

pub(super) fn show(ui: &mut Ui, p: &mut Progression, state: &mut ChordUi, out: &mut ChordOutcome) {
    egui::Frame::new().fill(theme::PANEL).corner_radius(6).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(state.mood.is_none(), "All").clicked() {
                state.mood = None;
            }
            for mood in moods() {
                if ui.selectable_label(state.mood == Some(mood), mood).clicked() {
                    state.mood = Some(mood);
                }
            }
        });
        ui.separator();
        let mut picked = None;
        egui::ScrollArea::vertical().id_salt("chord presets").max_height(220.0).show(ui, |ui| {
            for preset in PRESETS.iter().filter(|pr| state.mood.is_none_or(|m| m == pr.mood)) {
                let key = MusicKey { root: p.key.root, mode: preset.mode };
                let numerals: Vec<String> = preset.chords(key.root).iter().map(|c| numeral(c, key)).collect();
                ui.horizontal(|ui| {
                    if ui.button(preset.name).on_hover_text("Use these chords in this key").clicked() {
                        picked = Some(preset);
                    }
                    ui.label(numerals.join(" "));
                    ui.label(
                        RichText::new(format!("{} · {}", preset.mood, preset.mode.label())).small().color(theme::MUTED),
                    );
                });
            }
        });
        if let Some(preset) = picked {
            p.key.mode = preset.mode;
            p.chords = preset.chords(p.key.root);
            state.selected = None;
            state.presets_open = false;
            out.changed = true;
        }
    });
}
