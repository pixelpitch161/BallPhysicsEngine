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

/// The first row: play, pause, the clock, loop and mute.
pub(crate) fn controls(ui: &mut Ui, studio: &mut Studio) {
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
        ui.add_space(8.0);
        let target = studio.settings().loudness;
        meters(ui, studio.panels.sound.report.as_ref(), studio.time, target);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let hint = if studio.panels.sound.beats.is_empty() {
                "Drag the bar to scrub · ticks are hits"
            } else {
                "Drag the bar to scrub · ticks are hits · marks below are beats"
            };
            ui.label(RichText::new(hint).small().color(theme::MUTED));
        });
    });
}

/// The whole video as a bar: hits as ticks, beats as marks along the bottom, the playhead,
/// and dragging to scrub. The bar starts `inset` points in from the left, so the lanes below
/// it can line up; the rect of the bar itself comes back.
pub(crate) fn scrub_bar(ui: &mut Ui, studio: &mut Studio, inset: f32) -> Rect {
    let duration = studio.duration().max(0.001);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), Sense::click_and_drag());
    let track = Rect::from_min_max(Pos2::new(rect.left() + inset, rect.top() + 6.0), rect.max - egui::vec2(0.0, 6.0));
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
    // Beats, along the bottom.
    for &beat in &studio.panels.sound.beats {
        if beat < 0.0 || beat as f64 > duration {
            continue;
        }
        let x = track.left() + (beat as f64 / duration) as f32 * track.width();
        painter.line_segment(
            [Pos2::new(x, track.bottom() - 6.0), Pos2::new(x, track.bottom() - 1.0)],
            Stroke::new(1.0, theme::WARNING),
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
    track
}

/// Two small meters at the playhead: short-term loudness on a -40 to 0 LUFS scale with a tick
/// at the `target`, and the limiter's gain reduction on a 0 to 12 dB scale. Each reads "--"
/// until a mix has been measured.
pub(crate) fn meters(ui: &mut Ui, report: Option<&bpe_mix::MasterReport>, time: f64, target: f32) {
    let reading = report.map(|r| r.at(time));
    let bar = |ui: &mut Ui, fill: Option<f32>, tick: Option<f32>, color: Color32| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(60.0, 8.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, theme::RAISED);
        if let Some(f) = fill {
            let right = rect.left() + rect.width() * f.clamp(0.0, 1.0);
            painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(right, rect.bottom())), 3.0, color);
        }
        if let Some(t) = tick {
            let x = rect.left() + rect.width() * t.clamp(0.0, 1.0);
            painter.line_segment(
                [Pos2::new(x, rect.top() - 2.0), Pos2::new(x, rect.bottom() + 2.0)],
                Stroke::new(1.5, theme::TEXT),
            );
        }
    };
    let loud = reading.map(|(lufs, _)| lufs);
    bar(ui, loud.map(|l| (l + 40.0) / 40.0), Some((target + 40.0) / 40.0), theme::ACCENT);
    ui.label(RichText::new(loud.map_or("--".into(), |l| format!("{l:.0} LUFS"))).small().monospace())
        .on_hover_text("Loudness over the last 3 seconds; the tick is the target");
    let reduction = reading.map(|(_, gr)| gr);
    bar(ui, reduction.map(|g| g / 12.0), None, theme::WARNING);
    ui.label(RichText::new(reduction.map_or("--".into(), |g| format!("GR {g:.1} dB"))).small().monospace())
        .on_hover_text("How much the limiter is turning the mix down here");
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

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_mix::MasterReport;
    use egui_kittest::{Harness, kittest::Queryable};

    fn harness(report: Option<MasterReport>, time: f64) -> Harness<'static, Option<MasterReport>> {
        let mut h = Harness::builder()
            .with_size(egui::vec2(400.0, 60.0))
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(move |ui, r: &mut Option<MasterReport>| meters(ui, r.as_ref(), time, -14.0), report);
        h.run();
        h
    }

    #[test]
    fn meters_read_the_playhead() {
        let mut short_term = vec![-30.0; 40];
        short_term[10] = -20.0;
        let mut reduction = vec![0.0; 40];
        reduction[10] = 3.0;
        let report = MasterReport { integrated: -14.0, true_peak: -1.0, short_term, reduction };
        let h = harness(Some(report), 1.0);
        h.get_by_label("-20 LUFS");
        h.get_by_label("GR 3.0 dB");
        let h = harness(None, 1.0);
        assert_eq!(h.get_all_by_label("--").count(), 2, "no mix yet: both meters blank");
    }
}
