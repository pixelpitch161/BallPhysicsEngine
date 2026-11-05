//! The LFO's drawn shape, edited in place: one cycle across the whole width over a grid, its
//! points and the dots that bend each part, and a playhead while it plays.
//!
//! With the Points tool: drag a point (it snaps to the grid unless Shift is held), double-click
//! empty space to add one, double-click a point to take it away, drag a part's middle dot up or
//! down to bend it (Alt+click straightens it), right-click a point for more. The paint tools
//! paint a grid cell at a time where the pointer goes: a step, or a ramp up or down to its height.

use crate::style;
use bpe_core::CurvePoint;
use bpe_dsp::lfo::{LfoWave, bend_at, wave_at};
use bpe_kit::sound::lfo_curve::{self, Paint};
use eframe::egui::{self, Color32, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};

/// The view's height.
pub const HEIGHT: f32 = 210.0;
/// Space inside the view's edge, so the points at the ends can be grabbed.
const INSET: f32 = 8.0;
/// How near the pointer must be to grab a point or a bend dot.
const GRAB: f32 = 8.0;
/// How far a bend moves per pixel of drag, as a share of the view's height.
const BEND_PER_HEIGHT: f32 = 2.5;

/// What the pointer does in the view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Move, add, delete and bend points.
    #[default]
    Points,
    Step,
    RampUp,
    RampDown,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Points, Tool::Step, Tool::RampUp, Tool::RampDown];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Points => "Points",
            Tool::Step => "Step",
            Tool::RampUp => "Ramp up",
            Tool::RampDown => "Ramp down",
        }
    }

    fn paint(self) -> Option<Paint> {
        match self {
            Tool::Points => None,
            Tool::Step => Some(Paint::Step),
            Tool::RampUp => Some(Paint::RampUp),
            Tool::RampDown => Some(Paint::RampDown),
        }
    }
}

/// What the view shows and how it edits.
pub struct View<'a> {
    pub points: &'a [CurvePoint],
    pub wave: LfoWave,
    pub seed: u64,
    /// Grid cells across and up.
    pub grid: (i32, i32),
    pub snap: bool,
    pub tool: Tool,
    /// Where the LFO is now (0..1 across), while it plays.
    pub playhead: Option<f32>,
    pub colour: Color32,
}

/// What a press grabbed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
    Point(usize),
    Bend(usize),
}

/// What the view hands back.
#[derive(Default)]
pub struct Outcome {
    /// The shape after this frame's edit.
    pub points: Option<Vec<CurvePoint>>,
    /// The edit started a new undo step (a press, a double-click, a menu item).
    pub new_step: bool,
}

/// Where the shape's numbers sit in the view.
#[derive(Clone, Copy)]
pub struct Plot {
    pub rect: Rect,
}

impl Plot {
    pub fn new(outer: Rect) -> Self {
        Plot { rect: outer.shrink(INSET) }
    }
    pub fn pos(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.rect.left() + x * self.rect.width(), self.rect.bottom() - y * self.rect.height())
    }
    /// The shape's numbers (0..1 each) at a place in the view.
    pub fn at(&self, p: Pos2) -> (f32, f32) {
        let x = ((p.x - self.rect.left()) / self.rect.width()).clamp(0.0, 1.0);
        let y = ((self.rect.bottom() - p.y) / self.rect.height()).clamp(0.0, 1.0);
        (x, y)
    }
}

/// `v` on a grid of `n` steps.
pub fn snap_to(v: f32, n: i32) -> f32 {
    let n = n.max(1) as f32;
    (v * n).round() / n
}

/// The middle of part `i` (from point `i` to the next), where its bend dot sits; `None` for a
/// jump (no width).
pub fn bend_dot(points: &[CurvePoint], i: usize) -> Option<(f32, f32)> {
    let (a, b) = (points.get(i)?, points.get(i + 1)?);
    if b.x - a.x <= 1e-4 {
        return None;
    }
    Some(((a.x + b.x) / 2.0, a.y + (b.y - a.y) * bend_at(0.5, a.bend)))
}

