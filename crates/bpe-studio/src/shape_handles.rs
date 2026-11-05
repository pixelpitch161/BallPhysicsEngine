//! Moulding a shape in the preview: dragging its corners, sides and bend handles (a circle by its
//! edge, an ellipse by its edge and its width and height dots), the right-click menus on them, and
//! drawing the handles (and an ellipse's or circle's foci). Corners are kept in the shape's
//! own units (see [`ShapePath`]), so the world is turned back into them through the shape's
//! middle, turn and size.

use std::f32::consts::FRAC_PI_2;

use bpe_core::shape_path::{FLATTEN, MIRROR_REACH, ON_THE_LINE};
use bpe_core::{Address, Registry, Scene, Settings, ShapePath, ThingId, Value};
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
    /// It is an exact ellipse, moulded by its edge and its width and height dots.
    pub ellipse: Option<SeenEllipse>,
    /// How much it has shrunk or grown since the start: its Size times this is `frame.size`.
    pub scale: f32,
}

/// An ellipse as it is moulded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeenEllipse {
    /// Its half-axes in shape units (the larger one 1).
    pub axes: Vec2,
    /// Its Turn, in radians clockwise, on top of the shape's own.
    pub turn: f32,
    /// Its Width and Height as set.
    pub size: Vec2,
}

impl SeenEllipse {
    /// Where the Width (0) or Height (1) dot is, in shape units: the right end of the width
    /// axis, the top of the height axis, turned with the ellipse.
    pub fn dot(&self, which: u8) -> Vec2 {
        let end = if which == 0 { Vec2::new(self.axes.x, 0.0) } else { Vec2::new(0.0, -self.axes.y) };
        Vec2::from_angle(self.turn).rotate(end)
    }
}

/// Shape `id` as the world has it now, when it is a shape.
pub fn seen(world: &World, id: ThingId) -> Option<Seen> {
    let wall = world.walls.get(&id)?;
    let st = world.states.get(&id)?.as_any().downcast_ref::<ShapeState>()?;
    let s = &st.settings;
    let ellipse = (s.start == ShapeStart::Ellipse).then(|| SeenEllipse {
        axes: s.ellipse_axes(),
        turn: s.turn.to_radians(),
        size: Vec2::new(s.width, s.height),
    });
    // An even polygon's ring is turned a further quarter (see `base_outline`), and an ellipse's
    // outline by its Turn, which its shape units (and its outline path) carry instead.
    let angle = match wall.outline {
        Outline::Ring { sides, angle, .. } if sides >= 3 => angle - FRAC_PI_2,
        ref outline => outline.angle() - ellipse.map_or(0.0, |e| e.turn),
    };
    let frame = ShapeFrame { centre: wall.outline.center(), angle, size: s.unit() * st.scale };
    Some(Seen { frame, path: s.outline_path(), circle: s.start == ShapeStart::Circle, ellipse, scale: st.scale })
}

/// The marks for shape `id`'s foci as the world has it now: an ellipse's two, a circle's middle
/// (twice); none for any other shape.
pub fn focus_marks(world: &World, id: ThingId) -> Vec<Vec2> {
    bpe_kit::things::shape::foci(world, id).map(|f| f.to_vec()).unwrap_or_default()
}

/// Screen points per shape unit.
fn units(view: &View, frame: ShapeFrame) -> f32 {
    (frame.size * view.world_scale()).max(1e-6)
}

