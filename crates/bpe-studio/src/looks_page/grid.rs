//! The Looks tab's grid: Find a look, then a tile for every look shown, its picture and its name.

use bpe_core::{Scene, ThingId};
use bpe_kit::Kit;
use bpe_kit::ready_looks::{self, ReadyLook, Wears};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use eframe::egui_wgpu;

use super::LooksPage;
use crate::style;

/// A tile's picture, in points.
pub const PICTURE: f32 = 84.0;
/// A tile's name row under its picture.
const NAME_ROW: f32 = 18.0;

/// The looks the page shows for `w`: the family picked, filtered by the words typed.
pub fn shown(page: &LooksPage, w: Wears) -> Vec<&'static ReadyLook> {
    ready_looks::all()
        .iter()
        .filter(|l| l.wears == w && page.family.is_none_or(|f| f == l.family) && l.matches(page.find.trim()))
        .collect()
}

/// Draws the grid; returns the look under the pointer. A click on a tile picks it.
pub fn show(
    ui: &mut Ui,
    page: &mut LooksPage,
    render: Option<&egui_wgpu::RenderState>,
    scene: &Scene,
    kit: &Kit,
    id: ThingId,
    w: Wears,
) -> Option<&'static ReadyLook> {
    ui.horizontal(|ui| {
        let label = ui.label(RichText::new("Find a look").small().color(style::MUTED));
        let edit =
            egui::TextEdit::singleline(&mut page.find).desired_width(f32::INFINITY).hint_text("chrome, neon, fire…");
        ui.add(edit).labelled_by(label.id);
    });
    ui.add_space(4.0);
    let looks = shown(page, w);
    if looks.is_empty() {
        ui.label(RichText::new("No look has those words.").small().color(style::MUTED));
        return None;
    }
    let picked = super::picked(page, w).id;
    let mut hovered = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for look in looks {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(PICTURE, PICTURE + NAME_ROW), Sense::click());
            let on = look.id == picked;
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, look.name));
            let response = response.on_hover_text(look.help);
            if response.hovered() {
                hovered = Some(look);
            }
            if response.clicked() {
                page.picked.insert(w, look.id);
                page.done = None;
            }
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let picture = Rect::from_min_size(rect.min, Vec2::splat(PICTURE));
            let colours = super::colours_for(page, scene, id, look);
            let texture = page.thumbs.tile(ui, render, scene, kit, id, look, colours);
            paint_tile(ui, picture, look, texture);
            let ring = if on {
                Some(Stroke::new(1.5, style::TEXT))
            } else if response.hovered() {
                Some(Stroke::new(1.0, style::MUTED))
            } else {
                None
            };
            if let Some(ring) = ring {
                ui.painter().rect_stroke(picture, style::RADIUS, ring, egui::StrokeKind::Inside);
            }
            let at = Pos2::new(rect.center().x, picture.bottom() + NAME_ROW * 0.5);
            let ink = if on { style::TEXT_STRONG } else { style::TEXT };
            ui.painter().text(at, egui::Align2::CENTER_CENTER, look.name, egui::FontId::proportional(12.0), ink);
        }
    });
    hovered
}

/// A tile's picture: the rendered look, or its two colours painted when there is no picture yet.
pub fn paint_tile(ui: &Ui, rect: Rect, look: &ReadyLook, texture: Option<egui::TextureId>) {
    let painter = ui.painter();
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    if let Some(texture) = texture {
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(texture, rect.shrink(1.0), uv, style::UNTINTED);
        return;
    }
    swatch(painter, rect, look);
}

/// The look's two swatch colours as a small picture: a disc ringed in the second for a ball look, a rounded slab
/// for a shape look.
pub fn swatch(painter: &egui::Painter, rect: Rect, look: &ReadyLook) {
    let [a, b] = look.swatch.map(|c| Color32::from_rgb(c.r, c.g, c.b)); // scene colour
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.3;
    match look.wears {
        Wears::Balls => {
            painter.circle_filled(c, r, a);
            painter.circle_stroke(c, r, Stroke::new((r * 0.18).max(1.0), b));
        }
        Wears::Shape => {
            let slab = Rect::from_center_size(c, Vec2::new(r * 2.6, r * 1.6));
            painter.rect_filled(slab, r * 0.3, a);
            painter.rect_stroke(slab, r * 0.3, Stroke::new((r * 0.15).max(1.0), b), egui::StrokeKind::Inside);
        }
    }
}
