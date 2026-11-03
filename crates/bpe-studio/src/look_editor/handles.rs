//! Handles on the ball view for the picked layer: dots, lines and rings drawn over the picture
//! that set its settings when dragged. Each drag sets the layer's setting through the usual
//! edit, so a whole drag is one undo step, as a slider's is.

use bpe_core::{Params, Settings, Value, layered_key};
use bpe_kit::modules::look::layers::{
    self as kinds, GlowLayer, MaskFrom, MaskLayer, MaskShape, OutlineLayer, OutlineSits, PatternLayer, ShadingKind,
    ShadingLayer, ShineLayer, Turning,
};
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, Ui, Vec2 as EVec2};
use glam::Vec2;

use super::LookCx;
use super::ball_view::ViewMap;
use crate::inspector::Edit;
use crate::style;

/// One handle, in ball space (radii, y down, before the ball turns).
#[derive(Clone, Debug, PartialEq)]
pub struct Handle {
    /// What it sets, as the user would say it; its accessible name is "<name> handle".
    pub name: &'static str,
    pub at: Vec2,
    /// A line from this point to the handle.
    pub from: Option<Vec2>,
    /// A ring of this radius (in radii) round `ring_at`.
    pub ring: Option<(Vec2, f32)>,
    /// Its value in a few words, shown beside it.
    pub tag: String,
}

impl Handle {
    fn dot(name: &'static str, at: Vec2, tag: String) -> Handle {
        Handle { name, at, from: None, ring: None, tag }
    }
}

/// A point at `angle` degrees from straight up, clockwise (as the mask shapes turn), `r` out.
fn from_up(angle: f32, r: f32) -> Vec2 {
    let a = angle.to_radians();
    Vec2::new(a.sin(), -a.cos()) * r
}

/// The angle of `q` from straight up, clockwise, in degrees.
fn up_angle(q: Vec2) -> f32 {
    q.x.atan2(-q.y).to_degrees()
}

/// `degrees` brought into -180 to 180, to the nearest whole degree.
fn wrapped(degrees: f32) -> f32 {
    ((degrees + 180.0).rem_euclid(360.0) - 180.0).round()
}

fn hundredths(x: f32) -> f32 {
    (x * 100.0).round() / 100.0
}

/// Where the outline's handle sits out from the edge, per pixel of width.
fn outline_reach(sits: OutlineSits) -> f32 {
    match sits {
        OutlineSits::Inside => -1.0,
        OutlineSits::Centre => 0.5,
        OutlineSits::Outside => 1.0,
    }
}

/// The handles of a layer of kind `kind` with settings `p` on a ball of `radius` pixels.
pub fn handles(kind: &str, p: &Params, radius: f32) -> Vec<Handle> {
    let radius = radius.max(1.0);
    match kind {
        kinds::OUTLINE => {
            let o = OutlineLayer::from_params(p);
            let at = 1.0 + outline_reach(o.sits) * o.width / radius;
            vec![Handle::dot("Width", Vec2::new(at, 0.0), format!("{:.0} px", o.width))]
        }
        kinds::GLOW => {
            let g = GlowLayer::from_params(p);
            vec![Handle::dot("Size", Vec2::new(1.0 + g.size, 0.0), format!("size {:.2}", g.size))]
        }
        kinds::SHINE => {
            let s = ShineLayer::from_params(p);
            let at = Vec2::new(s.spot.x, s.spot.y);
            vec![Handle { ring: Some((at, 0.14)), ..Handle::dot("Spot", at, format!("{:.2}, {:.2}", at.x, at.y)) }]
        }
        kinds::PATTERN => {
            let l = PatternLayer::from_params(p);
            let a = l.inside.angle.to_radians();
            let at = Vec2::new(a.cos(), a.sin()) * 0.75;
            vec![Handle { from: Some(Vec2::ZERO), ..Handle::dot("Angle", at, format!("{:.0}°", l.inside.angle)) }]
        }
        kinds::SHADING => {
            let s = ShadingLayer::from_params(p);
            match s.kind {
                ShadingKind::Linear => {
                    let (from, to) = (Vec2::new(s.from.x, s.from.y), Vec2::new(s.to.x, s.to.y));
                    vec![
                        Handle::dot("Dark end", from, "dark".into()),
                        Handle { from: Some(from), ..Handle::dot("Light end", to, "light".into()) },
                    ]
                }
                ShadingKind::Radial => {
                    let centre = Vec2::new(s.centre.x, s.centre.y);
                    vec![
                        Handle::dot("Centre", centre, format!("{:.2}, {:.2}", centre.x, centre.y)),
                        Handle {
                            ring: Some((centre, s.size)),
                            ..Handle::dot("Size", centre + Vec2::new(s.size, 0.0), format!("size {:.2}", s.size))
                        },
                    ]
                }
                ShadingKind::Brush => Vec::new(),
            }
        }
        kinds::MASK => {
            let m = MaskLayer::from_params(p);
            if m.from != MaskFrom::Shape {
                return Vec::new();
            }
            let mut out = vec![Handle {
                from: Some(Vec2::ZERO),
                ..Handle::dot("Angle", from_up(m.angle, 0.85), format!("{:.0}°", m.angle))
            }];
            if matches!(m.shape, MaskShape::Wedge | MaskShape::Ring | MaskShape::Circle | MaskShape::Band) {
                out.push(Handle::dot("Size", from_up(m.angle + 90.0, m.size), format!("size {:.2}", m.size)));
            }
            out
        }
        kinds::TURNING => {
            let t = Turning::from_params(p);
            // Above the ball as it is turned now: the view turns the handle with the ball.
            vec![Handle {
                from: Some(Vec2::ZERO),
                ..Handle::dot("Turn", Vec2::new(0.0, -1.25), format!("{:.0}°", t.turn))
            }]
        }
        _ => Vec::new(),
    }
}

