//! The Select tool on Pieces and Walls (Water mode part 3): a piece or wall is picked by its body
//! as well as its marker and dragged with snapping (a corner to another solid's corner, else the
//! grid); a picked one shows its outline, a picked wall or Custom piece its points (drag one to move
//! it, double-click the line to add one, double-click a point to take it away), and a picked piece
//! a turn dot above it (15° steps, Alt for free) and a size dot at its box's corner.

use bpe_core::{SlotId, ThingId, Value};
use bpe_kit::modules::water::{MovesKind, PieceKind, PieceShape, WallLine};
use bpe_kit::things::water;
use bpe_kit::things::water::draft::{Form, SolidSetup};
use eframe::egui::{self, Pos2, Rect, Stroke, Ui, vec2};
use glam::Vec2;

use super::course_view::CourseView;
use super::piece_tool::{SNAP_REACH, draw_outlines, nearest_pair};
use crate::app::Studio;
use crate::style;

/// A handle's half side (screen points), and how near a press must land to take it.
const HANDLE: f32 = 4.0;
const HANDLE_REACH: f32 = 7.0;
/// A Position key's dot on a moving part's path (its radius, screen points).
const KEY_DOT: f32 = 3.5;
/// How far above a piece's box its turn dot sits (screen points).
const TURN_OFF: f32 = 24.0;
/// How near (screen points) a click must come to a piece's body to pick it.
const BODY_REACH: f32 = 4.0;
/// The turn dot's steps without Alt.
const TURN_STEP: f32 = 15.0;
/// The smallest a size handle makes a piece (pixels a side).
const LEAST_SIZE: f32 = 4.0;

/// A handle being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Grab {
    /// A wall's point, or a Custom piece's outline point.
    Point(ThingId, usize),
    Turn(ThingId),
    Size(ThingId),
}

/// The picked solid's handles on screen.
struct Handles {
    id: ThingId,
    solid: SolidSetup,
    points: Vec<Pos2>,
    turn: Option<(Pos2, Pos2)>,
    size: Option<Pos2>,
}

/// Piece or Wall `id` as the water sees it now (`None` for anything else, or one that is off).
fn solid_of(studio: &Studio, id: ThingId) -> Option<SolidSetup> {
    studio.resolved.things.contains_key(&id).then(|| SolidSetup::of(&studio.scene, &studio.resolved, id))?
}

/// The primary pick's handles, if it is a Piece or a Wall.
fn handles(studio: &Studio, view: &CourseView) -> Option<Handles> {
    let id = studio.selection.primary?;
    let solid = solid_of(studio, id)?;
    let screen = |frame: Vec2| view.to_screen(solid.to_world(frame));
    let (points, turn, size) = match solid.form() {
        Form::Wall(line) => (line.points.iter().map(|p| screen(Vec2::from(*p))).collect(), None, None),
        Form::Piece(shape) => {
            let points = match shape.piece {
                PieceKind::Custom => shape.path.points.iter().map(|p| screen(shape.frame_of(Vec2::from(*p)))).collect(),
                _ => Vec::new(),
            };
            let half = shape.size.abs() / 2.0;
            let top = Vec2::new(0.0, -half.y);
            let dot = Vec2::new(0.0, -half.y - TURN_OFF / view.scale());
            (points, Some((screen(top), screen(dot))), Some(screen(half)))
        }
    };
    Some(Handles { id, solid, points, turn, size })
}

impl Handles {
    fn grab_at(&self, at: Pos2) -> Option<Grab> {
        if let Some(i) = self.point_at(at) {
            return Some(Grab::Point(self.id, i));
        }
        if self.turn.is_some_and(|(_, dot)| dot.distance(at) <= HANDLE_REACH) {
            return Some(Grab::Turn(self.id));
        }
        if self.size.is_some_and(|p| p.distance(at) <= HANDLE_REACH) {
            return Some(Grab::Size(self.id));
        }
        None
    }

    fn point_at(&self, at: Pos2) -> Option<usize> {
        self.points.iter().position(|p| p.distance(at) <= HANDLE_REACH)
    }
}

/// The top-most Piece or Wall whose body is under screen point `at`.
pub fn body_at(studio: &Studio, view: &CourseView, at: Pos2) -> Option<ThingId> {
    studio.water_draft.setup.as_deref()?.solid_at(view.to_world(at), BODY_REACH / view.scale())
}