fn grab_at(plot: &Plot, points: &[CurvePoint], p: Pos2) -> Option<Grab> {
    let near = |q: Pos2| q.distance(p) <= GRAB;
    // The nearest point first; a bend dot only away from the points.
    let point = (0..points.len())
        .map(|i| (i, plot.pos(points[i].x, points[i].y).distance(p)))
        .filter(|(_, d)| *d <= GRAB)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| Grab::Point(i));
    point.or_else(|| {
        (0..points.len().saturating_sub(1))
            .find(|&i| bend_dot(points, i).is_some_and(|(x, y)| near(plot.pos(x, y))))
            .map(Grab::Bend)
    })
}

/// The view, the whole width of `ui`. Returns any edit made.
pub fn show(ui: &mut Ui, id: Id, name: &str, view: &View) -> Outcome {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), HEIGHT), Sense::click_and_drag());
    crate::sound_tab::name_it(ui, &response, name);
    let plot = Plot::new(rect);
    let drawn = view.wave == LfoWave::Curve;
    let mut out = Outcome::default();
    let shift = ui.input(|i| i.modifiers.shift);
    let snap = view.snap && !shift;
    let (gx, gy) = (view.grid.0.max(1), view.grid.1.max(1));
    let grab_id = id.with("grab");
    let paint_id = id.with("painted");
    let menu_id = id.with("menu point");
    let hover = response.hover_pos();

    if drawn {
        match view.tool.paint() {
            None => {
                if response.drag_started()
                    && let Some(p) = ui.input(|i| i.pointer.press_origin())
                {
                    let grab = grab_at(&plot, view.points, p);
                    ui.data_mut(|d| d.insert_temp(grab_id, (grab, view.points.to_vec())));
                    out.new_step = grab.is_some();
                }
                if response.dragged()
                    && let Some((Some(grab), start)) =
                        ui.data(|d| d.get_temp::<(Option<Grab>, Vec<CurvePoint>)>(grab_id))
                    && let (Some(now), Some(origin)) =
                        (response.interact_pointer_pos(), ui.input(|i| i.pointer.press_origin()))
                {
                    match grab {
                        Grab::Point(i) => {
                            let (mut x, mut y) = plot.at(now);
                            if snap {
                                x = snap_to(x, gx);
                                y = snap_to(y, gy);
                            }
                            out.points = Some(lfo_curve::move_point(&start, i, x, y));
                        }
                        Grab::Bend(i) => {
                            let rise = start.get(i + 1).map_or(0.0, |b| b.y - start[i].y);
                            let sign = if rise < 0.0 { -1.0 } else { 1.0 };
                            let up = (origin.y - now.y) / plot.rect.height();
                            let bend = start[i].bend + sign * up * BEND_PER_HEIGHT;
                            out.points = Some(lfo_curve::set_bend(&start, i, bend));
                        }
                    }
                }
                if response.drag_stopped() {
                    ui.data_mut(|d| d.remove::<(Option<Grab>, Vec<CurvePoint>)>(grab_id));
                }
                if response.double_clicked()
                    && let Some(p) = response.interact_pointer_pos()
                {
                    match grab_at(&plot, view.points, p) {
                        Some(Grab::Point(i)) => out.points = Some(lfo_curve::delete_point(view.points, i)),
                        Some(Grab::Bend(_)) => {}
                        None => {
                            let (mut x, mut y) = plot.at(p);
                            if snap {
                                x = snap_to(x, gx);
                                y = snap_to(y, gy);
                            }
                            let (added, at) = lfo_curve::add_point(view.points, x, y);
                            out.points = at.map(|_| added);
                        }
                    }
                    out.new_step = out.points.is_some();
                } else if response.clicked()
                    && ui.input(|i| i.modifiers.alt)
                    && let Some(p) = response.interact_pointer_pos()
                    && let Some(Grab::Bend(i)) = grab_at(&plot, view.points, p)
                {
                    out.points = Some(lfo_curve::set_bend(view.points, i, 0.0));
                    out.new_step = true;
                }
                if response.secondary_clicked()
                    && let Some(p) = response.interact_pointer_pos()
                {
                    let point = match grab_at(&plot, view.points, p) {
                        Some(Grab::Point(i)) => Some(i),
                        _ => None,
                    };
                    ui.data_mut(|d| d.insert_temp(menu_id, point));
                }
                let menu_point = ui.data(|d| d.get_temp::<Option<usize>>(menu_id)).flatten();
                let points = view.points;
                response.context_menu(|ui| {
                    let Some(i) = menu_point.filter(|i| *i < points.len()) else {
                        ui.label("Double-click to add a point");
                        return;
                    };
                    let end = i == 0 || i + 1 == points.len();
                    if ui.add_enabled(!end, egui::Button::new("Delete point")).clicked() {
                        out.points = Some(lfo_curve::delete_point(points, i));
                        out.new_step = true;
                        ui.close();
                    }
                    if ui.button("Straighten").clicked() {
                        let mut s = lfo_curve::set_bend(points, i, 0.0);
                        if i > 0 {
                            s = lfo_curve::set_bend(&s, i - 1, 0.0);
                        }
                        out.points = Some(s);
                        out.new_step = true;
                        ui.close();
                    }
                });
            }
            Some(paint) => {
                let pressed = response.drag_started() || response.clicked();
                if (pressed || response.dragged())
                    && let Some(p) = response.interact_pointer_pos()
                {
                    let (x, mut y) = plot.at(p);
                    if snap {
                        y = snap_to(y, gy);
                    }
                    let cell = ((x * gx as f32).floor() as i32).clamp(0, gx - 1);
                    let last = ui.data(|d| d.get_temp::<(i32, f32)>(paint_id));
                    if pressed || last != Some((cell, y)) {
                        let (x0, x1) = (cell as f32 / gx as f32, (cell + 1) as f32 / gx as f32);
                        out.points = Some(lfo_curve::paint_cell(view.points, x0, x1, y, paint));
                        out.new_step = pressed;
                        ui.data_mut(|d| d.insert_temp(paint_id, (cell, y)));
                    }
                }
                if response.drag_stopped() || (!response.dragged() && !pressed) {
                    ui.data_mut(|d| d.remove::<(i32, f32)>(paint_id));
                }
            }
        }
    }

    if ui.is_rect_visible(rect) {
        let points = out.points.as_deref().unwrap_or(view.points);
        paint_view(ui, rect, &plot, view, points, hover, drawn);
    }
    if view.tool == Tool::Points && drawn && hover.is_some_and(|p| grab_at(&plot, view.points, p).is_some()) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    out
}

