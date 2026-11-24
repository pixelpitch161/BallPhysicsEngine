//! The arrangement at the bottom of Video mode (part 2 plan, Task 4): a bar ruler that moves the
//! playhead, the Scenes lane with the clips, and the Song lane with the song's waveform.
//!
//! A clip's middle drags to move it, its edges trim it; where two clips overlap a striped block is
//! their transition (a click picks it). Every drag works on the video as it was when the drag
//! began, so snapping never drifts, and the whole drag is one undo step. Snapping follows the
//! video's Snap (Bar, Beat, Off); Alt drags free. A kept scene carried from the left column and let
//! go over the lane makes a clip there.

use super::{ArrangeView, VideoPick, arrange};
use crate::app::{Studio, clock};
use crate::style;
use bpe_core::video::{ClipId, KeptId, Snap, TransitionKind, Video};
use eframe::egui::{self, Pos2, Rect, RichText, Sense, Stroke, Ui};

/// The lane names' column.
pub const LABEL_WIDTH: f32 = 64.0;
pub const RULER_HEIGHT: f32 = 20.0;
pub const SCENES_HEIGHT: f32 = 48.0;
pub const SONG_HEIGHT: f32 = 34.0;
/// How near (points) a clip's edge grabs it for trimming.
pub const EDGE: f32 = 6.0;

/// Which part of a clip a drag holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The middle: the clip moves.
    Body,
    /// The left edge: start and offset move together.
    Left,
    /// The right edge: the length.
    Right,
}

/// What is under a point of the Scenes lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Clip(ClipId, Part),
    /// The overlap before this clip: its transition in.
    Transition(ClipId),
}

/// A clip being dragged.
#[derive(Clone, Debug)]
pub struct ClipDrag {
    pub clip: ClipId,
    pub part: Part,
    /// The video as the drag began: each frame starts again from it.
    pub before: Video,
    /// The pointer's video time when the drag began.
    pub grab: f64,
}

/// What is under `x` in a lane drawn with `view` from `left_x`.
pub fn hit(video: &Video, view: &ArrangeView, left_x: f32, x: f32) -> Option<Hit> {
    let at = |t: f64| view.x_of(left_x, t);
    // Edges first, nearest wins; where two clips meet, the side the pointer is on.
    let mut best: Option<(f32, Hit)> = None;
    for clip in &video.clips {
        for (edge, part) in [(at(clip.start), Part::Left), (at(clip.end()), Part::Right)] {
            let d = (x - edge).abs();
            let outside = match part {
                Part::Left => x < edge,
                _ => x > edge,
            };
            let rank = d + if outside { 0.5 } else { 0.0 };
            if d <= EDGE && best.is_none_or(|(b, _)| rank < b || (rank == b && part == Part::Left)) {
                best = Some((rank, Hit::Clip(clip.id, part)));
            }
        }
    }
    if let Some((_, hit)) = best {
        return Some(hit);
    }
    for i in 1..video.clips.len() {
        if video.overlap(i) > 0.0 && x >= at(video.clips[i].start) && x <= at(video.clips[i - 1].end()) {
            return Some(Hit::Transition(video.clips[i].id));
        }
    }
    // The later clip is drawn on top.
    video.clips.iter().rev().find(|c| x >= at(c.start) && x <= at(c.end())).map(|c| Hit::Clip(c.id, Part::Body))
}

/// The short name of a transition, for its block.
pub fn kind_name(kind: TransitionKind) -> &'static str {
    match kind {
        TransitionKind::Cut => "Cut",
        TransitionKind::Crossfade => "Crossfade",
        TransitionKind::FadeThrough { .. } => "Fade through",
        TransitionKind::Flash { .. } => "Flash",
        TransitionKind::Wipe { .. } => "Wipe",
        TransitionKind::Push { .. } => "Push",
        TransitionKind::ZoomThrough => "Zoom through",
    }
}

/// Fits the whole video (at least 8 s) into a strip `width` points wide.
pub fn fit(view: &mut ArrangeView, video: &Video, width: f32) {
    let seconds = video.end().max(8.0) * 1.1;
    view.px_per_second = (width.max(50.0) / seconds as f32).clamp(2.0, 2000.0);
    view.left = 0.0;
}

