//! The reverb editor: its settings, with the tail drawn as it dies away.

use super::super::cx::MixCx;
use super::{controls, panel, params};
use crate::style;
use bpe_core::{SettingDef, Settings};
use bpe_kit::sound::fx::ReverbSettings;
use eframe::egui::{self, Pos2, Sense, Shape, Stroke, Ui, Vec2};

pub fn show(ui: &mut Ui, cx: &mut MixCx, defs: &[SettingDef]) {
    let set = ReverbSettings::from_params(&params(cx, defs));
    panel(ui, |ui| {
        tail(ui, &set);
        controls(ui, cx, defs, &[]);
    });
}

/// The tail's level over time: the pre-delay, then a fall of 60 dB over the decay (flat while
/// frozen).
fn tail(ui: &mut Ui, set: &ReverbSettings) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 70.0), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Reverb tail"));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    let span = (set.decay * 1.2 + set.pre_delay / 1000.0).max(0.5);
    let pre = set.pre_delay / 1000.0;
    let line: Vec<Pos2> = (0..=120)
        .map(|i| {
            let t = span * i as f32 / 120.0;
            let db = if t < pre {
                -60.0
            } else if set.freeze {
                0.0
            } else {
                -60.0 * (t - pre) / set.decay.max(0.1)
            };
            let h = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            Pos2::new(rect.left() + rect.width() * t / span, rect.bottom() - 4.0 - (rect.height() - 8.0) * h)
        })
        .collect();
    painter.add(Shape::line(line, Stroke::new(2.0, style::TEXT)));
}
