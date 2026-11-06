//! Light handles in the preview (lighting spec, layout C): whenever a light is selected or the
//! Lights tab is open, each light shows its lamp, and the selected one its reach ring, a spot's
//! cone edges and aim, a sun's direction arrow, an area light's corner and aim, or a strip's
//! ends. Every one drags, keyed at the playhead when the setting has keys.

use super::{Handle, View};
use crate::app::{LeftPanel, Studio};
use crate::grid::snap_point;
use crate::style;
use bpe_core::{Address, Registry, Scene, Settings, ThingId, Value};
use bpe_kit::things::light::draw::{Place, aim_angle, places_in};
use bpe_kit::things::light::{AreaShape, LIGHT, LightSettings, LightType, Points, StripAlong};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Shape, Stroke};
use glam::Vec2;
use std::f32::consts::TAU;

/// The most lamps one light riding each ball draws.
pub const MOST_LAMPS: usize = 400;
/// How close to a dot, in screen points, counts as grabbing it.
const DOT_GRAB: f32 = 9.0;
/// How close to a line or ring, in screen points, counts as grabbing it.
const LINE_GRAB: f32 = 6.0;
/// A spot's and an area's aim dot, and a sun's arrow, sit this far out (screen points).
const AIM_OUT: f32 = 70.0;

/// A part of a light's handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightPart {
    /// The lamp at one of its places (a light riding each ball has one per ball).
    Lamp(u32),
    /// The ring its light reaches to.
    Reach,
    /// One edge of a spot's cone (false: the edge anticlockwise of its aim).
    Cone(bool),
    /// Which way it points.
    Aim,
    /// An area light's corner: its width and length.
    Corner,
    /// One end of a strip along a line (0 the end it points from, 1 the end it points to).
    End(u8),
}

/// What a light drag started from, so it moves by how far the pointer goes.
#[derive(Clone, Debug, PartialEq)]
pub struct LightGrab {
    /// Where the pointer was pressed (world pixels).
    pub press: Vec2,
    /// The light's settings at the playhead when grabbed.
    pub settings: LightSettings,
    /// The place grabbed (place k for a lamp, the first otherwise).
    pub place: Vec2,
    /// Which way it pointed then (radians).
    pub aim: f32,
    /// How far Spin had turned it by the playhead (degrees), taken off a new Direction.
    pub spun: f32,
}

/// The lights whose handles show: every light while the Lights tab is open, else the selected
/// light. Hidden lights show none.
pub fn shown(studio: &Studio) -> Vec<ThingId> {
    // A light that is off, or inside something hidden, isn't in the picture: no handles.
    let is_light = |id: &ThingId| studio.resolved.things.get(id).is_some_and(|r| r.kind == LIGHT);
    if studio.left == LeftPanel::Lights {
        return studio.scene.order.iter().copied().filter(is_light).collect();
    }
    studio.selection.primary.filter(is_light).into_iter().collect()
}

/// Light `id`'s settings at the playhead: each keyed setting at its value there.
pub fn settings_at(scene: &Scene, reg: &Registry, id: ThingId, time: f64) -> Option<LightSettings> {
    let thing = scene.things.get(&id)?;
    let mut params = thing.params.clone();
    for def in LightSettings::defs() {
        let a = Address::own(id, def.key);
        if scene.keyed(&a)
            && let Some(v) = scene.value_at_time(reg, &a, time)
        {
            params.set(def.key, v);
        }
    }
    Some(LightSettings::from_params(&params))
}

/// A light as its handles see it: its settings at the playhead and where it shines from.
struct Lamp {
    s: LightSettings,
    places: Vec<Place>,
    /// Which way it points at its first place (radians).
    aim: f32,
}

fn lamp(studio: &Studio, id: ThingId) -> Option<Lamp> {
    let time = studio.sim_time();
    let s = settings_at(&studio.scene, &studio.kit.registry, id, time)?;
    let parent = bpe_kit::things::rig::fixtures::ridden(&studio.scene, id);
    let places = places_in(studio.playback.world(), parent, &s);
    let aim = places.first().map_or(s.direction.to_radians(), |p| aim_angle(&s, p, time));
    Some(Lamp { s, places, aim })
}

/// Whether the light's aim can be dragged: not when it follows the ball it rides, and a point
/// light points nowhere.
fn aims(s: &LightSettings) -> bool {
    s.kind != LightType::Point && s.points != Points::WhereItsGoing
}

