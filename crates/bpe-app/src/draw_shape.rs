//! Drawing a custom shape on the preview: click for a corner, press and drag for a curve,
//! click the first point or press Enter to close it, Esc to cancel, Backspace to take back the
//! last point, and hold Shift to keep lines at 15° steps. A double-click finishes an open line,
//! for obstacles.
//!
//! The drawing lives in egui's temporary memory, so the Arena tab can start it and the preview
//! can run it without either knowing about the other.

use crate::inspector::shapes::add_shape;
use crate::theme;
use bpe_scene::shapes::{Path, Segment};
use bpe_scene::{Color, Container, Obstacle, Scene, Shape, Vec2};
use eframe::egui::{self, Color32, CursorIcon, Key, Pos2, Response, Stroke, Ui};

/// Screen points within which a click on the first point closes the shape.
const CLOSE_REACH: f32 = 10.0;
/// Shorter drags than this, in screen points, count as clicks.
const DRAG_MIN: f32 = 4.0;

/// What a finished drawing becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawTarget {
    /// The arena's outline, replacing the arena's shape.
    Arena,
    /// A new shape obstacle.
    Obstacle,
}

/// One point of the drawing and its curve handle: the direction the line leaves it, as an
/// offset in world pixels. No handle makes a sharp corner.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Point {
    at: Vec2,
    handle: Option<Vec2>,
}

#[derive(Clone, Debug, PartialEq)]
struct Drawing {
    target: DrawTarget,
    points: Vec<Point>,
    /// Where the pointer went down for the point being placed, in world pixels.
    pressed: Option<Vec2>,
}

fn id() -> egui::Id {
    egui::Id::unique("drawing a shape")
}

/// Starts drawing on the preview.
pub fn start(ctx: &egui::Context, target: DrawTarget) {
    ctx.data_mut(|d| d.insert_temp(id(), Drawing { target, points: Vec::new(), pressed: None }));
}

/// Stops drawing without making anything.
pub fn cancel(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Drawing>(id()));
}

pub fn active(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<Drawing>(id()).is_some())
}

/// Turns `to` so the line from `from` points at a whole number of 15° steps.
fn snap(from: Vec2, to: Vec2) -> Vec2 {
    let d = to - from;
    let step = 15f32.to_radians();
    let angle = (d.y.atan2(d.x) / step).round() * step;
    from + Vec2::from_angle(angle) * d.length()
}

/// Runs the drawing for one frame of the preview: handles the pointer and keys, draws the shape
/// so far, and edits the scene when it is finished. `to_world` and `to_screen` convert between
/// screen points and world pixels.
pub fn interact(
    ui: &Ui,
    response: &Response,
    scene: &mut Scene,
    to_world: impl Fn(Pos2) -> Vec2,
    to_screen: impl Fn(Vec2) -> Pos2,
) {
    let Some(mut drawing) = ui.ctx().data(|d| d.get_temp::<Drawing>(id())) else { return };
    let (escape, enter, back, shift) = ui.input(|i| {
        (i.key_pressed(Key::Escape), i.key_pressed(Key::Enter), i.key_pressed(Key::Backspace), i.modifiers.shift)
    });
    if escape {
        cancel(ui.ctx());
        return;
    }
    if back {
        drawing.points.pop();
    }
    let hover = response.hover_pos().or(response.interact_pointer_pos());
    let mut target = hover.map(&to_world);
    if shift && let (Some(last), Some(t)) = (drawing.points.last(), target) {
        target = Some(snap(last.at, t));
    }
    let first = drawing.points.first().map(|f| to_screen(f.at));
    let near_first = |p: Pos2| first.is_some_and(|f| f.distance(p) <= CLOSE_REACH);

    let mut finish: Option<bool> = None;
    if enter && drawing.points.len() >= 3 {
        finish = Some(true);
    }
    if response.double_clicked() && drawing.target == DrawTarget::Obstacle && drawing.points.len() >= 2 {
        // The double-click's first click already placed a point there.
        finish = Some(false);
    } else if let Some(pointer) = response.interact_pointer_pos() {
        if response.drag_started() || (response.clicked() && drawing.pressed.is_none()) {
            if drawing.points.len() >= 3 && near_first(pointer) {
                finish = Some(true);
            } else {
                drawing.pressed = target;
            }
        }
        // Dragging pulls a curve handle out of the point being placed.
        if let Some(at) = drawing.pressed
            && (response.drag_stopped() || response.clicked())
        {
            let dragged = to_screen(at).distance(pointer) > DRAG_MIN;
            let handle = dragged.then(|| to_world(pointer) - at);
            drawing.points.push(Point { at, handle });
            drawing.pressed = None;
        }
    }

    paint(
        ui,
        &drawing,
        target,
        response.interact_pointer_pos().map(&to_world),
        &to_screen,
        hover.is_some_and(near_first),
    );
    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);

    match finish {
        Some(closed) => {
            cancel(ui.ctx());
            apply(scene, &drawing, closed);
        }
        None => ui.ctx().data_mut(|d| {
            d.insert_temp(id(), drawing);
        }),
    }
}

