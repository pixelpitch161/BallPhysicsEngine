//! Painting helpers for the node canvas: the grid, wires and trigger pins.

use crate::theme;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, emath::TSTransform, epaint::CubicBezierShape, pos2, vec2};

/// Dots every 20 canvas units, thinning out when zoomed far out.
pub fn grid(painter: &egui::Painter, screen: Rect, to_screen: TSTransform) {
    let mut step = 20.0 * to_screen.scaling;
    while step < 12.0 {
        step *= 4.0;
    }
    let origin = to_screen.translation;
    let start_x = screen.left() + (origin.x - screen.left()).rem_euclid(step);
    let start_y = screen.top() + (origin.y - screen.top()).rem_euclid(step);
    let color = theme::LINE.gamma_multiply(0.8);
    let mut y = start_y;
    while y < screen.bottom() {
        let mut x = start_x;
        while x < screen.right() {
            painter.circle_filled(pos2(x, y), 1.0, color);
            x += step;
        }
        y += step;
    }
}

fn controls(a: Pos2, b: Pos2) -> [Pos2; 4] {
    let bend = ((b.x - a.x).abs() * 0.5).max(40.0);
    [a, a + vec2(bend, 0.0), b - vec2(bend, 0.0), b]
}

/// A wire from an output on the right of a block to an input on the left of another.
pub fn wire(painter: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    painter.add(CubicBezierShape::from_points_stroke(controls(a, b), false, Color32::TRANSPARENT, stroke));
}

/// Whether `p` is within `reach` of the wire from `a` to `b`.
pub fn near_wire(a: Pos2, b: Pos2, p: Pos2, reach: f32) -> bool {
    let bounds = Rect::from_two_pos(a, b).expand2(vec2(reach + (b.x - a.x).abs() * 0.5 + 40.0, reach));
    if !bounds.contains(p) {
        return false;
    }
    let curve = CubicBezierShape::from_points_stroke(controls(a, b), false, Color32::TRANSPARENT, Stroke::NONE);
    let points = curve.flatten(Some(1.0));
    points.windows(2).any(|w| distance_to_segment(p, w[0], w[1]) < reach)
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// Triggers get a small arrow instead of a dot, so "this happened" wires read differently from
/// values.
pub fn trigger_pin(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32, filled: bool) {
    let r = radius * 1.15;
    let points = vec![center + vec2(-r * 0.8, -r), center + vec2(r, 0.0), center + vec2(-r * 0.8, r)];
    let fill = if filled { color } else { theme::CARD };
    painter.add(egui::Shape::convex_polygon(points, fill, Stroke::new(2.0, color)));
}
