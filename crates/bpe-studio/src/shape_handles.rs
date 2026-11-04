//! Moulding a shape in the preview: dragging its corners, sides and bend handles (a circle by its
//! edge), the right-click menus on them, and drawing the handles. Corners are kept in the shape's
//! own units (see [`ShapePath`]), so the world is turned back into them through the shape's
//! middle, turn and size.

use std::f32::consts::FRAC_PI_2;

use bpe_core::shape_path::{FLATTEN, ON_THE_LINE};
use bpe_core::{Scene, Settings, ShapePath, ThingId, Value};
use bpe_kit::{ShapeSettings, ShapeStart, ShapeState};
use bpe_runtime::{Outline, World};
use eframe::egui::{self, Painter, Pos2, Rect, Stroke};
use glam::Vec2;

use crate::grid::{snap_length, snap_point};
use crate::preview::{Drag, Handle, View};
use crate::style;

/// How close to an outline, in screen points, counts as grabbing it.
pub const EDGE_GRAB: f32 = 12.0;
/// How close to a corner, in screen points, counts as grabbing it.
const CORNER_GRAB: f32 = 8.0;
/// How close to a bend handle, in screen points, counts as grabbing it.
const BEND_GRAB: f32 = 10.0;
/// How close to a circle's radius dot, in screen points, counts as grabbing it.
const DOT_GRAB: f32 = 8.0;
/// A hollow shape's band can be grabbed this close to its centre line at least, in screen points.
const BAND_GRAB: f32 = 6.0;
/// How close to a selected shape's middle, in screen points, counts as grabbing it to move it.
const MIDDLE_GRAB: f32 = 10.0;
/// The smallest Size a drag sets.
const SMALLEST: f32 = 20.0;

/// Where a shape is: its middle, its turn and its size, which take shape units to the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeFrame {
    pub centre: Vec2,
    /// Radians clockwise.
    pub angle: f32,
    /// World pixels per shape unit.
    pub size: f32,
}

impl ShapeFrame {
    pub fn to_shape(self, world: Vec2) -> Vec2 {
        Vec2::from_angle(-self.angle).rotate(world - self.centre) / self.size.max(1e-6)
    }

    pub fn to_world(self, shape: Vec2) -> Vec2 {
        self.centre + Vec2::from_angle(self.angle).rotate(shape * self.size)
    }
}

/// A shape as it is in the world right now: where it is, and its outline in shape units.
pub struct Seen {
    pub frame: ShapeFrame,
    /// Its outline: the moulded one, the even polygon's, or a circle's 8 curved sides.
    pub path: ShapePath,
    /// It starts from a circle, which is moulded by its radius only.
    pub circle: bool,
    /// How much it has shrunk or grown since the start: its Size times this is `frame.size`.
    pub scale: f32,
}

/// Shape `id` as the world has it now, when it is a shape.
pub fn seen(world: &World, id: ThingId) -> Option<Seen> {
    let wall = world.walls.get(&id)?;
    let st = world.states.get(&id)?.as_any().downcast_ref::<ShapeState>()?;
    // An even polygon's ring is turned a further quarter (see `base_outline`).
    let angle = match wall.outline {
        Outline::Ring { sides, angle, .. } if sides >= 3 => angle - FRAC_PI_2,
        ref outline => outline.angle(),
    };
    let frame = ShapeFrame { centre: wall.outline.center(), angle, size: st.settings.size * st.scale };
    Some(Seen {
        frame,
        path: st.settings.outline_path(),
        circle: st.settings.start == ShapeStart::Circle,
        scale: st.scale,
    })
}

/// Screen points per shape unit.
fn units(view: &View, frame: ShapeFrame) -> f32 {
    (frame.size * view.world_scale()).max(1e-6)
}

