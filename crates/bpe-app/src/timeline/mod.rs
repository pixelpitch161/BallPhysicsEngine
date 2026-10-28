//! The timeline under the preview: a ruler to scrub along, the video's end to drag, lanes that
//! show what happens over time (song, notes, bounces, rules, text), and a row of keyframes for
//! each animated setting.
//!
//! Each lane is its own file implementing [`lanes::Lane`]; the timeline stacks them under the
//! ruler, each with a header that collapses it.

mod bounces;
mod ease;
pub mod lanes;
mod notes;
/// WS5: settings that pulse with the music.
mod pulse;
mod rules;
mod song;
mod text;

pub use lanes::{Lane, LaneContext, Scale};
pub use rules::rule_color;
pub use song::MusicView;

use crate::inspector::Tab;
use crate::keys::{self, KeyState};
use crate::theme;
use bpe_scene::{Ease, Property, SAME_TIME, Scene, Value};
use bpe_sim::{Event, EventKind};
use eframe::egui::{self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, Ui};

/// Width of the names column on the left.
const HEADER: f32 = 190.0;
const RULER: f32 = 22.0;
const ROW: f32 = 26.0;
/// Height of a collapsed lane.
const COLLAPSED: f32 = 16.0;
/// Screen points within which a dragged keyframe snaps to the playhead, the start or the end.
const SNAP: f32 = 6.0;
/// Bounce counts are kept in slices of this many per second.
const BUCKETS_PER_SECOND: f32 = 20.0;

/// When balls hit things over the whole video, worked out with the soundtrack.
#[derive(Clone, Default)]
pub struct HitSummary {
    /// Hits in each slice of 1/[`BUCKETS_PER_SECOND`] seconds.
    counts: Vec<u32>,
    /// Times balls escaped a ring.
    escapes: Vec<f32>,
    total: usize,
    /// The whole video's event log, for lanes that show more than bounces.
    #[allow(dead_code)] // read by the rules, notes and text lanes as they fill in
    pub events: std::sync::Arc<Vec<Event>>,
    /// Music workstream: the song, beats and notes for the Song and Notes lanes.
    pub music: Option<std::sync::Arc<song::MusicView>>,
}

impl HitSummary {
    pub fn new(events: &[Event]) -> Self {
        let mut counts = Vec::new();
        let mut escapes = Vec::new();
        let mut total = 0;
        for hit in events.iter().filter(|e| e.kind.is_bounce()) {
            total += 1;
            if let EventKind::Escape(_) = hit.kind {
                escapes.push(hit.time as f32);
                continue;
            }
            let bucket = (hit.time as f32 * BUCKETS_PER_SECOND) as usize;
            if counts.len() <= bucket {
                counts.resize(bucket + 1, 0);
            }
            counts[bucket] += 1;
        }
        Self { counts, escapes, total, events: std::sync::Arc::new(events.to_vec()), music: None }
    }

    /// The most hits in any slice from `from` to `to` seconds.
    fn busiest(&self, from: f32, to: f32) -> u32 {
        let a = (from * BUCKETS_PER_SECOND).max(0.0) as usize;
        let b = ((to * BUCKETS_PER_SECOND).ceil() as usize).max(a + 1).min(self.counts.len());
        self.counts.get(a..b).and_then(|s| s.iter().max().copied()).unwrap_or(0)
    }
}

#[derive(Clone, PartialEq)]
enum Drag {
    /// Scrubbing the playhead.
    Scrub,
    /// Moving the end of the video.
    End,
    /// Moving a keyframe, by its setting and current index.
    Key { property: Property, index: usize },
}

pub struct Timeline {
    hits: Option<HitSummary>,
    /// Song, notes, bounces, rules and text, top to bottom.
    lanes: Vec<Box<dyn Lane>>,
    /// Lanes folded down to their header, by id.
    collapsed: std::collections::BTreeSet<&'static str>,
    drag: Option<Drag>,
    /// The selected keyframe, by setting and index.
    selected: Option<(Property, usize)>,
    /// Seconds the timeline shows, held still while the end of the video is dragged.
    held_span: Option<f32>,
    /// How many settings were animated last frame, to scroll to a newly animated one.
    rows_seen: usize,
    /// Caption clips (WS7).
    text: text::TextLane,
}