/// The setting a drag of handle `name` to ball point `q` changes, and its new value.
pub fn dragged(kind: &str, p: &Params, radius: f32, name: &str, q: Vec2) -> Option<(&'static str, Value)> {
    let radius = radius.max(1.0);
    let number = |x: f32| Value::Number(f64::from(x));
    let point = |v: Vec2| Value::Vec2([hundredths(v.x), hundredths(v.y)]);
    let inside = |v: Vec2| v.clamp(Vec2::splat(-1.0), Vec2::splat(1.0));
    match (kind, name) {
        (kinds::OUTLINE, "Width") => {
            let o = OutlineLayer::from_params(p);
            let w = (q.length() - 1.0) * radius / outline_reach(o.sits);
            Some(("width", number((w.clamp(0.0, 60.0) * 2.0).round() / 2.0)))
        }
        (kinds::GLOW, "Size") => Some(("size", number(hundredths((q.length() - 1.0).clamp(0.05, 4.0))))),
        (kinds::SHINE, "Spot") => Some(("spot", point(inside(q)))),
        (kinds::PATTERN, "Angle") => Some(("in_angle", number(wrapped(q.y.atan2(q.x).to_degrees())))),
        (kinds::SHADING, "Dark end") => Some(("from", point(inside(q)))),
        (kinds::SHADING, "Light end") => Some(("to", point(inside(q)))),
        (kinds::SHADING, "Centre") => Some(("centre", point(inside(q)))),
        (kinds::SHADING, "Size") => {
            let s = ShadingLayer::from_params(p);
            let size = (q - Vec2::new(s.centre.x, s.centre.y)).length();
            Some(("size", number(hundredths(size.clamp(0.1, 2.0)))))
        }
        (kinds::MASK, "Angle") => Some(("angle", number(wrapped(up_angle(q))))),
        (kinds::MASK, "Size") => Some(("size", number(hundredths(q.length().clamp(0.0, 1.0))))),
        (kinds::TURNING, "Turn") => {
            let t = Turning::from_params(p);
            // `q` is on the ball as it is turned now, so its angle from up is how much further.
            Some(("turn", number(wrapped(t.turn + up_angle(q)))))
        }
        _ => None,
    }
}

