//! The editor's state and frame: files, undo, shortcuts and the panel layout.

use crate::audio_out::AudioOut;
use crate::export_window::ExportWindow;
use crate::history::{History, UNDO_MERGE};
use crate::inspector::Inspector;
use crate::preview::Preview;
use crate::sound_job::SoundJob;
use crate::timeline::Timeline;
use crate::tree::Tree;
use crate::{inspector, preview, settings, theme, timeline, tree};
use bpe_core::{Resolved, Scene, Settings, ThingId, resolve};
use bpe_kit::{Kit, SceneSettings, TimeWarp};
use bpe_runtime::{Playback, fingerprint};
use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers, RichText, Stroke};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Most time spent catching the simulation up in one go. A long stretch, such as a scrub far
/// ahead on a huge scene, is worked out over several frames by the preview instead of
/// freezing the editor.
pub(crate) const CATCH_UP_BUDGET: Duration = Duration::from_millis(12);

/// Something that would throw away unsaved changes.
pub enum Pending {
    Close,
    Open,
    New,
}

/// What the panels remember between frames.
#[derive(Default)]
pub struct Panels {
    pub tree: Tree,
    pub inspector: Inspector,
    /// The preview's renderer, made on the first frame with a window.
    pub preview: Option<Preview>,
    pub export: ExportWindow,
    pub sound: SoundJob,
    pub timeline: Timeline,
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
    /// The playhead, in video seconds.
    pub time: f64,
    /// The beat sync's time warp from the last mix: which moment of the run each video time
    /// shows. The identity without beat sync.
    pub warp: TimeWarp,
    pub playing: bool,
    pub looping: bool,
    pub muted: bool,
    pub selected: Option<ThingId>,
    pub history: History,
    pub status: String,
    /// What is wrong with a shader module the preview skipped, shown in the status line.
    pub shader_problem: Option<String>,
    pub confirm: Option<Pending>,
    pub panels: Panels,
    /// The soundtrack no longer matches the scene.
    pub sound_dirty: bool,
    /// The speakers, when there are any.
    pub audio: Option<AudioOut>,
    /// Why there is no sound, when there isn't.
    pub audio_error: Option<String>,
    /// Scrubbing paused the playback; it starts again when the scrub ends.
    pub resume_after_scrub: bool,
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
        match AudioOut::new() {
            Ok(audio) => studio.audio = Some(audio),
            Err(error) => studio.audio_error = Some(format!("No sound: {error:#}")),
        }
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
            warp: TimeWarp::identity(),
            playing: false,
            looping: true,
            muted: false,
            selected: None,
            history: History::default(),
            status: "Ready.".into(),
            shader_problem: None,
            confirm: None,
            panels: Panels::default(),
            sound_dirty: true,
            audio: None,
            audio_error: None,
            resume_after_scrub: false,
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

