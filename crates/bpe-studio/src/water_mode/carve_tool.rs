//! The Water stage's Carve tool (Water mode part 2b): a brush that digs the course out of its
//! rock, or with Fill puts rock back. Each stroke goes on the Course's Carving card as one undo
//! step when it is let go; while it is painted, the stage redraws the rock from the stroke so far
//! over the water still playing, and the water starts over once it is let go.

use bpe_core::{SlotId, Stroke, Strokes, Value};
use bpe_kit::modules::water::{CarvePaint, mask_for};
use bpe_kit::things::water;
use eframe::egui::{self, Pos2, RichText, Stroke as Line, Ui, vec2};
use glam::Vec2;

use super::course_view::CourseView;
use crate::app::Studio;
use crate::style;

/// The brush, the editor's own (not saved with the scene).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarveBrush {
    /// Puts rock back instead of digging it away.
    pub fill: bool,
    /// The brush's width, world pixels.
    pub size: f32,
    /// 1: a crisp edge where strokes meet; lower rounds it off.
    pub hardness: f32,
    pub square: bool,
    /// Draws a straight line from where the stroke started (holding Shift does too).
    pub straight: bool,
    /// The brush trails the pointer on a string, for smooth curves.
    pub smooth: bool,
}

impl Default for CarveBrush {
    fn default() -> Self {
        CarveBrush { fill: false, size: 120.0, hardness: 1.0, square: false, straight: false, smooth: false }
    }
}

/// The brush's narrowest and widest.
pub const SIZES: (f32, f32) = (8.0, 600.0);

impl CarveBrush {
    pub fn radius(&self) -> f32 {
        self.size.clamp(SIZES.0, SIZES.1) / 2.0
    }

    /// A stroke through `points` (world pixels) with this brush; `alt` does the other of Dig and
    /// Fill.
    pub fn stroke(&self, points: Vec<[f32; 2]>, alt: bool) -> Stroke {
        Stroke {
            points,
            size: self.radius(),
            softness: 1.0 - self.hardness.clamp(0.0, 1.0),
            strength: 1.0,
            erase: self.fill != alt,
            density: 1.0,
            square: self.square,
        }
    }

    /// How far the pointer goes before the stroke gets another point.
    fn spacing(&self) -> f32 {
        (self.radius() / 4.0).max(2.0)
    }
}

/// A stroke being painted.
pub struct Painting {
    /// The Course's strokes before it.
    before: Strokes,
    paint: CarvePaint,
    /// Where it started, and where a smooth brush is.
    press: Vec2,
    lazy: Vec2,
    alt: bool,
}

/// The strip's brush settings while Carve is in hand: Dig or Fill, Brush size, Hardness, Round or
/// Square, Straight and Smooth.
pub fn strip(ui: &mut Ui, brush: &mut CarveBrush) {
    let pair = |ui: &mut Ui, on: &mut bool, off_name: &str, on_name: &str| {
        if ui.selectable_label(!*on, off_name).clicked() {
            *on = false;
        }
        if ui.selectable_label(*on, on_name).clicked() {
            *on = true;
        }
    };
    pair(ui, &mut brush.fill, "Dig", "Fill");
    ui.add_space(8.0);
    let label = ui.label(RichText::new("Brush size").color(style::MUTED));
    ui.add(egui::DragValue::new(&mut brush.size).range(SIZES.0..=SIZES.1).speed(1.0).suffix(" px"))
        .labelled_by(label.id);
    let label = ui.label(RichText::new("Hardness").color(style::MUTED));
    let mut percent = brush.hardness * 100.0;
    let changed = ui
        .add(egui::DragValue::new(&mut percent).range(0.0..=100.0).speed(1.0).suffix(" %"))
        .labelled_by(label.id)
        .changed();
    if changed {
        brush.hardness = (percent / 100.0).clamp(0.0, 1.0);
    }
    ui.add_space(8.0);
    pair(ui, &mut brush.square, "Round", "Square");
    ui.add_space(8.0);
    ui.toggle_value(&mut brush.straight, "Straight")
        .on_hover_text("A straight line from where you start (or hold Shift)");
    ui.toggle_value(&mut brush.smooth, "Smooth").on_hover_text("The brush trails the pointer for smooth curves");
}