/// The header row: Snap, Fit, the playhead.
fn header(ui: &mut Ui, studio: &mut Studio, strip_width: f32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("ARRANGEMENT").size(12.0).color(style::MUTED));
        ui.add_space(8.0);
        ui.label(RichText::new("Snap").small().color(style::MUTED));
        let now = studio.video.video.settings.snap;
        for (s, name, hint) in [
            (Snap::Bar, "Bar", "Clips land on bars (hold Alt to drag freely)"),
            (Snap::Beat, "Beat", "Clips land on beats (hold Alt to drag freely)"),
            (Snap::Off, "Off", "Clips land on any frame"),
        ] {
            let button = ui.add(style::toggle_button(name, now == s)).on_hover_text(hint);
            if button.clicked() && now != s {
                studio.video.change(false, |v| v.settings.snap = s);
            }
        }
        if ui.button("Fit").on_hover_text("Show the whole video").clicked() {
            fit(&mut studio.video.view, &studio.video.video, strip_width);
        }
        let s = &studio.video.video.settings;
        let (bar, beat) = arrange::bar_beat(s, studio.video.time);
        ui.label(RichText::new(format!("{} · bar {bar}.{beat}", clock(studio.video.time))).small().color(style::TEXT));
        if studio.video.video.clips.is_empty() {
            ui.add_space(12.0);
            if ui
                .add(style::primary_button("New video"))
                .on_hover_text("A video with the scene open in Build mode as its first clip")
                .clicked()
            {
                studio.ask_new_video(ui.ctx());
            }
        }
    });
}

/// Paints the beat lines (bars brighter) over a lane's strip.
fn paint_grid(painter: &egui::Painter, video: &Video, view: &ArrangeView, strip: Rect) {
    let (from, to) = (view.time_of(strip.left(), strip.left()), view.time_of(strip.left(), strip.right()));
    let beat_px = video.settings.beat_seconds() as f32 * view.px_per_second;
    for (t, bar) in arrange::grid_lines(&video.settings, from, to) {
        if !bar && beat_px < 6.0 {
            continue;
        }
        let x = view.x_of(strip.left(), t);
        let colour = if bar { style::LINE } else { style::LINE_SOFT };
        painter.line_segment([Pos2::new(x, strip.top()), Pos2::new(x, strip.bottom())], Stroke::new(1.0, colour));
    }
}

fn lane_name(ui: &Ui, rect: Rect, name: &str) {
    let cell = Rect::from_min_size(rect.min, egui::vec2(LABEL_WIDTH, rect.height()));
    ui.painter().text(
        cell.left_center() + egui::vec2(2.0, 0.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(11.0),
        style::MUTED,
    );
}

/// The ruler: bar numbers; a click or a drag moves the playhead.
fn ruler(ui: &mut Ui, studio: &mut Studio) -> Rect {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), RULER_HEIGHT), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "Playhead ruler"));
    let strip = Rect::from_min_max(Pos2::new(rect.left() + LABEL_WIDTH, rect.top()), rect.max);
    let view = studio.video.view;
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, style::GROUND);
    let s = &studio.video.video.settings;
    let (from, to) = (view.time_of(strip.left(), strip.left()), view.time_of(strip.left(), strip.right()));
    let bar_px = s.bar_seconds() as f32 * view.px_per_second;
    // Every bar's number when there's room, else every 2nd, 4th…
    let mut every = 1i64;
    while bar_px * (every as f32) < 28.0 && every < 4096 {
        every *= 2;
    }
    for (t, bar) in arrange::grid_lines(s, from, to) {
        let x = view.x_of(strip.left(), t);
        if bar {
            let (n, _) = arrange::bar_beat(s, t);
            painter.line_segment(
                [Pos2::new(x, strip.top() + 8.0), Pos2::new(x, strip.bottom())],
                Stroke::new(1.0, style::MUTED),
            );
            if (n - 1).rem_euclid(every) == 0 {
                painter.text(
                    Pos2::new(x + 3.0, strip.top() + 1.0),
                    egui::Align2::LEFT_TOP,
                    n.to_string(),
                    egui::FontId::proportional(10.0),
                    style::MUTED,
                );
            }
        } else if bar_px / f32::from(s.time_top.clamp(1, 16) as u8) >= 6.0 {
            painter.line_segment(
                [Pos2::new(x, strip.bottom() - 4.0), Pos2::new(x, strip.bottom())],
                Stroke::new(1.0, style::FAINT),
            );
        }
    }
    lane_name(ui, rect, "Bars");
    if (response.clicked() || response.dragged())
        && let Some(at) = response.interact_pointer_pos()
    {
        let t = view.time_of(strip.left(), at.x.max(strip.left()));
        studio.video.seek(studio.video.video.seconds(studio.video.video.frames(t)));
    }
    studio.video.view.ruler = Some(strip);
    strip
}