/// What the app should do after the timeline is drawn.
#[derive(Default)]
pub struct Outcome {
    pub seek: Option<f64>,
    /// Scrubbing started (true) or stopped (false) this frame.
    pub scrubbing: Option<bool>,
    /// A setting's name was clicked: show the tab it lives in.
    pub show_tab: Option<Tab>,
    /// A caption's clip was clicked: show it in the Text tab.
    pub caption: Option<usize>,
}

/// Rounds to the nearest video frame.
fn to_frame(time: f32, fps: u32) -> f32 {
    let fps = fps.max(1) as f32;
    (time * fps).round() / fps
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            hits: None,
            lanes: vec![
                Box::new(song::SongLane::default()),
                Box::new(notes::NotesLane::default()),
                Box::new(bounces::BouncesLane),
                Box::new(pulse::PulseLane::default()),
                Box::new(rules::RulesLane::default()),
            ],
            text: text::TextLane::default(),
            collapsed: Default::default(),
            drag: None,
            selected: None,
            held_span: None,
            rows_seen: 0,
        }
    }
}

impl Timeline {
    pub fn set_hits(&mut self, hits: HitSummary) {
        self.hits = Some(hits);
    }

    /// Forgets the bounces shown, while a changed scene is worked out again.
    pub fn clear_hits(&mut self) {
        self.hits = None;
    }

    /// Whether a keyframe is selected, so Delete removes it.
    pub fn selected(&self) -> bool {
        self.selected.is_some()
    }

    /// Deletes the selected keyframe. Deleting a setting's last keyframe stops animating it.
    pub fn delete_selected(&mut self, scene: &mut Scene) {
        let Some((property, index)) = self.selected.take() else { return };
        let Some(track) = scene.track_mut(&property) else { return };
        if index >= track.keys.len() {
            return;
        }
        let removed = track.keys.remove(index);
        if track.keys.is_empty() {
            scene.animation.retain(|t| t.property != property);
            property.set(scene, removed.value);
        }
    }

