//! The Scenes window (spec `docs/superpowers/specs/2026-11-10-built-in-scenes-design.md`): every built-in scene
//! as a tile with its still picture, filters by level and kind, Find a scene, and a preview of the scene playing
//! with its sound. Open scene makes it a new project, through the same question about unsaved work as Open.

pub mod filter;
pub mod pictures;
pub mod sound;
#[cfg(test)]
mod tests;

use bpe_kit::Kit;
use bpe_kit::library::{self, Entry, Kind, Level, SceneInfo};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::sync::Arc;

use crate::style;
use filter::Filter;
use pictures::{CLIP_FPS, Clip, Pictures};
use sound::PreviewSound;

/// The window's most size, in points, and its share of the editor's.
const MOST: Vec2 = Vec2::new(1320.0, 820.0);
const SHARE: f32 = 0.92;
const FILTERS_W: f32 = 210.0;
const PREVIEW_W: f32 = 300.0;
/// A tile's picture (9:16) and the rows under it.
const TILE_PICTURE: Vec2 = Vec2::new(126.0, 224.0);
const TILE_ROWS: f32 = 40.0;
/// The preview's picture at its biggest.
const PREVIEW_PICTURE: Vec2 = Vec2::new(236.0, 420.0);
/// How long a tile is hovered before its clip is asked for.
const HOVER_WAIT: f64 = 0.3;

/// What the window asks of the editor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowOutcome {
    /// Open this built-in scene as a new project.
    Open(&'static str),
    /// Closed without opening anything; the project played before it opened if `was_playing`.
    Closed { was_playing: bool },
}

