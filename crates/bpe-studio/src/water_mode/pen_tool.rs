//! The Water stage's Pen tool (Water mode part 3): a wall drawn point by point. Each click adds a
//! point (snapping to another solid's corner, else the grid), a line runs from the last one to the
//! pointer, and a double-click or Enter finishes the wall (two points at least) as one undo step;
//! Escape, or another tool, drops it. The strip sets the next wall's Thickness, Curved, Ends and
//! Cuts.

use bpe_core::{Settings, SlotId, ThingId};
use bpe_kit::modules::water::{LineEnds, WallLine};
use bpe_kit::things::water;
use eframe::egui::{self, Pos2, RichText, Stroke, Ui};
use glam::Vec2;

use super::course_view::CourseView;
use super::piece_tool::{SNAP_REACH, nearest_pair};
use crate::app::Studio;
use crate::style;

/// The next wall: the editor's own (not saved with the scene).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenBrush {
    pub thickness: f32,
    pub curved: bool,
    pub square: bool,
    pub cuts: bool,
}

impl Default for PenBrush {
    fn default() -> Self {
        PenBrush { thickness: 24.0, curved: false, square: false, cuts: false }
    }
}

/// The strip while Pen is in hand.
pub fn strip(ui: &mut Ui, brush: &mut PenBrush) {
    let label = ui.label(RichText::new("Thickness").color(style::MUTED));
    ui.add(egui::DragValue::new(&mut brush.thickness).range(1.0..=200.0).speed(0.5).suffix(" px"))
        .labelled_by(label.id);
    ui.toggle_value(&mut brush.curved, "Curved").on_hover_text("A smooth curve through the points");
    ui.add_space(8.0);
    if ui.selectable_label(!brush.square, "Round").clicked() {
        brush.square = false;
    }
    if ui.selectable_label(brush.square, "Square").clicked() {
        brush.square = true;
    }
    ui.add_space(8.0);
    ui.toggle_value(&mut brush.cuts, "Cuts").on_hover_text("Cut the wall out of the course instead: a channel");
}

/// Where a pen point lands for the pointer at world point `at`: on another solid's corner (or the
/// line's own first point) within reach, else on the grid (with its snapping on). Alt held turns
/// both off.
pub fn landing(studio: &Studio, view: &CourseView, at: Vec2, alt: bool) -> Vec2 {
    if !alt {
        let mut theirs = studio.water_draft.setup.as_deref().map(|s| s.snap_points(None)).unwrap_or_default();
        theirs.extend(studio.water_stage.line.as_ref().and_then(|l| l.first()).copied());
        if let Some((_, to)) = nearest_pair(&[at], &theirs, SNAP_REACH / view.scale()) {
            return to;
        }
    }
    crate::grid::snap_point(at, studio.grid.step(alt))
}

/// Draws with the Pen from `response` on the course `view`.
pub fn draw(ui: &Ui, response: &egui::Response, studio: &mut Studio, view: &CourseView) {
    // (Enter and Escape typed into a text field, such as the strip's Thickness, are the field's.)
    let typing = ui.ctx().text_edit_focused();
    let (alt, enter, escape) = ui.input(|i| {
        let key = |k| !typing && i.key_pressed(k);
        (i.modifiers.alt, key(egui::Key::Enter), key(egui::Key::Escape))
    });
    if escape {
        drop_line(studio);
        return;
    }
    if response.double_clicked() || enter {
        finish(studio);
        return;
    }
    if response.clicked()
        && let Some(at) = response.interact_pointer_pos().filter(|p| view.holds(*p))
    {
        let to = landing(studio, view, view.to_world(at), alt);
        let line = studio.water_stage.line.get_or_insert_with(Vec::new);
        if line.last().is_none_or(|last| last.distance(to) > 1e-3) {
            line.push(to);
        }
    }
}

/// Makes the line drawn so far a Wall (with two points at least), picks it and keeps the tool.
pub fn finish(studio: &mut Studio) -> Option<ThingId> {
    let points = studio.water_stage.line.take()?;
    if points.len() < 2 {
        return None;
    }
    let (lo, hi) = points.iter().fold((points[0], points[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let place = (lo + hi) / 2.0;
    let brush = studio.water_stage.pen;
    let line = WallLine {
        points: points.iter().map(|p| (*p - place).to_array()).collect(),
        thickness: brush.thickness.clamp(1.0, 1e9),
        curved: brush.curved,
        ends: if brush.square { LineEnds::Square } else { LineEnds::Round },
        cuts: brush.cuts,
    };
    match bpe_kit::add_thing(&mut studio.scene, &studio.kit.registry, water::WALL, ThingId::ROOT) {
        Ok(id) => {
            if let Some(thing) = studio.scene.things.get_mut(&id) {
                thing.transform.position = place;
                if let Some(card) = thing.slots.get_mut(&SlotId::Form) {
                    card.params = line.to_params();
                }
            }
            studio.selection.set(id);
            studio.separate_edit = true;
            Some(id)
        }
        Err(e) => {
            studio.status = format!("Couldn't add it: {e}");
            None
        }
    }
}

/// Drops the line being drawn, if any.
pub fn drop_line(studio: &mut Studio) {
    studio.water_stage.line = None;
}

/// The line drawn so far, with a line on to where the next point would land.
pub fn show_line(ui: &Ui, studio: &Studio, view: &CourseView, hover: Option<Pos2>) {
    let Some(line) = studio.water_stage.line.as_ref() else { return };
    let painter = ui.painter_at(view.clip(0.0));
    let mut points: Vec<Pos2> = line.iter().map(|p| view.to_screen(*p)).collect();
    if let Some(at) = hover.filter(|p| view.holds(*p)) {
        let alt = ui.input(|i| i.modifiers.alt);
        points.push(view.to_screen(landing(studio, view, view.to_world(at), alt)));
    }
    let width = (studio.water_stage.pen.thickness * view.scale()).max(1.0);
    painter.add(egui::Shape::line(points.clone(), Stroke::new(width, style::FAINT_MARK)));
    painter.add(egui::Shape::line(points, Stroke::new(1.0, style::TEXT_STRONG)));
    for p in line {
        painter.circle_filled(view.to_screen(*p), 3.0, style::TEXT_STRONG);
    }
}