    pub fn show(&mut self, ui: &mut Ui, scene: &mut Scene, time: f64) -> Outcome {
        let mut outcome = Outcome::default();
        let time = time as f32;
        let duration = scene.video.duration;
        let fps = scene.video.fps;
        // Keyframes are added on whole frames, so that is where "a keyframe here" is checked.
        let key_time = to_frame(time, fps);
        if let Some((property, index)) = &self.selected
            && scene.track(property).is_none_or(|t| *index >= t.keys.len())
        {
            self.selected = None;
        }

        let last_key = scene.animation.iter().filter_map(|t| t.keys.last()).map(|k| k.time).fold(0.0, f32::max);
        let span = self.held_span.unwrap_or_else(|| duration.max(last_key) * 1.04 + 0.2);
        let full = ui.available_rect_before_wrap();
        let scale = Scale { left: full.left() + HEADER, width: (full.width() - HEADER - 8.0).max(50.0), span };
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let primary_down = ui.input(|i| i.pointer.primary_down());

        // Ruler.
        let (ruler, ruler_response) = ui.allocate_exact_size(egui::vec2(full.width(), RULER), Sense::click_and_drag());
        let lanes_left = scale.left;
        self.draw_ruler(ui, ruler, scale, duration);
        let end_x = scale.x(duration);
        let end_rect = Rect::from_x_y_ranges(end_x - 5.0..=end_x + 5.0, ruler.y_range());
        let end_response = ui.interact(end_rect, ui.scope_id().with("video end"), Sense::drag());
        let end_response =
            end_response.on_hover_text("The end of the video. Drag it to make the video longer or shorter.");
        if end_response.hovered() || self.drag == Some(Drag::End) {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }

        // Lanes.
        let mut scrub_areas = vec![ruler_response];
        let context = LaneContext { scene, scale, time, hits: self.hits.as_ref() };
        for lane in &mut self.lanes {
            if !lane.visible(&context) {
                continue;
            }
            let id = lane.id();
            let collapsed = self.collapsed.contains(id);
            let height = if collapsed { COLLAPSED } else { lane.height(&context) };
            let (rect, response) = ui.allocate_exact_size(egui::vec2(full.width(), height), Sense::click_and_drag());
            let header = Rect::from_min_max(rect.min, Pos2::new(scale.left - 8.0, rect.bottom()));
            let area = Rect::from_x_y_ranges(scale.left..=rect.right(), rect.top() + 2.0..=rect.bottom() - 2.0);
            if lanes::lane_header(ui, header, id, &lane.title(&context), collapsed) {
                if collapsed {
                    self.collapsed.remove(id);
                } else {
                    self.collapsed.insert(id);
                }
            }
            if collapsed {
                ui.painter_at(area).rect_filled(area, 3.0, theme::CARD.gamma_multiply(0.5));
            } else {
                lane.draw(ui, area, &context);
            }
            if !lane.interact(ui, area, &response, &context) {
                scrub_areas.push(response);
            }
        }

        // Starting a drag. The end handle sits on top of the ruler, so it goes first.
        if self.drag.is_none() {
            if end_response.drag_started() {
                self.drag = Some(Drag::End);
                self.held_span = Some(span);
            } else if scrub_areas.iter().any(|r| r.is_pointer_button_down_on() || r.clicked())
                && pointer.is_some_and(|p| p.x >= lanes_left)
            {
                self.drag = Some(Drag::Scrub);
                self.selected = None;
                outcome.scrubbing = Some(true);
            }
        }

        // Caption clips.
        let text = self.text.show(ui, scene, scale.left, scale.width, scale.span);
        if let Some(index) = text.clicked {
            outcome.show_tab = Some(Tab::Text);
            outcome.caption = Some(index);
        }

        // Keyframe rows.
        // Fill the rest of the panel exactly, so it keeps the height it was given and the rows
        // scroll instead.
        let rows_height = ui.available_height().max(ROW);
        egui::ScrollArea::vertical()
            .id_salt("timeline rows")
            .max_height(rows_height)
            .auto_shrink([false, false])
            .show(ui, |ui| self.rows(ui, scene, scale, key_time, &mut outcome));

        // Carrying on a drag.
        // A quick click can press and let go within one frame, so the playhead moves before the
        // drag is checked for ending.
        if self.drag == Some(Drag::Scrub)
            && let Some(p) = pointer
        {
            // Keyframes pull the playhead onto them, so it is easy to land exactly on one.
            let near_key = scene
                .animation
                .iter()
                .flat_map(|t| &t.keys)
                .map(|k| k.time)
                .filter(|&t| (scale.x(t) - p.x).abs() <= SNAP)
                .min_by(|a, b| (scale.x(*a) - p.x).abs().total_cmp(&(scale.x(*b) - p.x).abs()));
            let target = near_key.unwrap_or_else(|| to_frame(scale.time(p.x), fps)).min(duration);
            outcome.seek = Some(target as f64);
        }
        match (&self.drag, pointer) {
            (Some(_), _) if !primary_down => {
                if self.drag == Some(Drag::Scrub) {
                    outcome.scrubbing = Some(false);
                }
                if let Some(Drag::Key { property, index }) = self.drag.take() {
                    merge_same_time(scene, &property, index, &mut self.selected);
                }
                self.drag = None;
                self.held_span = None;
            }
            (Some(Drag::End), Some(p)) => {
                let length = (scale.time(p.x) * 10.0).round() / 10.0;
                scene.video.duration = length.clamp(1.0, 600.0);
                // Holding the end against the right edge makes room for a longer video.
                if p.x > scale.left + scale.width - 12.0 {
                    let dt = ui.input(|i| i.stable_dt).min(0.1);
                    self.held_span = Some(span + span.max(5.0) * 0.3 * dt);
                    ui.ctx().request_repaint();
                }
            }
            (Some(Drag::Key { property, index }), Some(p)) => {
                let (property, index) = (property.clone(), *index);
                let mut target = to_frame(scale.time(p.x), fps);
                if !ui.input(|i| i.modifiers.shift) {
                    for magnet in [0.0, key_time, duration] {
                        if (scale.x(magnet) - p.x).abs() <= SNAP {
                            target = magnet;
                        }
                    }
                }
                if let Some(track) = scene.track_mut(&property)
                    && index < track.keys.len()
                {
                    let index = track.move_key(index, target);
                    self.drag = Some(Drag::Key { property: property.clone(), index });
                    self.selected = Some((property, index));
                    outcome.seek = Some(target as f64);
                } else {
                    self.drag = None;
                }
            }
            _ => {}
        }

        // The playhead, across the ruler, the bounces and the rows.
        let x = scale.x(time);
        let bottom = ui.min_rect().bottom();
        let painter = ui.painter_at(Rect::from_min_max(Pos2::new(lanes_left - 1.0, ruler.top()), full.right_bottom()));
        painter.line_segment([Pos2::new(x, ruler.top()), Pos2::new(x, bottom)], Stroke::new(1.5, Color32::WHITE));
        let head = vec![
            Pos2::new(x - 6.0, ruler.top()),
            Pos2::new(x + 6.0, ruler.top()),
            Pos2::new(x + 6.0, ruler.top() + 8.0),
            Pos2::new(x, ruler.top() + 14.0),
            Pos2::new(x - 6.0, ruler.top() + 8.0),
        ];
        painter.add(egui::Shape::convex_polygon(head, Color32::WHITE, Stroke::NONE));
        outcome
    }