pub struct ScenesWindow {
    pub open: bool,
    filter: Filter,
    picked: Option<&'static str>,
    /// The tile under the pointer and since when.
    hovered: Option<(&'static str, f64)>,
    pub(crate) pictures: Pictures,
    pub(crate) sound: PreviewSound,
    sound_on: bool,
    was_playing: bool,
    /// The posters of every scene have been asked for since the window opened.
    posters_asked: bool,
    /// The tiles shown last frame, whose posters go first.
    last_shown: Vec<&'static str>,
    /// Escape was pressed this frame outside a text box.
    escape: bool,
    /// Find a scene had the keyboard last frame (egui lets go of it on Escape before the frame is drawn).
    find_focused: bool,
}

impl Default for ScenesWindow {
    fn default() -> Self {
        ScenesWindow {
            open: false,
            filter: Filter::default(),
            picked: None,
            hovered: None,
            pictures: Pictures::default(),
            sound: PreviewSound::default(),
            sound_on: true,
            was_playing: false,
            posters_asked: false,
            last_shown: Vec::new(),
            escape: false,
            find_focused: false,
        }
    }
}

/// What a click in the window did this frame.
enum Act {
    Open(&'static str),
    Close,
}

/// m:ss.
fn minutes(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// "Starter · Bounce and drop · 12 s · one simple sound".
pub fn facts(info: &SceneInfo) -> String {
    format!("{} · {} · {} s · {}", info.level.label(), info.kinds_label(), info.seconds.round(), info.sound.label())
}

/// "Showing 5 scenes for "escape"."
pub fn showing(filter: &Filter, count: usize) -> String {
    let scenes = if count == 1 { "scene" } else { "scenes" };
    let words = filter.words.trim();
    if filter.is_clear() {
        format!("Showing all {count} {scenes}.")
    } else if words.is_empty() {
        format!("Showing {count} {scenes}.")
    } else {
        format!("Showing {count} {scenes} for \"{words}\".")
    }
}

impl ScenesWindow {
    /// Opens the window; `was_playing` is given back when it closes without opening a scene.
    pub fn open(&mut self, was_playing: bool, sound_on: bool) {
        self.open = true;
        self.was_playing = was_playing;
        self.sound_on = sound_on;
        self.sound.set_on(sound_on);
        self.posters_asked = false;
        self.last_shown.clear();
        self.hovered = None;
    }

    /// Takes the keys the editor behind the window would act on (Delete removing things or keys, F framing the
    /// world, Escape letting go of keys, Space playing), so they only reach the window. Called at the start of the
    /// editor's frame, before any panel reads them; Escape is kept for closing the window. Escape in a text box
    /// only leaves the box.
    pub fn take_keys(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let typing = self.find_focused || ctx.text_edit_focused();
        ctx.input_mut(|i| {
            // Escape is used up either way: in the box it only left the box.
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) && !typing {
                self.escape = true;
            }
            if !typing {
                let behind = [egui::Key::Delete, egui::Key::Backspace, egui::Key::F, egui::Key::Space];
                i.events.retain(|e| !matches!(e, egui::Event::Key { key, .. } if behind.contains(key)));
            }
        });
    }

    fn close(&mut self) {
        self.open = false;
        self.find_focused = false;
        self.sound.stop();
        self.pictures.stop();
        self.hovered = None;
    }

    /// The scene the preview shows: the one hovered for a moment, else the picked one while the filters show it,
    /// else the first shown. A pointer only passing over tiles leaves the preview as it is.
    fn previewed(&self, shown: &[&'static Entry], now: f64) -> Option<&'static Entry> {
        let hovered = self.hovered.filter(|(_, since)| now - since >= HOVER_WAIT).map(|h| h.0);
        let picked = self.picked.filter(|id| shown.iter().any(|e| e.info.id == *id));
        hovered.or(picked).and_then(library::find).or_else(|| shown.first().copied())
    }

    /// Shows the window while it is open.
    pub fn show(&mut self, ctx: &egui::Context, kit: &Arc<Kit>, volume: f32) -> Option<WindowOutcome> {
        if !self.open {
            return None;
        }
        self.take_keys(ctx);
        self.pictures.poll(ctx);
        if !self.posters_asked {
            for e in library::all() {
                self.pictures.ask_poster(e.info.id, kit, ctx);
            }
            self.posters_asked = true;
        }
        let shown = self.filter.shown(library::all());
        let ids: Vec<_> = shown.iter().map(|e| e.info.id).collect();
        if ids != self.last_shown {
            self.pictures.put_first(&ids);
            self.last_shown = ids;
        }
        let now = ctx.input(|i| i.time);
        self.follow_preview(&shown, kit, ctx, volume, now);

        let screen = ctx.content_rect();
        let size = Vec2::new((screen.width() * SHARE).min(MOST.x), (screen.height() * SHARE).min(MOST.y));
        let margin = 16.0;
        let frame = egui::Frame::new()
            .fill(style::PANEL)
            .stroke(Stroke::new(1.0, style::LINE))
            .corner_radius(style::RADIUS)
            .inner_margin(margin as i8);
        let inner = size - Vec2::splat(margin * 2.0);
        let mut act = None;
        egui::Modal::new(egui::Id::unique("scenes window")).frame(frame).show(ctx, |ui| {
            ui.set_min_size(inner);
            ui.set_max_size(inner);
            self.contents(ui, &shown, now, &mut act);
        });
        if std::mem::take(&mut self.escape) && act.is_none() {
            act = Some(Act::Close);
        }
        let repaint = self.sound.playing() || self.pictures.busy() || self.hovered.is_some();
        if repaint {
            ctx.request_repaint_after(std::time::Duration::from_millis(1000 / CLIP_FPS as u64));
        }
        match act {
            Some(Act::Open(id)) => {
                self.close();
                Some(WindowOutcome::Open(id))
            }
            Some(Act::Close) => {
                self.close();
                Some(WindowOutcome::Closed { was_playing: self.was_playing })
            }
            None => None,
        }
    }

    /// Asks for the previewed scene's clip (a hovered tile after a moment) and plays its sound once the clip is
    /// ready; a change of scene stops the sound.
    fn follow_preview(&mut self, shown: &[&'static Entry], kit: &Arc<Kit>, ctx: &egui::Context, volume: f32, now: f64) {
        let Some(entry) = self.previewed(shown, now) else {
            self.sound.stop();
            return;
        };
        let id = entry.info.id;
        // The first scene shown, nobody having asked for it, waits for the tiles' posters.
        let asked =
            self.picked == Some(id) || self.hovered.is_some_and(|(h, since)| h == id && now - since >= HOVER_WAIT);
        if asked || !self.pictures.posters_waiting() {
            self.pictures.ask_clip(id, kit, ctx);
        }
        if self.sound.previewing() != Some(id) {
            self.sound.stop();
            if matches!(self.pictures.clip(id), Clip::Ready(_)) {
                self.sound.play(entry, kit, volume);
                self.sound.set_on(self.sound_on);
            }
        }
    }

    fn contents(&mut self, ui: &mut Ui, shown: &[&'static Entry], now: f64, act: &mut Option<Act>) {
        self.header(ui, act);
        ui.add_space(12.0);
        let body = ui.available_size();
        let middle_w = (body.x - FILTERS_W - PREVIEW_W - 32.0).max(TILE_PICTURE.x + 8.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            let column = |w: f32| (Vec2::new(w, body.y), egui::Layout::top_down(egui::Align::Min));
            let (s, l) = column(FILTERS_W);
            ui.allocate_ui_with_layout(s, l, |ui| {
                ui.set_width(FILTERS_W);
                self.filters(ui);
            });
            let (s, l) = column(middle_w);
            ui.allocate_ui_with_layout(s, l, |ui| {
                ui.set_width(middle_w);
                self.grid(ui, shown, now, act);
            });
            let (s, l) = column(PREVIEW_W);
            ui.allocate_ui_with_layout(s, l, |ui| {
                ui.set_width(PREVIEW_W);
                egui::ScrollArea::vertical().id_salt("scene preview").show(ui, |ui| self.preview(ui, shown, now, act));
            });
        });
    }

    fn header(&mut self, ui: &mut Ui, act: &mut Option<Act>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Scenes").font(style::semibold(20.0)).color(style::TEXT_STRONG));
            ui.add_space(8.0);
            let count = library::all().len();
            ui.label(
                RichText::new(format!("{count} scenes, from simple starters to full spectacles")).color(style::MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if close_button(ui).on_hover_text("Close (Esc)").clicked() {
                    *act = Some(Act::Close);
                }
                ui.add_space(8.0);
                let edit = egui::TextEdit::singleline(&mut self.filter.words)
                    .desired_width(260.0)
                    .hint_text("bounce, escape, glass…");
                let edit = ui.add(edit);
                self.find_focused = edit.has_focus();
                let label = ui.label(RichText::new("Find a scene").color(style::MUTED));
                edit.labelled_by(label.id);
            });
        });
    }

    fn filters(&mut self, ui: &mut Ui) {
        let all = library::all();
        section(ui, "Level");
        let levels = [None, Some(Level::Starter), Some(Level::InBetween), Some(Level::Spectacle)];
        for level in levels {
            let name = level.map_or("All", Level::plural);
            let count = self.filter.level_count(all, level);
            if filter_row(ui, name, &format!("Level {name}"), count, self.filter.level == level).clicked() {
                self.filter.level = level;
            }
        }
        ui.add_space(14.0);
        section(ui, "Kind");
        for kind in Kind::ALL {
            let name = kind.label();
            let count = self.filter.kind_count(all, kind);
            let on = self.filter.kind == Some(kind);
            if filter_row(ui, name, &format!("Kind {name}"), count, on).clicked() {
                // The kind that is on turns off again.
                self.filter.kind = if on { None } else { Some(kind) };
            }
        }
    }

    fn grid(&mut self, ui: &mut Ui, shown: &[&'static Entry], now: f64, act: &mut Option<Act>) {
        ui.label(RichText::new(showing(&self.filter, shown.len())).color(style::MUTED));
        ui.add_space(8.0);
        let mut hovered = None;
        if shown.is_empty() {
            ui.label("No scenes match.");
            if ui.button("Clear the filters").clicked() {
                self.filter = Filter::default();
            }
        } else {
            egui::ScrollArea::vertical().id_salt("scene tiles").auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(12.0, 14.0);
                    for &entry in shown {
                        let response = self.tile(ui, entry);
                        if response.hovered() {
                            hovered = Some(entry.info.id);
                        }
                        if response.clicked() {
                            self.picked = Some(entry.info.id);
                        }
                        if response.double_clicked() {
                            *act = Some(Act::Open(entry.info.id));
                        }
                    }
                });
            });
        }
        self.hovered = match (hovered, self.hovered) {
            (Some(id), Some((was, since))) if was == id => Some((id, since)),
            (Some(id), _) => Some((id, now)),
            (None, _) => None,
        };
    }

