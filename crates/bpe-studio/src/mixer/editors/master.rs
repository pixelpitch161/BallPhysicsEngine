//! The Master's editor: Input, Output, Ceiling and Release, the level and turned-down meters,
//! the loudness readouts and the loudness graph with its -14 LUFS mark.

use super::super::master as strip;
use crate::sound_tab::panel;
use crate::style;
use bpe_core::Scene;
use bpe_kit::Kit;
use bpe_sound::MeterReading;
use eframe::egui::{self, RichText, Ui};

/// Returns whether a keyed setting was edited.
pub fn show(ui: &mut Ui, scene: &mut Scene, kit: &Kit, reading: &MeterReading, loudness: &[f32], time: f64) -> bool {
    let mut keyed = false;
    panel(ui, |ui| {
        // The knobs (wrapping in a narrow editor), then the meters.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for key in strip::KNOBS {
                keyed |= strip::knob(ui, scene, kit, key, time, 64.0);
            }
        });
        ui.horizontal_top(|ui| {
            style::level_meter(ui, reading.peak_l, reading.peak_r, 72.0).on_hover_text("Peak level, left and right");
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("Turned down").small().color(style::MUTED));
                ui.label(RichText::new(format!("{:.1} dB", reading.reduction.min(0.0))).font(style::mono(13.0)));
            });
        });
        strip::loudness(ui, reading, loudness);
    });
    keyed
}