/// Where solid `id` lands when dragged to `want`: moved so its corner nearest another solid's
/// corner meets it (within reach), else on the grid with its snapping on (a piece's place, a wall's
/// first point, so a wall drawn on the grid stays on it). Alt held turns both off.
fn landing(studio: &Studio, view: &CourseView, id: ThingId, want: Vec2, alt: bool) -> Vec2 {
    let Some(solid) = solid_of(studio, id) else { return want };
    let moved = |p: Vec2| p - solid.at + want;
    if !alt && let Some(setup) = studio.water_draft.setup.as_deref() {
        let mine: Vec<Vec2> = solid.corners().into_iter().map(moved).collect();
        if let Some((m, t)) = nearest_pair(&mine, &setup.snap_points(Some(id)), SNAP_REACH / view.scale()) {
            return want + (t - m);
        }
    }
    let step = studio.grid.step(alt);
    match (solid.kind, solid.corners().first()) {
        (water::WALL, Some(first)) => {
            let first = moved(*first);
            want + (crate::grid::snap_point(first, step) - first)
        }
        _ => crate::grid::snap_point(want, step),
    }
}

/// Where a dragged point lands: on another solid's corner within reach, else on the grid (with its
/// snapping on). Alt held turns both off.
fn point_landing(studio: &Studio, view: &CourseView, id: ThingId, want: Vec2, alt: bool) -> Vec2 {
    if !alt
        && let Some(setup) = studio.water_draft.setup.as_deref()
        && let Some((_, t)) = nearest_pair(&[want], &setup.snap_points(Some(id)), SNAP_REACH / view.scale())
    {
        return t;
    }
    crate::grid::snap_point(want, studio.grid.step(alt))
}

fn set_form(studio: &mut Studio, id: ThingId, key: &str, v: Value) {
    if let Some(card) = studio.scene.things.get_mut(&id).and_then(|t| t.slots.get_mut(&SlotId::Form))
        && card.params.get(key) != Some(&v)
    {
        card.params.set(key, v);
    }
}

fn shape_of(solid: &SolidSetup) -> Option<PieceShape> {
    match solid.form() {
        Form::Piece(shape) => Some(shape),
        Form::Wall(_) => None,
    }
}

fn line_of(solid: &SolidSetup) -> Option<WallLine> {
    match solid.form() {
        Form::Wall(line) => Some(line),
        Form::Piece(_) => None,
    }
}

/// Moves what `grab` holds to world point `at`.
fn apply(studio: &mut Studio, view: &CourseView, grab: Grab, at: Vec2, alt: bool) {
    match grab {
        Grab::Point(id, i) => {
            let Some(solid) = solid_of(studio, id) else { return };
            let to = solid.to_frame(point_landing(studio, view, id, at, alt));
            if let Some(mut line) = line_of(&solid) {
                if let Some(p) = line.points.get_mut(i) {
                    *p = to.to_array();
                    set_form(studio, id, "points", Value::Points(line.points));
                }
            } else if let Some(mut shape) = shape_of(&solid)
                && let Some(q) = shape.shape_of(to)
                && shape.path.move_point(i, q)
            {
                set_form(studio, id, "path", Value::Path(shape.path));
            }
        }
        Grab::Turn(id) => {
            let Some(thing) = studio.scene.things.get(&id) else { return };
            let d = at - thing.transform.position;
            if d.length() < 1e-3 {
                return;
            }
            let mut deg = d.x.atan2(-d.y).to_degrees();
            if !alt {
                deg = (deg / TURN_STEP).round() * TURN_STEP;
            }
            let turn = deg.to_radians();
            if let Some(thing) = studio.scene.things.get_mut(&id)
                && thing.transform.rotation != turn
            {
                thing.transform.rotation = turn;
            }
        }
        Grab::Size(id) => {
            let Some(solid) = solid_of(studio, id) else { return };
            let step = studio.grid.step(alt);
            let half = solid.to_frame(at).abs();
            let size = Vec2::new(
                crate::grid::snap_length(2.0 * half.x, step).max(LEAST_SIZE),
                crate::grid::snap_length(2.0 * half.y, step).max(LEAST_SIZE),
            );
            set_form(studio, id, "size", Value::Vec2(size.to_array()));
        }
    }
}