/// Paints with the Carve tool from `response` on the course `view`. Escape drops the stroke.
pub fn paint(ui: &Ui, response: &egui::Response, studio: &mut Studio, view: &CourseView) {
    let (alt, shift, escape) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift, i.key_pressed(egui::Key::Escape)));
    let brush = studio.water_stage.brush;
    let size = view.size;
    if escape && studio.water_stage.painting.take().is_some() {
        show_field(studio, None);
        return;
    }
    if response.drag_started()
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
        && view.holds(press)
    {
        let at = view.to_world(press);
        let before = water::carving_of(&studio.scene);
        let paint = CarvePaint::start(mask_for(size, &before), brush.stroke(vec![at.to_array()], alt));
        studio.water_stage.painting = Some(Painting { before, paint, press: at, lazy: at, alt });
    }
    if response.dragged()
        && let Some(at) = response.interact_pointer_pos()
        && let Some(painting) = studio.water_stage.painting.as_mut()
    {
        let w = view.to_world(at);
        let last = painting.paint.stroke().points.last().map_or(painting.press, |p| Vec2::from(*p));
        if brush.straight || shift {
            if w.distance(last) > 0.5 {
                let stroke = brush.stroke(vec![painting.press.to_array(), w.to_array()], painting.alt);
                painting.paint.reset(stroke);
            }
        } else {
            let to = if brush.smooth {
                let string = brush.radius() / 2.0;
                let d = w - painting.lazy;
                if d.length() > string {
                    painting.lazy += d * (1.0 - string / d.length());
                }
                painting.lazy
            } else {
                w
            };
            if to.distance(last) >= brush.spacing() {
                painting.paint.push(to.to_array());
            }
        }
        let mask = painting.paint.mask().clone();
        show_field(studio, Some(&mask));
    }
    let let_go =
        response.drag_stopped() || (studio.water_stage.painting.is_some() && !ui.input(|i| i.pointer.any_down()));
    if let_go && let Some(painting) = studio.water_stage.painting.take() {
        let mut strokes = painting.before;
        strokes.list.push(painting.paint.stroke().clone());
        carve(studio, strokes);
    } else if response.clicked()
        && let Some(at) = response.interact_pointer_pos()
        && view.holds(at)
    {
        // A click digs (or fills) one dab.
        let mut strokes = water::carving_of(&studio.scene);
        strokes.list.push(brush.stroke(vec![view.to_world(at).to_array()], alt));
        carve(studio, strokes);
    }
}

/// Puts `strokes` on the Course's Carving card (adding the card to a Course from before it) as
/// its own undo step.
fn carve(studio: &mut Studio, strokes: Strokes) {
    let Some(course) = water::course_of(&studio.scene) else { return };
    let has_card = studio.scene.things.get(&course).is_some_and(|t| t.slots.contains_key(&SlotId::Form));
    if !has_card && bpe_kit::fill_slot(&mut studio.scene, &studio.kit.registry, course, SlotId::Form).is_err() {
        studio.status = "Couldn't carve: the Course has no Carving card.".into();
        return;
    }
    if let Some(card) = studio.scene.things.get_mut(&course).and_then(|t| t.slots.get_mut(&SlotId::Form)) {
        card.params.set("carved", Value::Strokes(strokes));
        studio.separate_edit = true;
    }
}

/// Shows `mask`'s rock under the water while a stroke is painted (`None`: the body's own again).
fn show_field(studio: &mut Studio, mask: Option<&bpe_kit::modules::water::CarveMask>) {
    let Some(job) = studio.water_draft.job.as_ref() else { return };
    let Ok(mut live) = job.live().try_lock() else { return };
    let Some(live) = live.as_mut() else { return };
    match mask {
        Some(mask) if mask.size == studio.water_draft.setup.as_ref().map_or(mask.size, |s| s.size) => {
            let field = mask.field(live.field.layout.cell);
            if field.layout == live.field.layout {
                live.plain.set_field(&field);
            }
        }
        _ => live.plain.set_field(&live.field),
    }
    studio.water_stage.field_version += 1;
}

/// The brush's outline under the pointer.
pub fn outline(ui: &Ui, studio: &Studio, view: &CourseView, at: Pos2) {
    let brush = studio.water_stage.brush;
    let r = brush.radius() * view.scale();
    let painter = ui.painter_at(view.rect.expand(r));
    let ink = if brush.fill { style::TEXT } else { style::TEXT_STRONG };
    let lines = [Line::new(2.5, style::OUTLINE), Line::new(1.0, ink)];
    for line in lines {
        if brush.square {
            let rect = egui::Rect::from_center_size(at, vec2(2.0 * r, 2.0 * r));
            painter.rect_stroke(rect, 0.0, line, egui::StrokeKind::Middle);
        } else {
            painter.circle_stroke(at, r, line);
        }
    }
}

/// Drops the stroke being painted, if any (another tool was picked).
pub fn drop_stroke(studio: &mut Studio) {
    if studio.water_stage.painting.take().is_some() {
        show_field(studio, None);
    }
}

/// The stroke being painted, for tests.
#[cfg(test)]
pub fn stroke_so_far(studio: &Studio) -> Option<Stroke> {
    studio.water_stage.painting.as_ref().map(|p| p.paint.stroke().clone())
}
