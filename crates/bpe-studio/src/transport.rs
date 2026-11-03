//! Play, pause, the clock, loop, and the ruler to scrub with.

use crate::app::{Studio, clock};
use crate::style;
use eframe::egui::{self, Pos2, Rect, RichText, Sense, Stroke, Ui};

enum Icon {
    Play,
    Pause,
    Restart,
}

/// The first row: play, pause, the clock and loop.
pub(crate) fn controls(ui: &mut Ui, studio: &mut Studio) {
    ui.horizontal(|ui| {
        ui.set_min_height(40.0);
        let icon = if studio.playing { Icon::Pause } else { Icon::Play };
        if square_button(ui, icon).on_hover_text("Play or pause (Space)").clicked() {
            studio.set_playing(!studio.playing);
        }
        if square_button(ui, Icon::Restart).on_hover_text("Back to the start (Home)").clicked() {
            studio.seek(0.0);
        }
        ui.label(
            RichText::new(format!("{} / {}", clock(studio.time), clock(studio.duration()))).font(style::mono(14.0)),
        );
        ui.add_space(8.0);
        let looped = style::switch(ui, &mut studio.looping);
        let label = ui.label("Loop");
        looped.labelled_by(label.id).on_hover_text("Start again from the beginning at the end");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hint = "Drag the ruler to scrub";
            ui.label(RichText::new(hint).small().color(style::MUTED));
        });
    });
}

/// The ruler: the whole video as a thin bar with a mark per second and the playhead; click or
/// drag it to scrub. The bar starts `inset` points in from the left, so the lanes below it can
/// line up; the rect of the bar itself comes back.
pub(crate) fn scrub_bar(ui: &mut Ui, studio: &mut Studio, inset: f32) -> Rect {
    let duration = studio.duration().max(0.001);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), Sense::click_and_drag());
    let track = Rect::from_min_max(Pos2::new(rect.left() + inset, rect.top() + 2.0), rect.max - egui::vec2(0.0, 2.0));
    let painter = ui.painter_at(rect);
    painter.rect_filled(track, 0.0, style::GROUND);
    for second in 1..duration.ceil() as u32 {
        let x = track.left() + (second as f64 / duration) as f32 * track.width();
        let tall = if second % 5 == 0 { 7.0 } else { 4.0 };
        painter.vline(x, (track.bottom() - tall)..=track.bottom(), Stroke::new(1.0, style::LINE));
    }
    // The playhead, with a small handle to grab.
    let x = track.left() + (studio.time / duration) as f32 * track.width();
    painter.vline(x, rect.y_range(), Stroke::new(1.0, style::TEXT_STRONG));
    painter.add(egui::Shape::convex_polygon(
        vec![Pos2::new(x - 4.0, track.top()), Pos2::new(x + 4.0, track.top()), Pos2::new(x, track.top() + 5.0)],
        style::TEXT_STRONG,
        Stroke::NONE,
    ));

    if response.drag_started() {
        studio.resume_after_scrub = studio.playing;
        studio.set_playing(false);
    }
    if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let fraction = ((pos.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0) as f64;
        studio.seek(fraction * duration);
    }
    if response.drag_stopped() && std::mem::take(&mut studio.resume_after_scrub) {
        studio.set_playing(true);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    response.on_hover_text("Click or drag to move through the video");
    track
}

/// Play and pause: a light square with a dark icon. Restart: a frameless icon button.
fn square_button(ui: &mut Ui, icon: Icon) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), Sense::click());
    let painter = ui.painter();
    let ink = match icon {
        Icon::Play | Icon::Pause => {
            let fill = if response.hovered() { style::TEXT_STRONG } else { style::TEXT };
            painter.rect_filled(rect, style::RADIUS, fill);
            style::ON_LIGHT
        }
        Icon::Restart => {
            if response.hovered() {
                painter.rect_filled(rect, style::RADIUS, style::HOVER);
            }
            style::MUTED
        }
    };
    let c = rect.center();
    let s = 5.0;
    match icon {
        Icon::Play => {
            let points = vec![c + egui::vec2(-s * 0.7, -s), c + egui::vec2(s * 1.0, 0.0), c + egui::vec2(-s * 0.7, s)];
            painter.add(egui::Shape::convex_polygon(points, ink, Stroke::NONE));
        }
        Icon::Pause => {
            for dx in [-s * 0.5, s * 0.5] {
                let bar = Rect::from_center_size(c + egui::vec2(dx, 0.0), egui::vec2(s * 0.5, s * 2.0));
                painter.rect_filled(bar, 0.0, ink);
            }
        }
        Icon::Restart => {
            let bar = Rect::from_center_size(c + egui::vec2(-s * 0.8, 0.0), egui::vec2(2.0, s * 1.8));
            painter.rect_filled(bar, 0.0, ink);
            let points = vec![c + egui::vec2(s, -s * 0.9), c + egui::vec2(s, s * 0.9), c + egui::vec2(-s * 0.5, 0.0)];
            painter.add(egui::Shape::convex_polygon(points, ink, Stroke::NONE));
        }
    }
    let name = match icon {
        Icon::Play => "Play",
        Icon::Pause => "Pause",
        Icon::Restart => "Restart",
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    response
}