/// The screen points of each handle of `l`'s first place, in grab order (dots first).
fn dots(l: &Lamp, v: &View) -> Vec<(LightPart, Pos2)> {
    let Some(first) = l.places.first() else { return Vec::new() };
    let at = first.at;
    let dir = Vec2::from_angle(l.aim);
    let side = dir.perp();
    let out = AIM_OUT / v.world_scale().max(1e-6);
    let mut list = Vec::new();
    match l.s.kind {
        LightType::Spot | LightType::Sun if aims(&l.s) => list.push((LightPart::Aim, v.to_screen(at + dir * out))),
        LightType::Area => {
            let half = Vec2::new(0.5 * l.s.width.max(0.0), 0.5 * l.s.length.max(0.0));
            list.push((LightPart::Corner, v.to_screen(at + dir * half.x + side * half.y)));
            if aims(&l.s) {
                list.push((LightPart::Aim, v.to_screen(at + dir * (half.x + out))));
            }
        }
        LightType::Strip if l.s.strip_along == StripAlong::Line => {
            let half = dir * (0.5 * l.s.length);
            list.push((LightPart::End(0), v.to_screen(at - half)));
            list.push((LightPart::End(1), v.to_screen(at + half)));
        }
        _ => {}
    }
    list
}

/// What of the shown lights' handles is under `pos`: with `selected_only`, only the selected
/// light's (they come before the balls); otherwise only the other lights' lamps.
pub fn hit(studio: &Studio, view: &View, pos: Pos2, selected_only: bool) -> Option<Handle> {
    let selected = studio.selection.primary;
    for id in shown(studio) {
        if (Some(id) == selected) != selected_only {
            continue;
        }
        let Some(v) = view.of(studio, id) else { continue };
        let Some(l) = lamp(studio, id) else { continue };
        if selected_only {
            for (part, p) in dots(&l, &v) {
                if (p - pos).length() <= DOT_GRAB {
                    return Some(Handle::Light(id, part));
                }
            }
        }
        for (k, place) in l.places.iter().enumerate().take(MOST_LAMPS) {
            if (v.to_screen(place.at) - pos).length() <= DOT_GRAB + 2.0 {
                return Some(Handle::Light(id, LightPart::Lamp(k as u32)));
            }
        }
        if !selected_only {
            continue;
        }
        let Some(first) = l.places.first() else { continue };
        let centre = v.to_screen(first.at);
        let scale = v.world_scale();
        if l.s.kind == LightType::Spot {
            let reach = l.s.reach.max(0.0) * scale;
            for (edge, sign) in [(false, -1.0f32), (true, 1.0)] {
                let a = l.aim + sign * (0.5 * l.s.cone).to_radians();
                let end = v.to_screen(first.at + Vec2::from_angle(a) * (reach / scale.max(1e-6)));
                if distance_to_segment(pos, centre, end) <= LINE_GRAB {
                    return Some(Handle::Light(id, LightPart::Cone(edge)));
                }
            }
        }
        if l.s.kind != LightType::Sun {
            let r = l.s.reach.max(0.0) * scale;
            if ((pos - centre).length() - r).abs() <= LINE_GRAB {
                return Some(Handle::Light(id, LightPart::Reach));
            }
        }
    }
    None
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (p - (a + ab * t)).length()
}

/// The grab a drag of `part` on light `id` starts from, pressed at `press` (world pixels).
pub fn grab(studio: &Studio, id: ThingId, part: LightPart, press: Vec2) -> Option<LightGrab> {
    let l = lamp(studio, id)?;
    let k = match part {
        LightPart::Lamp(k) => k as usize,
        _ => 0,
    };
    let place = l.places.get(k).or(l.places.first()).copied().unwrap_or_default();
    let time = studio.sim_time();
    let spun = if l.s.points == Points::Spins { l.s.spin * time as f32 } else { 0.0 };
    Some(LightGrab { press, aim: aim_angle(&l.s, &place, time), place: place.at, settings: l.s, spun })
}

/// The angle of `v` in degrees, -180..180.
fn degrees(v: Vec2) -> f32 {
    v.y.atan2(v.x).to_degrees()
}

/// -180..180.
fn wrap(deg: f32) -> f32 {
    (deg + 180.0).rem_euclid(360.0) - 180.0
}

