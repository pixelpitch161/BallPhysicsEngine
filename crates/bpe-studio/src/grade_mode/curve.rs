//! The Tone Curve's graph: darks on the left, lights on the right, the curve over a quarter
//! grid. Parametric shows the parametric curve and its three splits; Point shows the chosen
//! channel's points, which are added by a click, moved by dragging and taken away by a
//! double-click (the end points move only up and down).

use super::cx::{GradeCx, GradeEdit, full_key};
use super::{Channel, GradeMode};
use crate::style;
use bpe_core::{CurvePoint, LayerId, MAX_CURVE_POINTS, Params, Settings, Value};
use bpe_gfx::grade::maths;
use bpe_kit::modules::grade::{CurveMode, ToneCurvePanel};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Ui};

/// How near (points) the pointer must be to grab a curve point.
const GRAB: f32 = 9.0;
/// The least room between two points' x.
const GAP: f32 = 0.01;

/// The panel's settings at the playhead.
fn panel_of(gx: &GradeCx, id: LayerId) -> ToneCurvePanel {
    let mut params = Params::default();
    for d in ToneCurvePanel::defs() {
        params.set(d.key, gx.get(Some(id), d.key));
    }
    ToneCurvePanel::from_params(&params)
}

pub fn channel_colour(c: Channel) -> Color32 {
    match c {
        Channel::Rgb => style::TEXT,
        Channel::Red => style::CHANNEL_RED,
        Channel::Green => style::CHANNEL_GREEN,
        Channel::Blue => style::CHANNEL_BLUE,
    }
}

pub fn show(
    ui: &mut Ui,
    mode: &mut GradeMode,
    gx: &GradeCx,
    id: LayerId,
    curve_mode: CurveMode,
    edits: &mut Vec<GradeEdit>,
) {
    let side = ui.available_width().clamp(120.0, 280.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Tone curve"));
    let painter = ui.painter_at(rect.expand(4.0));
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    for i in 1..4 {
        let t = i as f32 / 4.0;
        painter.vline(rect.left() + rect.width() * t, rect.y_range(), Stroke::new(1.0, style::LINE_SOFT));
        painter.hline(rect.x_range(), rect.top() + rect.height() * t, Stroke::new(1.0, style::LINE_SOFT));
    }
    painter.line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.0, style::LINE));
    let to_screen = |x: f32, y: f32| Pos2::new(rect.left() + x * rect.width(), rect.bottom() - y * rect.height());
    let panel = panel_of(gx, id);
    let gpu = panel.gpu();
    let trace = |f: &dyn Fn(f64) -> f64, ink: Color32| {
        let points: Vec<Pos2> = (0..=128)
            .map(|i| {
                let x = i as f64 / 128.0;
                to_screen(x as f32, f(x).clamp(0.0, 1.0) as f32)
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5, ink)));
    };
    if curve_mode == CurveMode::Parametric {
        for s in maths::splits(gpu.splits) {
            let x = rect.left() + rect.width() * s as f32;
            painter.vline(x, rect.y_range(), Stroke::new(1.0, style::FAINT_MARK));
        }
        let curve = maths::parametric(&gpu);
        trace(&|x| curve.at(x), style::TEXT);
        return;
    }
    let channel = mode.channel.get(&id).copied().unwrap_or_default();
    let mut points = match channel {
        Channel::Rgb => panel.rgb.clone(),
        Channel::Red => panel.red.clone(),
        Channel::Green => panel.green.clone(),
        Channel::Blue => panel.blue.clone(),
    };
    if !bpe_core::curve_is_valid(&points) {
        points = bpe_gfx::straight_curve();
    }
    let curve = maths::point_curve(&points);
    trace(&|x| curve.at(x), channel_colour(channel));
    let near = |points: &[CurvePoint], pos: Pos2| {
        points
            .iter()
            .enumerate()
            .map(|(i, p)| (i, to_screen(p.x, p.y).distance(pos)))
            .filter(|(_, d)| *d <= GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let at_pointer = |pos: Pos2| {
        (
            ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0),
        )
    };
    let dragging = mode.curve_drag.filter(|(p, c, _)| *p == id && *c == channel).map(|(_, _, i)| i);
    let mut changed = None;
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    if response.double_clicked() {
        if let Some(i) = pointer.and_then(|p| near(&points, p)).filter(|&i| i > 0 && i + 1 < points.len()) {
            points.remove(i);
            changed = Some(points.clone());
        }
    } else if response.drag_started()
        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
    {
        let i = match near(&points, pos) {
            Some(i) => Some(i),
            None => add_point(&mut points, at_pointer(pos)).inspect(|_| changed = Some(points.clone())),
        };
        mode.curve_drag = i.map(|i| (id, channel, i));
    } else if response.clicked()
        && let Some(pos) = pointer
        && near(&points, pos).is_none()
        && add_point(&mut points, at_pointer(pos)).is_some()
    {
        changed = Some(points.clone());
    }
    if let (Some(i), Some(pos)) = (dragging, response.interact_pointer_pos())
        && response.dragged()
        && i < points.len()
    {
        let (x, y) = at_pointer(pos);
        let last = points.len() - 1;
        let p = &mut points[i];
        p.y = y;
        if i > 0 && i < last {
            let (lo, hi) = (points[i - 1].x + GAP, points[i + 1].x - GAP);
            points[i].x = x.clamp(lo, hi.max(lo));
        }
        changed = Some(points.clone());
    }
    if response.drag_stopped() {
        mode.curve_drag = None;
    }
    let hovered = pointer.and_then(|p| near(&points, p));
    for (i, p) in points.iter().enumerate() {
        let on = Some(i) == dragging || Some(i) == hovered;
        let size = if on { 7.0 } else { 5.0 };
        let r = Rect::from_center_size(to_screen(p.x, p.y), egui::vec2(size, size));
        painter.rect_filled(r, 1.0, if on { style::TEXT_STRONG } else { channel_colour(channel) });
    }
    if let Some(points) = changed.filter(|p| bpe_core::curve_is_valid(p)) {
        edits.push(GradeEdit::Set(full_key(Some(id), channel.key()), Value::Curve(points)));
    }
}

/// Puts a point at `(x, y)` between the ones either side of it, unless it would sit on one or
/// the curve is full. Returns its place.
pub(super) fn add_point(points: &mut Vec<CurvePoint>, (x, y): (f32, f32)) -> Option<usize> {
    if points.len() >= MAX_CURVE_POINTS {
        return None;
    }
    let at = points.iter().position(|p| p.x > x)?;
    if at == 0 || x - points[at - 1].x < GAP || points[at].x - x < GAP {
        return None;
    }
    points.insert(at, CurvePoint::new(x, y, 0.0));
    Some(at)
}