/// The drawn points as a shape path in world pixels.
fn path(points: &[Point], closed: bool) -> Path {
    let start = points.first().map_or(Vec2::ZERO, |p| p.at);
    let count = if closed { points.len() } else { points.len().saturating_sub(1) };
    let segments = (0..count)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            match (a.handle, b.handle) {
                (None, None) => Segment::Line(b.at),
                (h_a, h_b) => {
                    // A point's handle leaves it forwards and arrives at it from the opposite side.
                    let c1 = a.at + h_a.unwrap_or(Vec2::ZERO);
                    let c2 = b.at - h_b.unwrap_or(Vec2::ZERO);
                    Segment::Cubic(c1, c2, b.at)
                }
            }
        })
        .collect();
    Path { closed, start, segments }
}

/// Puts the finished drawing in the scene.
fn apply(scene: &mut Scene, drawing: &Drawing, closed: bool) {
    let mut shape = Shape { name: "Drawn".into(), paths: vec![path(&drawing.points, closed)] };
    let Some((middle, reach)) = shape.normalize() else { return };
    let name = add_shape(&mut scene.shapes, shape);
    match drawing.target {
        DrawTarget::Arena => {
            let old = &scene.container;
            let (thickness, color) = (old.thickness(), old.color());
            // The arena's shrinking and pulsing scale it around its centre, so drawn sizes are
            // what you see at its starting size.
            let size = scene.arena_motion.size.max(0.01);
            scene.container = Container::Custom {
                shape: name,
                center: middle.round(),
                radius: (reach / size).clamp(20.0, 1200.0),
                rotation: 0.0,
                thickness,
                color,
            };
        }
        DrawTarget::Obstacle => scene.obstacles.push(Obstacle::Shape {
            shape: name,
            position: middle.round(),
            rotation: 0.0,
            scale: reach,
            thickness: 4.0,
            color: Color::rgb(0xc8, 0xd8, 0xff),
        }),
    }
}

fn paint(
    ui: &Ui,
    drawing: &Drawing,
    target: Option<Vec2>,
    pointer: Option<Vec2>,
    to_screen: &impl Fn(Vec2) -> Pos2,
    closing: bool,
) {
    let painter = ui.painter();
    let line = Stroke::new(2.0, theme::ACCENT_TEXT);
    let mut points = drawing.points.clone();
    // Show the point being placed, with its curve, as if let go now.
    if let (Some(at), Some(pointer)) = (drawing.pressed, pointer) {
        let handle = (to_screen(pointer).distance(to_screen(at)) > DRAG_MIN).then(|| pointer - at);
        points.push(Point { at, handle });
    } else if let Some(t) = target
        && !closing
    {
        points.push(Point { at: t, handle: None });
    }
    let flat = path(&points, closing).flatten(0.5);
    let screen: Vec<Pos2> = flat.points.iter().map(|&p| to_screen(p)).collect();
    if closing && screen.len() >= 3 {
        let mut ring = screen.clone();
        ring.push(screen[0]);
        painter.add(egui::Shape::line(ring, line));
    } else {
        painter.add(egui::Shape::line(screen, line));
    }
    for (i, p) in points.iter().enumerate() {
        let c = to_screen(p.at);
        if let Some(h) = p.handle {
            let (a, b) = (to_screen(p.at + h), to_screen(p.at - h));
            painter.line_segment([a, b], Stroke::new(1.0, Color32::from_white_alpha(120)));
            painter.circle_filled(a, 3.0, Color32::WHITE);
            painter.circle_filled(b, 3.0, Color32::WHITE);
        }
        let first = i == 0;
        let radius = if first && closing { 7.0 } else { 4.5 };
        painter.circle(c, radius, theme::ACCENT, Stroke::new(1.5, Color32::WHITE));
    }
    let hint = match drawing.target {
        DrawTarget::Arena => {
            "Click for corners · drag for curves · click the first point or press Enter to close · Shift snaps · Esc cancels"
        }
        DrawTarget::Obstacle => {
            "Click for corners · drag for curves · Enter closes · double-click ends an open line · Esc cancels"
        }
    };
    let rect = ui.max_rect();
    let galley = painter.layout_no_wrap(hint.to_string(), egui::FontId::proportional(12.5), theme::TEXT);
    let size = galley.size() + egui::vec2(24.0, 10.0);
    let pill = egui::Rect::from_center_size(rect.center_bottom() - egui::vec2(0.0, size.y / 2.0 + 10.0), size);
    painter.rect_filled(pill, size.y / 2.0, Color32::from_black_alpha(190));
    painter.galley(pill.min + egui::vec2(12.0, 5.0), galley, theme::TEXT);
}