    /// The moment of the run the playhead shows, through the beat sync's time warp.
    pub fn sim_time(&self) -> f64 {
        self.warp.sim_time(self.time)
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

    // ---------- playback ----------

    /// Moves the playhead. The simulation follows within the budget; the preview catches up
    /// the rest a budget per frame.
    pub fn seek(&mut self, time: f64) {
        self.time = time.clamp(0.0, self.duration());
        self.playback.seek_within(self.sim_time(), Some(CATCH_UP_BUDGET));
        if self.playing
            && let Some(audio) = &self.audio
        {
            audio.play_from(self.time);
        }
    }

    pub fn set_playing(&mut self, playing: bool) {
        if playing && self.time >= self.duration() {
            self.time = 0.0;
        }
        self.playing = playing;
        self.last_tick = None;
        if let Some(audio) = &self.audio {
            if playing {
                audio.play_from(self.time);
            } else {
                audio.pause();
            }
        }
    }

    /// Tells the speakers whether they are muted.
    pub fn sync_mute(&self) {
        if let Some(audio) = &self.audio {
            audio.set_muted(self.muted);
        }
    }

    /// Moves the playhead while playing: with the speakers' clock when there are any, so the
    /// picture and the sound stay together, else with the wall clock.
    fn tick(&mut self) {
        let now = Instant::now();
        let dt = self.last_tick.replace(now).map_or(0.0, |last| (now - last).as_secs_f64().min(0.25));
        if !self.playing {
            return;
        }
        let duration = self.duration();
        let mut next = self.time + dt;
        if let Some(audio) = &self.audio {
            // The speakers move in buffer-sized steps; follow them only when the frame clock has
            // drifted away from them.
            let heard = audio.position();
            if (next - heard).abs() > 0.05 {
                next = heard;
            }
        }
        if next < duration {
            self.time = next;
        } else if self.looping {
            self.time = 0.0;
            if let Some(audio) = &self.audio {
                audio.play_from(0.0);
            }
        } else {
            self.time = duration;
            self.set_playing(false);
        }
    }

    /// Hands a sound-changing edit to the mixer, and plays a finished mix.
    fn update_sound(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.sound_dirty) {
            self.panels.sound.mark(&self.scene, &self.resolved, &self.kit.sims);
        }
        if let Some(mixed) = self.panels.sound.poll(ctx, &self.scene, &self.resolved, &self.kit) {
            self.warp = mixed.warp;
            if let Some(audio) = &self.audio {
                audio.set_track(mixed.track);
                if self.playing {
                    audio.play_from(self.time);
                }
            }
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
        if ctx.input(|i| i.key_pressed(Key::Home)) {
            self.seek(0.0);
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
            if let Some(problem) = &self.shader_problem {
                ui.label(RichText::new(format!("· Shader skipped: {problem}")).small().color(theme::ERROR));
            }
            if self.sound_dirty || self.panels.sound.busy() {
                ui.label(RichText::new("· Updating sound…").small().color(theme::MUTED));
            }
            if self.panels.export.busy() {
                ui.label(RichText::new("· Exporting…").small().color(theme::MUTED));
            }
            if let Some(error) = &self.audio_error {
                ui.label(RichText::new(error.as_str()).small().color(theme::WARNING));
            }
            let song = self.settings().song;
            if !song.trim().is_empty() && bpe_kit::load_song(&song).is_none() {
                let text = format!("The song {song} couldn't be opened; the video has no song.");
                ui.label(RichText::new(text).small().color(theme::WARNING));
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

impl eframe::App for Studio {
    fn on_exit(&mut self) {
        // A running export would otherwise leave a half-written video behind.
        self.panels.export.shutdown();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && self.unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
        self.tick();
        self.update_sound(&ctx);
        let before = self.scene.clone();
        self.recorded = false;
        if self.confirm.is_none() {
            self.shortcuts(&ctx);
        }

        egui::Panel::top("toolbar").frame(bar_frame()).show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(12, 4)))
            .show(ui, |ui| self.status_line(ui));
        egui::Panel::bottom("timeline")
            .frame(bar_frame())
            .resizable(true)
            .default_size(190.0)
            .min_size(110.0)
            .max_size(520.0)
            .show(ui, |ui| timeline::show(ui, self));
        let tree = egui::Panel::left("tree")
            .default_size(260.0)
            .min_size(180.0)
            .max_size(480.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                tree::show(ui, &mut self.panels.tree, &mut self.scene, &self.kit, &self.resolved, &mut self.selected)
            })
            .inner;
        if tree.removed.is_some() {
            // Deleting is always its own undo step.
            self.recorded = true;
            self.after_separate_edit(before.clone());
        }
        egui::Panel::right("inspector")
            .default_size(420.0)
            .min_size(320.0)
            .max_size(640.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| {
                // Claim the panel's whole width, or it shrinks to its widest row on the first frame.
                ui.set_min_width(ui.available_width());
                let time = self.sim_time();
                inspector::show(
                    ui,
                    &mut self.panels.inspector,
                    &mut self.scene,
                    &self.kit,
                    &self.resolved,
                    self.selected,
                    time,
                )
            });
        let outcome = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BACKGROUND))
            .show(ui, |ui| preview::show(ui, frame, self))
            .inner;
        if outcome.clicked {
            self.set_playing(false);
        }
        if outcome.drag_started {
            // Dragging lays the scene out, so show where everything starts.
            self.set_playing(false);
            self.seek(0.0);
        }
        if outcome.selected.is_some() {
            self.selected = outcome.selected;
        }
        self.shader_problem = outcome.shader_problem;
        if outcome.catching_up {
            ctx.request_repaint();
        }
        let base_dir = self.base_dir();
        let settings = self.settings();
        self.panels.export.show(&ctx, &self.scene, &settings, base_dir.as_deref(), &self.kit);

        match self.clicked.take() {
            Some(ToolbarClick::New) => self.guard(Pending::New, &ctx),
            Some(ToolbarClick::Open) => self.guard(Pending::Open, &ctx),
            Some(ToolbarClick::Save) => _ = self.save(false),
            Some(ToolbarClick::SaveAs) => _ = self.save(true),
            Some(ToolbarClick::Undo) => _ = self.undo(),
            Some(ToolbarClick::Redo) => _ = self.redo(),
            Some(ToolbarClick::Export) => self.panels.export.open = true,
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

/// Seconds as m:ss.ff.
pub(crate) fn clock(seconds: f64) -> String {
    let hundredths = (seconds.max(0.0) * 100.0).floor() as u64;
    format!("{}:{:02}.{:02}", hundredths / 6000, hundredths / 100 % 60, hundredths % 100)
}

fn file_label(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Address, ModuleId, Owner, ThingId, Value};
    use bpe_kit::{Arrangement, BallsSettings};
    use std::path::Path;
    use std::time::Duration;

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
    fn seeking_goes_through_the_warp() {
        let mut studio = Studio::headless();
        let beats: Vec<f64> = (0..40).map(|k| k as f64 * 0.5).collect();
        studio.warp = TimeWarp::fit(&[1.1], &beats, 0.5, 20.0);
        studio.seek(1.0);
        assert_eq!(studio.time, 1.0, "the playhead stays in video time");
        assert!((studio.sim_time() - 1.1).abs() < 1e-6);
        assert!((studio.playback.time() - 1.1).abs() < 1.0 / 240.0 + 1e-9, "{}", studio.playback.time());
    }

    #[test]
    fn scrubbing_a_huge_scene_returns_within_its_budget() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        let block = BallsSettings {
            arrangement: Arrangement::Block,
            columns: 200,
            rows: 200,
            spacing: 4.0,
            radius: 1.5,
            ..BallsSettings::default()
        };
        studio.scene.things.get_mut(&ThingId(2)).unwrap().params = block.to_params();
        studio.after_edit(before);
        let started = Instant::now();
        studio.seek(20.0);
        assert!(started.elapsed() < Duration::from_millis(500), "a scrub blocked for {:?}", started.elapsed());
        assert_eq!(studio.time, 20.0);
        assert!(studio.playback.time() < 20.0, "the preview catches the rest up a budget per frame");
    }
}
