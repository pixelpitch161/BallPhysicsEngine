use crate::audio_out::AudioOut;
use crate::explorer::Explorer;
use crate::export::ExportPanel;
use crate::gallery::Gallery;
use crate::inspector::{self, Inspector, Tab};
use crate::preview::Preview;
use crate::{settings, theme};
use bpe_audio::Soundtrack;
use bpe_scene::Scene;
use bpe_sim::Playback;
use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers, RichText, Sense, Stroke};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// Wait this long after the last edit before remixing the soundtrack.
const SOUND_DEBOUNCE: Duration = Duration::from_millis(250);
/// Edits closer together than this, with no mouse button held, undo as one step.
const UNDO_MERGE: Duration = Duration::from_millis(600);
/// Undo steps kept.
const UNDO_LIMIT: usize = 200;

pub struct StudioApp {
    scene: Scene,
    scene_path: Option<PathBuf>,
    saved_scene: Scene,
    playback: Playback,
    time: f64,
    playing: bool,
    looping: bool,
    muted: bool,
    last_tick: Option<Instant>,
    preview: Option<Preview>,
    inspector: Inspector,
    history: History,
    audio: Option<AudioOut>,
    audio_error: Option<String>,
    sound: SoundJob,
    export: ExportPanel,
    gallery: Gallery,
    explorer: Explorer,
    /// An action waiting on "Save changes?".
    confirm: Option<Pending>,
    /// Set once the user has agreed to close, so the next close request goes through.
    closing: bool,
    /// The scene was swapped wholesale this frame (undo, redo, open), so it is not a new edit.
    swapped: bool,
    status: String,
}

/// Something that would throw away unsaved changes.
enum Pending {
    Close,
    Open,
    Replace(Box<Scene>),
}

#[derive(Default)]
struct History {
    undo: Vec<Scene>,
    redo: Vec<Scene>,
    /// The latest undo step is still growing: the mouse is down or edits are coming quickly.
    merging: bool,
    last_edit: Option<Instant>,
}

impl History {
    fn record(&mut self, before: Scene, separate: bool) {
        if !self.merging || separate {
            self.undo.push(before);
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.merging = !separate;
        }
        self.last_edit = Some(Instant::now());
    }
}

/// Mixes the soundtrack on a background thread whenever the scene changes.
struct SoundJob {
    generation: u64,
    dirty_since: Option<Instant>,
    running: bool,
    sender: Sender<(u64, anyhow::Result<Soundtrack>)>,
    receiver: Receiver<(u64, anyhow::Result<Soundtrack>)>,
    error: Option<String>,
}

impl StudioApp {
    pub fn new(cc: &eframe::CreationContext, scene_path: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut status = String::from("Press Space to play and pause.");
        let opened_from_command_line = scene_path.is_some();
        let scene_path = scene_path.or_else(settings::last_scene);
        let mut scene = None;
        if let Some(path) = &scene_path {
            match Scene::load(path) {
                Ok(loaded) => {
                    status = format!("Opened {}.", file_label(path));
                    scene = Some(loaded);
                }
                Err(error) => status = format!("Couldn't open {}: {error:#}", path.display()),
            }
        }
        let show_gallery = scene.is_none() && !opened_from_command_line;
        let scene_path = scene.is_some().then_some(scene_path).flatten();
        let scene = scene.unwrap_or_default();
        let (audio, audio_error) = match AudioOut::new() {
            Ok(audio) => (Some(audio), None),
            Err(_) => (
                None,
                Some("Preview sound is off because no speakers were found. Exported videos still have sound.".into()),
            ),
        };
        let (sender, receiver) = channel();
        Self {
            playback: Playback::new(&scene),
            saved_scene: scene.clone(),
            scene,
            scene_path,
            time: 0.0,
            playing: false,
            looping: true,
            muted: false,
            last_tick: None,
            preview: None,
            inspector: Inspector::default(),
            history: History::default(),
            audio,
            audio_error,
            sound: SoundJob {
                generation: 0,
                dirty_since: Some(Instant::now() - SOUND_DEBOUNCE),
                running: false,
                sender,
                receiver,
                error: None,
            },
            export: ExportPanel::new(),
            gallery: Gallery::new(show_gallery),
            explorer: Explorer::default(),
            confirm: None,
            closing: false,
            swapped: false,
            status,
        }
    }