fn paint_view(ui: &Ui, rect: Rect, plot: &Plot, view: &View, points: &[CurvePoint], hover: Option<Pos2>, drawn: bool) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.0, style::LINE_SOFT), egui::StrokeKind::Inside);
    let (gx, gy) = (view.grid.0.max(1), view.grid.1.max(1));
    for i in 0..=gx {
        let x = (plot.rect.left() + plot.rect.width() * i as f32 / gx as f32).round() + 0.5;
        painter.line_segment(
            [Pos2::new(x, plot.rect.top()), Pos2::new(x, plot.rect.bottom())],
            Stroke::new(1.0, style::LINE_SOFT),
        );
    }
    for j in 0..=gy {
        let y = (plot.rect.bottom() - plot.rect.height() * j as f32 / gy as f32).round() + 0.5;
        painter.line_segment(
            [Pos2::new(plot.rect.left(), y), Pos2::new(plot.rect.right(), y)],
            Stroke::new(1.0, style::LINE_SOFT),
        );
    }
    let mid = plot.pos(0.0, 0.5).y.round() + 0.5;
    painter.line_segment(
        [Pos2::new(plot.rect.left(), mid), Pos2::new(plot.rect.right(), mid)],
        Stroke::new(1.0, style::LINE),
    );

    // The wave: a drawn shape read point by point (jumps straight up and down), a random one
    // over eight cycles so its steps show.
    let numbers = bpe_kit::sound::curve_numbers(points);
    let line: Vec<Pos2> = if drawn {
        // The jumps exactly, so a step is square at any width.
        curve_line(plot, points)
    } else {
        let n = (plot.rect.width() as usize).clamp(64, 1200);
        (0..=n)
            .map(|i| {
                let x = i as f32 / n as f32;
                plot.pos(x, wave_at(view.wave, &numbers, (x * 8.0).min(8.0 - 1e-6) as f64, view.seed))
            })
            .collect()
    };
    let shade = view.colour.gamma_multiply(0.14);
    // A filled polygon must be convex in egui: fill strip by strip instead.
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (b.x - a.x).abs() < 0.01 {
            continue;
        }
        let base = plot.rect.bottom();
        painter.add(Shape::convex_polygon(vec![a, b, Pos2::new(b.x, base), Pos2::new(a.x, base)], shade, Stroke::NONE));
    }
    painter.add(Shape::line(line, Stroke::new(2.0, view.colour)));

    if drawn {
        for i in 0..points.len().saturating_sub(1) {
            if let Some((x, y)) = bend_dot(points, i) {
                let c = plot.pos(x, y);
                let hot = hover.is_some_and(|h| h.distance(c) <= GRAB);
                painter.circle_filled(c, 3.5, style::GROUND);
                painter.circle_stroke(c, 3.5, Stroke::new(1.2, if hot { style::TEXT_STRONG } else { style::MUTED }));
            }
        }
        for p in points {
            let c = plot.pos(p.x, p.y);
            let hot = hover.is_some_and(|h| h.distance(c) <= GRAB);
            painter.circle_filled(c, if hot { 6.0 } else { 4.5 }, if hot { style::TEXT_STRONG } else { style::TEXT });
        }
    }
    if let Some(at) = view.playhead {
        let x = at.clamp(0.0, 1.0);
        let v = if drawn { bpe_dsp::lfo::curve_at(&numbers, x as f64) } else { 0.5 };
        let top = plot.pos(x, 1.0);
        let px = top.x.round() + 0.5;
        painter.line_segment(
            [Pos2::new(px, plot.rect.top()), Pos2::new(px, plot.rect.bottom())],
            Stroke::new(1.0, style::TEXT_STRONG.gamma_multiply(0.6)),
        );
        if drawn {
            painter.circle_filled(plot.pos(x, v), 5.0, style::TEXT_STRONG);
        }
    }
}

