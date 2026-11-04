//! A Hit modulator's editor: which number it takes from each hit, Low, High, Curve and Smooth,
//! and the last hits as bars (what each gave).

use super::super::TabCx;
use crate::style;
use bpe_core::{LayerId, layered_key};
use bpe_kit::sound::HitPick;
use bpe_sound::Meters;
use bpe_sound::mods::hit::hit_value;
use eframe::egui::{self, RichText, Sense, Ui, Vec2};

fn plain(v: f64) -> String {
    format!("{v:.2}")
}

fn signed(v: f64) -> String {
    format!("{v:+.0}")
}

fn ms(v: f64) -> String {
    format!("{v:.0} ms")
}

/// Draws Hit `layer`'s editor.
pub fn show(ui: &mut Ui, cx: &mut TabCx, layer: LayerId, meters: Option<&Meters>) {
    let key = |k: &str| layered_key(layer, k);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Takes").small().color(style::MUTED));
        let chips: Vec<(HitPick, &str)> = HitPick::ALL.iter().map(|p| (*p, p.label())).collect();
        cx.chips(ui, &key("pick"), &chips);
    });
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        cx.knob(ui, &key("low"), "Low", &plain).on_hover_text("This or less gives 0");
        cx.knob(ui, &key("high"), "High", &plain).on_hover_text("This or more gives 1");
        cx.knob(ui, &key("curve"), "Curve", &signed);
        cx.knob(ui, &key("smooth"), "Smooth", &ms);
        ui.add_space(12.0);
        let pick: HitPick = cx.choice(&key("pick"));
        let (low, high, curve) =
            (cx.number(&key("low")) as f32, cx.number(&key("high")) as f32, cx.number(&key("curve")) as f32 / 100.0);
        let hits = meters.map(|m| m.recent_hits(cx.id.0)).unwrap_or_default();
        let values: Vec<f32> = hits.iter().map(|h| hit_value(pick, h, low, high, curve).clamp(0.0, 1.0)).collect();
        bars(ui, cx, &values);
    });
}

/// The last hits as bars, newest on the right.
fn bars(ui: &mut Ui, cx: &TabCx, values: &[f32]) {
    let n = bpe_sound::meters::RECENT_HITS;
    let (rect, r) = ui.allocate_exact_size(Vec2::new(160.0, 56.0), Sense::hover());
    super::super::name_it(ui, &r, "Recent hits");
    let r = r.on_hover_text("What the last hits gave, newest on the right");
    let _ = r;
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    let inner = rect.shrink(4.0);
    let w = inner.width() / n as f32;
    let start = n - values.len().min(n);
    for (i, v) in values.iter().enumerate() {
        let x = inner.left() + (start + i) as f32 * w;
        let h = (inner.height() * v).max(1.0);
        let bar = egui::Rect::from_min_size(egui::pos2(x + 1.0, inner.bottom() - h), Vec2::new(w - 2.0, h));
        painter.rect_filled(bar, 1.0, cx.colour);
    }
    if values.is_empty() {
        painter.text(
            inner.center(),
            egui::Align2::CENTER_CENTER,
            "Play to see hits",
            egui::FontId::proportional(11.0),
            style::FAINT,
        );
    }
}