    fn draw_ruler(&self, ui: &Ui, rect: Rect, scale: Scale, duration: f32) {
        let painter = ui.painter_at(rect);
        painter.text(
            rect.left_center() + egui::vec2(4.0, 0.0),
            Align2::LEFT_CENTER,
            "Time",
            FontId::proportional(12.0),
            theme::MUTED,
        );
        let lane = Rect::from_x_y_ranges(scale.left..=rect.right(), rect.y_range());
        painter.rect_filled(lane, 3.0, theme::BACKGROUND);
        // Past the end of the video.
        let end = scale.x(duration);
        painter.rect_filled(
            Rect::from_x_y_ranges(end..=lane.right(), lane.y_range()),
            0.0,
            Color32::from_black_alpha(90),
        );

        let per_second = scale.width / scale.span;
        let step = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0]
            .into_iter()
            .find(|s| s * per_second >= 56.0)
            .unwrap_or(300.0);
        let minor = step / 5.0;
        let mut k = 0;
        loop {
            let t = k as f32 * minor;
            if t > scale.span {
                break;
            }
            let x = scale.x(t);
            let labelled = k % 5 == 0;
            let height = if labelled { 9.0 } else { 4.0 };
            painter.line_segment(
                [Pos2::new(x, lane.bottom() - height), Pos2::new(x, lane.bottom())],
                Stroke::new(1.0, if labelled { theme::MUTED } else { theme::LINE }),
            );
            if labelled {
                painter.text(
                    Pos2::new(x + 3.0, lane.top() + 2.0),
                    Align2::LEFT_TOP,
                    ruler_label(t, step),
                    FontId::proportional(11.0),
                    theme::MUTED,
                );
            }
            k += 1;
        }

