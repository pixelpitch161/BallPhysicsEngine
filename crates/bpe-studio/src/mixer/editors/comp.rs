//! The compressor editor: its knobs, the sidechain, and a level graph (the curve from input to
//! output, the level going in now, and how far it is turning the sound down).

use super::super::cx::MixCx;
use super::{controls, panel, params};
use crate::style;
use bpe_core::{LayerId, SettingDef, Settings};
use bpe_dsp::fx::compressor::{CompParams, Compressor};
use bpe_kit::sound::fx::CompressorSettings;
use bpe_sound::{FxReading, Meters};
use eframe::egui::{self, Align2, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, Vec2};

/// The graph's side and the levels it shows.
const GRAPH: f32 = 180.0;
const LOW_DB: f32 = -60.0;

pub fn show(ui: &mut Ui, cx: &mut MixCx, meters: Option<&Meters>, id: LayerId, defs: &[SettingDef]) {
    let set = CompressorSettings::from_params(&params(cx, defs));
    let reading = meters.map_or(FxReading::SILENT, |m| m.read_fx(id.0));
    panel(ui, |ui| {
        ui.horizontal_top(|ui| {
            graph(ui, &set, &reading);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("Turned down").small().color(style::MUTED));
                ui.label(RichText::new(format!("{:.1} dB", reading.reduction.min(0.0))).font(style::mono(13.0)));
                gr_bar(ui, reading.reduction);
            });
        });
        controls(ui, cx, defs, &[]);
    });
}

/// The curve from input to output, the input level now and the turned-down amount.
fn graph(ui: &mut Ui, set: &CompressorSettings, reading: &FxReading) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(GRAPH), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Compressor levels"));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    let at = |inp: f32, out: f32| {
        let t = |db: f32| ((db - LOW_DB) / -LOW_DB).clamp(0.0, 1.0);
        Pos2::new(rect.left() + rect.width() * t(inp), rect.bottom() - rect.height() * t(out))
    };
    for db in [-48.0, -36.0, -24.0, -12.0] {
        let p = at(db, db);
        painter.line_segment(
            [Pos2::new(p.x, rect.top()), Pos2::new(p.x, rect.bottom())],
            Stroke::new(1.0, style::LINE_SOFT),
        );
        painter.line_segment(
            [Pos2::new(rect.left(), p.y), Pos2::new(rect.right(), p.y)],
            Stroke::new(1.0, style::LINE_SOFT),
        );
    }
    painter.line_segment([at(LOW_DB, LOW_DB), at(0.0, 0.0)], Stroke::new(1.0, style::LINE));
    let p = CompParams {
        threshold_db: set.threshold,
        ratio: set.ratio,
        attack_ms: set.attack,
        release_ms: set.release,
        knee_db: set.knee,
        makeup_db: set.makeup,
        sc_low_cut_hz: set.sc_low_cut,
        sc_high_cut_hz: set.sc_high_cut,
    };
    let line: Vec<Pos2> = (0..=60)
        .map(|i| {
            let inp = LOW_DB + i as f32;
            at(inp, Compressor::static_curve_db(&p, inp))
        })
        .collect();
    painter.add(Shape::line(line, Stroke::new(2.0, style::TEXT_STRONG)));
    if reading.input.is_finite() && reading.input > LOW_DB {
        let dot = at(reading.input, Compressor::static_curve_db(&p, reading.input));
        painter.circle_filled(dot, 4.0, style::TEXT);
    }
    let t = at(set.threshold, set.threshold);
    painter.text(
        Pos2::new(t.x + 4.0, rect.bottom() - 3.0),
        Align2::LEFT_BOTTOM,
        format!("{:.0}", set.threshold),
        style::mono(10.0),
        style::FAINT,
    );
}

/// A bar falling from the top as the compressor turns the sound down (0 to -24 dB).
fn gr_bar(ui: &mut Ui, reduction: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, GRAPH - 40.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 1.0, style::GROUND);
    let t = (-reduction / 24.0).clamp(0.0, 1.0);
    if t > 0.0 {
        let fill = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + rect.height() * t));
        painter.rect_filled(fill, 1.0, style::MUTED);
    }
}
