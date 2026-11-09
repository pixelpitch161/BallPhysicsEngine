//! Colour Grading's colour wheel: the angle round the disc is the tint's hue (0 red at the right,
//! turning anticlockwise through yellow, green, cyan, blue and magenta, as Lightroom's wheel), the
//! distance from the middle its saturation (0 in the middle, 100 at the rim). Dragging the puck
//! (or clicking the disc) moves both; with Shift held it moves a tenth as far as the pointer; a
//! double-click puts it back in the middle. The disc shows the video's colours, which the design system allows.

use crate::style;
use bpe_gfx::grade::maths;
use eframe::egui::{self, Pos2, Sense, Stroke, Ui, Vec2};

/// Where the puck is during a drag (from the middle, in points), and how far Shift has moved it since.
#[derive(Clone, Copy, Debug)]
struct Drag {
    start: Vec2,
    moved: Vec2,
}

/// The hue (0..360) and saturation (0..100) the puck at `offset` from the middle of a wheel of
/// radius `r` stands for.
pub fn at_offset(offset: Vec2, r: f32) -> (f32, f32) {
    let sat = (offset.length() / r.max(1.0) * 100.0).clamp(0.0, 100.0);
    if sat < 0.05 {
        return (0.0, 0.0);
    }
    let hue = (-offset.y).atan2(offset.x).to_degrees().rem_euclid(360.0);
    ((hue * 10.0).round() / 10.0 % 360.0, (sat * 10.0).round() / 10.0)
}

/// The puck's offset from the middle for `hue` and `sat` on a wheel of radius `r`.
pub fn offset_of(hue: f32, sat: f32, r: f32) -> Vec2 {
    let (a, d) = (hue.to_radians(), sat.clamp(0.0, 100.0) / 100.0 * r);
    Vec2::new(a.cos() * d, -a.sin() * d)
}

/// The wheel's colour at `hue` and `sat` (0..1 from the middle out): grey in the middle.
fn colour_at(hue: f32, t: f32) -> egui::Color32 {
    let c = maths::hsv_colour(hue as f64);
    let grey = 0.33;
    let t = t as f64;
    style::video([0, 1, 2].map(|i| grey + (c[i] * 0.85 - grey) * t))
}

/// A wheel `size` points across named `name`, showing `hue` and `sat`. Returns the new hue and
/// saturation when they were moved.
pub fn wheel(ui: &mut Ui, name: &str, size: f32, hue: f32, sat: f32) -> Option<(f32, f32)> {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let words = format!("{name} wheel");
    response.widget_info(|| egui::WidgetInfo::slider(true, sat as f64, &words));
    let centre = rect.center();
    let r = size / 2.0 - 6.0;
    let painter = ui.painter_at(rect.expand(2.0));
    painter.add(disc(centre, r));
    painter.circle_stroke(centre, r, Stroke::new(1.0, style::LINE));
    let drag_id = response.id.with("wheel drag");
    let fine = ui.input(|i| i.modifiers.shift);
    let mut out = None;
    if response.double_clicked() {
        ui.ctx().data_mut(|d| d.remove::<Drag>(drag_id));
        out = Some((0.0, 0.0));
    } else if response.drag_started() || response.dragged() {
        // Without Shift the puck is where the pointer is; with Shift it moves a tenth as far as the
        // pointer, from where it was.
        let mut drag = ui
            .ctx()
            .data(|d| d.get_temp::<Drag>(drag_id))
            .unwrap_or(Drag { start: offset_of(hue, sat, r), moved: Vec2::ZERO });
        match response.interact_pointer_pos() {
            Some(p) if !fine => drag = Drag { start: p - centre, moved: Vec2::ZERO },
            _ => drag.moved += response.drag_delta() * 0.1,
        }
        let place = drag.start + drag.moved;
        let place = if place.length() > r { place * (r / place.length()) } else { place };
        // Kept on the rim, so a fine move back in starts from where the puck shows.
        ui.ctx().data_mut(|d| d.insert_temp(drag_id, Drag { start: place, moved: Vec2::ZERO }));
        let now = at_offset(place, r);
        if now != (hue, sat) {
            out = Some(now);
        }
    } else if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let place = p - centre;
        let place = if place.length() > r { place * (r / place.length()) } else { place };
        out = Some(at_offset(place, r));
    }
    if response.drag_stopped() {
        ui.ctx().data_mut(|d| d.remove::<Drag>(drag_id));
    }
    let (h, s) = out.unwrap_or((hue, sat));
    let puck = centre + offset_of(h, s, r);
    let hot = response.hovered() || response.dragged();
    painter.circle_filled(puck, if hot { 6.0 } else { 5.0 }, style::GROUND);
    painter.circle_stroke(puck, if hot { 6.0 } else { 5.0 }, Stroke::new(2.0, style::TEXT_STRONG));
    response.on_hover_text(format!(
        "{name}: drag the dot to tint (round for the colour, out for how much). Shift moves it finely, a double-click \
         puts it back in the middle."
    ));
    out
}

/// The disc's colours as a mesh: rings from grey in the middle out to full colour at the rim.
fn disc(centre: Pos2, r: f32) -> egui::Shape {
    const SEGMENTS: usize = 72;
    const RINGS: usize = 8;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(centre, colour_at(0.0, 0.0));
    for ring in 1..=RINGS {
        let t = ring as f32 / RINGS as f32;
        for s in 0..SEGMENTS {
            let hue = s as f32 * 360.0 / SEGMENTS as f32;
            mesh.colored_vertex(centre + offset_of(hue, t * 100.0, r), colour_at(hue, t));
        }
    }
    let at = |ring: usize, s: usize| (1 + (ring - 1) * SEGMENTS + s % SEGMENTS) as u32;
    for s in 0..SEGMENTS {
        mesh.add_triangle(0, at(1, s), at(1, s + 1));
    }
    for ring in 1..RINGS {
        for s in 0..SEGMENTS {
            let (a, b, c, d) = (at(ring, s), at(ring, s + 1), at(ring + 1, s), at(ring + 1, s + 1));
            mesh.add_triangle(a, c, b);
            mesh.add_triangle(b, c, d);
        }
    }
    egui::Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_puck_s_place_and_its_hue_and_saturation_go_both_ways() {
        let r = 50.0;
        for (hue, sat) in [(0.0, 100.0), (90.0, 50.0), (210.0, 18.0), (359.0, 75.0)] {
            let back = at_offset(offset_of(hue, sat, r), r);
            assert!((back.0 - hue).abs() < 0.11 && (back.1 - sat).abs() < 0.11, "{hue}, {sat}: {back:?}");
        }
        assert_eq!(at_offset(Vec2::new(0.0, -50.0), r), (90.0, 100.0), "up is a quarter turn: yellow-green");
        assert_eq!(at_offset(Vec2::new(200.0, 0.0), r), (0.0, 100.0), "past the rim is the rim");
        assert_eq!(at_offset(Vec2::ZERO, r), (0.0, 0.0));
    }
}