/// The handle of selected shape `id` under `pos`: a bend handle, a corner, a circle's radius dot
/// or edge, or a side.
pub fn hit_handle(world: &World, view: &View, id: ThingId, pos: Pos2) -> Option<Handle> {
    let s = seen(world, id)?;
    let on_screen = |p: Vec2| view.to_screen(s.frame.to_world(p));
    let near = |p: Vec2, reach: f32| (on_screen(p) - pos).length() <= reach;
    // The middle moves the whole shape, as a band too thin to grab apart from its sides can't.
    let middle = (view.to_screen(s.frame.centre) - pos).length() <= MIDDLE_GRAB;
    if s.circle {
        if near(Vec2::X, DOT_GRAB) {
            return Some(Handle::Radius(id));
        }
        if middle {
            return Some(Handle::Body(id));
        }
        let from_middle = (view.to_world(pos) - s.frame.centre).length() * view.world_scale();
        let edge = (from_middle - units(view, s.frame)).abs() <= EDGE_GRAB;
        return edge.then_some(Handle::Radius(id));
    }
    let path = &s.path;
    for side in 0..path.side_count() {
        if let Some(bends) = path.bends[side] {
            for (which, h) in bends.iter().enumerate() {
                if near(Vec2::from(*h), BEND_GRAB) {
                    return Some(Handle::Bend(id, side, which as u8));
                }
            }
        }
    }
    let corner = (0..path.side_count())
        .map(|i| (i, (on_screen(path.point(i)) - pos).length()))
        .filter(|&(_, d)| d <= CORNER_GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((i, _)) = corner {
        return Some(Handle::Corner(id, i));
    }
    if middle {
        return Some(Handle::Body(id));
    }
    let (side, _, d) = path.nearest_side(s.frame.to_shape(view.to_world(pos)))?;
    (d * units(view, s.frame) <= EDGE_GRAB).then_some(Handle::Side(id, side))
}

/// Whether `pos` is on shape `id` itself: inside a filled one, or on a hollow one's band.
pub fn on_body(world: &World, view: &View, id: ThingId, pos: Pos2) -> bool {
    let Some(wall) = world.walls.get(&id) else { return false };
    if seen(world, id).is_none() {
        return false;
    }
    let p = view.to_world(pos);
    if wall.solid && wall.outline.inside(p) {
        return true;
    }
    let (q, _) = wall.outline.nearest(p);
    let reach = (wall.outline.thickness() * 0.5).max(BAND_GRAB / view.world_scale().max(1e-6));
    (q - p).length() <= reach
}

/// How a corner, side or bend drag mirrors, worked out when it starts.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mirror {
    /// The dragged corner or handle sits on the middle line, and the corner or handle across it.
    Point {
        on_line: bool,
        partner: Option<usize>,
    },
    Bend {
        on_line: bool,
        partner: Option<(usize, usize)>,
    },
    /// The side across the middle line (itself when it crosses the line evenly).
    Side {
        partner: Option<usize>,
    },
    /// A circle's edge: nothing to mirror.
    Nothing,
}

/// What a moulding drag holds from its start: the outline then (with the even polygon's corners
/// when it had none), how far the pointer was from the dragged point, and the mirror partners.
#[derive(Clone, Debug, PartialEq)]
pub struct Mould {
    /// Where the shape was shown when the drag began. The drag goes on through it, so a shape
    /// that was turned or shrunk at the playhead moulds as it was seen, whatever the world does
    /// once the edit rebuilds it.
    frame: ShapeFrame,
    /// [`Seen::scale`] then.
    scale: f32,
    start: ShapePath,
    /// The pointer at the press less the dragged point (a side's first corner), in world pixels.
    offset: Vec2,
    mirror: Mirror,
}

/// The side across the middle line from `side`: the one whose corners mirror its corners. A
/// corner on the line is its own mirror.
fn mirror_side(path: &ShapePath, side: usize) -> Option<usize> {
    let n = path.side_count();
    let image = |i: usize| {
        if path.point(i).x.abs() < ON_THE_LINE { Some(i) } else { path.mirror_partner_point(i) }
    };
    let (a, b) = (image(side)?, image((side + 1) % n)?);
    (0..n).find(|&t| (t == a && (t + 1) % n == b) || (t == b && (t + 1) % n == a))
}

/// The start of a drag on `handle`, pressed at `press` (world pixels). None for handles that
/// aren't a shape's corner, side or bend.
pub fn start(world: &World, handle: Handle, press: Vec2) -> Option<Mould> {
    let s = seen(world, handle.thing())?;
    let path = s.path;
    let (point, mirror) = match handle {
        Handle::Corner(_, i) if i < path.side_count() => {
            let p = path.point(i);
            (p, Mirror::Point { on_line: p.x.abs() < ON_THE_LINE, partner: path.mirror_partner_point(i) })
        }
        Handle::Side(_, side) if side < path.side_count() => {
            (path.point(side), Mirror::Side { partner: mirror_side(&path, side) })
        }
        Handle::Bend(_, side, which) => {
            let h = Vec2::from(path.bends.get(side).copied().flatten()?[usize::from(which.min(1))]);
            let partner = path.mirror_partner_bend(side, usize::from(which));
            (h, Mirror::Bend { on_line: h.x.abs() < ON_THE_LINE, partner })
        }
        Handle::Radius(_) => (Vec2::ZERO, Mirror::Nothing),
        _ => return None,
    };
    Some(Mould { frame: s.frame, scale: s.scale, offset: press - s.frame.to_world(point), start: path, mirror })
}

