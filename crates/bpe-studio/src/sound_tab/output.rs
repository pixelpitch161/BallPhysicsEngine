//! Where the shape's sound goes: Volume, Pan, Follow hit (the pan pulled toward where the hit is
//! across the picture), the bus and a level meter.

use super::{TabCx, osc::pan, panel};
use crate::style;
use eframe::egui::{self, RichText, Ui};

fn volume(v: f64) -> String {
    super::db(v)
}

fn percent(v: f64) -> String {
    format!("{v:.0} %")
}

/// Draws the Output panel; `level` is the shape's peak (dB) since the last look.
pub fn show(ui: &mut Ui, cx: &mut TabCx, level: f32) {
    panel(ui, |ui| {
        ui.label(RichText::new("Output").font(style::semibold(13.0)));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            cx.knob(ui, "out_volume", "Volume", &volume);
            cx.knob(ui, "out_pan", "Pan", &pan);
            cx.knob(ui, "out_follow_hit", "Follow hit", &percent)
                .on_hover_text("Pulls the pan toward where the hit is across the picture");
            ui.add_space(12.0);
            style::level_meter(ui, level, level, 64.0).on_hover_text("This shape's level");
        });
        ui.horizontal(|ui| {
            let label = ui.label(RichText::new("Bus").small().color(style::MUTED));
            ui.spacing_mut().item_spacing.x = 2.0;
            let one = ui.add(style::toggle_button("1", true).min_size(egui::vec2(24.0, 22.0)));
            let _ = one.labelled_by(label.id).on_hover_text("Bus 1 goes straight to the Master");
            for n in ["2", "3"] {
                ui.add_enabled(false, style::toggle_button(n, false).min_size(egui::vec2(24.0, 22.0)))
                    .on_disabled_hover_text("Buses 2 and 3 come with the Mixer");
            }
        });
    });
}
