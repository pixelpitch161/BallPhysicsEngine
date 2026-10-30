//! The editor's state and frame: files, undo, shortcuts and the panel layout.

use crate::audio_out::AudioOut;
use crate::batch_window::{self, BatchWindow};
use crate::dice::RandomizeWindow;
use crate::explorer::{self, Explorer};
use crate::export_window::ExportWindow;
use crate::grid::{self, Grid};
use crate::history::{History, UNDO_MERGE};
use crate::inspector::Inspector;
use crate::library::LibraryWindow;
use crate::nodes::NodePanel;
use crate::preview::Preview;
use crate::selection::Selection;
use crate::sound_job::SoundJob;
use crate::starters::{self, Choice, StartWindow};
use crate::timeline::Timeline;
use crate::tree::Tree;
use crate::{dice, inspector, library, preview, settings, theme, timeline, tree};
use bpe_core::{Resolved, Scene, Settings, ThingId};
use bpe_kit::random::{Roll, Scope, fresh_seed, roll};
use bpe_kit::{Kit, SceneSettings, SongState, TimeWarp, resolve_scene};
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
    /// A starter scene from the Start window.
    Starter(Box<Scene>),
    /// A recent file from the Start window.
    OpenPath(PathBuf),
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
    pub library: LibraryWindow,
    pub randomize: RandomizeWindow,
    pub explorer: Explorer,
    pub batch: BatchWindow,
    pub start: StartWindow,
    /// The node panel under the preview.
    pub nodes: NodePanel,
}