/// `p` across the middle line.
fn mirrored(p: Vec2) -> Vec2 {
    Vec2::new(-p.x, p.y)
}

/// The outline `m` started from, moulded by dragging `handle`'s point to `to` (shape units),
/// mirrored when `shift` is held.
fn moulded(m: &Mould, handle: Handle, to: Vec2, shift: bool) -> ShapePath {
    let mut path = m.start.clone();
    let n = path.side_count();
    match (handle, m.mirror) {
        (Handle::Corner(_, i), Mirror::Point { on_line, partner }) => {
            let to = if shift && on_line { Vec2::new(0.0, to.y) } else { to };
            path.move_point(i, to);
            if let (true, Some(j)) = (shift, partner) {
                path.move_point(j, mirrored(to));
            }
        }
        (Handle::Bend(_, side, which), Mirror::Bend { on_line, partner }) => {
            let to = if shift && on_line { Vec2::new(0.0, to.y) } else { to };
            path.move_bend(side, usize::from(which), to);
            if let (true, Some((s, w))) = (shift, partner) {
                path.move_bend(s, w, mirrored(to));
            }
        }
        (Handle::Side(_, side), Mirror::Side { partner }) => {
            let by = to - m.start.point(side);
            match partner.filter(|_| shift) {
                // A side across the line from itself stays even: it moves up and down only.
                Some(t) if t == side => {
                    path.move_side(side, Vec2::new(0.0, by.y));
                }
                Some(t) => {
                    let back = mirrored(by);
                    let mut moves: Vec<(usize, Vec2)> = vec![(side, by), ((side + 1) % n, by)];
                    for i in [t, (t + 1) % n] {
                        match moves.iter_mut().find(|(j, _)| *j == i) {
                            // A corner both sides share is on the line: it moves up and down only.
                            Some(shared) => shared.1 = Vec2::new(0.0, by.y),
                            None => moves.push((i, back)),
                        }
                    }
                    for (i, d) in moves {
                        path.move_point(i, m.start.point(i) + d);
                    }
                    for (s, d) in [(side, by), (t, back)] {
                        if let Some(b) = m.start.bends[s] {
                            for (w, h) in b.iter().enumerate() {
                                path.move_bend(s, w, Vec2::from(*h) + d);
                            }
                        }
                    }
                }
                None => {
                    path.move_side(side, by);
                }
            }
        }
        _ => {}
    }
    path
}

/// Moulds the shape `drag` holds to the pointer at `pointer` (world pixels), through the shape
/// as it was seen when the drag began: a corner, side or bend handle to it (snapped to a grid
/// `step`), or a circle's Size to its distance from the middle. The first change gives an even
/// polygon its own corners.
pub fn apply(scene: &mut Scene, drag: &Drag, pointer: Vec2, step: Option<f32>) {
    let id = drag.handle.thing();
    let Some(m) = &drag.mould else { return };
    let Some(thing) = scene.things.get_mut(&id) else { return };
    if let Handle::Radius(_) = drag.handle {
        // The Size that shows that far out, shrunk as it was seen.
        let shown = (pointer - m.frame.centre).length() / m.scale.max(1e-6);
        let size = snap_length(shown, step).round().max(SMALLEST);
        thing.params.set("size", Value::Number(f64::from(size)));
        return;
    }
    let to = m.frame.to_shape(snap_point(pointer - m.offset, step));
    let path = moulded(m, drag.handle, to, drag.shift);
    let had = ShapeSettings::from_params(&thing.params).moulded().is_some();
    if path.is_valid() && (had || path != m.start) {
        thing.params.set("path", Value::Path(path));
    }
}

/// What a right-click on a shape was on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuTarget {
    /// A side, `t` along it.
    Side(ThingId, usize, f32),
    Corner(ThingId, usize),
    /// A circle's edge.
    Circle(ThingId),
}

/// What right-clicking at `pos` on selected shape `id` opens a menu for.
pub fn menu_target(world: &World, view: &View, id: ThingId, pos: Pos2) -> Option<MenuTarget> {
    let s = seen(world, id)?;
    match hit_handle(world, view, id, pos)? {
        Handle::Radius(_) => Some(MenuTarget::Circle(id)),
        Handle::Corner(_, i) => Some(MenuTarget::Corner(id, i)),
        // A bend handle's menu is its side's.
        Handle::Side(_, side) | Handle::Bend(_, side, _) => {
            let (t, _, _) = s.path.nearest_on_side(side, s.frame.to_shape(view.to_world(pos)));
            Some(MenuTarget::Side(id, side, t))
        }
        _ => None,
    }
}