    fn base_dir(&self) -> Option<PathBuf> {
        self.scene_path.as_ref().and_then(|p| p.parent()).map(PathBuf::from)
    }

    fn unsaved(&self) -> bool {
        self.scene != self.saved_scene
    }

    fn scene_changed(&mut self) {
        self.playback = Playback::new(&self.scene);
        self.time = self.time.min(self.scene.video.duration as f64);
        self.sound.dirty_since = Some(Instant::now());
    }

    fn replace_scene(&mut self, scene: Scene, path: Option<PathBuf>) {
        self.saved_scene = scene.clone();
        self.scene = scene;
        self.scene_path = path;
        self.history = History::default();
        self.swapped = true;
        self.inspector.selected = None;
        self.time = 0.0;
        self.set_playing(false);
        self.scene_changed();
    }

    // ---------- undo ----------

    /// Records `before` as an undo step, unless this edit continues the previous one.
    /// A `separate` edit, like a dice roll, is always its own step.
    fn record_edit(&mut self, before: Scene, separate: bool) {
        self.history.record(before, separate);
    }

    fn end_merging(&mut self, ctx: &egui::Context) {
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        if self.history.merging && !pointer_down && self.history.last_edit.is_none_or(|t| t.elapsed() >= UNDO_MERGE) {
            self.history.merging = false;
        } else if self.history.merging {
            ctx.request_repaint_after(UNDO_MERGE);
        }
    }

    fn undo(&mut self) {
        if let Some(scene) = self.history.undo.pop() {
            self.history.redo.push(std::mem::replace(&mut self.scene, scene));
            self.history.merging = false;
            self.swapped = true;
            self.status = "Undone.".into();
            self.scene_changed();
        }
    }

    fn redo(&mut self) {
        if let Some(scene) = self.history.redo.pop() {
            self.history.undo.push(std::mem::replace(&mut self.scene, scene));
            self.history.merging = false;
            self.swapped = true;
            self.status = "Redone.".into();
            self.scene_changed();
        }
    }

    // ---------- playback ----------

    fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        self.last_tick = None;
        if let Some(audio) = &self.audio {
            if playing { audio.play_from(self.time) } else { audio.pause() }
        }
    }

    fn seek(&mut self, time: f64) {
        self.time = time.clamp(0.0, self.scene.video.duration as f64);
        if self.playing
            && let Some(audio) = &self.audio
        {
            audio.play_from(self.time);
        }
    }

    fn advance_clock(&mut self) {
        if !self.playing {
            return;
        }
        let now = Instant::now();
        if let Some(last) = self.last_tick {
            self.time += (now - last).as_secs_f64();
        }
        self.last_tick = Some(now);
        let duration = self.scene.video.duration as f64;
        if self.time >= duration {
            if self.looping {
                self.time = 0.0;
                if let Some(audio) = &self.audio {
                    audio.play_from(0.0);
                }
            } else {
                self.time = duration;
                self.set_playing(false);
            }
        }
    }

    fn update_soundtrack(&mut self, ctx: &egui::Context) {
        while let Ok((generation, result)) = self.sound.receiver.try_recv() {
            self.sound.running = false;
            if generation != self.sound.generation {
                continue;
            }
            match result {
                Ok(track) => {
                    self.sound.error = None;
                    if let Some(audio) = &self.audio {
                        audio.set_track(Arc::new(track.samples));
                        if self.playing {
                            audio.play_from(self.time);
                        }
                    }
                }
                Err(error) => self.sound.error = Some(format!("{error:#}")),
            }
        }
        let due = self.sound.dirty_since.is_some_and(|t| t.elapsed() >= SOUND_DEBOUNCE);
        if due && !self.sound.running {
            self.sound.dirty_since = None;
            self.sound.running = true;
            self.sound.generation += 1;
            let (generation, scene, base_dir, sender, ctx) =
                (self.sound.generation, self.scene.clone(), self.base_dir(), self.sound.sender.clone(), ctx.clone());
            std::thread::spawn(move || {
                let hits = bpe_sim::collect_wall_hits(&scene);
                let result = bpe_audio::render_soundtrack(&scene, &hits, base_dir.as_deref());
                let _ = sender.send((generation, result));
                ctx.request_repaint();
            });
        } else if self.sound.dirty_since.is_some() {
            ctx.request_repaint_after(SOUND_DEBOUNCE);
        }
    }

    // ---------- files ----------

    /// Runs `action` now, or asks to save first if there are unsaved changes.
    fn guard(&mut self, action: Pending, ctx: &egui::Context) {
        if self.unsaved() {
            self.confirm = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }

    fn perform(&mut self, action: Pending, ctx: &egui::Context) {
        match action {
            Pending::Close => {
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Pending::Open => self.open(),
            Pending::Replace(scene) => {
                self.status = format!("Started a new scene from \"{}\".", scene.name);
                self.replace_scene(*scene, None);
            }
        }
    }

    fn open(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Ball Studio scene", &["ron"]);
        if let Some(dir) = self.base_dir() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            match Scene::load(&path) {
                Ok(scene) => {
                    self.status = format!("Opened {}.", file_label(&path));
                    settings::remember_scene(&path);
                    self.replace_scene(scene, Some(path));
                }
                Err(error) => self.status = format!("Couldn't open {}: {error:#}", path.display()),
            }
        }
    }

    /// Saves, asking for a file name if the scene has none (or `choose_path` is set).
    /// Returns whether the scene was saved.
    fn save(&mut self, choose_path: bool) -> bool {
        let path = match (&self.scene_path, choose_path) {
            (Some(path), false) => Some(path.clone()),
            _ => {
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("Ball Studio scene", &["ron"])
                    .set_file_name(format!("{}.ron", self.scene.name));
                if let Some(dir) = self.base_dir() {
                    dialog = dialog.set_directory(dir);
                }
                dialog.save_file()
            }
        };
        let Some(path) = path else { return false };
        match self.scene.save(&path) {
            Ok(()) => {
                self.status = format!("Saved {}.", file_label(&path));
                self.saved_scene = self.scene.clone();
                settings::remember_scene(&path);
                self.scene_path = Some(path);
                true
            }
            Err(error) => {
                self.status = format!("Couldn't save: {error:#}");
                false
            }
        }
    }

    // ---------- panels ----------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused();
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)));
        if pressed(Modifiers::COMMAND, Key::S) {
            self.save(false);
        }
        if pressed(Modifiers::COMMAND, Key::O) {
            self.guard(Pending::Open, ctx);
        }
        if typing {
            return;
        }
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            self.redo();
        }
        if pressed(Modifiers::COMMAND, Key::Z) {
            self.undo();
        }
        if ctx.input(|i| i.key_pressed(Key::Space)) {
            self.set_playing(!self.playing);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            if ui.button("New").on_hover_text("Start a new scene from a preset").clicked() {
                self.gallery.open();
            }
            if ui.button("Open…").on_hover_text("Open a saved scene (Ctrl+O)").clicked() {
                self.guard(Pending::Open, &ctx);
            }
            if ui.button("Save").on_hover_text("Save this scene (Ctrl+S)").clicked() {
                self.save(false);
            }
            if ui.button("Save as…").on_hover_text("Save this scene under a new name").clicked() {
                self.save(true);
            }
            ui.separator();
            let undo = egui::Button::new("Undo");
            if ui.add_enabled(!self.history.undo.is_empty(), undo).on_hover_text("Undo (Ctrl+Z)").clicked() {
                self.undo();
            }
            let redo = egui::Button::new("Redo");
            if ui.add_enabled(!self.history.redo.is_empty(), redo).on_hover_text("Redo (Ctrl+Y)").clicked() {
                self.redo();
            }
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.scene.name).desired_width(200.0).hint_text("Scene name"))
                .on_hover_text("The scene's name. Exported videos are named after it.");
            let saved_label = match (&self.scene_path, self.unsaved()) {
                (None, _) => RichText::new("Not saved yet").color(theme::MUTED),
                (Some(_), true) => RichText::new("• Unsaved changes").color(theme::WARNING),
                (Some(path), false) => RichText::new(format!("Saved as {}", file_label(path))).color(theme::MUTED),
            };
            ui.label(saved_label.small());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let export = egui::Button::new(RichText::new("Export video…").strong().color(Color32::WHITE))
                    .fill(theme::ACCENT)
                    .min_size(egui::vec2(0.0, 30.0));
                if ui.add(export).on_hover_text("Make an mp4 you can upload").clicked() {
                    self.export.open = true;
                }
                if ui.button("Presets").on_hover_text("Start a new scene from a preset").clicked() {
                    self.gallery.open();
                }
                let explore = ui
                    .add(egui::Button::new("       Explore…"))
                    .on_hover_text("See random variations of this scene side by side and pick the one you like best");
                let icon = egui::Rect::from_center_size(
                    explore.rect.left_center() + egui::vec2(16.0, 0.0),
                    egui::vec2(20.0, 20.0),
                );
                inspector::paint_dice(ui.painter(), icon, false);
                if explore.clicked() {
                    self.set_playing(false);
                    self.explorer.open(&self.scene, &self.inspector.effects.library);
                }
            });
        });
    }

    fn transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if round_button(ui, 40.0, if self.playing { Icon::Pause } else { Icon::Play })
                .on_hover_text("Play or pause (Space)")
                .clicked()
            {
                self.set_playing(!self.playing);
            }
            if round_button(ui, 30.0, Icon::Restart).on_hover_text("Back to the start").clicked() {
                self.seek(0.0);
            }
            let duration = self.scene.video.duration as f64;
            ui.monospace(format!("{} / {}", clock(self.time), clock(duration)));
            let right_side = 330.0;
            let mut time = self.time;
            ui.style_mut().spacing.slider_width = (ui.available_width() - right_side).max(100.0);
            let response = ui.add(egui::Slider::new(&mut time, 0.0..=duration).show_value(false));
            if response.changed() {
                self.seek(time);
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Length").color(theme::MUTED));
            ui.add(egui::DragValue::new(&mut self.scene.video.duration).range(1.0..=600.0).speed(0.1).suffix(" s"))
                .on_hover_text("How long the video is, in seconds. Click to type, or drag.");
            ui.checkbox(&mut self.looping, "Loop");
            if ui.checkbox(&mut self.muted, "Mute").changed()
                && let Some(audio) = &self.audio
            {
                audio.set_muted(self.muted);
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.status).small().color(theme::MUTED));
            if self.sound.running || self.sound.dirty_since.is_some() {
                ui.label(RichText::new("· Updating sound…").small().color(theme::MUTED));
            }
            if let Some(error) = &self.audio_error {
                ui.label(RichText::new(error.as_str()).small().color(theme::WARNING));
            }
            if let Some(error) = &self.sound.error {
                ui.label(RichText::new(format!("The sound couldn't be made: {error}")).small().color(theme::ERROR));
            }
        });
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        if self.confirm.is_none() {
            return;
        }
        let name = self.scene.name.clone();
        let mut choice = None;
        egui::Modal::new(egui::Id::unique("save-changes")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(format!("Save changes to \"{name}\"?"));
            ui.label(RichText::new("Your changes will be lost if you don't save them.").color(theme::MUTED));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let save = egui::Button::new(RichText::new("Save").strong().color(Color32::WHITE)).fill(theme::ACCENT);
                if ui.add(save).clicked() {
                    choice = Some(true);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    self.confirm = None;
                }
            });
        });
        if let Some(save) = choice
            && let Some(action) = self.confirm.take()
            && (!save || self.save(false))
        {
            self.perform(action, ctx);
        }
    }
}

impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && self.unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
        let tab_open = self.inspector.tab == Tab::Effects;
        self.inspector.effects.poll(&ctx, &self.scene, self.base_dir(), tab_open);
        let modal_open = self.gallery.open || self.explorer.open || self.confirm.is_some();
        if !modal_open {
            self.shortcuts(&ctx);
        }
        self.advance_clock();
        self.update_soundtrack(&ctx);

        let before = self.scene.clone();
        egui::Panel::top("toolbar").frame(bar_frame()).show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("transport").frame(bar_frame()).show(ui, |ui| self.transport(ui));
        egui::Panel::left("inspector")
            .default_size(460.0)
            .min_size(340.0)
            .max_size(600.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::PANEL))
            .show(ui, |ui| inspector::show(ui, &mut self.inspector, &mut self.scene));

        let central = egui::Frame::new().fill(theme::BACKGROUND);
        let outcome = egui::CentralPanel::default()
            .frame(central)
            .show(ui, |ui| {
                Preview::show(
                    &mut self.preview,
                    ui,
                    frame,
                    &mut self.scene,
                    &mut self.playback,
                    self.time,
                    self.playing,
                    self.inspector.selected,
                    &self.inspector.effects.library,
                )
            })
            .inner;
        if let Some(preview) = &self.preview {
            self.inspector.effects.gpu_errors = preview.effect_gpu_errors().to_vec();
        }
        if outcome.clicked {
            self.set_playing(false);
        }
        if outcome.drag_started {
            self.seek(0.0);
        }
        if let Some(index) = outcome.selected {
            self.inspector.selected = Some(index);
            self.inspector.tab = Tab::Balls;
        }

        let base_dir = self.base_dir();
        self.export.show(&ctx, frame, &mut self.scene, base_dir, &self.inspector.effects.library);
        if let Some(scene) = self.gallery.show(&ctx, frame) {
            self.guard(Pending::Replace(Box::new(scene)), &ctx);
        }
        let mut separate = false;
        if let Some(picked) = self.explorer.show(&ctx, frame) {
            self.scene = picked.scene;
            self.status = format!("Using the variation from seed {}. Ctrl+Z puts your scene back.", picked.seed);
            separate = true;
        }
        if let Some(rolled) = self.inspector.rolled.take() {
            self.status = rolled;
            separate = true;
        }
        self.confirm_dialog(&ctx);

        if std::mem::take(&mut self.swapped) {
            // Already handled by undo, redo or opening a scene.
        } else if self.scene != before {
            // Effects only change the picture, so the simulation and sound can stay as they are.
            let effects_only = Scene { effects: before.effects.clone(), ..self.scene.clone() } == before;
            self.record_edit(before, separate);
            if !effects_only {
                self.scene_changed();
            }
        }
        self.end_merging(&ctx);

        if self.playing || self.export.busy() {
            ctx.request_repaint();
        }
    }
}