    fn tile(&self, ui: &mut Ui, entry: &'static Entry) -> egui::Response {
        let info = &entry.info;
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(TILE_PICTURE.x, TILE_PICTURE.y + TILE_ROWS), Sense::click());
        let on = self.picked == Some(info.id);
        let label = format!("Scene {}", info.name);
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, &label));
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let picture = Rect::from_min_size(rect.min, TILE_PICTURE);
        paint_picture(ui, picture, info, self.pictures.poster(info.id).map(|t| t.id()));
        badge(ui, picture, info.level.label());
        let ring = if on {
            Some(Stroke::new(1.5, style::TEXT))
        } else if response.hovered() {
            Some(Stroke::new(1.0, style::MUTED))
        } else {
            None
        };
        if let Some(ring) = ring {
            ui.painter().rect_stroke(picture, style::RADIUS, ring, egui::StrokeKind::Outside);
        }
        let x = picture.left();
        let ink = if on { style::TEXT_STRONG } else { style::TEXT };
        let name = one_line(ui, info.name, style::semibold(13.0), ink, rect.width());
        let elided = name.elided;
        ui.painter().galley(Pos2::new(x, picture.bottom() + 6.0), name, ink);
        let kinds = one_line(ui, &info.kinds_label(), FontId::proportional(11.0), style::MUTED, rect.width());
        ui.painter().galley(Pos2::new(x, picture.bottom() + 24.0), kinds, style::MUTED);
        if elided { response.on_hover_text(info.name) } else { response }
    }

    fn preview(&mut self, ui: &mut Ui, shown: &[&'static Entry], now: f64, act: &mut Option<Act>) {
        let Some(entry) = self.previewed(shown, now) else {
            ui.label(RichText::new("Pick a scene to see it here.").color(style::MUTED));
            return;
        };
        let info = &entry.info;
        let tall = (ui.available_height() * 0.56).clamp(220.0, PREVIEW_PICTURE.y);
        let size = Vec2::new(tall * 9.0 / 16.0, tall);
        let (outer, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), size.y), Sense::hover());
        let picture = Rect::from_center_size(outer.center(), size);
        let clock = self.sound.clock();
        let clip = self.pictures.clip(info.id);
        let frame = match (&clip, clock, self.sound.previewing() == Some(info.id)) {
            (Clip::Ready(frames), Some(t), true) => {
                let index = ((t * CLIP_FPS as f64) as usize).min(frames.len().saturating_sub(1));
                frames.get(index).map(|f| f.id())
            }
            (Clip::Ready(frames), _, _) => frames.first().map(|f| f.id()),
            _ => None,
        };
        if self.pictures.device_missing() {
            ui.painter().rect_filled(picture, style::RADIUS, style::GROUND);
            note_over(ui, picture, "No preview on this computer");
        } else {
            let poster = self.pictures.poster(info.id).map(|t| t.id());
            paint_picture(ui, picture, info, frame.or(poster));
            match clip {
                Clip::Making { done, of } => {
                    let bar = Rect::from_min_size(
                        Pos2::new(picture.left() + 16.0, picture.bottom() - 40.0),
                        Vec2::new(picture.width() - 32.0, 6.0),
                    );
                    note_over(ui, picture, "Getting the preview ready…");
                    ui.painter().rect_filled(bar, 2.0, style::PILL);
                    let k = if of == 0 { 0.0 } else { done as f32 / of as f32 };
                    let mut fill = bar;
                    fill.set_width(bar.width() * k.clamp(0.0, 1.0));
                    ui.painter().rect_filled(fill, 2.0, style::TEXT);
                }
                Clip::NotAsked => note_over(ui, picture, "Getting the preview ready…"),
                Clip::Failed(why) => {
                    let why = why.to_owned();
                    note_over(ui, picture, "The preview couldn't be drawn");
                    ui.label(RichText::new(why).small().color(style::MUTED));
                }
                Clip::Ready(_) => {}
            }
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let playing = self.sound.playing();
            let play = style::toggle_button(if playing { "Playing" } else { "Paused" }, playing);
            let hint = if playing { "Pause the preview" } else { "Play the preview" };
            if ui.add_enabled(self.sound.previewing().is_some(), play).on_hover_text(hint).clicked() {
                self.sound.set_playing(!playing);
            }
            let sound = style::toggle_button(if self.sound_on { "Sound on" } else { "Sound off" }, self.sound_on);
            if ui.add(sound).on_hover_text("Hear the preview, or not").clicked() {
                self.sound_on = !self.sound_on;
                self.sound.set_on(self.sound_on);
            }
            let at = if self.sound.previewing() == Some(info.id) { clock.unwrap_or(0.0) } else { 0.0 };
            ui.label(RichText::new(format!("{} / {}", minutes(at), minutes(info.seconds))).font(style::mono(12.0)));
        });
        ui.add_space(10.0);
        ui.label(RichText::new(info.name).font(style::semibold(16.0)).color(style::TEXT_STRONG));
        ui.label(RichText::new(facts(info)).small().color(style::MUTED));
        ui.add_space(6.0);
        ui.label(info.about);
        ui.add_space(4.0);
        ui.label(RichText::new(format!("Uses: {}", info.uses.join(", "))).small().color(style::MUTED));
        ui.add_space(8.0);
        ui.label(RichText::new("Good things to change first").font(style::semibold(12.0)).color(style::TEXT));
        ui.label(info.change_first);
        ui.add_space(10.0);
        ui.label(
            RichText::new("Opens as a new project, yours to change. Your current project asks to be saved first.")
                .small()
                .color(style::MUTED),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add(style::primary_button("Open scene")).clicked() {
                *act = Some(Act::Open(info.id));
            }
            if ui.button("Cancel").clicked() {
                *act = Some(Act::Close);
            }
        });
    }
}