        // The end of the video, a handle to drag.
        let active = self.drag == Some(Drag::End);
        let color = if active { theme::ACCENT_TEXT } else { theme::WARNING };
        painter.line_segment([Pos2::new(end, lane.top()), Pos2::new(end, lane.bottom())], Stroke::new(2.0, color));
        let grip = Rect::from_center_size(Pos2::new(end, lane.center().y), egui::vec2(7.0, 14.0));
        painter.rect_filled(grip, 2.0, color);
    }

    /// One row per animated setting: its name and buttons on the left, its keyframes on the right.
    fn rows(&mut self, ui: &mut Ui, scene: &mut Scene, scale: Scale, time: f32, outcome: &mut Outcome) {
        if scene.animation.is_empty() {
            self.rows_seen = 0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW + 6.0), Sense::hover());
            ui.painter().text(
                Pos2::new(scale.left + 8.0, rect.center().y),
                Align2::LEFT_CENTER,
                "Nothing is animated yet. Click the diamond beside a setting to add a keyframe at the playhead.",
                FontId::proportional(12.5),
                theme::MUTED,
            );
            return;
        }
        let added = scene.animation.len() > self.rows_seen;
        self.rows_seen = scene.animation.len();
        let mut stop = None;
        let names: Vec<String> = scene.animation.iter().map(|t| keys::name(&t.property, scene)).collect();
        for (row, name) in names.iter().enumerate() {
            let property = scene.animation[row].property.clone();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), Sense::hover());
            if added && row + 1 == names.len() {
                ui.scroll_to_rect(rect, None);
            }
            let header = Rect::from_min_max(rect.min, Pos2::new(scale.left - 8.0, rect.bottom()));
            let lane = Rect::from_x_y_ranges(scale.left..=rect.right(), rect.top() + 2.0..=rect.bottom() - 2.0);
            let painter = ui.painter_at(rect);
            if row % 2 == 0 {
                painter.rect_filled(lane, 3.0, theme::CARD);
            }
            painter.rect_filled(
                Rect::from_x_y_ranges(scale.x(scene.video.duration)..=lane.right(), lane.y_range()),
                0.0,
                Color32::from_black_alpha(70),
            );

            // Header: keyframe toggle, name, jump to previous and next keyframe, stop animating.
            let id = ui.scope_id().with(("track", &property));
            let track = &scene.animation[row];
            let key_here = track.key_near(time, SAME_TIME);
            let toggle_rect =
                Rect::from_center_size(Pos2::new(header.left() + 10.0, header.center().y), egui::vec2(18.0, 18.0));
            let toggle = ui.interact(toggle_rect, id.with("toggle"), Sense::click());
            let state = if key_here.is_some() { KeyState::KeyHere } else { KeyState::Animated };
            let color = if toggle.hovered() { Color32::WHITE } else { theme::ACCENT_TEXT };
            keys::paint_diamond(&painter, toggle_rect.center(), 5.0, state, color);
            let help = if key_here.is_some() {
                "Remove the keyframe at the playhead"
            } else {
                "Add a keyframe at the playhead"
            };
            if toggle.on_hover_text(help).clicked() {
                keys::request(ui.ctx(), keys::Request::Toggle(property.clone()));
            }

            let buttons = 3.0 * 20.0;
            let name_rect = Rect::from_x_y_ranges(header.left() + 22.0..=header.right() - buttons, header.y_range());
            let name_response = ui.interact(name_rect, id.with("name"), Sense::click());
            let name_color = if name_response.hovered() { theme::ACCENT_TEXT } else { theme::TEXT };
            ui.painter_at(name_rect).text(
                name_rect.left_center(),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(13.0),
                name_color,
            );
            if name_response.on_hover_text(format!("{name}: click to show this setting")).clicked() {
                outcome.show_tab = Some(keys::tab(&property));
            }
            let earlier = track.keys.iter().rev().find(|k| k.time < time - SAME_TIME).map(|k| k.time);
            let later = track.keys.iter().find(|k| k.time > time + SAME_TIME).map(|k| k.time);
            let mut x = header.right() - buttons;
            for (icon, target, help) in [
                (Icon::Previous, earlier, "Go to the previous keyframe"),
                (Icon::Next, later, "Go to the next keyframe"),
            ] {
                let button = Rect::from_min_size(Pos2::new(x, header.center().y - 9.0), egui::vec2(18.0, 18.0));
                if icon_button(ui, button, id.with(help), icon, target.is_some()).on_hover_text(help).clicked()
                    && let Some(target) = target
                {
                    outcome.seek = Some(target as f64);
                }
                x += 20.0;
            }
            let button = Rect::from_min_size(Pos2::new(x, header.center().y - 9.0), egui::vec2(18.0, 18.0));
            if icon_button(ui, button, id.with("stop"), Icon::Remove, true)
                .on_hover_text("Stop animating this setting. It keeps its value at the playhead.")
                .clicked()
            {
                stop = Some(property.clone());
            }

            // Lane: the value over time, then the keyframes on top.
            self.draw_curve(ui, lane, scale, &scene.animation[row]);
            let track = &scene.animation[row];
            if let (Some(first), Some(last)) = (track.keys.first(), track.keys.last()) {
                painter.line_segment(
                    [Pos2::new(scale.x(first.time), lane.center().y), Pos2::new(scale.x(last.time), lane.center().y)],
                    Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.5)),
                );
            }
            let mut action = None;
            for index in 0..scene.animation[row].keys.len() {
                let key = scene.animation[row].keys[index];
                let center = Pos2::new(scale.x(key.time), lane.center().y);
                let hit = Rect::from_center_size(center, egui::vec2(14.0, ROW - 4.0));
                let response = ui.interact(hit, id.with(("key", index)), Sense::click_and_drag());
                let selected = self.selected.as_ref().is_some_and(|(p, i)| *p == property && *i == index);
                let dragging =
                    matches!(&self.drag, Some(Drag::Key { property: p, index: i }) if *p == property && *i == index);
                let color = if selected || dragging {
                    Color32::WHITE
                } else if response.hovered() {
                    theme::ACCENT_TEXT
                } else {
                    theme::ACCENT
                };
                keys::paint_diamond(&painter, center, if selected { 6.5 } else { 5.5 }, KeyState::KeyHere, color);
                if selected {
                    keys::paint_diamond(&painter, center, 9.0, KeyState::Animated, theme::ACCENT_TEXT);
                }
                if response.hovered() && self.drag.is_none() {
                    ui.ctx().set_cursor_icon(CursorIcon::Grab);
                }
                let response = response.on_hover_text(format!(
                    "{name}: {} at {}, {}. Drag to move it (Shift turns off snapping), right-click for more.",
                    value_text(key.value),
                    crate::app::clock(key.time as f64),
                    key.ease.name().to_lowercase(),
                ));
                if response.drag_started() && self.drag.is_none() {
                    self.drag = Some(Drag::Key { property: property.clone(), index });
                    self.selected = Some((property.clone(), index));
                }
                if response.clicked() {
                    self.selected = Some((property.clone(), index));
                    outcome.seek = Some(key.time as f64);
                }
                response.context_menu(|ui| {
                    self.selected = Some((property.clone(), index));
                    ui.label(egui::RichText::new("Change towards the next keyframe").small().color(theme::MUTED));
                    for ease in Ease::ALL {
                        if ease::choice(ui, ease, key.ease) {
                            action = Some(KeyAction::Ease(index, ease));
                            ui.close();
                        }
                    }
                    let own = matches!(key.ease, Ease::Curve(_));
                    ui.horizontal(|ui| {
                        ease::icon(
                            ui,
                            Ease::Curve(key.ease.as_curve()),
                            if own { theme::ACCENT_TEXT } else { theme::MUTED },
                        );
                        ui.menu_button(if own { "Own curve (editing)" } else { "Own curve…" }, |ui| {
                            ui.label(
                                egui::RichText::new(
                                    "Drag the two handles. Steep parts change fast, flat parts slowly.",
                                )
                                .small()
                                .color(theme::MUTED),
                            );
                            let mut curve = key.ease.as_curve();
                            if ease::editor(ui, &mut curve) {
                                action = Some(KeyAction::Ease(index, Ease::Curve(curve)));
                            }
                        });
                    });
                    if ui
                        .button("Use this curve for the whole row")
                        .on_hover_text("Every keyframe of this setting eases the same way")
                        .clicked()
                    {
                        action = Some(KeyAction::EaseAll(key.ease));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Go to this keyframe").clicked() {
                        outcome.seek = Some(key.time as f64);
                        ui.close();
                    }
                    if ui.button("Delete keyframe").clicked() {
                        action = Some(KeyAction::Delete(index));
                        ui.close();
                    }
                });
            }
            match action {
                Some(KeyAction::Ease(index, ease)) => scene.animation[row].keys[index].ease = ease,
                Some(KeyAction::EaseAll(ease)) => scene.animation[row].keys.iter_mut().for_each(|k| k.ease = ease),
                Some(KeyAction::Delete(index)) => {
                    self.selected = Some((property.clone(), index));
                    self.delete_selected(scene);
                    break;
                }
                None => {}
            }
        }
        if let Some(property) = stop {
            keys::stop_animating(scene, &property, time);
            self.selected = None;
        }
    }

    /// A faint line of a number setting's value over time, or a strip of a colour's.
    fn draw_curve(&self, ui: &Ui, lane: Rect, scale: Scale, track: &bpe_scene::Track) {
        let painter = ui.painter_at(lane);
        let (Some(first), Some(last)) = (track.keys.first(), track.keys.last()) else { return };
        if track.property.is_color() {
            let strip = lane.shrink2(egui::vec2(0.0, lane.height() * 0.32));
            let mut x = strip.left();
            while x < strip.right() {
                if let Some(Value::Color(c)) = track.value_at(scale.time(x + 2.0)) {
                    let fill = Color32::from_rgb(c.r, c.g, c.b).gamma_multiply(0.85);
                    painter.rect_filled(Rect::from_x_y_ranges(x..=x + 4.0, strip.y_range()), 0.0, fill);
                }
                x += 4.0;
            }
            return;
        }
        let numbers: Vec<f32> = track.keys.iter().filter_map(|k| k.value.number()).collect();
        let (low, high) = numbers.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        if numbers.is_empty() || first.time == last.time && low == high {
            return;
        }
        let area = lane.shrink2(egui::vec2(0.0, 5.0));
        let y = |v: f32| {
            if high > low { area.bottom() - (v - low) / (high - low) * area.height() } else { area.center().y }
        };
        let mut points = Vec::new();
        let mut x = area.left();
        while x <= area.right() {
            if let Some(v) = track.value_at(scale.time(x)).and_then(Value::number) {
                points.push(Pos2::new(x, y(v)));
            }
            x += 3.0;
        }
        painter.add(egui::Shape::line(points, Stroke::new(1.2, theme::ACCENT_TEXT.gamma_multiply(0.45))));
    }
}