/// The Scenes lane: the clips, their overlaps and the drags on them.
fn scenes_lane(ui: &mut Ui, studio: &mut Studio) -> Rect {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), SCENES_HEIGHT), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Group, true, "Scenes lane"));
    let strip = Rect::from_min_max(Pos2::new(rect.left() + LABEL_WIDTH, rect.top()), rect.max);
    lane_name(ui, rect, "Scenes");
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, style::GROUND);
    let view = studio.video.view;
    let left = strip.left();
    paint_grid(&painter, &studio.video.video, &view, strip);
    let video = &studio.video.video;
    let picked = studio.video.pick();
    let body = |start: f64, end: f64| {
        Rect::from_min_max(
            Pos2::new(view.x_of(left, start), strip.top() + 4.0),
            Pos2::new(view.x_of(left, end), strip.bottom() - 4.0),
        )
    };
    for clip in &video.clips {
        let r = body(clip.start, clip.end());
        let Some(kept) = video.scenes.get(&clip.scene) else { continue };
        let (fill, edge) = style::clip_colours(Some(style::track_color(kept.colour)));
        painter.rect_filled(r, style::RADIUS, fill);
        let on = picked == VideoPick::Clip(clip.id);
        let stroke = if on { Stroke::new(2.0, style::TEXT_STRONG) } else { Stroke::new(1.0, edge) };
        painter.rect_stroke(r, style::RADIUS, stroke, egui::StrokeKind::Inside);
        let words = ui.painter_at(r.shrink(3.0));
        words.text(
            r.left_top() + egui::vec2(6.0, 5.0),
            egui::Align2::LEFT_TOP,
            &kept.name,
            egui::FontId::proportional(12.0),
            style::TEXT,
        );
        if clip.offset > 0.0 {
            words.text(
                r.left_bottom() + egui::vec2(6.0, -5.0),
                egui::Align2::LEFT_BOTTOM,
                format!("from {}", clock(clip.offset)),
                egui::FontId::proportional(10.0),
                style::MUTED,
            );
        }
    }
    for i in 1..video.clips.len() {
        if video.overlap(i) <= 0.0 {
            continue;
        }
        let incoming = &video.clips[i];
        let r = body(incoming.start, video.clips[i - 1].end());
        painter.rect_filled(r, 0.0, style::RAISED);
        let stripes = ui.painter_at(r);
        let mut x = r.left() - r.height();
        while x < r.right() {
            stripes.line_segment(
                [Pos2::new(x, r.bottom()), Pos2::new(x + r.height(), r.top())],
                Stroke::new(1.0, style::MUTED),
            );
            x += 7.0;
        }
        let on = picked == VideoPick::Transition(incoming.id);
        let stroke = if on { Stroke::new(2.0, style::TEXT_STRONG) } else { Stroke::new(1.0, style::TEXT) };
        painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
        if r.width() > 50.0 {
            let name = kind_name(incoming.transition_in.kind);
            let words = ui.painter_at(r);
            let pill = Rect::from_center_size(r.center(), egui::vec2(r.width().min(90.0) - 4.0, 16.0));
            words.rect_filled(pill, 3.0, style::RAISED);
            words.text(r.center(), egui::Align2::CENTER_CENTER, name, egui::FontId::proportional(10.0), style::TEXT);
        }
    }
    if video.clips.is_empty() {
        painter.text(
            strip.center(),
            egui::Align2::CENTER_CENTER,
            "Drop a scene here, or + Add scene",
            egui::FontId::proportional(12.0),
            style::MUTED,
        );
    }
    lane_input(ui, studio, &response, strip);
    studio.video.view.lane = Some(strip);
    strip
}

