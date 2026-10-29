//! Play, pause, the clock, and the scrub bar with its lane of hits.

use crate::app::{Studio, clock};
use crate::theme;
use bpe_runtime::EventKind;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui};

enum Icon {
    Play,
    Pause,
    Restart,
}

pub fn show(ui: &mut Ui, studio: &mut Studio) {
    ui.horizontal(|ui| {
        let icon = if studio.playing { Icon::Pause } else { Icon::Play };
        if round_button(ui, 34.0, icon).on_hover_text("Play or pause (Space)").clicked() {
            studio.set_playing(!studio.playing);
        }
        if round_button(ui, 28.0, Icon::Restart).on_hover_text("Back to the start (Home)").clicked() {
            studio.seek(0.0);
        }
        ui.monospace(format!("{} / {}", clock(studio.time), clock(studio.duration())));
        ui.add_space(8.0);
        ui.checkbox(&mut studio.looping, "Loop").on_hover_text("Start again from the beginning at the end");
        if ui.checkbox(&mut studio.muted, "Mute").on_hover_text("Preview without sound").changed() {
            studio.sync_mute();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hint = "Drag the bar to scrub · ticks are hits";
            ui.label(RichText::new(hint).small().color(theme::MUTED));
        });
    });
    ui.add_space(4.0);
    scrub_bar(ui, studio);
}

/// The whole video as a bar: hits as ticks, the playhead, and dragging to scrub.
fn scrub_bar(ui: &mut Ui, studio: &mut Studio) {
    let duration = studio.duration().max(0.001);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click_and_drag());
    let track = rect.shrink2(egui::vec2(0.0, 6.0));
    let painter = ui.painter_at(rect);
    painter.rect_filled(track, 6.0, theme::RAISED);

    // Hits per pixel column: from the finished mix when there is one, else what the playback
    // has got to so far.
    let columns = track.width().max(1.0) as usize;
    let mut counts = vec![0u32; columns];
    let mut count = |t: f32| {
        let column = ((t / duration as f32) * columns as f32) as usize;
        if let Some(c) = counts.get_mut(column.min(columns - 1)) {
            *c += 1;
        }
    };
    if studio.panels.sound.hits.is_empty() {
        let rate = studio.playback.world().steps_per_second;
        for e in studio.playback.log().events().iter().filter(|e| e.kind == EventKind::Hit) {
            count(e.time(rate) as f32);
        }
    } else {
        for &t in &studio.panels.sound.hits {
            count(t);
        }
    }
    for (column, &c) in counts.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let x = track.left() + column as f32 + 0.5;
        let alpha = (0.35 + c as f32 * 0.2).min(1.0);
        let color = theme::ACCENT_TEXT.gamma_multiply(alpha);
        painter.line_segment(
            [Pos2::new(x, track.top() + 5.0), Pos2::new(x, track.bottom() - 5.0)],
            Stroke::new(1.0, color),
        );
    }

    // Second marks.
    for second in 1..duration.ceil() as u32 {
        let x = track.left() + (second as f64 / duration) as f32 * track.width();
        painter.line_segment(
            [Pos2::new(x, track.bottom() - 3.0), Pos2::new(x, track.bottom())],
            Stroke::new(1.0, theme::LINE),
        );
    }

    // The playhead.
    let x = track.left() + (studio.time / duration) as f32 * track.width();
    painter.rect_filled(
        Rect::from_min_max(track.min, Pos2::new(x, track.max.y)),
        6.0,
        theme::ACCENT.gamma_multiply(0.2),
    );
    painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(2.0, theme::ACCENT));
    painter.circle_filled(Pos2::new(x, rect.top() + 3.0), 4.0, theme::ACCENT);

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
}

fn round_button(ui: &mut Ui, size: f32, icon: Icon) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
    let fill = match icon {
        Icon::Play | Icon::Pause => {
            if response.hovered() {
                theme::ACCENT_TEXT
            } else {
                theme::ACCENT
            }
        }
        Icon::Restart => {
            if response.hovered() {
                theme::HOVER
            } else {
                theme::RAISED
            }
        }
    };
    let painter = ui.painter();
    painter.circle_filled(rect.center(), size / 2.0, fill);
    let c = rect.center();
    let s = size * 0.2;
    let white = Color32::WHITE;
    match icon {
        Icon::Play => {
            let points = vec![c + egui::vec2(-s * 0.7, -s), c + egui::vec2(s * 1.1, 0.0), c + egui::vec2(-s * 0.7, s)];
            painter.add(egui::Shape::convex_polygon(points, white, Stroke::NONE));
        }
        Icon::Pause => {
            for dx in [-s * 0.55, s * 0.55] {
                let bar = Rect::from_center_size(c + egui::vec2(dx, 0.0), egui::vec2(s * 0.5, s * 2.0));
                painter.rect_filled(bar, 1.0, white);
            }
        }
        Icon::Restart => {
            let s = size * 0.22;
            let bar = Rect::from_center_size(c + egui::vec2(-s * 0.9, 0.0), egui::vec2(s * 0.35, s * 1.8));
            painter.rect_filled(bar, 0.5, white);
            let points = vec![c + egui::vec2(s, -s * 0.9), c + egui::vec2(s, s * 0.9), c + egui::vec2(-s * 0.6, 0.0)];
            painter.add(egui::Shape::convex_polygon(points, white, Stroke::NONE));
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