/// The drawn shape as a line through the view: each part in steps, jumps as straight lines.
pub fn curve_line(plot: &Plot, points: &[CurvePoint]) -> Vec<Pos2> {
    let mut line = Vec::new();
    let steps_per_px = 0.5;
    for (i, a) in points.iter().enumerate() {
        line.push(plot.pos(a.x, a.y));
        let Some(b) = points.get(i + 1) else { break };
        let w = b.x - a.x;
        if w <= 0.0 {
            continue;
        }
        let steps = ((w * plot.rect.width() * steps_per_px) as usize).clamp(1, 400);
        for s in 1..steps {
            let t = s as f32 / steps as f32;
            line.push(plot.pos(a.x + w * t, a.y + (b.y - a.y) * bend_at(t, a.bend)));
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_and_the_bend_dot() {
        assert_eq!(snap_to(0.31, 4), 0.25);
        assert_eq!(snap_to(0.9, 1), 1.0);
        let pts = [
            CurvePoint::new(0.0, 0.0, 0.0),
            CurvePoint::new(0.5, 0.0, 0.0),
            CurvePoint::new(0.5, 1.0, 0.0),
            CurvePoint::new(1.0, 0.0, 0.0),
        ];
        assert_eq!(bend_dot(&pts, 0), Some((0.25, 0.0)));
        assert_eq!(bend_dot(&pts, 1), None, "a jump has no dot");
        assert_eq!(bend_dot(&pts, 2), Some((0.75, 0.5)));
        assert_eq!(bend_dot(&pts, 3), None);
    }

    #[test]
    fn the_plot_turns_numbers_into_places_and_back() {
        let plot = Plot::new(Rect::from_min_size(Pos2::ZERO, Vec2::new(216.0, 116.0)));
        let p = plot.pos(0.25, 0.75);
        assert_eq!(p, Pos2::new(58.0, 33.0));
        assert_eq!(plot.at(p), (0.25, 0.75));
    }
}