/// Picking, dragging and dropping on the Scenes lane.
fn lane_input(ui: &mut Ui, studio: &mut Studio, response: &egui::Response, strip: Rect) {
    let left = strip.left();
    let view = studio.video.view;
    let free = ui.input(|i| i.modifiers.alt);
    if let Some(at) = response.hover_pos() {
        match hit(&studio.video.video, &view, left, at.x) {
            Some(Hit::Clip(_, Part::Left | Part::Right)) => {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal)
            }
            Some(Hit::Clip(_, Part::Body)) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
            _ => {}
        }
    }
    if response.clicked()
        && let Some(at) = response.interact_pointer_pos()
    {
        studio.video.pick = match hit(&studio.video.video, &view, left, at.x) {
            Some(Hit::Clip(id, _)) => VideoPick::Clip(id),
            Some(Hit::Transition(id)) => VideoPick::Transition(id),
            None => VideoPick::Video,
        };
    }
    if response.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        let grab = view.time_of(left, origin.x);
        let held = match hit(&studio.video.video, &view, left, origin.x) {
            Some(Hit::Clip(id, part)) => Some((id, part)),
            // Dragging a transition moves the clip coming in.
            Some(Hit::Transition(id)) => Some((id, Part::Body)),
            None => None,
        };
        if let Some((clip, part)) = held {
            studio.video.pick = VideoPick::Clip(clip);
            studio.video.drag = Some(ClipDrag { clip, part, before: studio.video.video.clone(), grab });
        }
    }
    if response.dragged()
        && let (Some(drag), Some(at)) = (&studio.video.drag, response.interact_pointer_pos())
    {
        let delta = view.time_of(left, at.x) - drag.grab;
        let mut video = drag.before.clone();
        let reg = &studio.kit.registry;
        if let Some(clip) = arrange::clip(&drag.before, drag.clip) {
            let s = &drag.before.settings;
            match drag.part {
                Part::Body => {
                    let start = arrange::snap_move(clip.start + delta, clip.length, s, free);
                    arrange::move_clip(&mut video, reg, drag.clip, start);
                }
                Part::Left => {
                    arrange::trim_start(&mut video, reg, drag.clip, arrange::snap(clip.start + delta, s, free))
                }
                Part::Right => {
                    arrange::trim_end(&mut video, reg, drag.clip, arrange::snap(clip.end() + delta, s, free))
                }
            }
        }
        if video != studio.video.video {
            studio.video.video = video;
            studio.video.touched();
        }
    }
    if response.drag_stopped()
        && let Some(drag) = studio.video.drag.take()
    {
        studio.video.commit(drag.before);
    }
    // A kept scene carried here from the left column and let go.
    let released = ui.input(|i| i.pointer.any_released());
    if released
        && let Some(at) = ui.ctx().pointer_latest_pos().filter(|p| strip.contains(*p))
        && let Some(kept) = egui::DragAndDrop::take_payload::<KeptId>(ui.ctx())
    {
        let start = arrange::snap(view.time_of(left, at.x), &studio.video.video.settings, free);
        drop_scene(studio, *kept, start);
    }
}

/// Makes a clip of kept scene `kept` at `start`, as one undo step, and picks it.
pub fn drop_scene(studio: &mut Studio, kept: KeptId, start: f64) {
    let kit = studio.kit.clone();
    let mut made = None;
    studio.video.change(false, |v| made = arrange::add_clip(v, &kit.registry, kept, start));
    if let Some(id) = made {
        studio.video.pick = VideoPick::Clip(id);
    }
}

