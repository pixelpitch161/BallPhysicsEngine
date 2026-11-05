//! The Emit tab's mini view: the emitter in the middle of a well, one round's directions as rays
//! in the colours its balls start in, the Start ring or line, the catch circle dashed, and what
//! can be dragged: the square sets the Aim direction, a Fan's edge dots its angle and a ray's end
//! the Speed (a ray is how far a ball flies in half a second).

use bpe_core::{Color, SlotId, Value};
use bpe_kit::things::emitter::{Catches, Directions, EmitterSettings, StartFrom, draw::round_colours, fire};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Ui};
use glam::Vec2;

use super::super::{Edit, Inspector, OwnCx};
use crate::style;

/// The view's height, in points.
pub const HEIGHT: f32 = 170.0;
/// How long a ray is: how far a ball flies in this many seconds.
pub const RAY_SECONDS: f32 = 0.5;
/// How close, in points, the pointer must be to grab a handle.
const GRAB: f32 = 10.0;
/// The aim square's half size, in points.
const SQUARE: f32 = 4.5;
/// The most rays drawn: a big round shows as many evenly picked.
const MOST_RAYS: u32 = 120;

/// What is being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MiniDrag {
    Aim,
    /// One of a fan's edges: the far one (true) or the near one.
    FanEdge(bool),
    /// The end of ball `k`'s ray.
    Speed(u32),
}

/// How the view maps the emitter's space (world pixels round its middle, unturned) to points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MiniMap {
    pub centre: Pos2,
    /// Points per world pixel.
    pub scale: f32,
}

impl MiniMap {
    pub fn to_view(self, v: Vec2) -> Pos2 {
        self.centre + egui::vec2(v.x, v.y) * self.scale
    }

    pub fn to_world(self, p: Pos2) -> Vec2 {
        Vec2::new(p.x - self.centre.x, p.y - self.centre.y) / self.scale.max(1e-6)
    }

    /// The direction (radians, clockwise from +x) of `p` from the middle.
    pub fn angle_of(self, p: Pos2) -> f32 {
        let v = self.to_world(p);
        v.y.atan2(v.x)
    }
}

/// How far a ray reaches (world pixels) for settings `s`.
pub fn ray_length(s: &EmitterSettings) -> f32 {
    s.speed.max(0.0) * RAY_SECONDS
}

/// The furthest the view must show from the middle, in world pixels.
fn reach(s: &EmitterSettings) -> f32 {
    let start = match s.start_from {
        StartFrom::Point => 0.0,
        StartFrom::Ring => s.start_radius.max(0.0),
        StartFrom::Line => s.start_length.max(0.0) * 0.5,
    };
    let catch = if s.catches == Catches::Nothing { 0.0 } else { s.catch_size.max(0.0) };
    (start + ray_length(s)).max(catch).max(start).max(20.0)
}

/// The map for a view `rect` showing settings `s`.
pub fn map_for(rect: Rect, s: &EmitterSettings) -> MiniMap {
    let room = (rect.width().min(rect.height()) * 0.5 - 14.0).max(10.0);
    MiniMap { centre: rect.center(), scale: room / reach(s) }
}

/// One ray: where it starts and ends (world pixels round the middle), and ball `k`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub k: u32,
    pub from: Vec2,
    pub to: Vec2,
}

/// One round's rays, at most [`MOST_RAYS`] of them picked evenly.
pub fn rays(s: &EmitterSettings) -> Vec<Ray> {
    let n = fire::first_count(s).max(1);
    let shown = n.min(MOST_RAYS);
    (0..shown)
        .map(|j| {
            let k = if shown == n { j } else { (j as u64 * n as u64 / shown as u64) as u32 };
            let way = fire::direction(s, k, n, 1, 0, 0);
            let from = fire::start_offset(s, k, n, way);
            Ray { k, from, to: from + Vec2::from_angle(way) * ray_length(s) }
        })
        .collect()
}

/// Where the aim square sits: along the aim, a ray's length out (past the start place).
pub fn aim_handle(s: &EmitterSettings) -> Vec2 {
    let aim = s.aim_angle.to_radians();
    let start = if s.start_from == StartFrom::Ring { s.start_radius.max(0.0) } else { 0.0 };
    Vec2::from_angle(aim) * (start + ray_length(s))
}

/// Where a fan's edge dots sit: (near, far) edge, a ray's length out.
pub fn fan_handles(s: &EmitterSettings) -> (Vec2, Vec2) {
    let (aim, half) = (s.aim_angle.to_radians(), s.fan_angle.to_radians() * 0.5);
    let len = aim_handle(s).length();
    (Vec2::from_angle(aim - half) * len, Vec2::from_angle(aim + half) * len)
}

/// The degrees `a` brought into -180 to 180, whole.
fn wrapped(degrees: f32) -> f32 {
    ((degrees + 180.0).rem_euclid(360.0) - 180.0).round()
}