/// A small muted heading over a list of filters.
fn section(ui: &mut Ui, name: &str) {
    ui.label(RichText::new(name).font(style::semibold(12.0)).color(style::MUTED));
    ui.add_space(2.0);
}

/// One filter button: its name and how many scenes it would show.
fn filter_row(ui: &mut Ui, name: &str, label: &str, count: usize, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if on {
            Some(style::SELECTED)
        } else if response.hovered() {
            Some(style::HOVER)
        } else {
            None
        };
        if let Some(fill) = fill {
            painter.rect_filled(rect, style::RADIUS, fill);
        }
        let ink = if on { style::TEXT_STRONG } else { style::TEXT };
        let font = if on { style::semibold(13.0) } else { FontId::proportional(13.0) };
        painter.text(rect.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, name, font, ink);
        let ink = if count == 0 { style::FAINT } else { style::MUTED };
        painter.text(rect.right_center() - Vec2::new(8.0, 0.0), Align2::RIGHT_CENTER, count, style::mono(12.0), ink);
    }
    response
}

/// The close button: a painted cross.
fn close_button(ui: &mut Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Close scenes"));
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter().rect_filled(rect, style::RADIUS, style::HOVER);
        }
        let c = rect.center();
        let r = 5.0;
        let stroke = Stroke::new(1.5, if response.hovered() { style::TEXT_STRONG } else { style::TEXT });
        ui.painter().line_segment([c + Vec2::new(-r, -r), c + Vec2::new(r, r)], stroke);
        ui.painter().line_segment([c + Vec2::new(-r, r), c + Vec2::new(r, -r)], stroke);
    }
    response
}

