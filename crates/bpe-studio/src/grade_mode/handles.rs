//! A gradient's handles on the preview in Grade mode: the selected mask's selected (or first)
//! Linear or Radial part, drawn where it is (its first place: on the screen, in the world, or on
//! the thing or ball it follows). A Linear Gradient shows its three lines (full effect, middle,
//! none), a pin that moves it and a dot that turns it; dragging an outer line changes Feather. A
//! Radial Gradient shows its edge and where its fade starts, a pin, four size dots and a turn dot.
//!
//! Geometry is in screen units (video pixels from the picture's middle, y down), as the grade
//! draws. A drag works from where the part was when it started, so the picture catching up
//! never moves the handles under the pointer; each drag is one undo step.

use super::cx::{GradeEdit, full_key};
use crate::app::Studio;
use crate::style;
use bpe_core::{LayerId, Slot, SlotId, ThingId, Value};
use bpe_gfx::{GradeLinear, GradePlace, GradeRadial, GradeShape, GradeStack};
use bpe_kit::modules::grade::{self, LINEAR, RADIAL};
use eframe::egui::{self, CursorIcon, Pos2, Rect, Stroke, Ui};
use std::collections::BTreeMap;

/// How far from a handle (preview pixels) a press still takes it.
const REACH: f32 = 8.0;
/// How far the turn dot is beyond the shape (preview pixels).
const TURN_GAP: f32 = 36.0;

/// What a drag holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Handle {
    /// The pin (or a Linear's middle line): moves it.
    Move,
    /// The turn dot.
    Turn,
    /// A Linear's outer line: its Feather.
    Feather,
    /// A Radial's size dot: 0 its width, 1 its height.
    Size(usize),
}

/// A handle being dragged.
#[derive(Clone, Debug, PartialEq)]
pub struct Grab {
    pub part: LayerId,
    pub handle: Handle,
    /// Where the press was, in screen units.
    pub press: [f32; 2],
    /// Where the pointer was last, so a still pointer asks for nothing.
    pub last: [f32; 2],
    /// The part as it was when the drag started.
    pub shape: Shape,
    pub place: GradePlace,
}

/// A gradient with handles.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Linear(GradeLinear),
    Radial(GradeRadial),
}

/// The part of `mask` whose handles show: the one picked in the column while it is there, else
/// its first gradient.
pub fn handle_part(card: &Slot, picked: &BTreeMap<LayerId, LayerId>, mask: LayerId) -> Option<LayerId> {
    let parts = grade::parts_of(card, mask);
    let gradient = |id: LayerId| parts.iter().any(|l| l.id == id && (l.kind == LINEAR || l.kind == RADIAL));
    picked
        .get(&mask)
        .copied()
        .filter(|p| gradient(*p))
        .or_else(|| parts.iter().find(|l| l.kind == LINEAR || l.kind == RADIAL).map(|l| l.id))
}