/// A right-click menu's item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    AddPoint,
    CreateCurve,
    Straighten,
    DeletePoint,
    TurnIntoPoints,
}

/// The outline shape `id` has in `scene`: its own, or the even polygon's or circle's.
fn outline_of(scene: &Scene, id: ThingId) -> Option<ShapeSettings> {
    scene
        .things
        .get(&id)
        .filter(|t| t.kind == bpe_kit::things::shape::KIND)
        .map(|t| ShapeSettings::from_params(&t.params))
}

/// Draws the menu for `target`. Returns the item picked.
pub fn menu(ui: &mut egui::Ui, scene: &Scene, target: MenuTarget) -> Option<MenuAction> {
    let mut picked = None;
    let mut item = |ui: &mut egui::Ui, enabled: bool, text: &str, help: &str, action: MenuAction| {
        let button = ui.add_enabled(enabled, egui::Button::new(text)).on_hover_text(help);
        if button.clicked() {
            picked = Some(action);
            ui.close();
        }
        button
    };
    ui.set_min_width(160.0);
    match target {
        MenuTarget::Side(id, side, _) => {
            let curved =
                outline_of(scene, id).is_some_and(|s| s.outline_path().bends.get(side).is_some_and(|b| b.is_some()));
            item(ui, true, "Add point", "A new corner here, which you can drag", MenuAction::AddPoint);
            if curved {
                item(ui, true, "Straighten", "Makes this side straight again", MenuAction::Straighten);
            } else {
                item(
                    ui,
                    true,
                    "Create curve",
                    "Bends this side, with two handles to shape it",
                    MenuAction::CreateCurve,
                );
            }
        }
        MenuTarget::Corner(id, _) => {
            let enough = outline_of(scene, id).is_some_and(|s| s.outline_path().side_count() > 3);
            item(ui, enough, "Delete point", "Takes this corner away", MenuAction::DeletePoint)
                .on_disabled_hover_text("A shape keeps at least 3 points");
        }
        MenuTarget::Circle(_) => {
            item(
                ui,
                true,
                "Turn into points",
                "Makes the circle a shape of 8 curved sides you can mould",
                MenuAction::TurnIntoPoints,
            );
        }
    }
    picked
}

/// Does `action` to `target`'s shape in `scene`. True when the scene changed. An even polygon
/// gets its own corners first.
pub fn act(scene: &mut Scene, target: MenuTarget, action: MenuAction) -> bool {
    let id = match target {
        MenuTarget::Side(id, ..) | MenuTarget::Corner(id, _) | MenuTarget::Circle(id) => id,
    };
    let Some(settings) = outline_of(scene, id) else { return false };
    let Some(thing) = scene.things.get_mut(&id) else { return false };
    if action == MenuAction::TurnIntoPoints {
        if settings.start != ShapeStart::Circle {
            return false;
        }
        thing.params.set("start", Value::Choice("polygon".into()));
        thing.params.set("path", Value::Path(ShapePath::circle()));
        return true;
    }
    if settings.start == ShapeStart::Circle {
        return false;
    }
    let mut path = settings.outline_path();
    let changed = match (target, action) {
        (MenuTarget::Side(_, side, t), MenuAction::AddPoint) => path.add_point(side, t),
        (MenuTarget::Side(_, side, _), MenuAction::CreateCurve) => path.create_curve(side),
        (MenuTarget::Side(_, side, _), MenuAction::Straighten) => path.straighten(side),
        (MenuTarget::Corner(_, i), MenuAction::DeletePoint) => path.delete_point(i),
        _ => false,
    };
    if changed {
        thing.params.set("path", Value::Path(path));
    }
    changed
}