/// Applies a drag of light `id`'s `part` to `pointer` (world pixels), from `g`. Settings with
/// keys get a key at `time`; true when one did. With a grid `step`, the lamp and a strip's ends
/// land on the grid.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    scene: &mut Scene,
    reg: &Registry,
    id: ThingId,
    part: LightPart,
    g: &LightGrab,
    pointer: Vec2,
    step: Option<f32>,
    time: f64,
) -> bool {
    let s = &g.settings;
    let mut keyed = false;
    let mut set = |scene: &mut Scene, key: &str, value: Value| {
        crate::inspector::set_or_key(scene, reg, &Address::own(id, key), value, time, &mut keyed);
    };
    let round = |x: f32, to: f32| ((x / to).round() * to) as f64;
    // What the light rides: its place less its Position.
    let anchor = g.place - s.position;
    match part {
        LightPart::Lamp(_) => {
            let place = snap_point(g.place + (pointer - g.press), step);
            let p = (place - anchor).round();
            set(scene, "position", Value::Vec2([p.x, p.y]));
        }
        LightPart::Reach => set(scene, "reach", Value::Number(round(pointer.distance(g.place), 1.0))),
        LightPart::Cone(_) => {
            let off = wrap(degrees(pointer - g.place) - g.aim.to_degrees()).abs();
            set(scene, "cone", Value::Number(round((2.0 * off).min(360.0), 0.1)));
        }
        LightPart::Aim => {
            let d = wrap(degrees(pointer - g.place) - g.spun);
            set(scene, "direction", Value::Number(round(d, 0.1)));
        }
        LightPart::Corner => {
            let local = Vec2::from_angle(-g.aim).rotate(pointer - g.place);
            set(scene, "width", Value::Number(round(2.0 * local.x.abs(), 1.0)));
            set(scene, "length", Value::Number(round(2.0 * local.y.abs(), 1.0)));
        }
        LightPart::End(end) => {
            let dir = Vec2::from_angle(g.aim);
            let half = dir * (0.5 * s.length);
            // The other end stays where it is.
            let (other, sign) = if end == 1 { (g.place - half, 1.0) } else { (g.place + half, -1.0) };
            let tip = snap_point(pointer, step);
            let span = (tip - other) * sign;
            let middle = ((tip + other) * 0.5 - anchor).round();
            set(scene, "position", Value::Vec2([middle.x, middle.y]));
            set(scene, "length", Value::Number(round(span.length(), 1.0)));
            if span.length_squared() > 1e-6 {
                set(scene, "direction", Value::Number(round(wrap(degrees(span) - g.spun), 0.1)));
            }
        }
    }
    keyed
}