/// Part `part` of mask `mask` as the preview last drew it, on its first place.
pub fn shown(stack: &GradeStack, mask: LayerId, part: LayerId) -> Option<(Shape, GradePlace)> {
    let m = stack.masks.iter().find(|m| m.id == mask.0)?;
    let p = m.area.as_ref()?.parts.iter().find(|p| p.id == part.0)?;
    match &p.shape {
        GradeShape::Linear(l) => {
            let place = *l.places.first().filter(|p| bpe_gfx::grade::maths::place_ok(p))?;
            Some((Shape::Linear(l.clone()), place))
        }
        GradeShape::Radial(r) => {
            let place = *r.places.first().filter(|p| bpe_gfx::grade::maths::place_ok(p))?;
            Some((Shape::Radial(r.clone()), place))
        }
        _ => None,
    }
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn mul(a: [f32; 2], k: f32) -> [f32; 2] {
    [a[0] * k, a[1] * k]
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

fn way(radians: f32) -> [f32; 2] {
    let (s, c) = radians.sin_cos();
    [c, s]
}

/// An angle in degrees, as −180..180.
fn wrapped(degrees: f32) -> f32 {
    let d = (degrees + 180.0).rem_euclid(360.0) - 180.0;
    if d.is_finite() { d } else { 0.0 }
}

/// A screen point back in a place's own units.
fn unplaced(place: &GradePlace, p: [f32; 2]) -> [f32; 2] {
    let d = sub(p, place.at);
    let (s, c) = place.turn.sin_cos();
    let local = [d[0] * c + d[1] * s, -d[0] * s + d[1] * c];
    mul(local, 1.0 / place.scale)
}

/// A Linear's middle, the way it fades (unit) and its Feather, on the screen.
fn linear_frame(l: &GradeLinear, place: &GradePlace) -> ([f32; 2], [f32; 2], f32) {
    let c = place.point(l.x, l.y);
    let d = way(l.angle.to_radians() + place.turn);
    (c, d, (l.feather.max(1.0) * place.scale).max(1e-3))
}

/// A Radial's middle, its across and down ways (unit), and how far its edge is along each.
fn radial_frame(r: &GradeRadial, place: &GradePlace) -> ([f32; 2], [f32; 2], [f32; 2], [f32; 2]) {
    let c = place.point(r.x, r.y);
    let t = r.angle.to_radians() + place.turn;
    let (u, v) = (way(t), way(t + std::f32::consts::FRAC_PI_2));
    (c, u, v, reach(r, place))
}

/// How far a Radial's edge is from its middle along its across and down ways (screen units).
fn reach(r: &GradeRadial, place: &GradePlace) -> [f32; 2] {
    let half = [0.5 * r.width.max(1.0) * place.scale, 0.5 * r.height.max(1.0) * place.scale];
    let short = half[0].min(half[1]).max(1e-3);
    let round = (r.roundness / 100.0).clamp(-1.0, 1.0).max(0.0);
    [0, 1].map(|i| 1.0 / ((1.0 - round) / half[i].max(1e-3) + round / short))
}

/// A Radial's outline at `level` (1 its edge, less where the fade starts), as screen points.
fn radial_outline(r: &GradeRadial, place: &GradePlace, level: f32) -> Vec<[f32; 2]> {
    let (c, u, v, a) = radial_frame(r, place);
    let round = (r.roundness / 100.0).clamp(-1.0, 1.0);
    let e = 2.0 - 6.0 * round.min(0.0);
    (0..=96)
        .map(|i| {
            let (s, co) = (i as f32 / 96.0 * std::f32::consts::TAU).sin_cos();
            let q = [co.signum() * co.abs().powf(2.0 / e), s.signum() * s.abs().powf(2.0 / e)];
            add(c, add(mul(u, q[0] * a[0] * level), mul(v, q[1] * a[1] * level)))
        })
        .collect()
}

/// The handles' dots: what each holds and where it is (screen units); `px` is one preview pixel
/// in screen units.
pub fn dots(shape: &Shape, place: &GradePlace, px: f32) -> Vec<(Handle, [f32; 2])> {
    match shape {
        Shape::Linear(l) => {
            let (c, d, _) = linear_frame(l, place);
            let n = [-d[1], d[0]];
            vec![(Handle::Move, c), (Handle::Turn, add(c, mul(n, TURN_GAP * 2.0 * px)))]
        }
        Shape::Radial(r) => {
            let (c, u, v, a) = radial_frame(r, place);
            vec![
                (Handle::Move, c),
                (Handle::Size(0), add(c, mul(u, a[0]))),
                (Handle::Size(0), sub(c, mul(u, a[0]))),
                (Handle::Size(1), add(c, mul(v, a[1]))),
                (Handle::Size(1), sub(c, mul(v, a[1]))),
                (Handle::Turn, sub(c, mul(v, a[1] + TURN_GAP * px))),
            ]
        }
    }
}

/// The handle at screen point `p`, if any: dots first, then a Linear's outer lines, then its
/// middle line.
pub fn hit(shape: &Shape, place: &GradePlace, p: [f32; 2], px: f32) -> Option<Handle> {
    let near = REACH * px;
    let closest = dots(shape, place, px)
        .into_iter()
        .map(|(h, at)| (h, sub(p, at)))
        .map(|(h, d)| (h, dot(d, d).sqrt()))
        .filter(|(_, d)| *d <= near)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((h, _)) = closest {
        return Some(h);
    }
    let Shape::Linear(l) = shape else { return None };
    let (c, d, f) = linear_frame(l, place);
    let along = dot(sub(p, c), d);
    if (along.abs() - 0.5 * f).abs() <= near {
        Some(Handle::Feather)
    } else if along.abs() <= near {
        Some(Handle::Move)
    } else {
        None
    }
}

/// The settings a drag of `grab` to screen point `p` asks for.
pub fn dragged(grab: &Grab, p: [f32; 2]) -> Vec<(&'static str, f32)> {
    let place = &grab.place;
    match (&grab.shape, grab.handle) {
        (Shape::Linear(GradeLinear { x, y, .. }) | Shape::Radial(GradeRadial { x, y, .. }), Handle::Move) => {
            let start = place.point(*x, *y);
            let to = unplaced(place, add(start, sub(p, grab.press)));
            vec![("x", to[0]), ("y", to[1])]
        }
        (Shape::Linear(l), Handle::Turn) => {
            let (c, _, _) = linear_frame(l, place);
            let d = sub(p, c);
            // The dot sits a quarter turn on from the way the effect fades.
            let t = d[1].atan2(d[0]) - std::f32::consts::FRAC_PI_2 - place.turn;
            vec![("angle", wrapped(t.to_degrees()))]
        }
        (Shape::Linear(l), Handle::Feather) => {
            let (c, d, _) = linear_frame(l, place);
            let f = 2.0 * dot(sub(p, c), d).abs() / place.scale;
            vec![("feather", f.max(1.0))]
        }
        (Shape::Radial(r), Handle::Turn) => {
            let (c, ..) = radial_frame(r, place);
            let d = sub(p, c);
            // The dot sits a quarter turn back from across.
            let t = d[1].atan2(d[0]) + std::f32::consts::FRAC_PI_2 - place.turn;
            vec![("angle", wrapped(t.to_degrees()))]
        }
        (Shape::Radial(r), Handle::Size(axis)) => {
            let (c, u, v, a) = radial_frame(r, place);
            let along = dot(sub(p, c), if axis == 0 { u } else { v }).abs();
            let size = if axis == 0 { r.width } else { r.height };
            // The edge follows the pointer (exactly for a plain ellipse; in proportion when rounder).
            let new = (size.max(1.0) * along / a[axis].max(1e-3)).max(1.0);
            vec![(if axis == 0 { "width" } else { "height" }, new)]
        }
        _ => Vec::new(),
    }
}

/// The handles in Grade mode: drawn, and dragged. `view_scale` is preview pixels per video pixel.
pub fn preview(ui: &mut Ui, response: &egui::Response, rect: Rect, view_scale: f32, studio: &mut Studio) {
    let found = (|| {
        let card = studio.scene.card(ThingId::ROOT, SlotId::Grade).filter(|c| c.kind == grade::GRADE)?;
        let mask = studio.grade.mask.filter(|m| grade::masks_of(card).iter().any(|l| l.id == *m))?;
        let part = handle_part(card, &studio.grade.part, mask)?;
        let (shape, place) = shown(&studio.grade.shown.borrow(), mask, part)?;
        Some((part, shape, place))
    })();
    let Some((part, shape, place)) = found else {
        studio.grade.handle = None;
        return;
    };
    let px = 1.0 / view_scale.max(1e-6);
    let units = |p: Pos2| [(p.x - rect.center().x) * px, (p.y - rect.center().y) * px];
    if studio.grade.handle.as_ref().is_some_and(|g| g.part != part) {
        studio.grade.handle = None;
    }

    // A drag starts on a handle.
    if studio.grade.handle.is_none()
        && response.drag_started_by(egui::PointerButton::Primary)
        && let Some(press) = ui.input(|i| i.pointer.press_origin())
        && let Some(handle) = hit(&shape, &place, units(press), px)
    {
        let at = units(press);
        studio.grade.handle = Some(Grab { part, handle, press: at, last: at, shape: shape.clone(), place });
        // Each drag is its own undo step.
        studio.panels.inspector.own_step = true;
    }
    if let Some(grab) = studio.grade.handle.as_mut() {
        if let Some(p) = response.interact_pointer_pos().map(units)
            && p != grab.last
        {
            grab.last = p;
            let edits = dragged(grab, p)
                .into_iter()
                .map(|(key, v)| GradeEdit::Set(full_key(Some(part), key), Value::Number(v as f64)))
                .collect();
            super::apply(studio, edits);
        }
        if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
            studio.grade.handle = None;
        }
    }

    // Draw where the part is now (the drag's start until the picture catches up is fine: the
    // picture is drawn from the same values a frame later).
    let held = studio.grade.handle.as_ref().map(|g| g.handle);
    let hovered = response.hover_pos().and_then(|p| hit(&shape, &place, units(p), px));
    match held.or(hovered) {
        Some(Handle::Move) => {
            ui.ctx().set_cursor_icon(if held.is_some() { CursorIcon::Grabbing } else { CursorIcon::Grab })
        }
        Some(_) => ui.ctx().set_cursor_icon(CursorIcon::Crosshair),
        None => {}
    }
    let at = |p: [f32; 2]| rect.center() + egui::vec2(p[0], p[1]) * view_scale;
    let painter = ui.painter_at(rect);
    let line = |a: [f32; 2], b: [f32; 2], strong: bool| {
        let width = if strong { 1.5 } else { 1.0 };
        painter.line_segment([at(a), at(b)], Stroke::new(width + 2.0, style::OUTLINE));
        painter.line_segment([at(a), at(b)], Stroke::new(width, style::TEXT_STRONG));
    };
    match &shape {
        Shape::Linear(l) => {
            let (c, d, f) = linear_frame(l, &place);
            let n = [-d[1], d[0]];
            let long = (rect.width() + rect.height()) * 2.0 * px;
            for k in [-0.5, 0.0, 0.5] {
                let mid = add(c, mul(d, k * f));
                line(sub(mid, mul(n, long)), add(mid, mul(n, long)), k == 0.0);
            }
            line(c, add(c, mul(n, TURN_GAP * 2.0 * px)), false);
        }
        Shape::Radial(r) => {
            let feather = (r.feather / 100.0).clamp(0.0, 1.0);
            for (level, strong) in [(1.0, true), (1.0 - feather, false)] {
                if level <= 0.0 {
                    continue;
                }
                let points = radial_outline(r, &place, level);
                let shown: Vec<Pos2> = points.into_iter().map(at).collect();
                let width = if strong { 1.5 } else { 1.0 };
                painter.add(egui::Shape::line(shown.clone(), Stroke::new(width + 2.0, style::OUTLINE)));
                painter.add(egui::Shape::line(shown, Stroke::new(width, style::TEXT_STRONG)));
            }
            let (c, _, v, a) = radial_frame(r, &place);
            line(sub(c, mul(v, a[1])), sub(c, mul(v, a[1] + TURN_GAP * px)), false);
        }
    }
    for (handle, p) in dots(&shape, &place, px) {
        let lit = held.or(hovered) == Some(handle);
        let radius = if handle == Handle::Move { 6.0 } else { 4.5 };
        let fill = if lit { style::TEXT_STRONG } else { style::SLIDER_FILL };
        painter.circle(at(p), radius, fill, Stroke::new(1.5, style::OUTLINE));
        if handle == Handle::Move {
            painter.circle_filled(at(p), 2.0, style::OUTLINE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radial() -> GradeRadial {
        GradeRadial {
            x: 100.0,
            y: -50.0,
            width: 400.0,
            height: 200.0,
            angle: 0.0,
            roundness: 0.0,
            feather: 50.0,
            places: vec![GradePlace::SCREEN],
        }
    }

    fn linear() -> GradeLinear {
        GradeLinear { x: 0.0, y: -400.0, angle: 90.0, feather: 400.0, places: vec![GradePlace::SCREEN] }
    }

    fn grab(shape: Shape, place: GradePlace, handle: Handle, press: [f32; 2]) -> Grab {
        Grab { part: LayerId(9), handle, press, last: press, shape, place }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn dragging_a_radials_pin_moves_its_middle_by_the_pointers_move() {
        let g = grab(Shape::Radial(radial()), GradePlace::SCREEN, Handle::Move, [100.0, -50.0]);
        let out = dragged(&g, [130.0, 10.0]);
        assert!(close(out[0].1, 130.0) && close(out[1].1, 10.0), "{out:?}");
        // Following a turned, bigger thing: the move is in its own units.
        let place = GradePlace { at: [500.0, 0.0], turn: std::f32::consts::FRAC_PI_2, scale: 2.0 };
        let start = place.point(100.0, -50.0);
        let g = grab(Shape::Radial(radial()), place, Handle::Move, start);
        let out = dragged(&g, add(start, [0.0, 20.0]));
        // Down on the screen is across (x) for a quarter turn, halved by the scale.
        assert!(close(out[0].1, 110.0) && close(out[1].1, -50.0), "{out:?}");
    }

    #[test]
    fn a_size_dot_puts_the_edge_under_the_pointer() {
        let r = radial();
        let place = GradePlace::SCREEN;
        let g = grab(Shape::Radial(r.clone()), place, Handle::Size(0), [300.0, -50.0]);
        let out = dragged(&g, [400.0, -50.0]);
        assert_eq!(out[0].0, "width");
        assert!(close(out[0].1, 600.0), "{out:?}");
        let g = grab(Shape::Radial(r), place, Handle::Size(1), [100.0, 50.0]);
        let out = dragged(&g, [100.0, -250.0]);
        assert!(close(out[0].1, 400.0), "{out:?}");
    }

    #[test]
    fn turning_follows_the_pointer_round_the_middle() {
        // A Radial's turn dot starts straight above: dragging it to the right turns it a quarter.
        let g = grab(Shape::Radial(radial()), GradePlace::SCREEN, Handle::Turn, [100.0, -200.0]);
        let out = dragged(&g, [300.0, -50.0]);
        assert!(close(out[0].1, 90.0), "{out:?}");
        // A Linear fading down has its turn dot to the left (a quarter turn on from down).
        let dot = dots(&Shape::Linear(linear()), &GradePlace::SCREEN, 1.0)[1].1;
        assert!(dot[0] < -1.0 && close(dot[1], -400.0), "{dot:?}");
        let g = grab(Shape::Linear(linear()), GradePlace::SCREEN, Handle::Turn, dot);
        // Pulled straight up, the dot is a quarter turn on from left: it fades to the left.
        let out = dragged(&g, [0.0, -600.0]);
        assert!(close(out[0].1, 180.0) || close(out[0].1, -180.0), "{out:?}");
    }

    #[test]
    fn a_linears_outer_lines_set_its_feather_and_its_middle_line_moves_it() {
        let l = Shape::Linear(linear());
        let place = GradePlace::SCREEN;
        assert_eq!(hit(&l, &place, [300.0, -600.0], 1.0), Some(Handle::Feather));
        assert_eq!(hit(&l, &place, [300.0, -200.0], 1.0), Some(Handle::Feather));
        assert_eq!(hit(&l, &place, [300.0, -400.0], 1.0), Some(Handle::Move));
        assert_eq!(hit(&l, &place, [0.0, -400.0], 1.0), Some(Handle::Move));
        assert_eq!(hit(&l, &place, [300.0, -300.0], 1.0), None);
        let g = grab(l, place, Handle::Feather, [300.0, -200.0]);
        let out = dragged(&g, [300.0, -100.0]);
        assert!(close(out[0].1, 600.0), "{out:?}");
    }

    #[test]
    fn a_radials_outline_reaches_its_size_dots() {
        let r = radial();
        let edge = radial_outline(&r, &GradePlace::SCREEN, 1.0);
        assert!(close(edge[0][0], 300.0) && close(edge[0][1], -50.0), "{:?}", edge[0]);
        assert!(close(edge[24][0], 100.0) && close(edge[24][1], 50.0), "{:?}", edge[24]);
        let inner = radial_outline(&r, &GradePlace::SCREEN, 0.5);
        assert!(close(inner[0][0], 200.0), "{:?}", inner[0]);
        let squarer = GradeRadial { roundness: -100.0, ..r };
        let corner = radial_outline(&squarer, &GradePlace::SCREEN, 1.0)[12];
        // A rounder rectangle reaches further into its corners than an ellipse.
        assert!(corner[0] - 100.0 > 0.75 * 200.0, "{corner:?}");
    }

    #[test]
    fn the_handles_part_is_the_picked_gradient_or_the_first() {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_scene(reg);
        let mask = grade::add_mask(&mut scene, reg, grade::LUMINANCE_RANGE).unwrap();
        let card = |scene: &bpe_core::Scene| scene.card(ThingId::ROOT, SlotId::Grade).unwrap().clone();
        assert_eq!(handle_part(&card(&scene), &BTreeMap::new(), mask), None, "a luminance range has no handles");
        let radial = grade::add_part(&mut scene, reg, mask, RADIAL, grade::Combine::Add).unwrap();
        let linear = grade::add_part(&mut scene, reg, mask, LINEAR, grade::Combine::Add).unwrap();
        let card = || card(&scene);
        assert_eq!(handle_part(&card(), &BTreeMap::new(), mask), Some(radial));
        assert_eq!(handle_part(&card(), &BTreeMap::from([(mask, linear)]), mask), Some(linear));
        let luminance = grade::parts_of(&card(), mask)[0].id;
        assert_eq!(handle_part(&card(), &BTreeMap::from([(mask, luminance)]), mask), Some(radial));
    }
}
