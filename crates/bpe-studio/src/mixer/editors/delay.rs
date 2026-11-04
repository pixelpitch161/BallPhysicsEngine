//! The delay editor: its settings, with the echoes drawn as they fall away.

use super::super::cx::MixCx;
use super::{controls, panel, params};
use crate::style;
use bpe_core::{SettingDef, Settings};
use bpe_kit::sound::fx::{DelayMode, DelaySettings};
use eframe::egui::{self, Pos2, Sense, Stroke, Ui, Vec2};

pub fn show(ui: &mut Ui, cx: &mut MixCx, defs: &[SettingDef]) {
    let set = DelaySettings::from_params(&params(cx, defs));
    panel(ui, |ui| {
        echoes(ui, &set);
        controls(ui, cx, defs, &[]);
    });
}

/// A line per echo over two seconds, as tall as it is loud (the feedback each time round).
fn echoes(ui: &mut Ui, set: &DelaySettings) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 70.0), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Echoes"));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    let seconds = if set.sync {
        // At the editor the tempo isn't known: a quarter at 120 bpm is half a second.
        set.time_note.wholes() * 2.0 * set.time_feel.factor()
    } else {
        set.time_ms as f64 / 1000.0
    };
    let span = 2.0;
    let x = |t: f64| rect.left() + rect.width() * (t / span) as f32;
    let mut lines = Vec::new();
    match set.mode {
        DelayMode::MultiTap => {
            for (t, db) in [
                (set.tap1_time, set.tap1_level),
                (set.tap2_time, set.tap2_level),
                (set.tap3_time, set.tap3_level),
                (set.tap4_time, set.tap4_level),
            ] {
                lines.push((seconds * t as f64 / 100.0, bpe_dsp::db_to_gain(db)));
            }
        }
        _ => {
            let mut g = 1.0f32;
            let mut t = seconds;
            while t <= span && g > 0.01 && seconds > 0.0 {
                lines.push((t, g));
                g *= set.feedback / 100.0;
                t += seconds;
            }
        }
    }
    let base = rect.bottom() - 4.0;
    painter.line_segment(
        [Pos2::new(rect.left(), base), Pos2::new(rect.left(), rect.top() + 4.0)],
        Stroke::new(2.0, style::MUTED),
    );
    for (t, g) in lines {
        let h = (rect.height() - 8.0) * g.clamp(0.0, 1.0);
        painter.line_segment([Pos2::new(x(t), base), Pos2::new(x(t), base - h)], Stroke::new(2.0, style::TEXT));
    }
}