/// Draws the shown lights' handles: every lamp, and the selected light's ring, cone, arrow, panel
/// or ends. `active` is the handle under the pointer or being dragged.
pub fn draw(painter: &egui::Painter, studio: &Studio, view: &View, active: Option<Handle>, selected: Option<ThingId>) {
    for id in shown(studio) {
        let Some(v) = view.of(studio, id) else { continue };
        let Some(l) = lamp(studio, id) else { continue };
        let picked = selected == Some(id);
        let track = style::colour_of(&studio.scene, id);
        let ink = if picked { track } else { style::FAINT_MARK };
        let hot = |part: LightPart| active == Some(Handle::Light(id, part));
        let [r, g, b] = l.s.display_colour();
        let glow = Color32::from_rgb(r, g, b); // scene colour
        let Some(first) = l.places.first().copied() else { continue };
        let centre = v.to_screen(first.at);
        let scale = v.world_scale();
        let dir = Vec2::from_angle(l.aim);
        if l.s.kind != LightType::Sun {
            let reach = l.s.reach.max(0.0) * scale;
            let stroke =
                Stroke::new(if hot(LightPart::Reach) { 2.0 } else { 1.0 }, line_ink(hot(LightPart::Reach), ink));
            if picked || hot(LightPart::Reach) {
                painter.add(Shape::dashed_line(&circle(centre, reach), stroke, 6.0, 4.0));
            } else {
                painter.add(Shape::closed_line(circle(centre, reach), Stroke::new(1.0, style::FAINT_MARK)));
            }
        }
        if picked {
            match l.s.kind {
                LightType::Spot => {
                    for (edge, sign) in [(false, -1.0f32), (true, 1.0)] {
                        let a = l.aim + sign * (0.5 * l.s.cone).to_radians();
                        let end = v.to_screen(first.at + Vec2::from_angle(a) * l.s.reach.max(0.0));
                        let on = hot(LightPart::Cone(edge));
                        painter.line_segment([centre, end], Stroke::new(if on { 2.0 } else { 1.2 }, line_ink(on, ink)));
                    }
                }
                LightType::Sun => {
                    let tip = v.to_screen(first.at + dir * (AIM_OUT / scale.max(1e-6)));
                    arrow(painter, centre, tip, line_ink(hot(LightPart::Aim), ink));
                }
                LightType::Area => {
                    let half = Vec2::new(0.5 * l.s.width.max(0.0), 0.5 * l.s.length.max(0.0));
                    let side = dir.perp();
                    let outline: Vec<Pos2> = if l.s.area_shape == AreaShape::Disc {
                        (0..=64)
                            .map(|i| {
                                let t = i as f32 / 64.0 * TAU;
                                v.to_screen(first.at + dir * half.x * t.cos() + side * half.y * t.sin())
                            })
                            .collect()
                    } else {
                        [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (1.0, 1.0)]
                            .into_iter()
                            .map(|(x, y)| v.to_screen(first.at + dir * half.x * x + side * half.y * y))
                            .collect()
                    };
                    painter.add(Shape::line(outline, Stroke::new(1.5, ink)));
                }
                LightType::Strip if l.s.strip_along == StripAlong::Line => {
                    let half = dir * (0.5 * l.s.length);
                    let (a, b) = (v.to_screen(first.at - half), v.to_screen(first.at + half));
                    painter.line_segment([a, b], Stroke::new(3.0, glow));
                }
                _ => {}
            }
            for (part, p) in dots(&l, &v) {
                if matches!(part, LightPart::Aim) && l.s.kind == LightType::Spot {
                    painter.line_segment([centre, p], Stroke::new(1.0, ink));
                }
                let on = hot(part);
                painter.circle(p, if on { 6.5 } else { 5.0 }, line_ink(on, ink), Stroke::new(1.5, style::OUTLINE));
            }
        }
        for (k, place) in l.places.iter().enumerate().take(MOST_LAMPS) {
            let on = hot(LightPart::Lamp(k as u32));
            lamp_icon(painter, v.to_screen(place.at), glow, line_ink(on, if picked { track } else { style::FAINT }));
        }
        if picked {
            // The placement ring and the name, as any thing with nothing of its own to show.
            let ring = circle(centre, 24.0);
            let colour = if hot(LightPart::Lamp(0)) { style::TEXT_STRONG } else { track };
            painter.add(Shape::dashed_line(&ring, Stroke::new(1.2, colour), 4.0, 4.0));
            if let Some(thing) = studio.scene.things.get(&id) {
                painter.text(
                    centre + egui::vec2(0.0, 30.0),
                    Align2::CENTER_TOP,
                    &thing.name,
                    FontId::proportional(12.0),
                    colour,
                );
            }
        }
    }
}

fn line_ink(hot: bool, ink: Color32) -> Color32 {
    if hot { style::TEXT_STRONG } else { ink }
}

fn circle(centre: Pos2, r: f32) -> Vec<Pos2> {
    (0..=96).map(|i| centre + egui::Vec2::angled(i as f32 / 96.0 * TAU) * r).collect()
}

/// A lamp: a dot of its light's colour with short rays round it.
fn lamp_icon(painter: &egui::Painter, at: Pos2, glow: Color32, ink: Color32) {
    for k in 0..8 {
        let d = egui::Vec2::angled(k as f32 / 8.0 * TAU);
        painter.line_segment([at + d * 8.5, at + d * 12.0], Stroke::new(1.3, ink));
    }
    painter.circle(at, 6.0, glow, Stroke::new(1.5, ink));
}

fn arrow(painter: &egui::Painter, from: Pos2, tip: Pos2, ink: Color32) {
    painter.line_segment([from, tip], Stroke::new(2.0, ink));
    let d = (tip - from).normalized();
    if d.length() > 0.5 {
        let side = egui::vec2(-d.y, d.x);
        let back = tip - d * 10.0;
        painter.add(Shape::convex_polygon(vec![tip, back + side * 5.0, back - side * 5.0], ink, Stroke::NONE));
    }
}

#[cfg(test)]
mod tests;