fn bar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .stroke(Stroke::new(1.0, theme::LINE))
}

fn file_label(path: &std::path::Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// Seconds as m:ss.
fn clock(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).floor() as u64;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

#[derive(Clone, Copy)]
enum Icon {
    Play,
    Pause,
    Restart,
}

/// A round button with a drawn icon, so it looks the same whatever fonts are installed.
fn round_button(ui: &mut egui::Ui, size: f32, icon: Icon) -> egui::Response {
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
                let bar = egui::Rect::from_center_size(c + egui::vec2(dx, 0.0), egui::vec2(s * 0.5, s * 2.0));
                painter.rect_filled(bar, 1.0, white);
            }
        }
        Icon::Restart => {
            let s = size * 0.22;
            let bar = egui::Rect::from_center_size(c + egui::vec2(-s * 0.9, 0.0), egui::vec2(s * 0.35, s * 1.8));
            painter.rect_filled(bar, 0.5, white);
            let points = vec![c + egui::vec2(s, -s * 0.9), c + egui::vec2(s, s * 0.9), c + egui::vec2(-s * 0.6, 0.0)];
            painter.add(egui::Shape::convex_polygon(points, white, Stroke::NONE));
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_dice_roll_is_its_own_undo_step() {
        let mut history = History::default();
        let scene = |name: &str| Scene { name: name.into(), ..Scene::default() };
        // A quick run of slider edits undoes as one step...
        history.record(scene("a"), false);
        history.record(scene("b"), false);
        assert_eq!(history.undo.len(), 1);
        // ...but rolls straight after it, or after each other, don't merge.
        history.record(scene("c"), true);
        history.record(scene("d"), true);
        assert_eq!(history.undo.len(), 3);
        // And the next slider edit starts a new step rather than joining the roll.
        history.record(scene("e"), false);
        assert_eq!(history.undo.len(), 4);
        assert_eq!(history.undo.last().unwrap().name, "e");
    }

    #[test]
    fn clock_reads_minutes_and_seconds() {
        assert_eq!(clock(0.0), "0:00.0");
        assert_eq!(clock(14.88), "0:14.8");
        assert_eq!(clock(83.0), "1:23.0");
    }
}