/// Draws layer `layer`'s handles over the view (the picture `view`) and turns their drags into
/// edits; a drag's start
/// sets `own_step`, so the drag is an undo step of its own. True when the pointer is on a
/// handle or dragging one, so the view leaves the pointer alone.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    cx: &LookCx,
    layer: &bpe_core::Layer,
    map: ViewMap,
    view: Rect,
    radius: f32,
    own_step: &mut bool,
    edits: &mut Vec<Edit>,
) -> bool {
    let Some(kind) = cx.reg().layer(&layer.kind) else { return false };
    let p = cx.params_of(layer.id, kind.settings());
    let list = handles(&layer.kind, &p, radius);
    let painter = ui.painter().clone();
    let line = Stroke::new(1.2, style::TEXT);
    let mut busy = false;
    for (i, h) in list.iter().enumerate() {
        // A handle past the picture would sit over the lists under it, so it waits at the
        // picture's edge, on the way out to where it is.
        let at = inside(view.shrink(8.0), map.centre, map.to_view(h.at));
        if let Some(from) = h.from {
            painter.line_segment([map.to_view(from), at], line);
        }
        if let Some((centre, r)) = h.ring {
            painter.circle_stroke(map.to_view(centre), r * map.radius, line);
        }
        let id = ui.make_persistent_id(("handle", cx.id, layer.id.0, i));
        let response = ui.interact(Rect::from_center_size(at, EVec2::splat(16.0)), id, Sense::drag());
        let name = format!("{} handle", h.name);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        let active = response.hovered() || response.dragged();
        busy |= active;
        painter.circle_filled(at, if active { 6.0 } else { 4.5 }, style::TEXT_STRONG);
        painter.circle_stroke(at, if active { 6.0 } else { 4.5 }, Stroke::new(1.5, style::OUTLINE));
        if active {
            tag(&painter, at, &h.tag);
        }
        if response.drag_started() {
            *own_step = true;
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
            && let Some((key, value)) = dragged(&layer.kind, &p, radius, h.name, map.to_ball(pointer))
        {
            edits.push(Edit::SetSlot(cx.slot, layered_key(layer.id, key), value));
        }
    }
    busy
}

/// `at`, or where the line from `centre` to it leaves `rect` when it is outside.
fn inside(rect: Rect, centre: Pos2, at: Pos2) -> Pos2 {
    if rect.contains(at) {
        return at;
    }
    let d = at - centre;
    let mut t: f32 = 1.0;
    for (reach, room) in [(d.x, rect.right() - centre.x), (-d.x, centre.x - rect.left())] {
        if reach > 0.0 {
            t = t.min(room / reach);
        }
    }
    for (reach, room) in [(d.y, rect.bottom() - centre.y), (-d.y, centre.y - rect.top())] {
        if reach > 0.0 {
            t = t.min(room / reach);
        }
    }
    centre + d * t.max(0.0)
}

/// A handle's value in a small dark pill beside it.
fn tag(painter: &egui::Painter, at: Pos2, words: &str) {
    let galley = painter.layout_no_wrap(words.to_owned(), style::mono(11.0), style::TEXT_STRONG);
    let corner = at + EVec2::new(10.0, -10.0 - galley.size().y);
    let rect = Rect::from_min_size(corner, galley.size()).expand2(EVec2::new(4.0, 2.0));
    painter.rect_filled(rect, style::RADIUS, style::PILL);
    painter.galley(corner, galley, style::TEXT_STRONG);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outline(width: f64, sits: &str) -> Params {
        let mut p = Params::default();
        p.set("width", Value::Number(width));
        p.set("sits", Value::Choice(sits.into()));
        p
    }

    #[test]
    fn the_outline_s_handle_sits_at_its_width_and_drags_it() {
        let p = outline(4.0, "inside");
        let h = handles(kinds::OUTLINE, &p, 40.0);
        assert_eq!(h[0].at, Vec2::new(0.9, 0.0));
        assert_eq!(
            dragged(kinds::OUTLINE, &p, 40.0, "Width", Vec2::new(0.0, 0.8)),
            Some(("width", Value::Number(8.0)))
        );
        let out = outline(4.0, "outside");
        assert_eq!(handles(kinds::OUTLINE, &out, 40.0)[0].at, Vec2::new(1.1, 0.0));
        let past = dragged(kinds::OUTLINE, &out, 40.0, "Width", Vec2::new(5.0, 0.0));
        assert_eq!(past, Some(("width", Value::Number(60.0))), "clamped to its range");
    }

    #[test]
    fn angles_and_points_come_from_where_the_handle_is_dragged() {
        let p = Params::default();
        let mask = |q| dragged(kinds::MASK, &p, 40.0, "Angle", q);
        assert_eq!(mask(Vec2::new(1.0, 0.0)), Some(("angle", Value::Number(90.0))), "right of up is 90°");
        let spot = dragged(kinds::SHINE, &p, 40.0, "Spot", Vec2::new(-2.0, 0.333));
        assert_eq!(spot, Some(("spot", Value::Vec2([-1.0, 0.33]))), "kept on the ball");
        let pattern = dragged(kinds::PATTERN, &p, 40.0, "Angle", Vec2::new(0.0, 1.0));
        assert_eq!(pattern, Some(("in_angle", Value::Number(90.0))));
        assert_eq!(dragged(kinds::BODY, &p, 40.0, "Angle", Vec2::ZERO), None);
    }
}