/// What a drag of `what` to view point `p` sets, as (key, value).
pub fn dragged(what: MiniDrag, s: &EmitterSettings, map: MiniMap, p: Pos2) -> (&'static str, Value) {
    match what {
        MiniDrag::Aim => ("aim_angle", Value::Number(wrapped(map.angle_of(p).to_degrees()) as f64)),
        MiniDrag::FanEdge(_) => {
            let off = wrapped(map.angle_of(p).to_degrees() - s.aim_angle).abs();
            ("fan_angle", Value::Number((off * 2.0).clamp(0.0, 360.0) as f64))
        }
        MiniDrag::Speed(k) => {
            // How far along the dragged ray from its own start place (on a ring or a line, not
            // the middle) the pointer is.
            let n = fire::first_count(s).max(1);
            let way = fire::direction(s, k.min(n - 1), n, 1, 0, 0);
            let from = fire::start_offset(s, k.min(n - 1), n, way);
            let along = (map.to_world(p) - from).dot(Vec2::from_angle(way)).max(0.0);
            ("speed", Value::Number(((along / RAY_SECONDS).round()).clamp(0.0, 20000.0) as f64))
        }
    }
}

/// The handle under view point `p`, nearest first.
pub fn handle_at(s: &EmitterSettings, map: MiniMap, p: Pos2) -> Option<MiniDrag> {
    let near = |v: Vec2| map.to_view(v).distance(p);
    let mut best: Option<(f32, MiniDrag)> = None;
    let mut consider = |d: f32, what: MiniDrag| {
        if d <= GRAB && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, what));
        }
    };
    if s.directions != Directions::Random {
        consider(near(aim_handle(s)), MiniDrag::Aim);
    }
    if s.directions == Directions::Fan {
        let (a, b) = fan_handles(s);
        consider(near(a), MiniDrag::FanEdge(false));
        consider(near(b), MiniDrag::FanEdge(true));
    }
    for ray in rays(s) {
        // The aim square sits on the first ray's end; it wins there.
        consider(near(ray.to) + 0.5, MiniDrag::Speed(ray.k));
    }
    best.map(|(_, w)| w)
}

/// A scene colour as the view shows it.
fn shown(c: [f32; 4]) -> Color32 {
    let c = Color::from_linear(c);
    Color32::from_rgb(c.r, c.g, c.b) // scene colour
}

