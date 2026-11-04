//! The drawn pieces of the Design System that egui doesn't have as they are: inverted buttons,
//! square switches, section headers, the changed dot, key diamonds and the track colour picker.

use super::fonts::semibold;
use super::tokens::*;
use super::track::track_color;
use bpe_core::TrackColour;
use eframe::egui::{self, Button, Color32, Painter, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};

/// The one primary action on screen (Export video…): a light fill with a dark semibold label.
pub fn primary_button(text: &str) -> Button<'static> {
    Button::new(RichText::new(text).font(semibold(13.0)).color(ON_LIGHT)).fill(TEXT).stroke(Stroke::new(1.0, TEXT))
}

/// A button that stays down while `on` (Grid, Snap): inverted when on, plain when off.
pub fn toggle_button(text: &str, on: bool) -> Button<'static> {
    if on {
        Button::new(RichText::new(text).color(ON_LIGHT)).fill(TEXT).stroke(Stroke::new(1.0, TEXT))
    } else {
        Button::new(text.to_owned())
    }
}

/// A toolbar button: no outline, only the hover fill.
pub fn toolbar_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(Button::new(text.to_owned()).frame_when_inactive(false))
}

/// A square on/off switch: a 30 × 16 outline with a square knob, light and to the right when on.
/// Reads as a checkbox to screen readers and tests.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(30.0, 16.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let state = *on;
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), state, ""));
    if ui.is_rect_visible(rect) {
        let ink = if !ui.is_enabled() {
            FAINT
        } else if state {
            TEXT
        } else if response.hovered() {
            MUTED
        } else {
            FAINT
        };
        let painter = ui.painter();
        painter.rect_stroke(rect, RADIUS, Stroke::new(1.0, ink), egui::StrokeKind::Inside);
        let knob = Vec2::splat(10.0);
        let x = if state { rect.right() - 3.0 - knob.x } else { rect.left() + 3.0 };
        painter.rect_filled(Rect::from_min_size(Pos2::new(x, rect.center().y - knob.y / 2.0), knob), 1.0, ink);
    }
    response
}

/// A chevron pointing right when folded and down when open.
pub fn paint_chevron(painter: &Painter, rect: Rect, open: bool, ink: Color32) {
    let c = rect.center();
    let r = 3.5;
    let points = if open {
        [c + Vec2::new(-r, -r * 0.5), c + Vec2::new(0.0, r * 0.5), c + Vec2::new(r, -r * 0.5)]
    } else {
        [c + Vec2::new(-r * 0.5, -r), c + Vec2::new(r * 0.5, 0.0), c + Vec2::new(-r * 0.5, r)]
    };
    painter.line(points.to_vec(), Stroke::new(1.5, ink));
}

/// The small ring before a label: this thing changed the setting from its default.
pub fn changed_dot(painter: &Painter, center: Pos2) {
    painter.circle_stroke(center, 2.5, Stroke::new(1.0, TEXT));
}

/// A keyframe's diamond: white with a dark outline. A selected key gets a light ring around it.
pub fn paint_diamond(painter: &Painter, center: Pos2, size: f32, selected: bool) {
    let points = |s: f32| {
        vec![
            center + Vec2::new(0.0, -s),
            center + Vec2::new(s, 0.0),
            center + Vec2::new(0.0, s),
            center + Vec2::new(-s, 0.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(points(size), TEXT_STRONG, Stroke::new(1.0, GROUND)));
    if selected {
        painter.add(egui::Shape::closed_line(points(size + 3.0), Stroke::new(1.5, TEXT_STRONG)));
    }
}

/// A 1px divider across the space left.
pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, LINE_SOFT);
}

/// A thin vertical separator between toolbar groups.
pub fn bar_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(13.0, 18.0), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0, LINE));
}

/// A colour picker for a thing's track colour, showing `current`: the hue and shade square, the
/// hue strip and a hex box named "Colour hex". The colour comes back when it is changed.
pub fn track_picker(ui: &mut Ui, current: Option<TrackColour>) -> Option<TrackColour> {
    let mut picked = None;
    let mut color = track_color(current.unwrap_or(TrackColour::PINK));
    ui.spacing_mut().slider_width = 180.0;
    if egui::color_picker::color_picker_color32(ui, &mut color, egui::color_picker::Alpha::Opaque) {
        picked = Some(super::track::track_of(color));
    }
    ui.horizontal(|ui| {
        let label = ui.label(RichText::new("Hex").small().color(MUTED));
        // The box keeps what is being typed until it reads as a colour.
        let id = ui.make_persistent_id("track colour hex");
        let shown = current.map(|c| c.hex()).unwrap_or_default();
        let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| shown.clone());
        let edit = ui.add(egui::TextEdit::singleline(&mut text).desired_width(80.0)).labelled_by(label.id);
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Colour hex"));
        if edit.changed()
            && let Some(c) = TrackColour::parse(&text)
        {
            picked = Some(c);
        }
        if edit.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, text));
        } else {
            ui.data_mut(|d| d.remove::<String>(id));
        }
    });
    picked
}

/// A small square showing a track colour, as the button that opens the picker.
pub fn swatch_button(ui: &mut Ui, colour: Option<TrackColour>, name: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(CONTROL_H), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    let painter = ui.painter();
    painter.rect_filled(rect, RADIUS, GROUND);
    let edge = if response.hovered() { MUTED } else { LINE };
    painter.rect_stroke(rect, RADIUS, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    let fill = colour.map_or(FAINT, track_color);
    painter.rect_filled(Rect::from_center_size(rect.center(), Vec2::splat(14.0)), 0.0, fill);
    response
}

/// The fill and edge of a timeline clip for a thing of `color` (its track colour, or `None` for
/// the scene settings).
pub fn clip_colours(color: Option<Color32>) -> (Color32, Color32) {
    match color {
        Some(c) => (super::track::mix_over_panel(c), c),
        None => (RAISED, MUTED),
    }
}
