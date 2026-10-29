//! The editor's state and frame: files, undo, shortcuts and the panel layout.

use crate::history::{History, UNDO_MERGE};
use crate::{settings, theme};
use bpe_core::{Resolved, Scene, Settings, ThingId, resolve};
use bpe_kit::{Kit, SceneSettings};
use bpe_runtime::{Playback, fingerprint};
use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers, RichText, Stroke};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Something that would throw away unsaved changes.
pub enum Pending {
    Close,
    Open,
    New,
}

/// What the panels remember between frames. Later tasks add the tree, inspector, preview,
/// transport, export window and sound job.
#[derive(Default)]
pub struct Panels {
    pub export_open: bool,
}

enum ToolbarClick {
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Export,
}

pub struct Studio {
    pub kit: Arc<Kit>,
    pub scene: Scene,
    pub resolved: Resolved,
    /// The scene as it was last saved or opened.
    pub saved: Scene,
    pub path: Option<PathBuf>,
    pub playback: Playback,
    /// The physics fingerprint `playback` was built from.
    pub fingerprint: u64,
    /// The playhead, in seconds.
    pub time: f64,
    pub playing: bool,
    pub looping: bool,
    pub muted: bool,
    pub selected: Option<ThingId>,
    pub history: History,
    pub status: String,
    pub confirm: Option<Pending>,
    pub panels: Panels,
    /// The soundtrack no longer matches the scene.
    pub sound_dirty: bool,
    /// This frame's change to the scene is already in the history (undo, open, delete…).
    recorded: bool,
    closing: bool,
    last_tick: Option<Instant>,
    clicked: Option<ToolbarClick>,
}

impl Studio {
    pub fn new(cc: &eframe::CreationContext, scene_path: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut studio = Self::headless();
        if let Some(path) = scene_path.or_else(settings::last_scene) {
            studio.open_path(&path);
        }
        studio
    }

    /// The state without a window: no audio, no renderer. A brand-new scene is open.
    pub fn headless() -> Self {
        let kit = Arc::new(bpe_kit::kit());
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve(&scene, &kit.registry);
        let fingerprint = fingerprint(&scene, &resolved, &kit.sims);
        let playback = Playback::new(&scene, &resolved, kit.sims.clone());
        Self {
            saved: scene.clone(),
            scene,
            resolved,
            path: None,
            playback,
            fingerprint,
            kit,
            time: 0.0,
            playing: false,
            looping: true,
            muted: false,
            selected: None,
            history: History::default(),
            status: "Ready.".into(),
            confirm: None,
            panels: Panels::default(),
            sound_dirty: true,
            recorded: false,
            closing: false,
            last_tick: None,
            clicked: None,
        }
    }

    // ---------- the scene ----------

    /// The root's settings as resolved: the video's size, length and frame rate.
    pub fn settings(&self) -> SceneSettings {
        self.resolved.things.get(&ThingId::ROOT).map(|r| SceneSettings::from_params(&r.params)).unwrap_or_default()
    }

    pub fn duration(&self) -> f64 {
        self.settings().duration.max(0.0) as f64
    }

    pub fn unsaved(&self) -> bool {
        self.scene != self.saved
    }

    /// Records the edit that turned `before` into the scene as it is now, unless nothing
    /// changed, and brings the simulation and sound along.
    pub fn after_edit(&mut self, before: Scene) {
        self.edited(before, false);
    }

    /// [`after_edit`](Self::after_edit) for an edit that is always its own undo step.
    pub fn after_separate_edit(&mut self, before: Scene) {
        self.edited(before, true);
    }

    fn edited(&mut self, before: Scene, separate: bool) {
        if self.scene == before {
            return;
        }
        self.history.record(before, separate);
        self.scene_changed();
    }

    /// Re-resolves after any change. The playback starts over only when the physics changed:
    /// a new colour keeps the balls where they are.
    fn scene_changed(&mut self) {
        self.resolved = resolve(&self.scene, &self.kit.registry);
        let fingerprint = fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        if fingerprint != self.fingerprint {
            self.fingerprint = fingerprint;
            self.playback = Playback::new(&self.scene, &self.resolved, self.kit.sims.clone());
        }
        self.sound_dirty = true;
        self.time = self.time.min(self.duration());
    }