/// The Song lane: the song's waveform where it plays, dimmed while it is off.
fn song_lane(ui: &mut Ui, studio: &Studio) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), SONG_HEIGHT), Sense::hover());
    let strip = Rect::from_min_max(Pos2::new(rect.left() + LABEL_WIDTH, rect.top()), rect.max);
    lane_name(ui, rect, "Song");
    let painter = ui.painter_at(strip);
    painter.rect_filled(strip, 0.0, style::GROUND);
    let view = studio.video.view;
    paint_grid(&painter, &studio.video.video, &view, strip);
    let Some(song) = &studio.video.video.song else {
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, "Song lane, no song"));
        painter.text(
            strip.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "No song: load one in the video's settings (click an empty part of the Scenes lane)",
            egui::FontId::proportional(11.0),
            style::FAINT,
        );
        return;
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, format!("Song lane, {}", song.name)));
    let Some(Ok(data)) = bpe_sound::SampleBank::ready(&song.audio.bytes) else {
        painter.text(
            strip.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "Reading the song…",
            egui::FontId::proportional(11.0),
            style::MUTED,
        );
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        return;
    };
    let peaks = data.peaks();
    let seconds = data.seconds();
    let colour = if song.on { style::TEXT.gamma_multiply(0.7) } else { style::FAINT };
    let mid = strip.center().y;
    let half = strip.height() * 0.45;
    let start = song.starts_at;
    let left = strip.left();
    let (from, to) = (view.x_of(left, start).max(strip.left()), view.x_of(left, start + seconds).min(strip.right()));
    let mut x = from.floor();
    let n = peaks.len();
    while x < to && n > 0 {
        let (a, b) = (view.time_of(left, x) - start, view.time_of(left, x + 1.0) - start);
        let i = ((a.max(0.0) / seconds * n as f64) as usize).min(n - 1);
        let j = ((b.max(0.0) / seconds * n as f64).ceil() as usize).clamp(i + 1, n);
        let peak = peaks[i..j].iter().fold(0.0f32, |m, v| m.max(*v));
        let h = (peak.min(1.0) * half).max(0.5);
        painter.line_segment([Pos2::new(x + 0.5, mid - h), Pos2::new(x + 0.5, mid + h)], Stroke::new(1.0, colour));
        x += 1.0;
    }
}

/// The whole arrangement.
pub fn show(ui: &mut Ui, studio: &mut Studio) {
    let strip_width = (ui.available_width() - LABEL_WIDTH).max(50.0);
    if studio.video.view.px_per_second <= 0.0 {
        fit(&mut studio.video.view, &studio.video.video, strip_width);
    }
    header(ui, studio, strip_width);
    ui.add_space(4.0);
    let top = ui.cursor().top();
    let ruler = ruler(ui, studio);
    let lane = scenes_lane(ui, studio);
    ui.add_space(2.0);
    song_lane(ui, studio);
    let bottom = ui.cursor().top();
    // Wheel scrolls along the video; Ctrl and the wheel zooms about the pointer.
    let lanes = Rect::from_min_max(Pos2::new(lane.left(), top), Pos2::new(lane.right(), bottom));
    if let Some(at) = ui.ctx().pointer_hover_pos().filter(|p| lanes.contains(*p)) {
        let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        let view = &mut studio.video.view;
        if zoom != 1.0 {
            let t = view.time_of(lane.left(), at.x);
            view.px_per_second = (view.px_per_second * zoom).clamp(2.0, 2000.0);
            view.left = (t - f64::from(at.x - lane.left()) / f64::from(view.px_per_second)).max(0.0);
        } else {
            let along = if scroll.x != 0.0 { scroll.x } else { scroll.y };
            if along != 0.0 {
                view.left = (view.left - f64::from(along / view.px_per_second)).max(0.0);
            }
        }
    }
    // The playhead across the ruler and the lanes.
    let x = studio.video.view.x_of(ruler.left(), studio.video.time);
    if x >= ruler.left() && x <= ruler.right() {
        ui.painter().line_segment([Pos2::new(x, top), Pos2::new(x, bottom)], Stroke::new(1.5, style::TEXT_STRONG));
    }
}
