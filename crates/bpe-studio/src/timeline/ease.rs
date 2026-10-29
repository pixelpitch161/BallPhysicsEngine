//! Choosing how a key eases into the next: the named eases, or a curve of the user's own.

use bpe_core::Ease;
use eframe::egui::{self, DragValue, Pos2, Sense, Stroke, Ui};

/// A starting point for a curve of one's own: a gentle ease in and out.
pub const OWN_CURVE: Ease = Ease::Curve { x1: 0.42, y1: 0.0, x2: 0.58, y2: 1.0 };

/// The ease menu's items: every named ease and "Own curve…". The pick, if any.
pub fn ease_menu(ui: &mut Ui, current: &Ease) -> Option<Ease> {
    let mut picked = None;
    for ease in Ease::NAMED {
        if ui.selectable_label(*current == ease, ease.name()).clicked() {
            picked = Some(ease);
            ui.close();
        }
    }
    let own = matches!(current, Ease::Curve { .. });
    if ui.selectable_label(own, "Own curve…").clicked() {
        picked = Some(if own { *current } else { OWN_CURVE });
        ui.close();
    }
    picked
}

/// Four numbers and a picture of the curve they make. True when a number changed. Does nothing
/// for a named ease.
pub fn curve_editor(ui: &mut Ui, ease: &mut Ease) -> bool {
    let mut changed = false;
    if let Ease::Curve { x1, y1, x2, y2 } = ease {
        ui.horizontal(|ui| {
            for (name, value, range) in
                [("x1", x1, 0.0..=1.0), ("y1", y1, -1.0..=2.0), ("x2", x2, 0.0..=1.0), ("y2", y2, -1.0..=2.0)]
            {
                let label = ui.label(name);
                let drag = ui.add(DragValue::new(value).speed(0.01).range(range).fixed_decimals(2));
                changed |= drag.labelled_by(label.id).changed();
            }
        });
    } else {
        return false;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 80.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, crate::theme::CARD);
    let inner = rect.shrink2(egui::vec2(6.0, 16.0));
    let at = |t: f32, v: f32| Pos2::new(inner.left() + t * inner.width(), inner.bottom() - v * inner.height());
    painter.line_segment([at(0.0, 0.0), at(1.0, 0.0)], Stroke::new(1.0, crate::theme::LINE));
    painter.line_segment([at(0.0, 1.0), at(1.0, 1.0)], Stroke::new(1.0, crate::theme::LINE));
    let points: Vec<Pos2> = (0..=48).map(|i| i as f32 / 48.0).map(|t| at(t, ease.apply(t))).collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.0, crate::theme::ACCENT_TEXT)));
    changed
}