    pub fn replace_scene(&mut self, scene: Scene, path: Option<PathBuf>) {
        self.saved = scene.clone();
        self.scene = scene;
        self.path = path;
        self.history = History::default();
        self.selected = None;
        self.time = 0.0;
        self.set_playing(false);
        self.recorded = true;
        self.resolved = resolve(&self.scene, &self.kit.registry);
        self.fingerprint = fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        self.playback = Playback::new(&self.scene, &self.resolved, self.kit.sims.clone());
        self.sound_dirty = true;
    }

    pub fn new_scene(&mut self) {
        self.replace_scene(bpe_kit::new_scene(&self.kit.registry), None);
        self.status = "Started a new scene.".into();
    }

    pub fn undo(&mut self) -> bool {
        if !self.history.undo(&mut self.scene) {
            return false;
        }
        self.status = "Undone.".into();
        self.recorded = true;
        self.scene_changed();
        true
    }

    pub fn redo(&mut self) -> bool {
        if !self.history.redo(&mut self.scene) {
            return false;
        }
        self.status = "Redone.".into();
        self.recorded = true;
        self.scene_changed();
        true
    }

    /// Removes the selected thing and everything under it. The scene's defaults stay.
    pub fn delete_selected(&mut self) {
        let Some(id) = self.selected else { return };
        let before = self.scene.clone();
        match self.scene.remove_thing(id) {
            Ok(_) => {
                self.selected = None;
                self.recorded = true;
                self.after_separate_edit(before);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    // ---------- playback ----------

    pub fn seek(&mut self, time: f64) {
        self.time = time.clamp(0.0, self.duration());
        self.playback.seek(self.time);
    }

    pub fn set_playing(&mut self, playing: bool) {
        if playing && self.time >= self.duration() {
            self.time = 0.0;
        }
        self.playing = playing;
        self.last_tick = None;
    }

    /// Moves the playhead with the clock while playing.
    fn tick(&mut self) {
        let now = Instant::now();
        let dt = self.last_tick.replace(now).map_or(0.0, |last| (now - last).as_secs_f64().min(0.25));
        if !self.playing {
            return;
        }
        let duration = self.duration();
        let next = self.time + dt;
        if next < duration {
            self.time = next;
        } else if self.looping {
            self.time = 0.0;
        } else {
            self.time = duration;
            self.playing = false;
        }
    }

    // ---------- files ----------

    fn base_dir(&self) -> Option<PathBuf> {
        self.path.as_ref().and_then(|p| p.parent()).map(PathBuf::from)
    }

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
            Pending::New => self.new_scene(),
        }
    }

    pub fn open(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Ball Studio scene", &["ron"]);
        if let Some(dir) = self.base_dir() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_path(&path);
        }
    }

