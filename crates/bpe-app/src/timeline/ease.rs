//! Picking how a keyframe eases into the next one: small pictures of each ready-made curve, and
//! a curve of your own with two handles to drag, like the graph editors of video editors.

use crate::theme;
use bpe_scene::{Bezier, Ease};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Ui};

/// Size of the curve pictures in menus.
const ICON: egui::Vec2 = egui::vec2(26.0, 18.0);
/// Size of the curve editor.
const EDITOR: egui::Vec2 = egui::vec2(240.0, 200.0);
/// The editor shows values from this far below 0 to this far above 1, for overshoots.
const MARGIN: f32 = 0.45;

/// Points along an ease from (0, 0) to (1, 1), mapped into `rect` by `to_screen`.
fn points(ease: Ease, steps: usize, to_screen: impl Fn(f32, f32) -> Pos2) -> Vec<Pos2> {
    (0..=steps)
        .map(|i| {
            let x = i as f32 / steps as f32;
            // Hold stays put until the very end, then jumps.
            let y = if ease == Ease::Hold && i == steps { 1.0 } else { ease.apply(x) };
            to_screen(x, y)
        })
        .collect()
}

/// A small picture of an ease's curve.
pub fn icon(ui: &mut Ui, ease: Ease, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(ICON, Sense::hover());
    let area = rect.shrink2(egui::vec2(2.0, 4.0));
    let to_screen = |x: f32, y: f32| Pos2::new(area.left() + x * area.width(), area.bottom() - y * area.height());
    ui.painter_at(rect.expand(2.0)).add(egui::Shape::line(points(ease, 24, to_screen), Stroke::new(1.4, color)));
}

/// One ready-made ease in a menu: its picture and name. True when picked.
pub fn choice(ui: &mut Ui, ease: Ease, current: Ease) -> bool {
    let picked = std::mem::discriminant(&ease) == std::mem::discriminant(&current);
    let inner = ui.horizontal(|ui| {
        icon(ui, ease, if picked { theme::ACCENT_TEXT } else { theme::MUTED });
        ui.radio(picked, ease.name())
    });
    inner.inner.clicked()
}

/// The curve of your own: drag the two handles. Returns true when the curve changed.
pub fn editor(ui: &mut Ui, curve: &mut Bezier) -> bool {
    let before = *curve;
    let (rect, _) = ui.allocate_exact_size(EDITOR, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, theme::BACKGROUND);
    let area = rect.shrink(10.0);
    let span = 1.0 + 2.0 * MARGIN;
    let to_screen =
        |x: f32, y: f32| Pos2::new(area.left() + x * area.width(), area.bottom() - (y + MARGIN) / span * area.height());
    let from_screen =
        |p: Pos2| ((p.x - area.left()) / area.width(), (area.bottom() - p.y) / area.height() * span - MARGIN);
    // The square the value moves through: from the keyframe (bottom left) to the next (top right).
    let square = Rect::from_two_pos(to_screen(0.0, 0.0), to_screen(1.0, 1.0));
    painter.rect_stroke(square, 0.0, Stroke::new(1.0, theme::LINE), egui::StrokeKind::Middle);
    for t in [0.25, 0.5, 0.75] {
        let faint = Stroke::new(1.0, theme::LINE.gamma_multiply(0.5));
        painter.line_segment([to_screen(t, 0.0), to_screen(t, 1.0)], faint);
        painter.line_segment([to_screen(0.0, t), to_screen(1.0, t)], faint);
    }
    painter.line_segment([to_screen(0.0, 0.0), to_screen(1.0, 1.0)], Stroke::new(1.0, theme::LINE));

    // Handles and their arms.
    let id = ui.make_persistent_id("own curve");
    let mut moved = false;
    for (n, (anchor, handle)) in
        [((0.0, 0.0), (curve.x1, curve.y1)), ((1.0, 1.0), (curve.x2, curve.y2))].into_iter().enumerate()
    {
        let at = to_screen(handle.0, handle.1);
        painter.line_segment([to_screen(anchor.0, anchor.1), at], Stroke::new(1.2, theme::MUTED));
        let response = ui.interact(Rect::from_center_size(at, egui::vec2(16.0, 16.0)), id.with(n), Sense::drag());
        let hot = response.hovered() || response.dragged();
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let (x, y) = from_screen(pointer);
            if n == 0 {
                (curve.x1, curve.y1) = (x, y);
            } else {
                (curve.x2, curve.y2) = (x, y);
            }
            *curve = curve.tidy();
            moved = true;
        }
        let response = response.on_hover_text("Drag to shape the curve");
        let fill = if hot { Color32::WHITE } else { theme::ACCENT_TEXT };
        painter.circle_filled(response.rect.center(), 5.0, fill);
    }
    painter.add(egui::Shape::line(points(Ease::Curve(*curve), 64, to_screen), Stroke::new(2.0, theme::ACCENT_TEXT)));
    let label = |pos: Pos2, text: &str, align| {
        painter.text(pos, align, text, egui::FontId::proportional(11.0), theme::MUTED);
    };
    label(to_screen(0.0, 0.0) + egui::vec2(2.0, -2.0), "this keyframe", egui::Align2::LEFT_BOTTOM);
    label(to_screen(1.0, 1.0) + egui::vec2(-2.0, 2.0), "next one", egui::Align2::RIGHT_TOP);

    // Ready-made shapes to start from, and the four numbers for exact values.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for ease in [Ease::Linear, Ease::Smooth, Ease::EaseIn, Ease::EaseOut, Ease::Overshoot] {
            if ui.small_button(ease.name()).on_hover_text("Start from this shape").clicked() {
                *curve = ease.as_curve();
                moved = true;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let reach = -Bezier::REACH..=1.0 + Bezier::REACH;
        for (value, range, name) in [
            (&mut curve.x1, 0.0..=1.0, "First handle across"),
            (&mut curve.y1, reach.clone(), "First handle up"),
            (&mut curve.x2, 0.0..=1.0, "Second handle across"),
            (&mut curve.y2, reach, "Second handle up"),
        ] {
            let response = ui.add(egui::DragValue::new(value).range(range).speed(0.01).fixed_decimals(2));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, true, name));
            moved |= response.changed();
        }
    })
    .response
    .on_hover_text(
        "The two handles as numbers: first handle across and up, then the second. The same as CSS cubic-bezier.",
    );
    moved && *curve != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_draws_and_handles_stay_usable() {
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(300.0, 300.0)).build_ui(|ui| {
            let mut curve = Bezier::new(0.3, 1.4, 0.6, 1.0);
            editor(ui, &mut curve);
            for ease in Ease::ALL {
                choice(ui, ease, Ease::Curve(curve));
            }
        });
        harness.run();
        // A handle dragged out of the box is pulled back so the curve never runs backwards.
        let tidy = Bezier { x1: 3.0, y1: 9.0, x2: -1.0, y2: f32::NAN }.tidy();
        assert_eq!((tidy.x1, tidy.y1, tidy.x2, tidy.y2), (1.0, 1.0 + Bezier::REACH, 0.0, 1.0));
    }
}
