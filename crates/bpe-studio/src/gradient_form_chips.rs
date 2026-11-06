//! The Shape tab's form chips (gradients part 2): each form painted as the gradient laid out that
//! way, worked out on the CPU (`bpe_core::gradient_form::area_colour`) and kept per gradient,
//! form and layout so a frame redraws none it drew before.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use bpe_core::gradient_form::{Layout, area_colour};
use bpe_core::{Color, Gradient, GradientForm};
use eframe::egui::{self, Color32, Sense, Stroke, Ui};

use crate::style;

/// A chip's preview, in cells.
pub const CELLS: (usize, usize) = (14, 10);
/// One cell's side in points.
const CELL: f32 = 2.0;

/// The colours of `g` laid out as `form` by `layout`, a row at a time from the top left: the
/// middle of each of `w` × `h` cells, the shorter side running -1 to 1.
pub fn preview(g: &Gradient, form: GradientForm, layout: &Layout, (w, h): (usize, usize)) -> Vec<Color> {
    let side = w.min(h).max(1) as f32;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let q = glam::Vec2::new((2.0 * x as f32 + 1.0 - w as f32) / side, (2.0 * y as f32 + 1.0 - h as f32) / side);
            out.push(Color::from_linear(area_colour(g, form, layout, q, 0.0)));
        }
    }
    out
}

/// [`preview`], remembered (a few hundred at most, then forgotten all at once).
fn kept(g: &Gradient, form: GradientForm, layout: &Layout) -> Vec<Color> {
    static KEPT: LazyLock<Mutex<HashMap<String, Vec<Color>>>> = LazyLock::new(Default::default);
    let key = format!("{g:?}{form:?}{layout:?}");
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    if kept.len() > 512 {
        kept.clear();
    }
    kept.entry(key).or_insert_with(|| preview(g, form, layout, CELLS)).clone()
}

/// A colour as the chip shows it.
fn shown(c: Color) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b) // scene colour
}

/// A chip showing `g` as `form`, ringed when `on`, named `name` (its tooltip the form's name).
/// True when clicked.
pub fn chip(ui: &mut Ui, g: &Gradient, form: GradientForm, layout: &Layout, on: bool, name: String) -> bool {
    let size = egui::vec2(CELLS.0 as f32 * CELL + 4.0, CELLS.1 as f32 * CELL + 4.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    ui.ctx().accesskit_node_builder(r.id, |node| node.set_label(name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let inner = rect.shrink(2.0);
        for (i, c) in kept(g, form, layout).into_iter().enumerate() {
            let (x, y) = ((i % CELLS.0) as f32, (i / CELLS.0) as f32);
            let cell = egui::Rect::from_min_size(inner.min + egui::vec2(x * CELL, y * CELL), egui::vec2(CELL, CELL));
            painter.rect_filled(cell, 0.0, shown(c));
        }
        let ring = if on {
            Stroke::new(2.0, style::TEXT_STRONG)
        } else if r.hovered() {
            Stroke::new(1.0, style::MUTED)
        } else {
            Stroke::new(1.0, style::LINE)
        };
        painter.rect_stroke(rect.shrink(1.0), 3.0, ring, egui::StrokeKind::Inside);
    }
    r.on_hover_text(form.label()).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_differ_by_form_and_straight_runs_top_to_bottom() {
        let g = Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
        let layout = Layout::default();
        let straight = preview(&g, GradientForm::Straight, &layout, CELLS);
        assert_eq!(straight.len(), CELLS.0 * CELLS.1);
        let (top, bottom) = (straight[CELLS.0 / 2], straight[CELLS.0 * (CELLS.1 - 1) + CELLS.0 / 2]);
        assert!(top.r > top.b && bottom.b > bottom.r, "{top:?} {bottom:?}");
        for form in GradientForm::ALL.into_iter().filter(|f| *f != GradientForm::Straight) {
            assert_ne!(preview(&g, form, &layout, CELLS), straight, "{form:?} looks like Straight");
        }
    }
}
