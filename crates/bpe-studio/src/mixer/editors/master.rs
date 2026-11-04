//! The Master's editor: Input, Output, Ceiling and Release, the level and turned-down meters,
//! the loudness readouts and the loudness graph with its -14 LUFS mark.

use super::super::master as strip;
use crate::sound_tab::panel;
use crate::style;
use bpe_core::Scene;
use bpe_kit::Kit;
use bpe_sound::MeterReading;
use eframe::egui::{RichText, Ui};

pub fn show(ui: &mut Ui, scene: &mut Scene, kit: &Kit, reading: &MeterReading, loudness: &[f32], time: f64) {
    panel(ui, |ui| {
        ui.horizontal_top(|ui| {
            for key in strip::KNOBS {
                strip::knob(ui, scene, kit, key, time, 64.0);
            }
            ui.add_space(10.0);
            style::level_meter(ui, reading.peak_l, reading.peak_r, 72.0).on_hover_text("Peak level, left and right");
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("Turned down").small().color(style::MUTED));
                ui.label(RichText::new(format!("{:.1} dB", reading.reduction.min(0.0))).font(style::mono(13.0)));
            });
        });
        strip::loudness(ui, reading, loudness);
    });
}