/// A double-click on the picked solid: on a point, takes it away (a wall keeps two, a Custom piece
/// three); on a wall's line or a Custom piece's outline, adds a point there. Whether it did.
fn double_click(studio: &mut Studio, view: &CourseView, at: Pos2) -> bool {
    let Some(h) = handles(studio, view) else { return false };
    let id = h.id;
    if let Some(i) = h.point_at(at) {
        if let Some(mut line) = line_of(&h.solid) {
            if line.points.len() > 2 {
                line.points.remove(i);
                set_form(studio, id, "points", Value::Points(line.points));
                studio.separate_edit = true;
                return true;
            }
        } else if let Some(mut shape) = shape_of(&h.solid)
            && shape.path.delete_point(i)
        {
            set_form(studio, id, "path", Value::Path(shape.path));
            studio.separate_edit = true;
            return true;
        }
        return false;
    }
    let world = view.to_world(at);
    let local = h.solid.to_frame(world);
    let reach = HANDLE_REACH / view.scale();
    if let Some(mut line) = line_of(&h.solid) {
        let points: Vec<Vec2> = line.points.iter().map(|p| Vec2::from(*p)).collect();
        let best = points
            .windows(2)
            .enumerate()
            .map(|(i, w)| (i, bpe_core::shape_path::nearest_on_segment(w[0], w[1], local)))
            .min_by(|a, b| a.1.2.total_cmp(&b.1.2));
        if let Some((i, (_, p, d))) = best
            && d <= reach
        {
            line.points.insert(i + 1, p.to_array());
            set_form(studio, id, "points", Value::Points(line.points));
            studio.separate_edit = true;
            return true;
        }
    } else if let Some(mut shape) = shape_of(&h.solid)
        && shape.piece == PieceKind::Custom
        && let Some(q) = shape.shape_of(local)
        && let Some((side, t, _)) = shape.path.nearest_side(q)
        && shape.frame_of(shape.path.side_point(side, t)).distance(local) <= reach
        && shape.path.add_point(side, t)
    {
        set_form(studio, id, "path", Value::Path(shape.path));
        studio.separate_edit = true;
        return true;
    }
    false
}

/// The Select tool on the stage: handles of the picked piece or wall first, then markers, then
/// pieces' and walls' bodies.
pub fn select(ui: &Ui, response: &egui::Response, studio: &mut Studio, view: &CourseView, markers: &[(ThingId, Vec2)]) {
    let alt = ui.input(|i| i.modifiers.alt);
    // (Only on the part shown: in the Shot view the course reaches past the shot.)
    if response.drag_started()
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
        && view.holds(press)
    {
        if let Some(grab) = handles(studio, view).and_then(|h| h.grab_at(press)) {
            studio.water_stage.grab = Some(grab);
        } else {
            let marker = super::stage::marker_at(view, markers, press).map(|(id, _)| id);
            if let Some(id) = marker.or_else(|| body_at(studio, view, press))
                && let Some(thing) = studio.scene.things.get(&id)
            {
                let held = thing.transform.position - view.to_world(press);
                studio.selection.set(id);
                studio.water_stage.drag = Some((id, held));
            }
        }
    }
    if response.dragged()
        && let Some(at) = response.interact_pointer_pos()
    {
        let world = view.to_world(at);
        if let Some(grab) = studio.water_stage.grab {
            apply(studio, view, grab, world, alt);
            studio.water_stage.moulding = true;
        } else if let Some((id, held)) = studio.water_stage.drag {
            let solid =
                studio.scene.things.get(&id).is_some_and(|t| [water::PIECE, water::WALL].contains(&t.kind.as_str()));
            let to = if solid {
                landing(studio, view, id, world + held, alt)
            } else {
                crate::grid::snap_point(world + held, studio.grid.step(alt))
            };
            if let Some(thing) = studio.scene.things.get_mut(&id)
                && thing.transform.position != to
            {
                thing.transform.position = to;
            }
            studio.water_stage.moulding = solid;
        }
    }
    if response.double_clicked()
        && let Some(at) = response.interact_pointer_pos()
        && view.holds(at)
        && double_click(studio, view, at)
    {
        return;
    }
    if response.clicked()
        && let Some(at) = response.interact_pointer_pos()
        && view.holds(at)
    {
        let on_handle = handles(studio, view).is_some_and(|h| h.grab_at(at).is_some());
        if !on_handle {
            let marker = super::stage::marker_at(view, markers, at).map(|(id, _)| id);
            match marker.or_else(|| body_at(studio, view, at)) {
                Some(id) => studio.selection.set(id),
                None => studio.selection.clear(),
            }
        }
    }
}

