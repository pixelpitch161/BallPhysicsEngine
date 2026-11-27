//! The Water stage's Piece tool (Water mode part 3): a palette of ready-made pieces in the strip,
//! with Turn, Flip and Cuts for the next one; a ghost of it follows the pointer, snapping a corner
//! to another solid's, and a click places it (one undo step), picks it and keeps the tool in hand.
//! **Make custom** turns a picked one-outline piece into a Custom piece of that outline.

use bpe_core::{Choice, Settings, SlotId, ThingId, Value};
use bpe_kit::modules::water::pieces::{PALETTE, corners, default_size, outlines, placed};
use bpe_kit::modules::water::{PieceKind, PieceShape};
use bpe_kit::things::water;
use bpe_kit::things::water::draft::WaterSetup;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui, vec2};
use glam::Vec2;

use super::course_view::CourseView;
use crate::app::Studio;
use crate::style;

/// How near (screen points) a corner must come to another solid's corner to snap to it.
pub const SNAP_REACH: f32 = 10.0;
/// A palette button's side.
const SWATCH: f32 = 26.0;

/// The next piece to place: the editor's own (not saved with the scene).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceBrush {
    pub kind: PieceKind,
    /// Its turn, degrees clockwise.
    pub turn_deg: f32,
    pub flip: bool,
    pub cuts: bool,
}

impl Default for PieceBrush {
    fn default() -> Self {
        PieceBrush { kind: PieceKind::Ramp, turn_deg: 0.0, flip: false, cuts: false }
    }
}

impl PieceBrush {
    /// The Shape card the next piece gets.
    pub fn shape(&self) -> PieceShape {
        PieceShape {
            piece: self.kind,
            size: default_size(self.kind),
            flip: self.flip,
            cuts: self.cuts,
            ..PieceShape::default()
        }
    }

    /// Its turn, radians clockwise.
    pub fn turn(&self) -> f32 {
        if self.turn_deg.is_finite() { self.turn_deg.to_radians() } else { 0.0 }
    }
}

/// A piece kind's name on the palette.
pub fn label(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::Ramp => "Ramp",
        PieceKind::Step => "Step",
        PieceKind::Pillar => "Pillar",
        PieceKind::Basin => "Basin",
        PieceKind::Funnel => "Funnel",
        PieceKind::Pipe => "Pipe",
        PieceKind::Sieve => "Sieve",
        PieceKind::Bucket => "Bucket",
        PieceKind::Wheel => "Wheel",
        PieceKind::Gate => "Gate",
        PieceKind::Float => "Float",
        PieceKind::Custom => "Custom",
    }
}

/// The strip while Piece is in hand: the palette, Turn, Flip and Cuts.
pub fn strip(ui: &mut Ui, brush: &mut PieceBrush) {
    for kind in PALETTE {
        let on = brush.kind == kind;
        let (rect, response) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, label(kind)));
        let fill = if on {
            style::TEXT
        } else if response.hovered() {
            style::HOVER
        } else {
            style::RAISED
        };
        ui.painter().rect_filled(rect, 3.0, fill);
        let ink = if on { style::ON_LIGHT } else { style::TEXT };
        swatch(ui.painter(), rect.shrink(4.0), kind, ink);
        if response.on_hover_text(label(kind)).clicked() {
            brush.kind = kind;
        }
    }
    ui.add_space(8.0);
    let label = ui.label(RichText::new("Turn").color(style::MUTED));
    ui.add(egui::DragValue::new(&mut brush.turn_deg).range(-360.0..=360.0).speed(1.0).suffix("°"))
        .labelled_by(label.id);
    ui.toggle_value(&mut brush.flip, "Flip").on_hover_text("Mirror the piece left to right");
    ui.toggle_value(&mut brush.cuts, "Cuts")
        .on_hover_text("Cut the piece's shape out of the course instead of adding it");
}

/// A piece kind's own outline, fitted into `rect`.
fn swatch(painter: &Painter, rect: Rect, kind: PieceKind, ink: Color32) {
    let shape = PieceShape { piece: kind, size: default_size(kind), ..PieceShape::default() };
    let size = shape.size.max(Vec2::ONE);
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let at = |p: Vec2| rect.center() + vec2(p.x, p.y) * scale;
    for line in outlines(&shape.parts()) {
        let points: Vec<Pos2> = line.iter().map(|p| at(*p)).collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.2, ink)));
    }
}

/// Where a piece of `shape` turned `turn` lands when asked for at world point `at`: moved so the
/// corner of its hull nearest another solid's corner (within `reach` world pixels) meets it, or
/// `None` when no corner is that near.
pub fn snap_piece(
    setup: &WaterSetup,
    except: Option<ThingId>,
    shape: &PieceShape,
    at: Vec2,
    turn: f32,
    reach: f32,
) -> Option<Vec2> {
    let hull: Vec<_> = shape.hull().iter().map(|s| placed(s, at, turn)).collect();
    nearest_pair(&corners(&hull), &setup.snap_points(except), reach).map(|(mine, theirs)| at + (theirs - mine))
}