enum KeyAction {
    Ease(usize, Ease),
    EaseAll(Ease),
    Delete(usize),
}

/// After a keyframe is dropped on top of another one in its row, the dropped one wins.
fn merge_same_time(scene: &mut Scene, property: &Property, index: usize, selected: &mut Option<(Property, usize)>) {
    let Some(track) = scene.track_mut(property) else { return };
    let Some(time) = track.keys.get(index).map(|k| k.time) else { return };
    let mut kept = index;
    let mut i = 0;
    track.keys.retain(|k| {
        let keep = i == index || (k.time - time).abs() > SAME_TIME;
        if !keep && i < index {
            kept -= 1;
        }
        i += 1;
        keep
    });
    *selected = Some((property.clone(), kept));
}

fn ruler_label(seconds: f32, step: f32) -> String {
    if seconds >= 60.0 {
        let whole = seconds.round() as u32;
        format!("{}:{:02}", whole / 60, whole % 60)
    } else if step < 1.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{seconds:.0}s")
    }
}

fn value_text(value: Value) -> String {
    match value {
        Value::Number(v) if v.abs() >= 100.0 => format!("{v:.0}"),
        Value::Number(v) => format!("{v:.2}"),
        Value::Color(c) => c.to_hex(),
    }
}

#[derive(Clone, Copy)]
enum Icon {
    Previous,
    Next,
    Remove,
}