/// Makes a custom arena out of a shape in world pixels, as a test helper.
#[cfg(test)]
pub(crate) fn draw_for_test(scene: &mut Scene, corners: &[Vec2], target: DrawTarget, closed: bool) {
    let points: Vec<Point> = corners.iter().map(|&at| Point { at, handle: None }).collect();
    apply(scene, &Drawing { target, points, pressed: None }, closed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_keeps_length_and_rounds_the_angle() {
        let to = snap(Vec2::ZERO, Vec2::new(100.0, 9.0));
        assert!((to.length() - Vec2::new(100.0, 9.0).length()).abs() < 1e-3);
        assert!(to.y.abs() < 1e-3);
        let diagonal = snap(Vec2::ZERO, Vec2::new(50.0, 47.0));
        assert!((diagonal.x - diagonal.y).abs() < 1e-3);
    }

    #[test]
    fn a_drawn_triangle_becomes_the_arena() {
        let mut scene = Scene::default();
        let corners = [Vec2::new(-300.0, 200.0), Vec2::new(300.0, 200.0), Vec2::new(0.0, -300.0)];
        draw_for_test(&mut scene, &corners, DrawTarget::Arena, true);
        let Container::Custom { shape, center, radius, .. } = &scene.container else { panic!("not custom") };
        assert_eq!(shape, "Drawn");
        assert!(scene.shape(shape).unwrap().is_closed());
        // The arena is placed where it was drawn.
        assert!((center.y - (-50.0)).abs() < 1.0, "{center}");
        assert!(*radius > 250.0 && *radius <= crate::inspector::shapes::ARENA_RADIUS, "{radius}");
    }

    #[test]
    fn curves_come_from_handles() {
        let points = [
            Point { at: Vec2::ZERO, handle: Some(Vec2::new(0.0, -50.0)) },
            Point { at: Vec2::new(100.0, 0.0), handle: None },
        ];
        let p = path(&points, false);
        assert!(matches!(p.segments[..], [Segment::Cubic(..)]));
        let lines = path(&[points[1], points[1]], false);
        assert!(matches!(lines.segments[..], [Segment::Line(_)]));
    }

    #[test]
    fn a_drawn_open_line_becomes_an_obstacle() {
        let mut scene = Scene::default();
        draw_for_test(&mut scene, &[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 50.0)], DrawTarget::Obstacle, false);
        assert!(matches!(scene.obstacles[..], [Obstacle::Shape { .. }]));
        // Two drawings get two names.
        draw_for_test(&mut scene, &[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 50.0)], DrawTarget::Obstacle, false);
        assert_eq!(scene.shapes[1].name, "Drawn 2");
        // A drawing with no size makes nothing.
        draw_for_test(&mut scene, &[Vec2::ONE, Vec2::ONE], DrawTarget::Obstacle, false);
        assert_eq!(scene.obstacles.len(), 2);
    }
}