/// The nearest of `mine` to any of `theirs`, within `reach`: `(mine, theirs)`.
pub fn nearest_pair(mine: &[Vec2], theirs: &[Vec2], reach: f32) -> Option<(Vec2, Vec2)> {
    let mut best: Option<(f32, Vec2, Vec2)> = None;
    for m in mine {
        for t in theirs {
            let d = m.distance(*t);
            if d <= reach && best.is_none_or(|(b, ..)| d < b) {
                best = Some((d, *m, *t));
            }
        }
    }
    best.map(|(_, m, t)| (m, t))
}

/// The world point a piece of `shape` lands on for the pointer at `at`: snapped to another solid's
/// corner, else to the grid (with snapping on and Alt not held).
pub fn landing(
    studio: &Studio,
    view: &CourseView,
    shape: &PieceShape,
    turn: f32,
    at: Vec2,
    alt: bool,
    except: Option<ThingId>,
) -> Vec2 {
    let step = studio.grid.step(alt);
    if step.is_some()
        && let Some(setup) = studio.water_draft.setup.as_deref()
        && let Some(snapped) = snap_piece(setup, except, shape, at, turn, SNAP_REACH / view.scale())
    {
        return snapped;
    }
    crate::grid::snap_point(at, step)
}

/// Places the brush's piece where the stage is clicked, picks it and keeps the tool.
pub fn place(ui: &Ui, response: &egui::Response, studio: &mut Studio, view: &CourseView) {
    if !response.clicked() {
        return;
    }
    let Some(at) = response.interact_pointer_pos().filter(|p| view.holds(*p)) else { return };
    let alt = ui.input(|i| i.modifiers.alt);
    let brush = studio.water_stage.piece;
    let shape = brush.shape();
    let to = landing(studio, view, &shape, brush.turn(), view.to_world(at), alt, None);
    match bpe_kit::add_thing(&mut studio.scene, &studio.kit.registry, water::PIECE, ThingId::ROOT) {
        Ok(id) => {
            if let Some(thing) = studio.scene.things.get_mut(&id) {
                thing.transform.position = to;
                thing.transform.rotation = brush.turn();
                thing.name = label(brush.kind).into();
                if let Some(card) = thing.slots.get_mut(&SlotId::Form) {
                    card.params = shape.to_params();
                }
            }
            studio.selection.set(id);
            studio.separate_edit = true;
        }
        Err(e) => studio.status = format!("Couldn't add it: {e}"),
    }
}

/// The brush's piece as a faint outline where a click would put it.
pub fn ghost(ui: &Ui, studio: &Studio, view: &CourseView, at: Pos2) {
    let alt = ui.input(|i| i.modifiers.alt);
    let brush = studio.water_stage.piece;
    let shape = brush.shape();
    let to = landing(studio, view, &shape, brush.turn(), view.to_world(at), alt, None);
    let solids: Vec<_> = shape.parts().iter().map(|s| placed(s, to, brush.turn())).collect();
    let painter = ui.painter_at(view.rect);
    draw_outlines(&painter, view, &outlines(&solids), style::MUTED);
}

/// `lines` (world pixels) drawn on the stage, with a dark edge so they show over any water.
pub fn draw_outlines(painter: &Painter, view: &CourseView, lines: &[Vec<Vec2>], ink: Color32) {
    for line in lines {
        let points: Vec<Pos2> = line.iter().map(|p| view.to_screen(*p)).collect();
        painter.add(egui::Shape::line(points.clone(), Stroke::new(2.5, style::OUTLINE)));
        painter.add(egui::Shape::line(points, Stroke::new(1.0, ink)));
    }
}

/// Whether picked thing `id` is a ready-made piece of one outline (Make custom can mould it).
pub fn can_make_custom(studio: &Studio, id: ThingId) -> bool {
    shape_of(studio, id).is_some_and(|s| s.piece != PieceKind::Custom && s.outline().is_some())
}

/// Piece `id`'s Shape card as resolved.
pub fn shape_of(studio: &Studio, id: ThingId) -> Option<PieceShape> {
    let thing = studio.scene.things.get(&id).filter(|t| t.kind == water::PIECE)?;
    let params = studio.resolved.things.get(&id)?.slots.get(&SlotId::Form)?.params.clone();
    let _ = thing;
    Some(PieceShape::from_params(&params))
}

/// Turns piece `id` into a Custom piece of its own outline (one undo step). Whether it did.
pub fn make_custom(studio: &mut Studio, id: ThingId) -> bool {
    if !can_make_custom(studio, id) {
        return false;
    }
    let Some(outline) = shape_of(studio, id).and_then(|s| s.outline()) else { return false };
    let Some(card) = studio.scene.things.get_mut(&id).and_then(|t| t.slots.get_mut(&SlotId::Form)) else {
        return false;
    };
    card.params.set("piece", Value::Choice(PieceKind::Custom.name().into()));
    card.params.set("path", Value::Path(outline));
    studio.separate_edit = true;
    true
}