    /// Opens the scene file at `path`. A file that can't be read leaves the current scene and
    /// says why in the status line.
    pub fn open_path(&mut self, path: &Path) -> bool {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                self.status = format!("Couldn't read {}: {error}", path.display());
                return false;
            }
        };
        match bpe_core::load(&text, &self.kit.registry) {
            Ok(scene) => {
                settings::remember_scene(path);
                self.replace_scene(scene, Some(path.to_path_buf()));
                self.status = format!("Opened {}.", file_label(path));
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    /// Saves, asking for a file name if the scene has none (or `choose` is set).
    /// Returns whether the scene was saved.
    pub fn save(&mut self, choose: bool) -> bool {
        let path = match (&self.path, choose) {
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
        match std::fs::write(&path, bpe_core::save(&self.scene)) {
            Ok(()) => {
                self.status = format!("Saved {}.", file_label(&path));
                self.saved = self.scene.clone();
                settings::remember_scene(&path);
                self.path = Some(path);
                true
            }
            Err(error) => {
                self.status = format!("Couldn't save {}: {error}", path.display());
                false
            }
        }
    }

    // ---------- the frame ----------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused();
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)));
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::S) {
            self.save(true);
        }
        if pressed(Modifiers::COMMAND, Key::S) {
            self.save(false);
        }
        if pressed(Modifiers::COMMAND, Key::O) {
            self.guard(Pending::Open, ctx);
        }
        if pressed(Modifiers::COMMAND, Key::N) {
            self.guard(Pending::New, ctx);
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
        // Other keys belong to a slider or button that has the keyboard focus.
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let (home, delete) = ctx.input(|i| (i.key_pressed(Key::Home), i.key_pressed(Key::Delete)));
        if home {
            self.seek(0.0);
        }
        if delete {
            self.delete_selected();
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("New").on_hover_text("Start a new scene (Ctrl+N)").clicked() {
                self.clicked = Some(ToolbarClick::New);
            }
            if ui.button("Open…").on_hover_text("Open a saved scene (Ctrl+O)").clicked() {
                self.clicked = Some(ToolbarClick::Open);
            }
            if ui.button("Save").on_hover_text("Save this scene (Ctrl+S)").clicked() {
                self.clicked = Some(ToolbarClick::Save);
            }
            if ui.button("Save as…").on_hover_text("Save this scene under a new name (Ctrl+Shift+S)").clicked() {
                self.clicked = Some(ToolbarClick::SaveAs);
            }
            ui.separator();
            let undo = egui::Button::new("Undo");
            if ui.add_enabled(self.history.can_undo(), undo).on_hover_text("Undo (Ctrl+Z)").clicked() {
                self.clicked = Some(ToolbarClick::Undo);
            }
            let redo = egui::Button::new("Redo");
            if ui.add_enabled(self.history.can_redo(), redo).on_hover_text("Redo (Ctrl+Y)").clicked() {
                self.clicked = Some(ToolbarClick::Redo);
            }
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.scene.name).desired_width(200.0).hint_text("Scene name"))
                .on_hover_text("The scene's name. Exported videos are named after it.");
            let saved_label = match (&self.path, self.unsaved()) {
                (None, _) => RichText::new("Not saved yet").color(theme::MUTED),
                (Some(_), true) => RichText::new("• Unsaved changes").color(theme::WARNING),
                (Some(path), false) => RichText::new(format!("Saved as {}", file_label(path))).color(theme::MUTED),
            };
            ui.label(saved_label.small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let export = egui::Button::new(RichText::new("Export video…").strong().color(Color32::WHITE))
                    .fill(theme::ACCENT);
                if ui.add(export).on_hover_text("Render the scene to an mp4 with sound").clicked() {
                    self.clicked = Some(ToolbarClick::Export);
                }
            });
        });
    }

    fn status_line(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.status).small().color(theme::MUTED));
            if self.sound_dirty {
                ui.label(RichText::new("· Updating sound…").small().color(theme::MUTED));
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

    /// Play, the playhead and the loop and mute switches. A later task adds the scrubber.
    fn transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let label = if self.playing { "Pause" } else { "Play" };
            if ui.button(label).on_hover_text("Play or pause (Space)").clicked() {
                self.set_playing(!self.playing);
            }
            if ui.button("Start").on_hover_text("Back to the start (Home)").clicked() {
                self.seek(0.0);
            }
            ui.label(RichText::new(format!("{} / {}", clock(self.time), clock(self.duration()))).monospace());
            ui.checkbox(&mut self.looping, "Loop");
            ui.checkbox(&mut self.muted, "Mute");
        });
    }

    fn placeholder(ui: &mut egui::Ui, name: &str) {
        ui.add_space(8.0);
        ui.label(RichText::new(name).strong().color(theme::MUTED));
    }
}

impl eframe::App for Studio {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && self.unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
        self.tick();
        let before = self.scene.clone();
        self.recorded = false;
        if self.confirm.is_none() {
            self.shortcuts(&ctx);
        }