/// Draws shape `id`'s outline and, when it is `selected`, its handles, with `active` (dragged or
/// under the pointer) brighter.
pub fn draw(
    painter: &Painter,
    scene: &Scene,
    world: &World,
    view: &View,
    id: ThingId,
    selected: bool,
    active: Option<Handle>,
) {
    let Some(s) = seen(world, id) else { return };
    let on_screen = |p: Vec2| view.to_screen(s.frame.to_world(p));
    let mark = style::colour_of(scene, id);
    let outline: Vec<Pos2> = s.path.flatten(FLATTEN).into_iter().map(on_screen).collect();
    if !selected {
        painter.add(egui::Shape::closed_line(outline, Stroke::new(1.0, style::FAINT_MARK)));
        return;
    }
    let is_active = |h: Handle| active == Some(h);
    let body_active = is_active(Handle::Body(id)) || is_active(Handle::Radius(id));
    painter.add(egui::Shape::closed_line(
        outline,
        Stroke::new(if body_active { 2.5 } else { 1.5 }, if body_active { style::TEXT_STRONG } else { mark }),
    ));
    // The middle, which moves the shape: a ring like a ball's.
    let moving = is_active(Handle::Body(id));
    painter.circle_stroke(
        view.to_screen(s.frame.centre),
        6.0,
        Stroke::new(if moving { 2.0 } else { 1.2 }, if moving { style::TEXT_STRONG } else { mark }),
    );
    if s.circle {
        let dot = on_screen(Vec2::X);
        let lit = is_active(Handle::Radius(id));
        painter.circle(
            dot,
            if lit { 6.5 } else { 5.0 },
            if lit { style::TEXT_STRONG } else { mark },
            Stroke::new(1.5, style::OUTLINE),
        );
        return;
    }
    let path = &s.path;
    let n = path.side_count();
    // The bend handles, dashed to their corners.
    for side in 0..n {
        let Some(bends) = path.bends[side] else { continue };
        let (a, b) = path.side_ends(side);
        for (which, (h, corner)) in bends.iter().zip([a, b]).enumerate() {
            let at = on_screen(Vec2::from(*h));
            let lit = is_active(Handle::Bend(id, side, which as u8));
            let ink = if lit { style::TEXT_STRONG } else { mark };
            painter.add(egui::Shape::dashed_line(&[on_screen(corner), at], Stroke::new(1.0, ink), 4.0, 3.0));
            let r = if lit { 5.5 } else { 4.5 };
            let diamond = vec![
                at - egui::vec2(0.0, r),
                at + egui::vec2(r, 0.0),
                at + egui::vec2(0.0, r),
                at - egui::vec2(r, 0.0),
            ];
            painter.add(egui::Shape::closed_line(diamond, Stroke::new(1.5, ink)));
        }
    }
    // A dot at each side's middle.
    for side in 0..n {
        let lit = is_active(Handle::Side(id, side));
        let middle = on_screen(path.side_point(side, 0.5));
        painter.circle_filled(middle, if lit { 4.0 } else { 2.25 }, if lit { style::TEXT_STRONG } else { mark });
        if lit {
            let line: Vec<Pos2> =
                (0..=FLATTEN).map(|k| on_screen(path.side_point(side, k as f32 / FLATTEN as f32))).collect();
            painter.add(egui::Shape::line(line, Stroke::new(2.5, style::TEXT_STRONG)));
        }
    }
    // The corners: white squares with a track-colour border.
    for i in 0..n {
        let lit = is_active(Handle::Corner(id, i));
        let half = if lit { 5.5 } else { 4.5 };
        let square = Rect::from_center_size(on_screen(path.point(i)), egui::Vec2::splat(half * 2.0));
        let border = if lit { style::OUTLINE } else { mark };
        painter.rect(square, 1.0, style::TEXT_STRONG, Stroke::new(1.5, border), egui::StrokeKind::Middle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Studio;
    use crate::preview::hit_test;
    use bpe_runtime::CameraView;
    use eframe::egui::{Pos2, Rect, vec2};

    const SHAPE: ThingId = ThingId(1);

    /// The blank project's shape, its params changed by `edit`.
    fn studio_with(edit: impl FnOnce(&mut bpe_core::Params)) -> Studio {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        edit(&mut studio.scene.things.get_mut(&SHAPE).unwrap().params);
        studio.after_edit(before);
        studio.history.end_merging();
        studio
    }

    /// An even polygon of `sides` corners 400 px from the middle.
    fn polygon(sides: i64) -> Studio {
        studio_with(|p| {
            p.set("start", Value::Choice("polygon".into()));
            p.set("sides", Value::Int(sides));
            p.set("size", Value::Number(400.0));
        })
    }

    fn view() -> View {
        View {
            rect: Rect::from_min_size(Pos2::ZERO, vec2(270.0, 480.0)),
            scale: 0.25,
            camera: CameraView::default(),
            world: false,
        }
    }

    fn path(studio: &Studio) -> ShapePath {
        match studio.scene.things[&SHAPE].params.get("path") {
            Some(Value::Path(p)) => p.clone(),
            _ => ShapePath::default(),
        }
    }

    /// Drags `handle` from its point at `press` (world pixels) by `by`.
    fn drag(studio: &mut Studio, handle: Handle, press: Vec2, by: Vec2, shift: bool, step: Option<f32>) {
        let world = studio.playback.world().clone();
        let mould = start(&world, handle, press);
        let drag = Drag { mould, shift, ..Drag::new(handle, Vec2::ZERO) };
        apply(&mut studio.scene, &drag, press + by, step);
    }

    /// Corner `i` of the even polygon in world pixels (400 px shape at the middle).
    fn corner(sides: u32, i: usize) -> Vec2 {
        ShapePath::even(sides).point(i) * 400.0
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn the_frame_turns_world_points_into_shape_units_and_back() {
        let f = ShapeFrame { centre: Vec2::new(100.0, -50.0), angle: 0.5, size: 200.0 };
        let p = Vec2::new(0.3, -0.7);
        assert!(close(f.to_shape(f.to_world(p)), p));
        assert!(close(f.to_world(Vec2::X), f.centre + Vec2::from_angle(0.5) * 200.0));
        // The world's even polygon is where the path says its corners are.
        let studio = polygon(6);
        let s = seen(studio.playback.world(), SHAPE).unwrap();
        assert_eq!(s.frame, ShapeFrame { centre: Vec2::ZERO, angle: 0.0, size: 400.0 });
        let world_corners = studio.playback.world().walls[&SHAPE].outline.corners();
        for k in 0..6 {
            assert!(world_corners.iter().any(|c| (*c - s.frame.to_world(s.path.point(k))).length() < 1e-2));
        }
    }

    #[test]
    fn the_first_corner_drag_gives_the_polygon_its_corners_and_moves_only_that_one() {
        let mut studio = polygon(6);
        drag(&mut studio, Handle::Corner(SHAPE, 2), corner(6, 2), Vec2::ZERO, false, None);
        assert!(path(&studio).is_empty(), "a drag that moves nothing changes nothing");
        drag(&mut studio, Handle::Corner(SHAPE, 2), corner(6, 2), Vec2::new(40.0, 20.0), false, None);
        let p = path(&studio);
        let even = ShapePath::even(6);
        assert_eq!(p.side_count(), 6);
        assert!(close(p.point(2), even.point(2) + Vec2::new(0.1, 0.05)));
        for k in [0, 1, 3, 4, 5] {
            assert_eq!(p.point(k), even.point(k), "corner {k} stays");
        }
    }

    #[test]
    fn a_corner_lands_on_the_grid() {
        let mut studio = polygon(6);
        drag(&mut studio, Handle::Corner(SHAPE, 2), corner(6, 2), Vec2::new(33.0, 17.0), false, Some(60.0));
        assert!(close(path(&studio).point(2) * 400.0, Vec2::new(420.0, 0.0)), "{}", path(&studio).point(2) * 400.0);
    }

    #[test]
    fn a_side_drag_moves_both_its_corners() {
        let mut studio = polygon(6);
        let grab = (corner(6, 1) + corner(6, 2)) * 0.5;
        drag(&mut studio, Handle::Side(SHAPE, 1), grab, Vec2::new(40.0, -20.0), false, None);
        let (p, even) = (path(&studio), ShapePath::even(6));
        for k in [1, 2] {
            assert!(close(p.point(k), even.point(k) + Vec2::new(0.1, -0.05)), "corner {k}");
        }
        assert_eq!(p.point(3), even.point(3));
    }

    #[test]
    fn shift_mirrors_the_partner_across_the_middle_line() {
        // Corner 2 (right) and corner 5 (left) of a hexagon mirror each other.
        let mut studio = polygon(6);
        drag(&mut studio, Handle::Corner(SHAPE, 2), corner(6, 2), Vec2::new(40.0, 20.0), true, None);
        let p = path(&studio);
        assert!(close(p.point(2), Vec2::new(1.1, 0.05)));
        assert!(close(p.point(5), Vec2::new(-1.1, 0.05)), "{}", p.point(5));
        // Side 1 (corners 1 and 2) mirrors side 5 (corners 5 and 0).
        let mut studio = polygon(6);
        drag(&mut studio, Handle::Side(SHAPE, 1), corner(6, 1), Vec2::new(40.0, 20.0), true, None);
        let (p, even) = (path(&studio), ShapePath::even(6));
        for (k, d) in [
            (1, Vec2::new(0.1, 0.05)),
            (2, Vec2::new(0.1, 0.05)),
            (5, Vec2::new(-0.1, 0.05)),
            (0, Vec2::new(-0.1, 0.05)),
        ] {
            assert!(close(p.point(k), even.point(k) + d), "corner {k}: {}", p.point(k));
        }
        // The top side is its own mirror: it moves up and down only.
        let mut studio = polygon(6);
        drag(&mut studio, Handle::Side(SHAPE, 0), corner(6, 0), Vec2::new(40.0, 20.0), true, None);
        let p = path(&studio);
        for k in [0, 1] {
            assert!(close(p.point(k), even.point(k) + Vec2::new(0.0, 0.05)), "corner {k}");
        }
    }

    #[test]
    fn a_point_on_the_middle_line_stays_on_it_with_shift() {
        // A pentagon's top corner is on the line.
        let mut studio = polygon(5);
        drag(&mut studio, Handle::Corner(SHAPE, 0), corner(5, 0), Vec2::new(30.0, -20.0), true, None);
        assert!(close(path(&studio).point(0), Vec2::new(0.0, -1.05)));
        let mut studio = polygon(5);
        drag(&mut studio, Handle::Corner(SHAPE, 0), corner(5, 0), Vec2::new(30.0, -20.0), false, None);
        assert!(close(path(&studio).point(0), Vec2::new(0.075, -1.05)), "without Shift it moves freely");
    }

    #[test]
    fn a_bend_handle_drags_and_a_circle_sizes_by_its_edge() {
        let mut studio = polygon(6);
        let before = studio.scene.clone();
        assert!(act(&mut studio.scene, MenuTarget::Side(SHAPE, 1, 0.5), MenuAction::CreateCurve));
        studio.after_edit(before);
        let bends = path(&studio).bends[1].unwrap();
        let handle = Vec2::from(bends[0]) * 400.0;
        drag(&mut studio, Handle::Bend(SHAPE, 1, 0), handle, Vec2::new(-20.0, 40.0), false, None);
        let moved = path(&studio).bends[1].unwrap();
        assert!(close(Vec2::from(moved[0]), Vec2::from(bends[0]) + Vec2::new(-0.05, 0.1)));
        assert_eq!(moved[1], bends[1], "the other handle stays");
        // The circle's dot and edge set its Size, never below 20 px.
        let mut studio = Studio::headless();
        studio.selection.set(SHAPE);
        let v = view();
        assert_eq!(hit_test(&studio, &v, v.to_screen(Vec2::new(508.0, 0.0))), Some(Handle::Radius(SHAPE)));
        assert_eq!(hit_test(&studio, &v, v.to_screen(Vec2::new(0.0, 508.0))), Some(Handle::Radius(SHAPE)));
        drag(&mut studio, Handle::Radius(SHAPE), Vec2::new(508.0, 0.0), Vec2::new(-500.0, 0.0), false, None);
        assert_eq!(studio.scene.things[&SHAPE].params.get("size"), Some(&Value::Number(20.0)));
    }

    #[test]
    fn a_spinning_shape_moulds_as_it_is_shown_at_the_playhead() {
        let mut studio = studio_with(|p| {
            p.set("start", Value::Choice("polygon".into()));
            p.set("size", Value::Number(400.0));
            p.set("spin", Value::Number(90.0));
        });
        studio.playback.seek_within(1.0, None);
        let shown = studio.playback.world().clone();
        let frame = seen(&shown, SHAPE).unwrap().frame;
        assert!((frame.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-2, "a quarter turn after 1 s: {}", frame.angle);
        // Grabbed where it is drawn now; the edit then rebuilds the world from its start, and the
        // drag goes on through the shape as it was seen.
        let handle = Handle::Corner(SHAPE, 2);
        let press = frame.to_world(ShapePath::even(6).point(2));
        let drag = Drag { mould: start(&shown, handle, press), ..Drag::new(handle, Vec2::ZERO) };
        let by = Vec2::new(0.0, 40.0);
        apply(&mut studio.scene, &drag, press + by, None);
        let expected = ShapePath::even(6).point(2) + Vec2::from_angle(-frame.angle).rotate(by) / 400.0;
        assert!(close(path(&studio).point(2), expected), "{} vs {expected}", path(&studio).point(2));
    }

    #[test]
    fn a_selected_shape_moves_by_its_middle() {
        for mut studio in [Studio::headless(), polygon(6)] {
            studio.selection.set(SHAPE);
            let v = view();
            assert_eq!(hit_test(&studio, &v, v.to_screen(Vec2::new(3.0, -2.0))), Some(Handle::Body(SHAPE)));
        }
    }

    #[test]
    fn the_selected_shape_s_handles_are_found_in_order() {
        let mut studio = polygon(6);
        let v = view();
        let at = |p: Vec2| v.to_screen(p);
        // Not selected: its band moves it, its corners are not handles.
        assert_eq!(hit_test(&studio, &v, at(corner(6, 2))), Some(Handle::Body(SHAPE)));
        studio.selection.set(SHAPE);
        assert_eq!(hit_test(&studio, &v, at(corner(6, 2))), Some(Handle::Corner(SHAPE, 2)));
        let middle = (corner(6, 2) + corner(6, 3)) * 0.5;
        assert_eq!(hit_test(&studio, &v, at(middle)), Some(Handle::Side(SHAPE, 2)));
        assert_eq!(hit_test(&studio, &v, at(Vec2::new(0.0, 250.0))), None, "inside a hollow shape is empty");
    }

    #[test]
    fn clicking_inside_a_filled_shape_grabs_it() {
        let studio = studio_with(|p| p.set("fill", Value::Choice("filled".into())));
        let v = view();
        assert_eq!(hit_test(&studio, &v, v.to_screen(Vec2::new(0.0, 250.0))), Some(Handle::Body(SHAPE)));
        assert_eq!(hit_test(&studio, &v, v.to_screen(Vec2::new(0.0, 700.0))), None);
    }

    #[test]
    fn right_click_menus_find_their_target() {
        let mut studio = polygon(6);
        studio.selection.set(SHAPE);
        let v = view();
        let w = studio.playback.world().clone();
        let middle = (corner(6, 2) + corner(6, 3)) * 0.5;
        match menu_target(&w, &v, SHAPE, v.to_screen(middle)) {
            Some(MenuTarget::Side(SHAPE, 2, t)) => assert!((t - 0.5).abs() < 0.01, "{t}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(menu_target(&w, &v, SHAPE, v.to_screen(corner(6, 4))), Some(MenuTarget::Corner(SHAPE, 4)));
        let circle = Studio::headless();
        let w = circle.playback.world().clone();
        assert_eq!(menu_target(&w, &v, SHAPE, v.to_screen(Vec2::new(0.0, -508.0))), Some(MenuTarget::Circle(SHAPE)));
    }

    #[test]
    fn each_menu_action_changes_the_outline_as_one_undo_step() {
        let mut studio = polygon(6);
        let steps = studio.history.undo.len();
        let step = |studio: &mut Studio, target, action| {
            let before = studio.scene.clone();
            let changed = act(&mut studio.scene, target, action);
            studio.panels.inspector.own_step = true;
            studio.after_edit(before);
            changed
        };
        assert!(step(&mut studio, MenuTarget::Side(SHAPE, 0, 0.5), MenuAction::AddPoint));
        assert_eq!(path(&studio).words(), "7 points");
        assert!(step(&mut studio, MenuTarget::Side(SHAPE, 3, 0.5), MenuAction::CreateCurve));
        assert_eq!(path(&studio).words(), "7 points, 1 curve");
        assert!(step(&mut studio, MenuTarget::Side(SHAPE, 3, 0.5), MenuAction::Straighten));
        assert_eq!(path(&studio).words(), "7 points");
        assert!(step(&mut studio, MenuTarget::Corner(SHAPE, 1), MenuAction::DeletePoint));
        assert_eq!(path(&studio).words(), "6 points");
        assert_eq!(studio.history.undo.len(), steps + 4, "four quick actions, four steps");
        assert!(studio.undo());
        assert_eq!(path(&studio).words(), "7 points");
        // A triangle keeps its 3 points.
        let mut studio = polygon(3);
        assert!(!step(&mut studio, MenuTarget::Corner(SHAPE, 0), MenuAction::DeletePoint));
        assert!(path(&studio).is_empty());
        // Turn into points: a polygon of 8 curved sides the same size.
        let mut studio = Studio::headless();
        assert!(step(&mut studio, MenuTarget::Circle(SHAPE), MenuAction::TurnIntoPoints));
        let s = ShapeSettings::from_params(&studio.scene.things[&SHAPE].params);
        assert_eq!((s.start, s.size, path(&studio)), (ShapeStart::Polygon, 508.0, ShapePath::circle()));
        assert_eq!(path(&studio).words(), "8 points, 8 curves");
    }
}