/// A small flat button with a drawn icon, so it looks the same whatever fonts are installed.
fn icon_button(ui: &Ui, rect: Rect, id: egui::Id, icon: Icon, enabled: bool) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(rect, id, sense);
    let painter = ui.painter();
    if enabled && response.hovered() {
        painter.rect_filled(rect, 4.0, theme::HOVER);
    }
    let color = if enabled { theme::TEXT } else { theme::MUTED.gamma_multiply(0.4) };
    let c = rect.center();
    let s = 4.0;
    match icon {
        Icon::Previous | Icon::Next => {
            let d = if matches!(icon, Icon::Next) { 1.0 } else { -1.0 };
            let points = vec![c + egui::vec2(-s * d, -s), c + egui::vec2(s * d, 0.0), c + egui::vec2(-s * d, s)];
            painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
        }
        Icon::Remove => {
            let stroke = Stroke::new(1.5, color);
            painter.line_segment([c + egui::vec2(-s, -s), c + egui::vec2(s, s)], stroke);
            painter.line_segment([c + egui::vec2(-s, s), c + egui::vec2(s, -s)], stroke);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::{Key, Track};
    use bpe_sim::Simulation;

    #[test]
    fn hit_summary_counts_hits_and_escapes() {
        let mut scene = Scene::escape_rings();
        scene.video.duration = 6.0;
        let hits = bpe_sim::collect_wall_hits(&scene);
        let summary = HitSummary::new(&hits);
        let escapes = hits.iter().filter(|h| matches!(h.kind, EventKind::Escape(_))).count();
        assert!(escapes > 0);
        assert_eq!(summary.escapes.len(), escapes);
        assert_eq!(summary.counts.iter().sum::<u32>() as usize, hits.len() - escapes);
        assert!(summary.busiest(0.0, 6.0) > 0);
        assert_eq!(summary.busiest(100.0, 200.0), 0);
        let _ = Simulation::new(&scene);
    }

    #[test]
    fn frames_and_labels() {
        assert_eq!(to_frame(1.0 / 60.0 * 10.4, 60), 10.0 / 60.0);
        assert_eq!(ruler_label(5.0, 1.0), "5s");
        assert_eq!(ruler_label(2.5, 0.5), "2.5s");
        assert_eq!(ruler_label(75.0, 15.0), "1:15");
    }

    #[test]
    fn dropping_a_keyframe_on_another_replaces_it() {
        let mut scene = Scene::default();
        let key = |time, v| Key { time, value: Value::Number(v), ease: Ease::Smooth };
        scene
            .animation
            .push(Track { property: Property::Bloom, keys: vec![key(1.0, 1.0), key(1.0, 2.0), key(3.0, 3.0)] });
        let mut selected = None;
        merge_same_time(&mut scene, &Property::Bloom, 1, &mut selected);
        let keys = &scene.track(&Property::Bloom).unwrap().keys;
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].value, Value::Number(2.0));
        assert_eq!(selected, Some((Property::Bloom, 0)));
    }

    #[test]
    fn deleting_the_last_keyframe_stops_animating() {
        let mut scene = Scene::default();
        let mut track = Track::new(Property::Exposure);
        track.set_key(2.0, Value::Number(1.7));
        scene.animation.push(track);
        let mut timeline = Timeline { selected: Some((Property::Exposure, 0)), ..Timeline::default() };
        timeline.delete_selected(&mut scene);
        assert!(scene.animation.is_empty());
        assert_eq!(scene.look.exposure, 1.7);
    }
}