/// Draws the view; true when a drag starts (its own undo step).
pub fn show(ui: &mut Ui, inspector: &mut Inspector, cx: &OwnCx, s: &EmitterSettings, edits: &mut Vec<Edit>) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Emitter view"));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    let map = map_for(rect, s);
    // One round's rays in the colours its balls start in.
    let colours = round_colours(cx.resolved.things.get(&cx.id).and_then(|r| r.slots.get(&SlotId::Look)), s);
    for ray in rays(s) {
        let c = colours.get(ray.k as usize).copied().unwrap_or([1.0; 4]);
        painter.line_segment([map.to_view(ray.from), map.to_view(ray.to)], Stroke::new(1.5, shown(c)));
        painter.circle_filled(map.to_view(ray.to), 2.5, shown(c));
    }
    // Where they start from.
    match s.start_from {
        StartFrom::Point => {}
        StartFrom::Ring => {
            painter.circle_stroke(map.centre, s.start_radius.max(0.0) * map.scale, Stroke::new(1.0, style::FAINT));
        }
        StartFrom::Line => {
            let across = Vec2::from_angle(s.aim_angle.to_radians()).perp() * s.start_length.max(0.0) * 0.5;
            painter.line_segment([map.to_view(-across), map.to_view(across)], Stroke::new(1.5, style::FAINT));
        }
    }
    // What it catches in.
    if s.catches != Catches::Nothing {
        let r = s.catch_size.max(0.0) * map.scale;
        let ring: Vec<Pos2> =
            (0..=64).map(|i| map.centre + egui::Vec2::angled(std::f32::consts::TAU * i as f32 / 64.0) * r).collect();
        painter.extend(egui::Shape::dashed_line(&ring, Stroke::new(1.0, style::MUTED), 4.0, 3.0));
    }
    // The emitter itself.
    painter.circle_stroke(map.centre, 6.0, Stroke::new(1.5, style::TEXT));
    painter.circle_filled(map.centre, 2.0, style::TEXT);
    // The handles.
    if s.directions != Directions::Random {
        let at = map.to_view(aim_handle(s));
        let square = Rect::from_center_size(at, egui::Vec2::splat(SQUARE * 2.0));
        painter.rect_filled(square, 1.0, style::TEXT_STRONG);
        painter.rect_stroke(square, 1.0, Stroke::new(1.0, style::OUTLINE), egui::StrokeKind::Outside);
        painter.text(
            at + egui::vec2(8.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            format!("Aim {}°", s.aim_angle.round()),
            style::mono(10.0),
            style::TEXT,
        );
    }
    if s.directions == Directions::Fan {
        let (a, b) = fan_handles(s);
        for v in [a, b] {
            painter.circle_filled(map.to_view(v), 4.0, style::SLIDER_FILL);
            painter.circle_stroke(map.to_view(v), 4.0, Stroke::new(1.0, style::OUTLINE));
        }
    }
    painter.text(
        rect.left_bottom() + egui::vec2(6.0, -4.0),
        egui::Align2::LEFT_BOTTOM,
        "Drag the square to aim · drag a ray end for speed",
        egui::FontId::proportional(10.0),
        style::MUTED,
    );

    // Dragging.
    let mut started = false;
    let pointer = response.interact_pointer_pos();
    if response.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        inspector.emitter_drag = handle_at(s, map, p).map(|w| (cx.id, w));
        started = inspector.emitter_drag.is_some();
    }
    if let (Some((id, what)), Some(p)) = (inspector.emitter_drag, pointer)
        && id == cx.id
        && response.dragged()
    {
        // The map stays as it was when the drag started for Speed, so the ray follows the
        // pointer rather than the view rescaling under it.
        let held = if matches!(what, MiniDrag::Speed(_)) {
            ui.data(|d| d.get_temp::<f32>(response.id)).map_or(map, |scale| MiniMap { scale, ..map })
        } else {
            map
        };
        if started {
            ui.data_mut(|d| d.insert_temp(response.id, map.scale));
        }
        let (key, value) = dragged(what, s, held, p);
        edits.push(Edit::SetOwn(key.into(), value));
    }
    if response.drag_stopped() {
        inspector.emitter_drag = None;
        ui.data_mut(|d| d.remove::<f32>(response.id));
    }
    if let Some(p) = response.hover_pos()
        && handle_at(s, map, p).is_some()
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    started
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> MiniMap {
        MiniMap { centre: Pos2::new(100.0, 85.0), scale: 0.25 }
    }

    #[test]
    fn dragging_the_square_aims_and_a_ray_end_sets_the_speed() {
        let s = EmitterSettings::default();
        let m = map();
        let (key, v) = dragged(MiniDrag::Aim, &s, m, m.to_view(Vec2::new(10.0, 0.0)));
        assert_eq!((key, v), ("aim_angle", Value::Number(0.0)));
        let (_, v) = dragged(MiniDrag::Aim, &s, m, m.to_view(Vec2::new(0.0, -10.0)));
        assert_eq!(v, Value::Number(-90.0), "up the view is -90°");
        let (key, v) = dragged(MiniDrag::Speed(0), &s, m, m.to_view(Vec2::new(200.0, 0.0)));
        assert_eq!((key, v), ("speed", Value::Number(400.0)), "200 px in half a second");
    }

    #[test]
    fn a_ray_from_a_line_sets_the_speed_from_its_own_start() {
        // Balls from a line 200 px long, all fired one way (down): the end ray starts 100 px off
        // the middle, so its end 50 px below its start is 100 px/s, not the distance from the middle.
        let s = EmitterSettings {
            start_from: StartFrom::Line,
            start_length: 200.0,
            directions: Directions::OneWay,
            aim_angle: 90.0,
            count: 5,
            ..EmitterSettings::default()
        };
        let m = map();
        let ray = rays(&s)[0];
        let end = ray.from + Vec2::new(0.0, 50.0);
        assert_eq!(dragged(MiniDrag::Speed(ray.k), &s, m, m.to_view(end)).1, Value::Number(100.0));
        assert_eq!(handle_at(&s, m, m.to_view(ray.to)), Some(MiniDrag::Speed(ray.k)));
    }

    #[test]
    fn a_fans_edges_set_its_angle_both_ways_from_the_aim() {
        let s = EmitterSettings { directions: Directions::Fan, aim_angle: 0.0, ..EmitterSettings::default() };
        let m = map();
        let at = |deg: f32| m.to_view(Vec2::from_angle(deg.to_radians()) * 100.0);
        assert_eq!(dragged(MiniDrag::FanEdge(true), &s, m, at(30.0)).1, Value::Number(60.0));
        assert_eq!(dragged(MiniDrag::FanEdge(false), &s, m, at(-45.0)).1, Value::Number(90.0));
    }

    #[test]
    fn the_handles_are_found_where_they_are_drawn() {
        let s = EmitterSettings { directions: Directions::Fan, ..EmitterSettings::default() };
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(300.0, HEIGHT));
        let m = map_for(rect, &s);
        assert_eq!(handle_at(&s, m, m.to_view(aim_handle(&s))), Some(MiniDrag::Aim));
        let (near, far) = fan_handles(&s);
        assert_eq!(handle_at(&s, m, m.to_view(near)), Some(MiniDrag::FanEdge(false)));
        assert_eq!(handle_at(&s, m, m.to_view(far)), Some(MiniDrag::FanEdge(true)));
        assert_eq!(handle_at(&s, m, m.centre), None);
        // A round of ten all round: a ray end off the aim sets the speed.
        let all = EmitterSettings::default();
        let m = map_for(rect, &all);
        let side = rays(&all)[3].to;
        assert_eq!(handle_at(&all, m, m.to_view(side)), Some(MiniDrag::Speed(3)));
    }

    #[test]
    fn a_big_round_shows_at_most_so_many_rays() {
        let s = EmitterSettings { count: 5000, ..EmitterSettings::default() };
        assert_eq!(rays(&s).len(), MOST_RAYS as usize);
    }
}