/// The handle of selected shape `id` under `pos`: a bend handle, a corner, a circle's radius dot
/// or edge, an ellipse's width or height dot or edge, or a side.
pub fn hit_handle(world: &World, view: &View, id: ThingId, pos: Pos2) -> Option<Handle> {
    let s = seen(world, id)?;
    let on_screen = |p: Vec2| view.to_screen(s.frame.to_world(p));
    let near = |p: Vec2, reach: f32| (on_screen(p) - pos).length() <= reach;
    // The middle moves the whole shape, as a band too thin to grab apart from its sides can't.
    let middle = (view.to_screen(s.frame.centre) - pos).length() <= MIDDLE_GRAB;
    if let Some(e) = s.ellipse {
        for which in [0, 1] {
            if near(e.dot(which), DOT_GRAB) {
                return Some(Handle::Axis(id, which));
            }
        }
        if middle {
            return Some(Handle::Body(id));
        }
        let p = view.to_world(pos);
        let (q, _) = world.walls.get(&id)?.outline.nearest(p);
        return ((q - p).length() * view.world_scale() <= EDGE_GRAB).then_some(Handle::Radius(id));
    }
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

/// How a corner, side or bend drag mirrors, worked out when it starts. Shift mirrors across the
/// shape's up-down middle line; a corner or side that is its own mirror there (a square's top or
/// bottom side, a diamond's top corner) mirrors across the left-right middle line instead, onto
/// `flip`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mirror {
    /// The dragged corner or handle sits on the middle line, and the corner or handle across it.
    Point {
        on_line: bool,
        partner: Option<usize>,
        flip: Option<usize>,
    },
    Bend {
        on_line: bool,
        partner: Option<(usize, usize)>,
    },
    /// The side across the middle line (itself when it crosses the line evenly).
    Side {
        partner: Option<usize>,
        flip: Option<usize>,
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
    /// An ellipse's Width and Height then.
    ellipse: Option<SeenEllipse>,
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

/// The other corner nearest the image of corner `i` across the left-right middle line (y → −y),
/// within [`MIRROR_REACH`]. None when corner `i` is on that line or nothing is near its image.
fn flip_partner_point(path: &ShapePath, i: usize) -> Option<usize> {
    let p = path.point(i);
    if p.y.abs() < ON_THE_LINE {
        return None;
    }
    let image = Vec2::new(p.x, -p.y);
    (0..path.side_count())
        .filter(|&j| j != i && path.point(j).y.abs() >= ON_THE_LINE)
        .map(|j| (j, (path.point(j) - image).length()))
        .filter(|&(_, d)| d <= MIRROR_REACH)
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map(|(j, _)| j)
}

/// The side across the left-right middle line from `side`, sharing no corner with it: a square's
/// bottom side for its top.
fn flip_side(path: &ShapePath, side: usize) -> Option<usize> {
    let n = path.side_count();
    let (a, b) = (flip_partner_point(path, side)?, flip_partner_point(path, (side + 1) % n)?);
    (0..n)
        .find(|&t| (t == a && (t + 1) % n == b) || (t == b && (t + 1) % n == a))
        .filter(|&t| t != side && ![t, (t + 1) % n].iter().any(|c| [side, (side + 1) % n].contains(c)))
}

/// The start of a drag on `handle`, pressed at `press` (world pixels). None for handles that
/// aren't a shape's corner, side or bend.
pub fn start(world: &World, handle: Handle, press: Vec2) -> Option<Mould> {
    let s = seen(world, handle.thing())?;
    let path = s.path;
    let (point, mirror) = match handle {
        Handle::Corner(_, i) if i < path.side_count() => {
            let p = path.point(i);
            let on_line = p.x.abs() < ON_THE_LINE;
            let flip = if on_line { flip_partner_point(&path, i) } else { None };
            (p, Mirror::Point { on_line, partner: path.mirror_partner_point(i), flip })
        }
        Handle::Side(_, side) if side < path.side_count() => {
            let partner = mirror_side(&path, side);
            let flip = if partner == Some(side) { flip_side(&path, side) } else { None };
            (path.point(side), Mirror::Side { partner, flip })
        }
        Handle::Bend(_, side, which) => {
            let h = Vec2::from(path.bends.get(side).copied().flatten()?[usize::from(which.min(1))]);
            let partner = path.mirror_partner_bend(side, usize::from(which));
            (h, Mirror::Bend { on_line: h.x.abs() < ON_THE_LINE, partner })
        }
        Handle::Radius(_) => (Vec2::ZERO, Mirror::Nothing),
        Handle::Axis(_, which) => (s.ellipse?.dot(which), Mirror::Nothing),
        _ => return None,
    };
    let offset = press - s.frame.to_world(point);
    Some(Mould { frame: s.frame, scale: s.scale, offset, start: path, mirror, ellipse: s.ellipse })
}

/// `p` across the middle line.
fn mirrored(p: Vec2) -> Vec2 {
    Vec2::new(-p.x, p.y)
}

/// `p` across the left-right middle line.
fn flipped(p: Vec2) -> Vec2 {
    Vec2::new(p.x, -p.y)
}

/// The outline `m` started from, moulded by dragging `handle`'s point to `to` (shape units),
/// mirrored when `shift` is held.
fn moulded(m: &Mould, handle: Handle, to: Vec2, shift: bool) -> ShapePath {
    let mut path = m.start.clone();
    let n = path.side_count();
    match (handle, m.mirror) {
        (Handle::Corner(_, i), Mirror::Point { on_line, partner, flip }) => {
            let to = if shift && on_line { Vec2::new(0.0, to.y) } else { to };
            path.move_point(i, to);
            if let (true, Some(j)) = (shift, partner) {
                path.move_point(j, mirrored(to));
            }
            // A corner on the up-down line mirrors the one across the left-right line.
            if let (true, Some(j)) = (shift, flip) {
                path.move_point(j, flipped(to));
            }
        }
        (Handle::Bend(_, side, which), Mirror::Bend { on_line, partner }) => {
            let to = if shift && on_line { Vec2::new(0.0, to.y) } else { to };
            path.move_bend(side, usize::from(which), to);
            if let (true, Some((s, w))) = (shift, partner) {
                path.move_bend(s, w, mirrored(to));
            }
        }
        (Handle::Side(_, side), Mirror::Side { partner, flip }) => {
            let by = to - m.start.point(side);
            match partner.filter(|_| shift) {
                // A side across the line from itself stays even: it moves up and down only, and
                // the side across the left-right line (a square's bottom for its top) moves the
                // other way.
                Some(t) if t == side => {
                    path.move_side(side, Vec2::new(0.0, by.y));
                    if let Some(f) = flip {
                        path.move_side(f, Vec2::new(0.0, -by.y));
                    }
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
/// `step`), a circle's Size to its distance from the middle, an ellipse's Width or Height to twice
/// the pointer's distance along that axis, or both by its edge in step. The first change gives an
/// even polygon its own corners. A keyframed Size, Width or Height gets a key at `time`; true
/// when one did.
pub fn apply(scene: &mut Scene, reg: &Registry, drag: &Drag, pointer: Vec2, step: Option<f32>, time: f64) -> bool {
    let id = drag.handle.thing();
    let Some(m) = &drag.mould else { return false };
    let snapped = |shown: f32| f64::from(snap_length(shown, step).round().max(SMALLEST));
    match (drag.handle, m.ellipse) {
        (Handle::Radius(_), Some(e)) => {
            // Both, by how much further out the pointer is than where the drag began.
            let k = (pointer - m.frame.centre).length() / (m.offset.length()).max(1e-3);
            let width = set_number(scene, reg, id, "width", snapped(e.size.x * k), time);
            let height = set_number(scene, reg, id, "height", snapped(e.size.y * k), time);
            return width || height;
        }
        (Handle::Radius(_), None) => {
            // The Size that shows that far out, shrunk as it was seen.
            let shown = (pointer - m.frame.centre).length() / m.scale.max(1e-6);
            return set_number(scene, reg, id, "size", snapped(shown), time);
        }
        (Handle::Axis(_, which), Some(e)) => {
            let axis = Vec2::from_angle(m.frame.angle + e.turn).rotate(if which == 0 { Vec2::X } else { Vec2::Y });
            let shown = 2.0 * (pointer - m.offset - m.frame.centre).dot(axis).abs() / m.scale.max(1e-6);
            return set_number(scene, reg, id, if which == 0 { "width" } else { "height" }, snapped(shown), time);
        }
        (Handle::Axis(..), None) => return false,
        _ => {}
    }
    let Some(thing) = scene.things.get_mut(&id) else { return false };
    let to = m.frame.to_shape(snap_point(pointer - m.offset, step));
    let path = moulded(m, drag.handle, to, drag.shift);
    let had = ShapeSettings::from_params(&thing.params).moulded().is_some();
    if path.is_valid() && (had || path != m.start) {
        thing.params.set("path", Value::Path(path));
    }
    false
}

/// Sets shape `id`'s own number `key` to `value`: a key at `time` when it is keyframed (true then).
fn set_number(scene: &mut Scene, reg: &Registry, id: ThingId, key: &str, value: f64, time: f64) -> bool {
    let a = Address::own(id, key);
    if scene.keyed(&a) {
        return scene.set_key(reg, &a, time, Value::Number(value)).is_ok();
    }
    if let Some(thing) = scene.things.get_mut(&id) {
        thing.params.set(key, Value::Number(value));
    }
    false
}

/// What a right-click on a shape was on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuTarget {
    /// A side, `t` along it.
    Side(ThingId, usize, f32),
    Corner(ThingId, usize),
    /// A circle's or an ellipse's edge.
    Circle(ThingId),
}

/// What right-clicking at `pos` on selected shape `id` opens a menu for.
pub fn menu_target(world: &World, view: &View, id: ThingId, pos: Pos2) -> Option<MenuTarget> {
    let s = seen(world, id)?;
    match hit_handle(world, view, id, pos)? {
        Handle::Radius(_) | Handle::Axis(..) => Some(MenuTarget::Circle(id)),
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
        MenuTarget::Circle(id) => {
            let ellipse = outline_of(scene, id).is_some_and(|s| s.start == ShapeStart::Ellipse);
            let help = if ellipse {
                "Makes the ellipse a shape of 16 curved sides you can mould. It is no longer an exact ellipse: \
                 balls fired from a focus will no longer meet perfectly"
            } else {
                "Makes the circle a shape of 8 curved sides you can mould"
            };
            item(ui, true, "Turn into points", help, MenuAction::TurnIntoPoints);
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
        match settings.start {
            ShapeStart::Circle => {}
            // Its 16 curves, turned by its Turn, at its size.
            ShapeStart::Ellipse => thing.params.set("size", Value::Number(f64::from(settings.unit()))),
            ShapeStart::Polygon => return false,
        }
        thing.params.set("start", Value::Choice("polygon".into()));
        thing.params.set("path", Value::Path(settings.outline_path()));
        return true;
    }
    if settings.start != ShapeStart::Polygon {
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

/// Draws shape `id`'s outline and, when it is `selected`, the handle under the pointer or being
/// dragged (`active`): a side lights with its two corners, a corner with its two sides, a circle's
/// edge with its radius dot. The rest of a selected shape shows only its faint outline, its middle
/// ring and the bend handles of its curved sides, so the handles never cover the shape.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    painter: &Painter,
    scene: &Scene,
    world: &World,
    view: &View,
    id: ThingId,
    selected: bool,
    active: Option<Handle>,
    lit: bool,
) {
    let Some(s) = seen(world, id) else { return };
    let on_screen = |p: Vec2| view.to_screen(s.frame.to_world(p));
    let mark = style::colour_of(scene, id);
    let outline: Vec<Pos2> = s.path.flatten(FLATTEN).into_iter().map(on_screen).collect();
    // Lit (it plays into the bus strip under the pointer): in its own colour.
    let line = if lit { Stroke::new(2.0, mark) } else { Stroke::new(1.0, style::FAINT_MARK) };
    painter.add(egui::Shape::closed_line(outline.clone(), line));
    if !selected {
        return;
    }
    let is_active = |h: Handle| active == Some(h);
    let moving = is_active(Handle::Body(id));
    if moving {
        painter.add(egui::Shape::closed_line(outline.clone(), Stroke::new(2.0, style::TEXT_STRONG)));
    }
    // The middle, which moves the shape: a ring like a ball's.
    painter.circle_stroke(
        view.to_screen(s.frame.centre),
        6.0,
        Stroke::new(if moving { 2.0 } else { 1.2 }, if moving { style::TEXT_STRONG } else { mark }),
    );
    // The foci of an ellipse (and a circle's, at its middle): where balls fired from one meet.
    for f in focus_marks(world, id) {
        painter.circle_stroke(view.to_screen(f), 4.0, Stroke::new(1.2, style::TEXT));
    }
    if let Some(e) = s.ellipse {
        if is_active(Handle::Radius(id)) {
            painter.add(egui::Shape::closed_line(outline, Stroke::new(2.0, mark)));
        }
        for which in [0, 1] {
            let lit = is_active(Handle::Axis(id, which)) || is_active(Handle::Radius(id));
            let r = if lit { 5.5 } else { 4.5 };
            let (fill, ring) = if lit { (style::TEXT_STRONG, style::OUTLINE) } else { (style::TEXT_STRONG, mark) };
            painter.circle(on_screen(e.dot(which)), r, fill, Stroke::new(1.5, ring));
        }
        return;
    }
    if s.circle {
        if is_active(Handle::Radius(id)) {
            let ring: Vec<Pos2> = s.path.flatten(FLATTEN).into_iter().map(on_screen).collect();
            painter.add(egui::Shape::closed_line(ring, Stroke::new(2.0, mark)));
            painter.circle(on_screen(Vec2::X), 5.5, style::TEXT_STRONG, Stroke::new(1.5, style::OUTLINE));
        }
        return;
    }
    let path = &s.path;
    let n = path.side_count();
    // The bend handles, dashed to their corners: they sit off the shape, so they always show.
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
    // The side and corners the pointer is on.
    let (sides, corners): (Vec<usize>, Vec<usize>) = match active {
        Some(Handle::Side(h, side)) if h == id && side < n => (vec![side], vec![side, (side + 1) % n]),
        Some(Handle::Corner(h, i)) if h == id && i < n => (vec![(i + n - 1) % n, i], vec![i]),
        Some(Handle::Bend(h, side, _)) if h == id && side < n => (vec![side], vec![]),
        _ => (vec![], vec![]),
    };
    for side in sides {
        let lit = is_active(Handle::Side(id, side));
        let line: Vec<Pos2> =
            (0..=FLATTEN).map(|k| on_screen(path.side_point(side, k as f32 / FLATTEN as f32))).collect();
        painter.add(egui::Shape::line(line, Stroke::new(if lit { 2.5 } else { 2.0 }, mark)));
    }
    for i in corners {
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
        let kit = studio.kit.clone();
        apply(&mut studio.scene, &kit.registry, &drag, press + by, step, 0.0);
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
        // The top side is its own mirror: it moves up and down only, and the bottom side the
        // other way.
        let mut studio = polygon(6);
        drag(&mut studio, Handle::Side(SHAPE, 0), corner(6, 0), Vec2::new(40.0, 20.0), true, None);
        let p = path(&studio);
        for k in [0, 1] {
            assert!(close(p.point(k), even.point(k) + Vec2::new(0.0, 0.05)), "corner {k}");
        }
        for k in [3, 4] {
            assert!(close(p.point(k), even.point(k) + Vec2::new(0.0, -0.05)), "corner {k}");
        }
        for k in [2, 5] {
            assert_eq!(p.point(k), even.point(k), "corner {k} stays");
        }
    }

    #[test]
    fn shift_on_a_squares_top_or_bottom_mirrors_the_other() {
        // A square's corners: 0 top left, 1 top right, 2 bottom right, 3 bottom left.
        let even = ShapePath::even(4);
        // Dragging the top side up 20 px pulls the bottom side down 20 px.
        let mut studio = polygon(4);
        drag(&mut studio, Handle::Side(SHAPE, 0), corner(4, 0), Vec2::new(30.0, -20.0), true, None);
        let p = path(&studio);
        for (k, d) in [(0, -0.05), (1, -0.05), (2, 0.05), (3, 0.05)] {
            assert!(close(p.point(k), even.point(k) + Vec2::new(0.0, d)), "corner {k}: {}", p.point(k));
        }
        // And the bottom side pushes the top the other way.
        let mut studio = polygon(4);
        drag(&mut studio, Handle::Side(SHAPE, 2), corner(4, 2), Vec2::new(0.0, -40.0), true, None);
        let p = path(&studio);
        for (k, d) in [(0, 0.1), (1, 0.1), (2, -0.1), (3, -0.1)] {
            assert!(close(p.point(k), even.point(k) + Vec2::new(0.0, d)), "corner {k}: {}", p.point(k));
        }
        // Without Shift only the dragged side moves, freely.
        let mut studio = polygon(4);
        drag(&mut studio, Handle::Side(SHAPE, 0), corner(4, 0), Vec2::new(40.0, -20.0), false, None);
        let p = path(&studio);
        assert!(close(p.point(0), even.point(0) + Vec2::new(0.1, -0.05)));
        assert_eq!(p.point(2), even.point(2));
        // The left and right sides still mirror each other as before.
        let mut studio = polygon(4);
        drag(&mut studio, Handle::Side(SHAPE, 1), corner(4, 1), Vec2::new(40.0, 0.0), true, None);
        let p = path(&studio);
        for (k, d) in [(1, 0.1), (2, 0.1), (3, -0.1), (0, -0.1)] {
            assert!(close(p.point(k), even.point(k) + Vec2::new(d, 0.0)), "corner {k}: {}", p.point(k));
        }
    }

    #[test]
    fn shift_on_a_diamonds_top_corner_mirrors_the_bottom_one() {
        let mut studio = studio_with(|p| {
            p.set("start", Value::Choice("polygon".into()));
            p.set("sides", Value::Int(4));
            p.set("size", Value::Number(400.0));
            p.set(
                "path",
                Value::Path(ShapePath {
                    points: vec![[0.0, -1.0], [1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]],
                    bends: vec![None; 4],
                }),
            );
        });
        drag(&mut studio, Handle::Corner(SHAPE, 0), Vec2::new(0.0, -400.0), Vec2::new(20.0, -40.0), true, None);
        let p = path(&studio);
        assert!(close(p.point(0), Vec2::new(0.0, -1.1)), "{}", p.point(0));
        assert!(close(p.point(2), Vec2::new(0.0, 1.1)), "{}", p.point(2));
        assert_eq!(p.point(1), Vec2::new(1.0, 0.0));
        assert_eq!(p.point(3), Vec2::new(-1.0, 0.0));
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
    fn dragging_a_keyed_circles_edge_keys_its_size_at_the_playhead() {
        let mut studio = Studio::headless();
        let kit = studio.kit.clone();
        let size = Address::own(SHAPE, "size");
        studio.scene.set_key(&kit.registry, &size, 0.0, Value::Number(508.0)).unwrap();
        let world = studio.playback.world().clone();
        let handle = Handle::Radius(SHAPE);
        let press = Vec2::new(508.0, 0.0);
        let drag = Drag { mould: start(&world, handle, press), ..Drag::new(handle, Vec2::ZERO) };
        let keyed = apply(&mut studio.scene, &kit.registry, &drag, Vec2::new(300.0, 0.0), None, 2.0);
        assert!(keyed, "a keyed edit, so playing pauses");
        let keys = &studio.scene.tracks[&size].keys;
        assert_eq!(keys.iter().map(|k| k.time).collect::<Vec<_>>(), [0.0, 2.0]);
        assert_eq!(studio.scene.value_at_time(&kit.registry, &size, 2.0), Some(Value::Number(300.0)));
        // Not keyed: the plain value, and no key.
        studio.scene.tracks.clear();
        assert!(!apply(&mut studio.scene, &kit.registry, &drag, Vec2::new(250.0, 0.0), None, 2.0));
        assert_eq!(studio.scene.things[&SHAPE].params.get("size"), Some(&Value::Number(250.0)));
        assert!(studio.scene.tracks.is_empty());
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
        let kit = studio.kit.clone();
        apply(&mut studio.scene, &kit.registry, &drag, press + by, None, 1.0);
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

    /// A 900 × 1300 ellipse at the middle, turned `turn` degrees.
    fn ellipse(turn: f64) -> Studio {
        studio_with(|p| {
            p.set("start", Value::Choice("ellipse".into()));
            p.set("width", Value::Number(900.0));
            p.set("height", Value::Number(1300.0));
            p.set("turn", Value::Number(turn));
        })
    }

    fn number(studio: &Studio, key: &str) -> f64 {
        match studio.scene.things[&SHAPE].params.get(key) {
            Some(Value::Number(x)) => *x,
            other => panic!("{key}: {other:?}"),
        }
    }

    #[test]
    fn an_ellipse_shows_width_and_height_dots() {
        let studio = ellipse(0.0);
        let world = studio.playback.world();
        let v = view();
        let s = seen(world, SHAPE).unwrap();
        assert_eq!(s.frame.size, 650.0);
        let screen = |p: Vec2| v.to_screen(p);
        assert_eq!(hit_handle(world, &v, SHAPE, screen(Vec2::new(450.0, 0.0))), Some(Handle::Axis(SHAPE, 0)));
        assert_eq!(hit_handle(world, &v, SHAPE, screen(Vec2::new(0.0, -650.0))), Some(Handle::Axis(SHAPE, 1)));
        let edge = Vec2::from_angle(-0.6) * 1.0;
        let on = world.walls[&SHAPE].outline.point(edge.to_angle());
        assert_eq!(hit_handle(world, &v, SHAPE, screen(on)), Some(Handle::Radius(SHAPE)));
        assert_eq!(hit_handle(world, &v, SHAPE, screen(Vec2::ZERO)), Some(Handle::Body(SHAPE)));
        assert_eq!(hit_handle(world, &v, SHAPE, screen(Vec2::new(100.0, 100.0))), None);
        // Turned, the dots turn with it.
        let studio = ellipse(90.0);
        let world = studio.playback.world();
        assert_eq!(hit_handle(world, &v, SHAPE, screen(Vec2::new(0.0, 450.0))), Some(Handle::Axis(SHAPE, 0)));
        assert_eq!(hit_handle(world, &v, SHAPE, screen(Vec2::new(650.0, 0.0))), Some(Handle::Axis(SHAPE, 1)));
    }

    #[test]
    fn dragging_an_axis_dot_sets_width_or_height() {
        let mut studio = ellipse(0.0);
        drag(&mut studio, Handle::Axis(SHAPE, 0), Vec2::new(450.0, 0.0), Vec2::new(50.0, 30.0), false, None);
        assert_eq!(number(&studio, "width"), 1000.0);
        assert_eq!(number(&studio, "height"), 1300.0);
        let mut studio = ellipse(0.0);
        drag(&mut studio, Handle::Axis(SHAPE, 1), Vec2::new(0.0, -650.0), Vec2::new(0.0, 100.0), false, None);
        assert_eq!(number(&studio, "height"), 1100.0);
        // A keyed Height gets a key at the playhead instead.
        let mut studio = ellipse(0.0);
        let kit = studio.kit.clone();
        let a = Address::own(SHAPE, "height");
        studio.scene.set_key(&kit.registry, &a, 0.0, Value::Number(1300.0)).unwrap();
        let world = studio.playback.world().clone();
        let handle = Handle::Axis(SHAPE, 1);
        let drag = Drag { mould: start(&world, handle, Vec2::new(0.0, -650.0)), ..Drag::new(handle, Vec2::ZERO) };
        assert!(apply(&mut studio.scene, &kit.registry, &drag, Vec2::new(0.0, -700.0), None, 2.0));
        assert_eq!(studio.scene.tracks[&a].keys.len(), 2);
    }

    #[test]
    fn dragging_an_ellipse_edge_scales_both() {
        let mut studio = ellipse(0.0);
        drag(&mut studio, Handle::Radius(SHAPE), Vec2::new(0.0, 650.0), Vec2::new(0.0, 65.0), false, None);
        assert_eq!((number(&studio, "width"), number(&studio, "height")), (990.0, 1430.0));
    }

    #[test]
    fn turn_into_points_makes_16_curved_sides() {
        let mut studio = ellipse(30.0);
        assert!(act(&mut studio.scene, MenuTarget::Circle(SHAPE), MenuAction::TurnIntoPoints));
        let s = ShapeSettings::from_params(&studio.scene.things[&SHAPE].params);
        assert_eq!((s.start, s.size), (ShapeStart::Polygon, 650.0));
        let p = path(&studio);
        assert_eq!((p.side_count(), p.curve_count()), (16, 16));
        let mut want = ShapeSettings::ellipse(900.0, 1300.0, 8.0);
        want.turn = 30.0;
        assert_eq!(p, want.outline_path());
        // A polygon has nothing to turn into points.
        let mut studio = polygon(6);
        assert!(!act(&mut studio.scene, MenuTarget::Circle(SHAPE), MenuAction::TurnIntoPoints));
    }

    #[test]
    fn foci_marks_show_for_an_ellipse_and_a_circle() {
        let studio = ellipse(0.0);
        let marks = focus_marks(studio.playback.world(), SHAPE);
        let c = (650f32 * 650.0 - 450.0 * 450.0).sqrt();
        assert_eq!(marks.len(), 2);
        assert!((marks[0] - Vec2::new(0.0, -c)).length() < 1e-2 && (marks[1] - Vec2::new(0.0, c)).length() < 1e-2);
        let circle = studio_with(|_| {});
        assert_eq!(focus_marks(circle.playback.world(), SHAPE).len(), 2);
        assert!(focus_marks(polygon(6).playback.world(), SHAPE).is_empty());
    }
}