        egui::Panel::top("toolbar").frame(bar_frame()).show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(12, 4)))
            .show(ui, |ui| self.status_line(ui));
        egui::Panel::bottom("transport")
            .frame(bar_frame())
            .resizable(true)
            .default_size(110.0)
            .min_size(80.0)
            .max_size(320.0)
            .show(ui, |ui| self.transport(ui));
        egui::Panel::left("tree")
            .default_size(260.0)
            .min_size(180.0)
            .max_size(480.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| Self::placeholder(ui, "Scene tree"));
        egui::Panel::right("inspector")
            .default_size(420.0)
            .min_size(320.0)
            .max_size(640.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| Self::placeholder(ui, "Inspector"));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BACKGROUND))
            .show(ui, |ui| Self::placeholder(ui, "Preview"));

        match self.clicked.take() {
            Some(ToolbarClick::New) => self.guard(Pending::New, &ctx),
            Some(ToolbarClick::Open) => self.guard(Pending::Open, &ctx),
            Some(ToolbarClick::Save) => _ = self.save(false),
            Some(ToolbarClick::SaveAs) => _ = self.save(true),
            Some(ToolbarClick::Undo) => _ = self.undo(),
            Some(ToolbarClick::Redo) => _ = self.redo(),
            Some(ToolbarClick::Export) => self.panels.export_open = true,
            None => {}
        }
        self.confirm_dialog(&ctx);

        if !std::mem::take(&mut self.recorded) {
            self.after_edit(before);
        }
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        if self.history.merge_expired(pointer_down) {
            self.history.end_merging();
        } else if self.history.merging {
            ctx.request_repaint_after(UNDO_MERGE);
        }
        if self.playing {
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

/// Seconds as m:ss.t.
pub(crate) fn clock(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).floor() as u64;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

fn file_label(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Address, ModuleId, Owner, ThingId, Value};
    use std::path::Path;

    #[test]
    fn opening_an_old_scene_reports_it_and_keeps_the_current_one() {
        let mut studio = Studio::headless();
        let old = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenes/reference-reel.ron");
        studio.open_path(Path::new(old));
        assert_eq!(studio.status, "This scene was made with the old Ball Studio");
        assert_eq!(studio.scene.things.len(), 3);
    }

    #[test]
    fn a_look_edit_keeps_the_playback_and_a_gravity_edit_restarts_it() {
        let mut studio = Studio::headless();
        studio.seek(2.0);
        let before = studio.scene.clone();
        // Module ids start at 0: the root's "Plain look" is the first module the new scene adds.
        let look = Address { owner: Owner::Module(ModuleId(0)), slot: None, key: "glow".into() };
        studio.scene.set_at(&studio.kit.registry, &look, Value::Number(1.0)).unwrap();
        studio.after_edit(before);
        assert!(studio.playback.time() >= 2.0);
        // Quick edits merge into one undo step; the frame loop ends the step once the pointer
        // is up and the edits have stopped. The next edit is a step of its own.
        studio.history.end_merging();
        let before = studio.scene.clone();
        let gravity = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "gravity".into() };
        studio.scene.set_at(&studio.kit.registry, &gravity, Value::Vec2([0.0, 10.0])).unwrap();
        studio.after_edit(before);
        assert_eq!(studio.playback.time(), 0.0);
        assert_eq!(studio.history.undo.len(), 2);
    }

    #[test]
    fn new_scene_and_undo_keep_the_saved_state_honest() {
        let mut studio = Studio::headless();
        assert!(!studio.unsaved());
        let before = studio.scene.clone();
        studio.scene.name = "Changed".into();
        studio.after_edit(before);
        assert!(studio.unsaved());
        assert!(studio.undo());
        assert!(!studio.unsaved());
        assert!(studio.redo());
        assert_eq!(studio.scene.name, "Changed");
        studio.new_scene();
        assert_eq!(studio.scene.name, "New scene");
        assert!(!studio.unsaved());
        assert!(studio.history.undo.is_empty());
    }

    #[test]
    fn deleting_the_selected_thing_records_an_undo_step_and_clears_the_selection() {
        let mut studio = Studio::headless();
        studio.selected = Some(ThingId(2));
        studio.delete_selected();
        assert_eq!(studio.scene.things.len(), 2);
        assert_eq!(studio.selected, None);
        assert_eq!(studio.history.undo.len(), 1);
        studio.selected = Some(ThingId::ROOT);
        studio.delete_selected();
        assert_eq!(studio.scene.things.len(), 2, "the root can't be deleted");
    }
}