enum ToolbarClick {
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Export,
    Randomize,
    Dice,
    Explore,
    Batch,
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
    /// Goes up whenever `resolved` or `playback` is rebuilt for a changed scene, so the
    /// preview knows its picture is out of date. The scene itself can be mid-edit during a
    /// frame, ahead of both, so the preview never compares scenes.
    pub revision: u64,
    /// The playhead, in video seconds.
    pub time: f64,
    /// The beat sync's time warp from the last mix: which moment of the run each video time
    /// shows. The identity without beat sync.
    pub warp: TimeWarp,
    pub playing: bool,
    pub looping: bool,
    pub muted: bool,
    pub selection: Selection,
    pub history: History,
    pub status: String,
    /// What is wrong with a shader module the preview skipped, shown in the status line.
    pub shader_problem: Option<String>,
    pub confirm: Option<Pending>,
    pub panels: Panels,
    /// The editor's grid over the preview and whether drags snap to it.
    pub grid: Grid,
    /// What the preview shows: the video's shot, or the whole world.
    pub preview_mode: crate::preview::PreviewMode,
    /// Where the World view looks.
    pub world_nav: crate::preview::WorldNav,
    /// The world point at the middle of the preview, where new things are added.
    pub view_centre: glam::Vec2,
    /// Remember grid changes in the settings folder (off without a window, in tests).
    persist_grid: bool,
    /// The soundtrack no longer matches the scene.
    pub sound_dirty: bool,
    /// The speakers, when there are any.
    pub audio: Option<AudioOut>,
    /// Why there is no sound, when there isn't.
    pub audio_error: Option<String>,
    /// Scrubbing paused the playback; it starts again when the scrub ends.
    pub resume_after_scrub: bool,
    /// The scene's song is decoding on a thread of its own; the run and the mix are rebuilt
    /// when it lands.
    pub song_loading: bool,
    /// The song setting as last seen, so a re-picked file that failed before is tried again.
    last_song: String,
    /// This frame's change to the scene is already in the history (undo, open, delete…).
    pub recorded: bool,
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
        studio.panels.explorer.weights = settings::load_weights();
        studio.panels.explorer.persist = true;
        studio.grid = Grid::load();
        studio.persist_grid = true;
        match scene_path {
            Some(path) => _ = studio.open_path(&path),
            None => {
                let opened = settings::last_scene().is_some_and(|path| studio.open_path(&path));
                if !opened {
                    studio.new_scene();
                }
                studio.show_start();
            }
        }
        studio
    }

    /// The state without a window: no audio, no renderer. A brand-new scene is open.
    pub fn headless() -> Self {
        let kit = Arc::new(bpe_kit::kit());
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = resolve_scene(&scene, &kit.registry);
        let fingerprint = fingerprint(&scene, &resolved, &kit.sims);
        let playback = Playback::new(&scene, &resolved, kit.sims.clone());
        Self {
            saved: scene.clone(),
            scene,
            resolved,
            path: None,
            playback,
            fingerprint,
            revision: 0,
            kit,
            time: 0.0,
            warp: TimeWarp::identity(),
            playing: false,
            looping: true,
            muted: false,
            selection: Selection::default(),
            history: History::default(),
            status: "Ready.".into(),
            shader_problem: None,
            confirm: None,
            panels: Panels::default(),
            grid: Grid::default(),
            preview_mode: Default::default(),
            world_nav: Default::default(),
            view_centre: glam::Vec2::ZERO,
            persist_grid: false,
            sound_dirty: true,
            audio: None,
            audio_error: None,
            resume_after_scrub: false,
            song_loading: false,
            last_song: String::new(),
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
        self.resolved = resolve_scene(&self.scene, &self.kit.registry);
        self.start_song();
        let fingerprint = fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        if fingerprint != self.fingerprint {
            self.fingerprint = fingerprint;
            self.playback = Playback::new(&self.scene, &self.resolved, self.kit.sims.clone());
        }
        self.sound_dirty = true;
        self.time = self.time.min(self.duration());
        self.revision += 1;
    }

    pub fn replace_scene(&mut self, mut scene: Scene, path: Option<PathBuf>) {
        // Every scene the editor holds has a camera; an older file gets one at the identity, so
        // it looks and sounds as it always did.
        bpe_kit::ensure_camera(&mut scene, &self.kit.registry);
        self.saved = scene.clone();
        self.scene = scene;
        self.path = path;
        self.history = History::default();
        self.selection.clear();
        self.time = 0.0;
        self.set_playing(false);
        self.recorded = true;
        self.resolved = resolve_scene(&self.scene, &self.kit.registry);
        self.start_song();
        self.fingerprint = fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        self.playback = Playback::new(&self.scene, &self.resolved, self.kit.sims.clone());
        self.sound_dirty = true;
        self.revision += 1;
    }

    /// Starts decoding the scene's song on a thread of its own before the run is built, so the
    /// window never waits for it. Until it lands the run and the mix have no song; `poll_song`
    /// rebuilds them when it does. A file that failed before is tried again when re-picked.
    fn start_song(&mut self) {
        let song = self.settings().song;
        if song != self.last_song {
            if matches!(bpe_kit::load_song_in_background(&song), Some(SongState::Unreadable)) {
                bpe_kit::forget_song(&song);
            }
            self.last_song = song.clone();
        }
        self.song_loading = matches!(bpe_kit::load_song_in_background(&song), Some(SongState::Loading));
    }

    /// Call every frame: when a song has finished loading, the run and the mix are built again
    /// with it.
    fn poll_song(&mut self, ctx: &egui::Context) {
        if !self.song_loading {
            return;
        }
        match bpe_kit::load_song_in_background(&self.settings().song) {
            Some(SongState::Loading) => ctx.request_repaint_after(Duration::from_millis(100)),
            _ => {
                self.song_loading = false;
                self.playback = Playback::new(&self.scene, &self.resolved, self.kit.sims.clone());
                self.revision += 1;
                self.panels.sound.remix();
            }
        }
    }

    pub fn new_scene(&mut self) {
        self.replace_scene(bpe_kit::new_studio_scene(&self.kit.registry), None);
        self.status = "Started a new scene.".into();
    }

    /// Rolls the dice on `scope` from the scene as it is: one undo step back to `before` (the
    /// scene as the user last saw it, which may differ from what the roll starts from), and a
    /// word in the status line. Returns how many settings changed.
    pub fn roll_recording(&mut self, before: Scene, scope: Scope, seed: u32, amount: f32) -> usize {
        let rolled = roll(&mut self.scene, &self.kit.registry, &scope, Roll { seed, amount });
        let changed = rolled.changed.len();
        self.after_separate_edit(before);
        self.recorded = true;
        self.status = match changed {
            0 => "The dice found nothing to change (everything locked, keyed or never random).".to_owned(),
            1 => format!("Rolled 1 setting (seed {seed})."),
            n => format!("Rolled {n} settings (seed {seed})."),
        };
        changed
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
            Pending::Starter(scene) => {
                let name = scene.name.clone();
                self.replace_scene(*scene, None);
                self.status = format!("Started from {name}. Save it to keep it.");
            }
            Pending::OpenPath(path) => _ = self.open_path(&path),
        }
    }

    /// Does what was chosen in the Start window, asking to save changes first.
    pub fn choose_start(&mut self, choice: Choice, ctx: &egui::Context) {
        match choice {
            Choice::Nothing => {}
            Choice::Blank => self.guard(Pending::New, ctx),
            Choice::Scene(scene) => self.guard(Pending::Starter(scene), ctx),
            Choice::Open => self.guard(Pending::Open, ctx),
            Choice::Recent(path) => self.guard(Pending::OpenPath(path), ctx),
        }
    }

    /// Opens the Start window with the recent files.
    pub fn show_start(&mut self) {
        self.panels.start.open_with(settings::recent_scenes());
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
        // A node canvas under the pointer uses Space, Delete and the arrows itself.
        let canvas_keys = crate::nodes::take_wants_keyboard(ctx);
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
            self.show_start();
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
        if canvas_keys {
            return;
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
            if ui.button("New").on_hover_text("Start a new scene, blank or from a starter (Ctrl+N)").clicked() {
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
            if dice::dice_button(ui, "Roll the dice").on_hover_text("Roll the dice on the whole scene").clicked() {
                self.clicked = Some(ToolbarClick::Dice);
            }
            if ui.button("Randomize…").on_hover_text("Roll the dice with a seed, an amount and locks").clicked() {
                self.clicked = Some(ToolbarClick::Randomize);
            }
            if ui
                .button("Explore…")
                .on_hover_text("See nine variations of this scene side by side and pick one")
                .clicked()
            {
                self.clicked = Some(ToolbarClick::Explore);
            }
            if ui.button("Batch…").on_hover_text("Render many seeds of this scene and keep the best").clicked() {
                self.clicked = Some(ToolbarClick::Batch);
            }
            ui.separator();
            self.grid_controls(ui);
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.scene.name).desired_width(150.0).hint_text("Scene name"))
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
                let library = egui::Button::selectable(self.panels.library.open, "Library");
                if ui
                    .add(library)
                    .on_hover_text("Shared modules: rename, duplicate, delete, drag onto a thing to link")
                    .clicked()
                {
                    self.panels.library.open = !self.panels.library.open;
                }
            });
        });
    }

    /// The grid buttons: show the lines, snap drags to them, and the size of one square.
    fn grid_controls(&mut self, ui: &mut egui::Ui) {
        let before = self.grid;
        let grid = ui
            .add(egui::Button::selectable(self.grid.show, "Grid"))
            .on_hover_text("Show grid lines over the preview. They never show in the video.");
        if grid.clicked() {
            self.grid.show = !self.grid.show;
        }
        if ui
            .add(egui::Button::selectable(self.grid.snap, "Snap"))
            .on_hover_text("Drags land on the grid: moving, resizing the arena and aiming. Hold Alt to place freely.")
            .clicked()
        {
            self.grid.snap = !self.grid.snap;
        }
        ui.add(
            egui::DragValue::new(&mut self.grid.spacing).range(grid::SPACING).speed(1.0).max_decimals(0).suffix(" px"),
        )
        .labelled_by(grid.id)
        .on_hover_text("The size of one grid square, in video pixels");
        if self.grid != before && self.persist_grid {
            self.grid.save();
        }
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
            match bpe_kit::load_song_in_background(&song) {
                Some(SongState::Loading) => {
                    ui.label(RichText::new("· Loading the song…").small().color(theme::MUTED));
                }
                Some(SongState::Unreadable) => {
                    let text = format!("The song {song} couldn't be opened; the video has no song.");
                    ui.label(RichText::new(text).small().color(theme::WARNING));
                }
                _ => {}
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
        self.panels.batch.shutdown();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && self.unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
        self.tick();
        self.poll_song(&ctx);
        self.update_sound(&ctx);
        let before = self.scene.clone();
        let revision = self.revision;
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
        // Declared after the timeline so it sits above it, right under the preview.
        self.panels.nodes.show(ui, &mut self.scene, &self.kit, &self.resolved, Some(self.playback.world()));
        let tree = egui::Panel::left("tree")
            .default_size(260.0)
            .min_size(180.0)
            .max_size(480.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                tree::show(ui, &mut self.panels.tree, &mut self.scene, &self.kit, &self.resolved, &mut self.selection)
            })
            .inner;
        if let Some(message) = tree.message {
            self.status = message;
        }
        // A new thing goes in the middle of whatever the preview is looking at.
        if let Some(id) = tree.added
            && let Some(thing) = self.scene.things.get_mut(&id)
            && !thing.rides()
        {
            thing.transform.position = self.view_centre.round();
        }
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
                self.panels.inspector.nodes_open = self.panels.nodes.open;
                let time = self.sim_time();
                inspector::show(
                    ui,
                    &mut self.panels.inspector,
                    &mut self.scene,
                    &self.kit,
                    &self.resolved,
                    &self.selection,
                    time,
                )
            });
        if let Some((thing, slot)) = self.panels.inspector.nodes_request.take() {
            self.panels.nodes.open_at(thing, slot);
        }
        if std::mem::take(&mut self.panels.inspector.keyed_edit) {
            // A key landed at the playhead; a drag while playing would otherwise key every frame.
            self.set_playing(false);
        }
        if let Some(module) = self.panels.inspector.open_library.take() {
            self.panels.library.open = true;
            self.panels.library.selected = Some(module);
        }
        let library = library::show(&ctx, &mut self.panels.library, &mut self.scene, &self.kit, &self.resolved);
        if let Some(ids) = library.select {
            self.selection.clear();
            if let Some((&first, rest)) = ids.split_first() {
                self.selection.set(first);
                self.selection.extra.extend(rest.iter().copied());
            }
        }
        if let Some(message) = library.message {
            self.status = message;
        }
        let outcome = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BACKGROUND))
            .show(ui, |ui| preview::show(ui, frame, self))
            .inner;
        if outcome.clicked {
            self.set_playing(false);
        }
        if outcome.drag_started {
            // Dragging lays the scene out, so show where everything starts. The camera stays at
            // the playhead, so a keyed shot gets its key where it is being looked at.
            self.set_playing(false);
            if !outcome.drag_camera {
                self.seek(0.0);
            }
        }
        if let Some(id) = outcome.selected {
            self.selection.set(id);
        }
        self.shader_problem = outcome.shader_problem;
        if outcome.catching_up {
            ctx.request_repaint();
        }
        let base_dir = self.base_dir();
        let settings = self.settings();
        self.panels.export.show(&ctx, &self.scene, &settings, base_dir.as_deref(), &self.kit);
        dice::show(&ctx, self);
        if let explorer::Outcome::Batch { base, amount } = explorer::show(&ctx, self, frame.wgpu_render_state()) {
            self.panels.explorer.close();
            self.panels.batch.open_with(&base, amount);
        }
        let weights = self.panels.explorer.weights;
        batch_window::show(&ctx, &mut self.panels.batch, &self.kit, weights);
        let kit = self.kit.clone();
        let choice = starters::show(&ctx, &mut self.panels.start, &kit, frame.wgpu_render_state());
        self.choose_start(choice, &ctx);

        match self.clicked.take() {
            Some(ToolbarClick::New) => self.show_start(),
            Some(ToolbarClick::Open) => self.guard(Pending::Open, &ctx),
            Some(ToolbarClick::Save) => _ = self.save(false),
            Some(ToolbarClick::SaveAs) => _ = self.save(true),
            Some(ToolbarClick::Undo) => _ = self.undo(),
            Some(ToolbarClick::Redo) => _ = self.redo(),
            Some(ToolbarClick::Export) => self.panels.export.open = true,
            Some(ToolbarClick::Randomize) => self.panels.randomize.open = true,
            Some(ToolbarClick::Dice) => {
                let amount = self.panels.randomize.amount;
                dice::roll_from_base(self, Scope::Scene, fresh_seed(), amount);
            }
            Some(ToolbarClick::Explore) => self.panels.explorer.open_from(&self.scene, &self.kit),
            Some(ToolbarClick::Batch) => {
                let amount = self.panels.batch.spec.amount;
                self.panels.batch.open_with(&self.scene, amount);
            }
            None => {}
        }
        self.confirm_dialog(&ctx);

        if !std::mem::take(&mut self.recorded) {
            self.after_edit(before);
        }
        if self.revision != revision {
            // The preview drew this frame before the edit was resolved; draw it again now.
            ctx.request_repaint();
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