/// A scene's picture: `texture`, else its two swatch colours top to bottom.
fn paint_picture(ui: &Ui, rect: Rect, info: &SceneInfo, texture: Option<egui::TextureId>) {
    let painter = ui.painter();
    if let Some(texture) = texture {
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        painter.image(texture, rect, uv, style::UNTINTED);
        return;
    }
    let [a, b] = info.swatch.map(|c| Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)); // scene colour
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), a);
    mesh.colored_vertex(rect.right_top(), a);
    mesh.colored_vertex(rect.left_bottom(), b);
    mesh.colored_vertex(rect.right_bottom(), b);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(mesh);
}

/// The level badge in a picture's top left corner.
fn badge(ui: &Ui, picture: Rect, text: &str) {
    let painter = ui.painter();
    let font = style::semibold(10.0);
    let galley = painter.layout_no_wrap(text.to_owned(), font, style::TEXT);
    let pill = Rect::from_min_size(picture.min + Vec2::new(6.0, 6.0), galley.size() + Vec2::new(10.0, 4.0));
    painter.rect_filled(pill, style::RADIUS, style::PILL);
    painter.galley(pill.min + Vec2::new(5.0, 2.0), galley, style::TEXT);
}

/// A line of words on a dark band across the middle of a picture (laid over it, taking no room of its own).
fn note_over(ui: &mut Ui, picture: Rect, text: &str) {
    let band = Rect::from_center_size(picture.center(), Vec2::new(picture.width(), 28.0));
    ui.painter().rect_filled(band, 0.0, style::PILL);
    let layout = egui::Layout::centered_and_justified(egui::Direction::TopDown);
    let mut over = ui.new_child(egui::UiBuilder::new().max_rect(band).layout(layout));
    over.add(egui::Label::new(RichText::new(text).size(12.0).color(style::TEXT)).selectable(false));
}

/// `text` on one line at most `width` wide, ending in "…" when it is longer.
fn one_line(ui: &Ui, text: &str, font: FontId, ink: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, ink);
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, ..Default::default() };
    ui.painter().layout_job(job)
}