/// Lets go of what Select was dragging.
pub fn let_go(studio: &mut Studio) {
    studio.water_stage.drag = None;
    studio.water_stage.grab = None;
    studio.water_stage.moulding = false;
}

/// The picked piece's or wall's outline and handles (with Select in hand), and every other picked
/// one's outline, inside the stage's `room`.
pub fn paint(ui: &Ui, studio: &Studio, view: &CourseView, room: Rect, with_handles: bool) {
    // (In the Shot view only over the shot: the course reaches past it.)
    let painter = ui.painter_at(if view.shot.is_some() { room.intersect(view.seen_rect()) } else { room });
    for id in studio.selection.all(&studio.scene) {
        let Some(solid) = solid_of(studio, id) else { continue };
        match path_of(studio, id) {
            // (A moving part: its path and its Position keys, its place (where the handles are)
            // faint, and where it is at the playhead.)
            Some((path, keys)) => {
                let points: Vec<Pos2> = path.iter().map(|p| view.to_screen(*p)).collect();
                painter.add(egui::Shape::line(points.clone(), Stroke::new(2.5, style::OUTLINE)));
                painter.add(egui::Shape::line(points, Stroke::new(1.0, style::MUTED)));
                for k in keys {
                    painter.circle(view.to_screen(k), KEY_DOT, style::TEXT_STRONG, Stroke::new(1.0, style::OUTLINE));
                }
                draw_outlines(&painter, view, &solid.outline(), style::FAINT);
                draw_outlines(&painter, view, &solid.outline_at(studio.time), style::TEXT_STRONG);
            }
            None => draw_outlines(&painter, view, &solid.outline(), style::TEXT_STRONG),
        }
    }
    if !with_handles {
        return;
    }
    let Some(h) = handles(studio, view) else { return };
    let square = |at: Pos2| Rect::from_center_size(at, vec2(2.0 * HANDLE, 2.0 * HANDLE));
    for p in &h.points {
        painter.rect_filled(square(*p), 1.0, style::TEXT_STRONG);
        painter.rect_stroke(square(*p), 1.0, Stroke::new(1.0, style::OUTLINE), egui::StrokeKind::Outside);
    }
    if let Some((top, dot)) = h.turn {
        painter.line_segment([top, dot], Stroke::new(1.0, style::TEXT_STRONG));
        painter.circle_filled(dot, HANDLE + 1.0, style::TEXT_STRONG);
        painter.circle_stroke(dot, HANDLE + 1.0, Stroke::new(1.0, style::OUTLINE));
    }
    if let Some(at) = h.size {
        painter.rect_filled(square(at), 0.0, style::PANEL);
        painter.rect_stroke(square(at), 0.0, Stroke::new(1.5, style::TEXT_STRONG), egui::StrokeKind::Inside);
    }
}

/// The water time between two points of a moving part's path (seconds).
const PATH_STEP: f64 = 1.0 / 30.0;

/// Piece or Wall `id`'s path, when it moves (Keyframed, with something to it): where it is every
/// [`PATH_STEP`] of water time over the scene's Length, and where its Position keys put it (world
/// pixels).
pub fn path_of(studio: &Studio, id: ThingId) -> Option<(Vec<Vec2>, Vec<Vec2>)> {
    let solid = solid_of(studio, id)?;
    if solid.motion() != MovesKind::Keyframed || solid.moving_part().is_none() {
        return None;
    }
    let root = studio.resolved.things.get(&ThingId::ROOT);
    let settings = root.map(|r| <bpe_kit::SceneSettings as bpe_core::Settings>::from_params(&r.params));
    let length = f64::from(settings.unwrap_or_default().duration.max(0.0));
    let steps = (length / PATH_STEP).ceil().min(MOST_PATH_STEPS) as usize;
    let path = (0..=steps).map(|i| solid.pose_at((i as f64 * PATH_STEP).min(length)).at).collect();
    Some((path, solid.position_keys()))
}

/// The most steps a path is drawn in (ten minutes' worth).
const MOST_PATH_STEPS: f64 = 18_000.0;

/// The picked handles on screen, for tests: points, turn dot, size dot.
#[cfg(test)]
pub fn handle_points(studio: &Studio, view: &CourseView) -> Option<(Vec<Pos2>, Option<Pos2>, Option<Pos2>)> {
    handles(studio, view).map(|h| (h.points, h.turn.map(|t| t.1), h.size))
}
