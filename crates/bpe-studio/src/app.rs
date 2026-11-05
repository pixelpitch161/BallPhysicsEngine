//! The editor's state and frame: files, undo, shortcuts and the panel layout.

use crate::audio_out::AudioOut;
use crate::export_window::ExportWindow;
use crate::grid::{self, Grid};
use crate::history::{History, UNDO_MERGE};
use crate::inspector::Inspector;
use crate::mixer::{self, Mixer};
use crate::preview::Preview;
use crate::selection::Selection;
use crate::settings::ListenSettings;
use crate::sound_job::SoundJob;
use crate::timeline::Timeline;
use crate::tree::Tree;
use crate::{inspector, preview, settings, style, timeline, tree};
use bpe_core::{Resolved, Scene, Settings, ThingId};
use bpe_dsp::{WarpModeId, WaveBank, WaveShape};
use bpe_kit::{Kit, SceneSettings, resolve_scene};
use bpe_runtime::world::sound_fingerprint;
use bpe_runtime::{Playback, fingerprint};
use bpe_sound::SoundPatch;
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText, Stroke};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Most time spent catching the simulation up in one go. A long stretch, such as a scrub far
/// ahead on a huge scene, is worked out over several frames by the preview instead of
/// freezing the editor.
pub(crate) const CATCH_UP_BUDGET: Duration = Duration::from_millis(12);
/// How often missing speakers are looked for again.
const SPEAKERS_RETRY: Duration = Duration::from_secs(2);
/// The longest wait between looks for speakers (each look waits twice the last).
const SPEAKERS_RETRY_MOST: Duration = Duration::from_secs(30);

/// Something that would throw away unsaved changes.
pub enum Pending {
    Close,
    Open,
    New,
    /// A recent file from the toolbar's Recent menu.
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
    pub timeline: Timeline,
    pub mixer: Mixer,
}

/// What the left panel shows: the things in the scene, or the Mixer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeftPanel {
    #[default]
    Things,
    Mixer,
}

/// The two chips on top of the left panel.
fn left_switch(ui: &mut egui::Ui, left: &mut LeftPanel) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (panel, name) in [(LeftPanel::Things, "Things"), (LeftPanel::Mixer, "Mixer")] {
            let hint = match panel {
                LeftPanel::Things => "The things in the scene",
                LeftPanel::Mixer => "Tempo, the Master and the meters",
            };
            if ui.add(style::toggle_button(name, *left == panel)).on_hover_text(hint).clicked() {
                *left = panel;
            }
        }
    });
    ui.add_space(6.0);
}

enum ToolbarClick {
    New,
    Open,
    Recent(PathBuf),
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
    /// The part of it the sound hears ([`sound_fingerprint`]): the sound starts its run over
    /// only when this changes.
    pub sound_fingerprint: u64,
    /// The scene's sound as the picture reads it, while some setting follows sound.
    pub follow: Option<std::sync::Arc<bpe_sound::ControlSampler>>,
    /// Goes up whenever `resolved` or `playback` is rebuilt for a changed scene, so the
    /// preview knows its picture is out of date. The scene itself can be mid-edit during a
    /// frame, ahead of both, so the preview never compares scenes.
    pub revision: u64,
    /// The playhead, in video seconds.
    pub time: f64,
    pub playing: bool,
    pub looping: bool,
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
    /// Remember grid and panel changes in the settings folder (off without a window, in tests).
    persist_settings: bool,
    /// The inspector's width as last saved.
    inspector_width: f32,
    /// The inspector's width on a shape's Sound tab as last saved.
    sound_width: f32,
    /// The inspector's width last frame.
    inspector_seen: f32,
    /// The inspector's edge is being dragged: its width changed while a button was down.
    inspector_resizing: bool,
    /// The inspector showed a Sound tab last frame.
    inspector_sound: bool,
    /// The tree panel's width when it was last drawn: the Mixer opens at it.
    tree_width: f32,
    /// How many times the left panel has switched to the Mixer. Its panel's id counts them, so
    /// each switch opens it afresh at the tree's width.
    mixer_opens: u32,
    /// Scrubbing paused the playback; it starts again when the scrub ends.
    pub resume_after_scrub: bool,
    /// This frame's change to the scene is already in the history (undo, open, delete…).
    pub recorded: bool,
    closing: bool,
    last_tick: Option<Instant>,
    clicked: Option<ToolbarClick>,
    /// The sound thread and the speakers; `None` without a sound device (or in tests), when
    /// the playhead follows the wall clock.
    pub sound: Option<SoundJob>,
    /// When to look for the speakers again (they failed, or there were none at the start).
    speakers_retry: Option<Instant>,
    /// How long the next look for speakers waits.
    speakers_wait: Duration,
    /// The sound thread itself stopped (not the speakers): it isn't started again by itself.
    sound_broken: bool,
    /// The listening volume changed and is saved once it has rested (not on every frame of a
    /// drag).
    listen_unsaved: Option<Instant>,
    /// The speaker button and the listening volume (only what you hear).
    pub listen: ListenSettings,
    /// The left panel's face (not saved in the scene).
    pub left: LeftPanel,
    /// Wavetables baked in the background when the sound was last built
    /// ([`WaveBank::baked`]); more means it is built again.
    tables_baked: u64,
}

impl Studio {
    pub fn new(cc: &eframe::CreationContext, scene_path: Option<PathBuf>) -> Self {
        style::apply(&cc.egui_ctx);
        crate::palettes::install(&cc.egui_ctx, crate::palettes::PaletteStore::open());
        let mut studio = Self::blank();
        studio.grid = Grid::load();
        studio.inspector_width = settings::load_inspector_width();
        studio.sound_width = settings::load_sound_width();
        studio.persist_settings = true;
        studio.listen = settings::load_listen();
        if let Some(path) = scene_path {
            studio.open_path(&path);
        }
        // The starter sound's wavetables (Triangle, and the subs' Square), so Enable sound is
        // heard at once.
        for wave in [WaveShape::Triangle, WaveShape::Square] {
            WaveBank::ready(wave, WarpModeId::Sync);
        }
        studio.sound =
            AudioOut::open().and_then(|out| SoundJob::start(&studio.scene, &studio.resolved, studio.kit.clone(), out));
        studio.apply_listen();
        studio
    }

    /// The state without a window, open on the blank project the editor always starts with.
    pub fn blank() -> Self {
        let kit = Arc::new(bpe_sound::kit());
        let scene = bpe_kit::new_studio_scene(&kit.registry);
        Self::with_scene(kit, scene)
    }

    /// The state without a window for tests: the test scene with an arena and a ball.
    #[cfg(test)]
    pub fn headless() -> Self {
        let kit = Arc::new(bpe_sound::kit());
        let scene = bpe_kit::new_scene(&kit.registry);
        Self::with_scene(kit, scene)
    }

    fn with_scene(kit: Arc<Kit>, mut scene: Scene) -> Self {
        // Every scene the editor holds has its sound's Master.
        bpe_kit::ensure_mixer(&mut scene, &kit.registry);
        let resolved = resolve_scene(&scene, &kit.registry);
        let fingerprint = fingerprint(&scene, &resolved, &kit.sims);
        let sound_fingerprint = sound_fingerprint(&scene, &resolved, &kit.sims);
        let playback = Playback::new(&scene, &resolved, kit.sims.clone());
        let follow = follow_of(&scene, &resolved);
        Self {
            saved: scene.clone(),
            scene,
            resolved,
            path: None,
            playback,
            fingerprint,
            sound_fingerprint,
            follow,
            revision: 0,
            kit,
            time: 0.0,
            playing: false,
            looping: true,
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
            persist_settings: false,
            inspector_width: settings::INSPECTOR_DEFAULT,
            sound_width: settings::SOUND_WIDTH_DEFAULT,
            inspector_seen: settings::INSPECTOR_DEFAULT,
            inspector_resizing: false,
            inspector_sound: false,
            tree_width: style::TREE_W,
            mixer_opens: 0,
            resume_after_scrub: false,
            recorded: false,
            closing: false,
            last_tick: None,
            clicked: None,
            sound: None,
            speakers_retry: None,
            speakers_wait: SPEAKERS_RETRY,
            sound_broken: false,
            listen_unsaved: None,
            listen: ListenSettings::default(),
            left: LeftPanel::Things,
            tables_baked: WaveBank::baked(),
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

    /// The moment of the run the playhead shows: the video clock is the simulation's.
    pub fn sim_time(&self) -> f64 {
        self.time
    }

    pub fn unsaved(&self) -> bool {
        self.scene != self.saved
    }

    /// Puts a new thing in the middle of whatever the preview is looking at. A thing stuck to
    /// the screen goes in the middle of the picture, which is where the camera isn't.
    pub fn place_added(&mut self, id: ThingId) {
        let stuck = self
            .scene
            .things
            .get(&id)
            .is_some_and(|t| self.kit.registry.thing(&t.kind).is_some_and(|k| t.sticks_to_screen(k)));
        if let Some(thing) = self.scene.things.get_mut(&id)
            && !thing.rides()
        {
            thing.transform.position = if stuck { glam::Vec2::ZERO } else { self.view_centre.round() };
        }
    }

    /// Records the edit that turned `before` into the scene as it is now, unless nothing
    /// changed, and brings the simulation along.
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
        if std::mem::take(&mut self.panels.inspector.own_step) {
            self.history.end_merging();
        }
        self.history.record(before, separate);
        self.scene_changed();
    }

    /// Re-resolves after any change. The playback starts over only when the physics changed:
    /// a new colour keeps the balls where they are.
    fn scene_changed(&mut self) {
        self.resolved = resolve_scene(&self.scene, &self.kit.registry);
        let fingerprint = fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        let rebuilt = fingerprint != self.fingerprint;
        self.follow = follow_of(&self.scene, &self.resolved);
        if rebuilt {
            self.fingerprint = fingerprint;
            self.playback = Playback::new(&self.scene, &self.resolved, self.kit.sims.clone());
        }
        self.time = self.time.min(self.duration());
        self.revision += 1;
        // The sound follows: a physics change starts its world over (no hits from the old
        // layout), anything else swaps the settings in where it is. How much of the balls' past
        // trails keep is the editor's alone: the sound plays on through it.
        let heard = sound_fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        let sound_rebuilt = heard != self.sound_fingerprint;
        self.sound_fingerprint = heard;
        if let Some(job) = &mut self.sound {
            if sound_rebuilt {
                job.rebuild(&self.scene, &self.resolved);
                if self.playing {
                    job.seek(self.time);
                }
            } else {
                job.patch(SoundPatch::build_ready(&self.scene, &self.resolved, &self.kit));
            }
        }
    }

    pub fn replace_scene(&mut self, mut scene: Scene, path: Option<PathBuf>) {
        // Every scene the editor holds has a camera; an older file gets one at the identity, so
        // it looks as it always did.
        bpe_kit::ensure_camera(&mut scene, &self.kit.registry);
        // And a mixer card, for its sound.
        bpe_kit::ensure_mixer(&mut scene, &self.kit.registry);
        self.saved = scene.clone();
        self.scene = scene;
        self.path = path;
        self.history = History::default();
        self.selection.clear();
        self.time = 0.0;
        self.set_playing(false);
        self.recorded = true;
        self.resolved = resolve_scene(&self.scene, &self.kit.registry);
        self.fingerprint = fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        self.sound_fingerprint = sound_fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        self.playback = Playback::new(&self.scene, &self.resolved, self.kit.sims.clone());
        self.follow = follow_of(&self.scene, &self.resolved);
        self.revision += 1;
        // The file chosen to export to was the last scene's.
        self.panels.export.path = None;
        if let Some(job) = &mut self.sound {
            job.rebuild(&self.scene, &self.resolved);
        }
    }

    pub fn new_scene(&mut self) {
        self.replace_scene(bpe_kit::new_studio_scene(&self.kit.registry), None);
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

    /// Gives the inspector the selected emitter's rounds as the playback has them so far, and
    /// what its aim should warn about.
    fn fill_rounds(&mut self) {
        let rounds = &mut self.panels.inspector.rounds;
        rounds.clear();
        self.panels.inspector.aim_checks.clear();
        let Some(id) = self
            .selection
            .primary
            .filter(|id| self.scene.things.get(id).is_some_and(|t| t.kind == bpe_kit::things::emitter::KIND))
        else {
            return;
        };
        let duration =
            bpe_kit::SceneSettings::from_params(&self.scene.things[&bpe_core::ThingId::ROOT].params).duration;
        rounds.insert(id, bpe_kit::things::emitter::sim::round_marks(self.playback.world(), id, duration as f64));
        if let Some(check) = bpe_kit::things::emitter::aim::check(self.playback.world(), id) {
            self.panels.inspector.aim_checks.insert(id, check);
        }
    }

    // ---------- playback ----------

    /// Moves the playhead. The simulation follows within the budget; the preview catches up
    /// the rest a budget per frame.
    pub fn seek(&mut self, time: f64) {
        self.time = time.clamp(0.0, self.duration());
        self.playback.seek_within(self.sim_time(), Some(CATCH_UP_BUDGET));
        if self.playing
            && let Some(job) = &mut self.sound
        {
            job.seek(self.time);
        }
    }

    pub fn set_playing(&mut self, playing: bool) {
        if playing && self.time >= self.duration() {
            self.time = 0.0;
        }
        if let Some(job) = &mut self.sound
            && playing != self.playing
        {
            if playing { job.play(self.time) } else { job.pause() }
        }
        self.playing = playing;
        self.last_tick = None;
    }

    /// Sets the speaker button and listening volume, and remembers them.
    pub fn set_listen(&mut self, listen: ListenSettings) {
        if listen == self.listen {
            return;
        }
        self.listen = listen;
        self.apply_listen();
        if self.persist_settings {
            self.listen_unsaved = Some(Instant::now());
        }
    }

    /// Saves the listening settings once they have rested half a second, and on the way out.
    fn save_listen_if_rested(&mut self, now: Option<Instant>) {
        if let Some(changed) = self.listen_unsaved
            && now.is_none_or(|t| t.duration_since(changed) >= Duration::from_millis(500))
        {
            settings::save_listen(self.listen);
            self.listen_unsaved = None;
        }
    }

    /// The speakers: after they fail (unplugged, or the default output changed) or when there
    /// were none at the start, they are looked for again every two seconds, and the sound picks
    /// up where the playhead is.
    fn keep_speakers(&mut self, now: Instant) {
        if !self.persist_settings {
            // Tests (and anything without a window) never open a device.
            return;
        }
        if let Some(job) = &self.sound
            && job.failed()
        {
            if job.device_failed() {
                self.status = "The speakers stopped working. Looking for them again…".into();
            } else {
                // A sound thread that stopped by itself would stop again: no looping restarts.
                self.status = "The sound stopped working. Save, then close and reopen Ball Studio.".into();
                self.sound_broken = true;
            }
            self.sound = None;
            self.speakers_wait = SPEAKERS_RETRY;
            self.speakers_retry = Some(now + SPEAKERS_RETRY);
        }
        if self.sound.is_some() || self.sound_broken {
            return;
        }
        let due = self.speakers_retry.get_or_insert(now + self.speakers_wait);
        if now < *due {
            return;
        }
        // Opening a device can take a moment: look less and less often while there is none.
        self.speakers_wait = (self.speakers_wait * 2).min(SPEAKERS_RETRY_MOST);
        *due = now + self.speakers_wait;
        let Some(mut job) =
            AudioOut::open().and_then(|out| SoundJob::start(&self.scene, &self.resolved, self.kit.clone(), out))
        else {
            return;
        };
        if self.playing {
            job.play(self.time);
        } else {
            // Keys clicked while stopped read the settings at the playhead.
            job.seek(self.time);
        }
        self.sound = Some(job);
        self.apply_listen();
        self.speakers_retry = None;
        self.speakers_wait = SPEAKERS_RETRY;
        self.status = "The speakers are back.".into();
    }

    fn apply_listen(&self) {
        if let Some(job) = &self.sound {
            job.shared().set_volume(self.listen.volume);
            job.shared().set_muted(!self.listen.on);
        }
    }

    /// Moves the playhead while playing: with the sound the speakers have played, or with the
    /// wall clock when there are no speakers.
    fn tick(&mut self) {
        let now = Instant::now();
        let dt = self.last_tick.replace(now).map_or(0.0, |last| (now - last).as_secs_f64().min(0.25));
        self.save_listen_if_rested(Some(now));
        self.keep_speakers(now);
        if !self.playing {
            return;
        }
        let duration = self.duration();
        let next = match &self.sound {
            Some(job) => job.clock(),
            None => self.time + dt,
        };
        if next < duration {
            self.time = next;
        } else if self.looping {
            self.time = 0.0;
            if let Some(job) = &mut self.sound {
                job.seek(0.0);
            }
        } else {
            self.time = duration;
            self.set_playing(false);
        }
    }

    // ---------- files ----------

    pub(crate) fn base_dir(&self) -> Option<PathBuf> {
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
            Pending::OpenPath(path) => _ = self.open_path(&path),
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
                settings::remember_scene(&path);
                self.path = Some(path);
                self.saved = self.scene.clone();
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
        // F11: fully fullscreen (no title bar), and back to the window.
        if pressed(Modifiers::NONE, Key::F11) {
            let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
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
        // Export sits at the right, as wide as it was last drawn; everything else gets the
        // room left of it and wraps onto a second line rather than running under it.
        let right_id = ui.make_persistent_id("toolbar right");
        let right_width = ui.ctx().data(|d| d.get_temp::<f32>(right_id)).unwrap_or(SAVE_STATUS_WIDTH);
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
            let left = egui::vec2((ui.available_width() - right_width).max(0.0), 0.0);
            let wrap = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
            ui.allocate_ui_with_layout(left, wrap, |ui| {
                ui.set_width(left.x);
                self.toolbar_left(ui);
            });
            let right = ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let export = style::primary_button("Export video…").fill(style::TEXT);
                if ui.add(export).on_hover_text("Render the scene to an mp4").clicked() {
                    self.clicked = Some(ToolbarClick::Export);
                }
            });
            // The gap before Export counts as part of the right side.
            let width = right.response.rect.width() + ui.spacing().item_spacing.x;
            if (width - right_width).abs() > 0.5 {
                ui.ctx().data_mut(|d| d.insert_temp(right_id, width));
                ui.ctx().request_discard("the toolbar's right side changed width");
            }
        });
    }

    /// The toolbar's buttons left of Export, ending with the scene's name and its save status.
    fn toolbar_left(&mut self, ui: &mut egui::Ui) {
        if style::toolbar_button(ui, "New")
            .on_hover_text("Start a new blank scene with just a camera (Ctrl+N)")
            .clicked()
        {
            self.clicked = Some(ToolbarClick::New);
        }
        if style::toolbar_button(ui, "Open…").on_hover_text("Open a saved scene (Ctrl+O)").clicked() {
            self.clicked = Some(ToolbarClick::Open);
        }
        ui.menu_button("Recent", |ui| {
            let recent = settings::recent_scenes();
            if recent.is_empty() {
                ui.label("No scenes opened yet");
            }
            for path in recent {
                if ui.button(file_label(&path)).on_hover_text(path.display().to_string()).clicked() {
                    self.clicked = Some(ToolbarClick::Recent(path));
                }
            }
        })
        .response
        .on_hover_text("Open a scene you opened or saved lately");
        if style::toolbar_button(ui, "Save").on_hover_text("Save this scene (Ctrl+S)").clicked() {
            self.clicked = Some(ToolbarClick::Save);
        }
        if style::toolbar_button(ui, "Save as…")
            .on_hover_text("Save this scene under a new name (Ctrl+Shift+S)")
            .clicked()
        {
            self.clicked = Some(ToolbarClick::SaveAs);
        }
        style::bar_separator(ui);
        let undo = egui::Button::new("Undo").frame_when_inactive(false);
        if ui.add_enabled(self.history.can_undo(), undo).on_hover_text("Undo (Ctrl+Z)").clicked() {
            self.clicked = Some(ToolbarClick::Undo);
        }
        let redo = egui::Button::new("Redo").frame_when_inactive(false);
        if ui.add_enabled(self.history.can_redo(), redo).on_hover_text("Redo (Ctrl+Y)").clicked() {
            self.clicked = Some(ToolbarClick::Redo);
        }
        style::bar_separator(ui);
        self.grid_controls(ui);
        style::bar_separator(ui);
        ui.add(egui::TextEdit::singleline(&mut self.scene.name).desired_width(170.0).hint_text("Scene name"))
            .on_hover_text("The scene's name. Exported videos are named after it.");
        let saved_label = match (&self.path, self.unsaved()) {
            (None, _) => RichText::new("Not saved yet").color(style::MUTED),
            (Some(_), true) => RichText::new("• Unsaved changes").color(style::TEXT),
            (Some(path), false) => RichText::new(format!("Saved as {}", file_label(path))).color(style::MUTED),
        };
        // A slot of its own, so a long file name is cut short; with too little room on this
        // line it goes to the next.
        let full = saved_label.text().to_owned();
        // What is left of this line (a wrapping layout's available width is a whole line).
        let room = ui.max_rect().right() - ui.cursor().left();
        let width = if room >= MIN_SAVE_STATUS_WIDTH { room.min(SAVE_STATUS_WIDTH) } else { SAVE_STATUS_WIDTH };
        let status = ui
            .allocate_ui_with_layout(
                egui::vec2(width, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_width(width);
                    ui.add(egui::Label::new(saved_label.small()).truncate().show_tooltip_when_elided(false))
                },
            )
            .inner;
        status.on_hover_text(full);
    }

    /// The inspector on the right, as wide as the user last dragged it. Returns its width.
    /// The left panel, Things or Mixer, with the switch on top. The tree keeps its width; the
    /// Mixer opens at the tree's width each time it is switched to, and can be dragged wider
    /// while open. Returns the width it was drawn at.
    pub(crate) fn left_panel(&mut self, ui: &mut egui::Ui, before: &Scene) -> f32 {
        let mut left = self.left;
        let width = match self.left {
            LeftPanel::Things => {
                let shown = egui::Panel::left("tree")
                    .default_size(style::TREE_W)
                    .min_size(180.0)
                    .max_size(480.0)
                    .resizable(true)
                    .frame(side_frame().inner_margin(egui::Margin::symmetric(8, 10)))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        left_switch(ui, &mut left);
                        tree::show(
                            ui,
                            &mut self.panels.tree,
                            &mut self.scene,
                            &self.kit,
                            &self.resolved,
                            &mut self.selection,
                        )
                    });
                let tree = shown.inner;
                if let Some(message) = tree.message {
                    self.status = message;
                }
                if let Some(id) = tree.added {
                    self.place_added(id);
                }
                if tree.removed.is_some() {
                    // Deleting is always its own undo step.
                    self.recorded = true;
                    self.after_separate_edit(before.clone());
                }
                self.tree_width = shown.response.rect.width();
                self.tree_width
            }
            LeftPanel::Mixer => {
                let meters = self.sound.as_ref().map(SoundJob::meters);
                let reading = meters.as_ref().map(|m| m.read());
                let shown = egui::Panel::left(("mixer", self.mixer_opens))
                    .default_size(self.tree_width)
                    .min_size(180.0)
                    .max_size(960.0)
                    .resizable(true)
                    .frame(side_frame().inner_margin(egui::Margin::symmetric(10, 10)))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        left_switch(ui, &mut left);
                        let keyed = mixer::show(
                            ui,
                            &mut self.panels.mixer,
                            &mut self.scene,
                            &self.kit,
                            &self.resolved,
                            reading.as_ref(),
                            meters.as_deref(),
                            self.time,
                        );
                        if keyed {
                            self.panels.inspector.keyed_edit = true;
                        }
                    });
                if self.playing {
                    // The meters move while the sound plays.
                    ui.ctx().request_repaint();
                }
                shown.response.rect.width()
            }
        };
        if left == LeftPanel::Mixer && self.left != LeftPanel::Mixer {
            self.mixer_opens += 1;
        }
        self.left = left;
        width
    }

    pub(crate) fn inspector_panel(&mut self, ui: &mut egui::Ui) -> f32 {
        // A shape's Sound tab is wide (Osc A and Osc B side by side), with a width of its own:
        // a panel of its own id keeps each width as it was left.
        // (The Mixer's editors open in the Mixer itself: the inspector always shows the selection.)
        let sound = self.panels.inspector.sound_open(&self.scene, self.selection.primary);
        let (id, width, range) = if sound {
            ("inspector sound", self.sound_width, settings::SOUND_WIDTH_RANGE)
        } else {
            ("inspector", self.inspector_width, settings::INSPECTOR_RANGE)
        };
        if sound != self.inspector_sound {
            // The other width shows from this frame: not a drag of the edge.
            self.inspector_sound = sound;
            self.inspector_seen = width;
            self.inspector_resizing = false;
        }
        let panel = egui::Panel::right(id)
            .default_size(width)
            .min_size(*range.start())
            .max_size(*range.end())
            .resizable(true)
            .frame(side_frame())
            .show(ui, |ui| {
                // Claim the panel's whole width, or it shrinks to its widest row on the first frame.
                ui.set_min_width(ui.available_width());
                // The contents go in a child that only reports the panel's own rect, so a row that
                // is too wide for a frame (a grid's first frame lays out unmeasured) is clipped
                // instead of widening the panel for good.
                let rect = ui.available_rect_before_wrap();
                let mut contents = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(*ui.layout()));
                contents.shrink_clip_rect(rect);
                let time = self.sim_time();
                self.panels.inspector.meters = self.sound.as_ref().map(SoundJob::meters);
                self.panels.inspector.playing = self.playing;
                self.fill_rounds();
                inspector::show(
                    &mut contents,
                    &mut self.panels.inspector,
                    &mut self.scene,
                    &self.kit,
                    &self.resolved,
                    &self.selection,
                    time,
                );
                ui.advance_cursor_after_rect(rect);
            });
        let shown = panel.response.rect.width();
        // Only a drag of the edge is remembered, once the mouse lets go; a window squeezing the
        // panel is not.
        let down = ui.input(|i| i.pointer.any_down());
        if down && (shown - self.inspector_seen).abs() >= 0.5 {
            self.inspector_resizing = true;
        }
        self.inspector_seen = shown;
        if !down && std::mem::take(&mut self.inspector_resizing) && (shown - width).abs() >= 0.5 {
            if sound {
                self.sound_width = shown;
                if self.persist_settings {
                    settings::save_sound_width(shown);
                }
            } else {
                self.inspector_width = shown;
                if self.persist_settings {
                    settings::save_inspector_width(shown);
                }
            }
        }
        shown
    }

    /// Plays the notes clicked on a Sound tab's keyboard, after this frame's edit has reached
    /// the sound (a key that was just turned on plays with its new note list).
    /// When wavetables finish baking in the background, the sound is built again with them
    /// (until then the oscillators that need them are silent). Looks again shortly while a bake
    /// is under way.
    fn follow_bakes(&mut self, ctx: &egui::Context) {
        let baked = WaveBank::baked();
        if baked != self.tables_baked {
            self.tables_baked = baked;
            if let Some(job) = &mut self.sound {
                job.patch(SoundPatch::build_ready(&self.scene, &self.resolved, &self.kit));
            }
        }
        if WaveBank::busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    fn send_auditions(&mut self) {
        let auditions = std::mem::take(&mut self.panels.inspector.auditions);
        if let Some(job) = &mut self.sound
            && !auditions.is_empty()
        {
            // While stopped, a key sounds as the sound is at the playhead.
            job.park(self.time);
            for (thing, note) in auditions {
                job.audition(crate::sound_job::Audition { thing, note, velocity: 0.8, on: true });
            }
        }
    }

    /// The grid buttons: show the lines, snap drags to them, and the size of one square.
    fn grid_controls(&mut self, ui: &mut egui::Ui) {
        let before = self.grid;
        let grid = ui
            .add(style::toggle_button("Grid", self.grid.show))
            .on_hover_text("Show grid lines over the preview. They never show in the video.");
        if grid.clicked() {
            self.grid.show = !self.grid.show;
        }
        if ui
            .add(style::toggle_button("Snap", self.grid.snap))
            .on_hover_text("Drags land on the grid: moving, moulding shapes and aiming. Hold Alt to place freely.")
            .clicked()
        {
            self.grid.snap = !self.grid.snap;
        }
        style::wells(ui, |ui| {
            ui.add(
                egui::DragValue::new(&mut self.grid.spacing)
                    .range(grid::SPACING)
                    .speed(1.0)
                    .max_decimals(0)
                    .suffix(" px"),
            )
        })
        .labelled_by(grid.id)
        .on_hover_text("The size of one grid square, in video pixels");
        if self.grid != before && self.persist_settings {
            self.grid.save();
        }
    }

    fn status_line(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            ui.label(RichText::new(&self.status).small().color(style::MUTED));
            if let Some(problem) = &self.shader_problem {
                ui.label(RichText::new(format!("· Shader skipped: {problem}")).small().color(style::TEXT));
            }
            if self.panels.export.busy() {
                ui.label(RichText::new("· Exporting…").small().color(style::MUTED));
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
            ui.label(RichText::new("Your changes will be lost if you don't save them.").color(style::MUTED));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let save = style::primary_button("Save");
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
        self.save_listen_if_rested(None);
        self.sound = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && self.unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
        self.tick();
        if self.panels.inspector.render.is_none() {
            // The Look editor's ball view draws with the window's GPU.
            self.panels.inspector.render = frame.wgpu_render_state().cloned();
        }
        let before = self.scene.clone();
        let revision = self.revision;
        self.recorded = false;
        if self.confirm.is_none() {
            self.shortcuts(&ctx);
        }

        egui::Panel::top("toolbar").frame(bar_frame()).min_size(style::TOOLBAR_H).show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(style::GROUND)
                    .inner_margin(egui::Margin::symmetric(10, 0))
                    .stroke(Stroke::new(1.0, style::LINE_SOFT)),
            )
            .exact_size(style::STATUS_H)
            .show(ui, |ui| self.status_line(ui));
        egui::Panel::bottom("timeline")
            .frame(
                egui::Frame::new()
                    .fill(style::PANEL)
                    .inner_margin(egui::Margin { left: 10, right: 10, top: 0, bottom: 6 })
                    .stroke(Stroke::new(1.0, style::LINE_SOFT)),
            )
            .resizable(true)
            .default_size(190.0)
            .min_size(110.0)
            .max_size(520.0)
            .show(ui, |ui| timeline::show(ui, self));
        self.left_panel(ui, &before);
        self.inspector_panel(ui);
        if std::mem::take(&mut self.panels.inspector.keyed_edit) {
            // A key landed at the playhead; a drag while playing would otherwise key every frame.
            self.set_playing(false);
        }
        let outcome = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(style::STAGE))
            .show(ui, |ui| preview::show(ui, frame, self))
            .inner;
        if outcome.clicked {
            self.set_playing(false);
        }
        if outcome.drag_started {
            // Dragging lays the scene out, so show where everything starts. The camera stays at
            // the playhead, so a keyed shot gets its key where it is being looked at, and so does
            // moulding a shape, which goes on where it was grabbed.
            self.set_playing(false);
            if !outcome.keep_playhead {
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
        match self.clicked.take() {
            Some(ToolbarClick::New) => self.guard(Pending::New, &ctx),
            Some(ToolbarClick::Open) => self.guard(Pending::Open, &ctx),
            Some(ToolbarClick::Recent(path)) => self.guard(Pending::OpenPath(path), &ctx),
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
        self.send_auditions();
        self.follow_bakes(&ctx);
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

/// The toolbar's frame: the panel grey with a soft line under it.
fn bar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(style::PANEL)
        .inner_margin(egui::Margin::symmetric(8, 7))
        .stroke(Stroke::new(1.0, style::LINE_SOFT))
}

/// The tree's and the inspector's frame: the panel grey, touching its neighbours with a soft line.
fn side_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(style::PANEL)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .stroke(Stroke::new(1.0, style::LINE_SOFT))
}

/// Seconds as m:ss.ff.
pub(crate) fn clock(seconds: f64) -> String {
    let hundredths = (seconds.max(0.0) * 100.0).floor() as u64;
    format!("{}:{:02}.{:02}", hundredths / 6000, hundredths / 100 % 60, hundredths % 100)
}

/// How wide the toolbar's save status may be.
const SAVE_STATUS_WIDTH: f32 = 180.0;
/// The narrowest the save status is cut to before it goes to the toolbar's next line.
const MIN_SAVE_STATUS_WIDTH: f32 = 90.0;

fn file_label(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// The scene's sound for the picture, when any setting follows it.
fn follow_of(scene: &Scene, resolved: &bpe_core::Resolved) -> Option<std::sync::Arc<bpe_sound::ControlSampler>> {
    bpe_sound::follows_sound(scene).then(|| std::sync::Arc::new(bpe_sound::ControlSampler::new(scene, resolved)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Address, SlotId, ThingId, Value};
    use bpe_kit::BallsSettings;
    use std::path::Path;
    use std::time::Duration;

    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    #[test]
    fn no_device_falls_back_to_the_wall_clock() {
        let mut studio = Studio::headless();
        assert!(studio.sound.is_none(), "no speakers in tests");
        studio.set_playing(true);
        studio.tick();
        std::thread::sleep(Duration::from_millis(30));
        studio.tick();
        assert!(studio.time > 0.02, "the wall clock moves the playhead: {}", studio.time);
    }

    #[test]
    fn a_physics_edit_rebuilds_and_a_tempo_edit_patches() {
        let mut studio = Studio::headless();
        studio.sound = Some(SoundJob::recording());
        studio.set_playing(true);
        let ball = studio.scene.order.iter().copied().find(|id| studio.scene.things[id].kind == "balls").unwrap();
        let before = studio.scene.clone();
        studio.scene.things.get_mut(&ball).unwrap().transform.position.x += 40.0;
        studio.after_edit(before);
        let before = studio.scene.clone();
        studio.scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("tempo", Value::Number(97.0));
        studio.after_edit(before);
        studio.set_playing(false);
        let sent = studio.sound.as_ref().unwrap().recorded();
        assert_eq!(sent, ["seek", "play", "rebuild", "seek", "patch", "pause"]);
    }

    #[test]
    fn a_trail_length_edit_patches_the_sound_and_keeps_it_playing() {
        let mut studio = Studio::headless();
        let kit = studio.kit.clone();
        let before = studio.scene.clone();
        let trail = studio
            .scene
            .add_layer(&kit.registry, ThingId(2), SlotId::Look, bpe_kit::modules::look::layers::TRAIL)
            .unwrap();
        studio.after_edit(before);
        studio.sound = Some(SoundJob::recording());
        studio.set_playing(true);
        let physics = studio.fingerprint;
        let before = studio.scene.clone();
        // Longer when fast stretches its half-second Length to 3.5 s.
        let faster = Address::on(ThingId(2), SlotId::Look, &bpe_core::layered_key(trail, "faster"));
        studio.scene.set_at(&kit.registry, &faster, Value::Number(2.0)).unwrap();
        studio.after_edit(before);
        assert_ne!(studio.fingerprint, physics, "the editor keeps a longer history for the trail");
        let sent = studio.sound.as_ref().unwrap().recorded();
        assert_eq!(sent, ["seek", "play", "patch"], "the sound plays on");
    }

    #[test]
    fn a_key_clicked_while_stopped_sounds_at_the_playhead() {
        let mut studio = Studio::headless();
        studio.sound = Some(SoundJob::recording());
        let click = |studio: &mut Studio| {
            studio.panels.inspector.auditions.push((ThingId(1), 60));
            studio.send_auditions();
        };
        // At the start the sound is where the playhead is.
        click(&mut studio);
        // Scrubbing tells it nothing; the click takes it there first.
        studio.seek(3.0);
        studio.seek(5.0);
        click(&mut studio);
        click(&mut studio);
        // A rebuild starts the sound at 0 again.
        let before = studio.scene.clone();
        studio.scene.things.get_mut(&ThingId(2)).unwrap().transform.position.x += 40.0;
        studio.after_edit(before);
        click(&mut studio);
        let sent = studio.sound.as_ref().unwrap().recorded();
        assert_eq!(sent, ["audition", "seek", "audition", "audition", "rebuild", "seek", "audition"]);
    }

    #[test]
    fn another_scene_forgets_the_export_file_chosen() {
        let mut studio = Studio::headless();
        studio.panels.export.path = Some(PathBuf::from("/videos/A.mp4"));
        studio.new_scene();
        assert_eq!(studio.panels.export.path, None, "exporting the new scene doesn't overwrite A.mp4");
        studio.panels.export.path = Some(PathBuf::from("/videos/A.mp4"));
        studio.replace_scene(bpe_kit::new_scene(&studio.kit.registry), Some(PathBuf::from("/scenes/B.json")));
        assert_eq!(studio.panels.export.path, None);
    }

    #[test]
    fn seeking_while_playing_tells_the_sound() {
        let mut studio = Studio::headless();
        studio.sound = Some(SoundJob::recording());
        studio.seek(1.0);
        studio.set_playing(true);
        studio.seek(2.0);
        let sent = studio.sound.as_ref().unwrap().recorded();
        assert_eq!(sent, ["seek", "play", "seek"], "a seek while stopped waits for play");
        assert_eq!(studio.sound.as_ref().unwrap().clock(), 2.0, "the clock holds at the seek");
    }

    /// The studio's left panel beside an empty middle; each frame's change is recorded as the
    /// editor does. The state keeps the panel's width as drawn.
    fn left_harness() -> Harness<'static, (Studio, f32)> {
        let studio = Studio::headless();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1400.0, 900.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                |ui, (studio, width): &mut (Studio, f32)| {
                    let before = studio.scene.clone();
                    studio.recorded = false;
                    *width = studio.left_panel(ui, &before);
                    egui::CentralPanel::default().show(ui, |_| {});
                    if !std::mem::take(&mut studio.recorded) {
                        studio.after_edit(before);
                    }
                },
                (studio, 0.0),
            );
        h.run();
        h
    }

    #[test]
    fn switching_to_mixer_shows_tempo_and_master() {
        let mut h = left_harness();
        assert!(h.query_by_label("Tempo").is_none());
        h.get_by_label("Mixer").click();
        h.run();
        assert_eq!(h.state().0.left, LeftPanel::Mixer);
        for name in ["Tempo", "Beats in a bar", "Beat note", "Master", "Input", "Output", "Release", "Whole clip"] {
            assert!(h.query_by_label(name).is_some(), "{name} is shown");
        }
        assert!((h.state().1 - style::TREE_W).abs() < 1.0, "the Mixer opens as wide as the tree: {}", h.state().1);
    }

    #[test]
    fn turning_the_ceiling_knob_edits_the_mixer_card_and_undoes() {
        let mut h = left_harness();
        h.get_by_label("Mixer").click();
        h.run();
        let dial = h
            .query_all_by_label("Ceiling")
            .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
            .unwrap()
            .rect()
            .center();
        h.hover_at(dial);
        h.run();
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        h.event(press(dial, true));
        h.step();
        for k in 1..=4 {
            h.event(egui::Event::PointerMoved(dial + egui::vec2(0.0, 50.0 * k as f32 / 4.0)));
            h.step();
        }
        h.event(press(dial + egui::vec2(0.0, 50.0), false));
        h.run();
        let ceiling =
            |s: &Studio| s.scene.card(ThingId::ROOT, SlotId::Mixer).unwrap().params.get("master_ceiling").cloned();
        // 50 px down of a 200 px turn over 12 dB.
        assert_eq!(ceiling(&h.state().0), Some(Value::Number(-4.0)));
        assert!(h.state_mut().0.undo());
        assert_eq!(ceiling(&h.state().0), None, "back to the -1 dBTP default");
    }

    #[test]
    fn master_output_takes_keyframes_but_ceiling_doesnt() {
        let mut h = left_harness();
        h.get_by_label("Mixer").click();
        h.run();
        let knob = |h: &Harness<'_, (Studio, f32)>, label: &str| {
            h.query_all_by_label(label)
                .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
                .unwrap()
                .click_secondary()
        };
        knob(&h, "Output");
        h.run();
        assert!(h.query_by_label("Copy Value").is_some(), "the rest of the value menu is there");
        h.get_by_label("Create Keyframe").click();
        h.run();
        let a = Address::on(ThingId::ROOT, SlotId::Mixer, "master_output");
        assert!(h.state().0.scene.keyed(&a), "Output is keyed");
        h.key_press(egui::Key::Escape);
        h.run();
        knob(&h, "Ceiling");
        h.run();
        assert!(h.get_by_label("Create Keyframe").accesskit_node().is_disabled(), "Ceiling takes no keys");
    }

    /// Drags the left panel's edge from where it is by `dx`.
    fn drag_left_edge(h: &mut Harness<'_, (Studio, f32)>, dx: f32) {
        // Just outside the panel: inside it, its scroll bar is on top of the edge.
        let edge = egui::pos2(h.state().1 + 2.0, 450.0);
        crate::mixer::strips::tests::drag(h, edge, edge + egui::vec2(dx, 0.0));
    }

    #[test]
    fn the_mixer_opens_at_the_trees_width() {
        let mut h = left_harness();
        drag_left_edge(&mut h, -200.0);
        let tree = h.state().1;
        // (The drag starts past egui's drag threshold, a few pixels short.)
        assert!((tree - (style::TREE_W - 200.0)).abs() < 10.0, "the tree was dragged narrower: {tree}");
        h.get_by_label("Mixer").click();
        h.run();
        assert!((h.state().1 - tree).abs() < 0.5, "the Mixer opens at the tree's {tree}: {}", h.state().1);
        // Dragged wider while open, it stays so until it is switched away.
        drag_left_edge(&mut h, 300.0);
        let wide = h.state().1;
        assert!(wide > tree + 250.0, "the Mixer was dragged wider: {wide}");
        h.get_by_label("Things").click();
        h.run();
        assert_eq!(h.state().0.left, LeftPanel::Things);
        assert!((h.state().1 - tree).abs() < 0.5, "{} back to {tree}", h.state().1);
        // Opened again, it is the tree's width once more.
        h.get_by_label("Mixer").click();
        h.run();
        assert!((h.state().1 - tree).abs() < 0.5, "{} opens at {tree} again", h.state().1);
    }

    #[test]
    fn a_narrow_mixer_still_draws_its_strips() {
        let mut h = left_harness();
        drag_left_edge(&mut h, -400.0);
        assert!((h.state().1 - 180.0).abs() < 1.0, "the tree at its narrowest: {}", h.state().1);
        h.get_by_label("Mixer").click();
        h.run();
        assert!((h.state().1 - 180.0).abs() < 1.0, "the Mixer as narrow: {}", h.state().1);
        for name in ["Bus 1 fader", "Bus 3 fader", "Ceiling", "Pan"] {
            assert!(h.query_all_by_label(name).next().is_some(), "{name} is drawn");
        }
        // The strips keep their order, side by side at their narrowest (48 px and a 6 px gap),
        // scrolling sideways past the edge.
        let x = |h: &Harness<'_, (Studio, f32)>, name: &str| h.query_all_by_label(name).next().unwrap().rect().left();
        let step = x(&h, "Bus 2 fader") - x(&h, "Bus 1 fader");
        assert!((step - 54.0).abs() < 1.0, "{step}");
        assert!(x(&h, "Bus 3 fader") - x(&h, "Bus 2 fader") == step);
        // A little wider, all four fit.
        h.get_by_label("Things").click();
        h.run();
        drag_left_edge(&mut h, 120.0);
        h.get_by_label("Mixer").click();
        h.run();
        let width = h.state().1;
        assert!((280.0..320.0).contains(&width), "{width}");
        for name in ["Ceiling", "Release"] {
            let knob = h.query_all_by_label(name).find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider);
            let r = knob.unwrap().rect();
            assert!(r.right() <= width, "{name} at {r:?} is inside the {width} px Mixer");
        }
    }

    /// The studio's inspector beside an empty middle, in a wide window. The state keeps the
    /// inspector's width as drawn.
    fn inspector_harness(selected: ThingId) -> Harness<'static, (Studio, f32)> {
        let mut studio = Studio::headless();
        studio.selection.set(selected);
        let mut h = Harness::builder()
            .with_size(egui::vec2(1400.0, 900.0))
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                |ui, (studio, width): &mut (Studio, f32)| {
                    *width = studio.inspector_panel(ui);
                    egui::CentralPanel::default().show(ui, |_| {});
                },
                (studio, 0.0),
            );
        h.run();
        h
    }

    #[test]
    fn the_inspector_width_holds_when_selecting_scene_settings() {
        let mut h = inspector_harness(ThingId(2));
        let ball = h.state().1;
        h.state_mut().0.selection.set(ThingId::ROOT);
        h.run();
        let scene = h.state().1;
        assert!((ball - scene).abs() <= 1.0, "a ball's inspector is {ball} wide, Scene settings' {scene}");
    }

    #[test]
    fn a_squeezed_window_does_not_change_the_saved_width() {
        let mut h = inspector_harness(ThingId(2));
        h.set_size(egui::vec2(380.0, 900.0));
        h.run();
        assert!(h.state().1 < 370.0, "the window squeezes the panel to {}", h.state().1);
        assert_eq!(h.state().0.inspector_width, 370.0);
    }

    #[test]
    fn dragging_the_inspector_edge_saves_its_width() {
        let mut h = inspector_harness(ThingId(2));
        let edge = egui::pos2(1400.0 - 370.0, 450.0);
        let to = egui::pos2(1400.0 - 500.0, 450.0);
        h.hover_at(edge);
        h.step();
        h.event(egui::Event::PointerButton {
            pos: edge,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
        h.event(egui::Event::PointerMoved(to));
        h.step();
        assert_eq!(h.state().0.inspector_width, 370.0, "nothing is saved while the button is down");
        h.event(egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
        let (saved, shown) = (h.state().0.inspector_width, h.state().1);
        // The drag starts a few pixels in, past egui's drag threshold, so it lands near 500.
        assert!(saved > 480.0 && saved == shown, "the dragged width {shown} is saved, got {saved}");
    }

    /// Where the toolbar's save status ends and Export starts, for a scene saved as `name` in
    /// a window `width` wide.
    fn status_and_export(name: &str, width: f32) -> (egui::Rect, egui::Rect) {
        let mut studio = Studio::headless();
        studio.path = Some(PathBuf::from(format!("/scenes/{name}")));
        let mut h = Harness::builder()
            .with_size(egui::vec2(width, 80.0))
            .with_max_steps(20)
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(|ui, studio: &mut Studio| studio.toolbar(ui), studio);
        h.run();
        let saved = |n: &egui_kittest::Node<'_>| {
            n.accesskit_node().role() == egui::accesskit::Role::Label
                && n.accesskit_node().value().is_some_and(|v| v.starts_with("Saved as"))
        };
        let status = h.root().children_recursive().find(saved).expect("the save status").rect();
        let export = h.get_by_label("Export video…").rect();
        (status, export)
    }

    #[test]
    fn a_long_save_status_does_not_reach_export() {
        let long = format!("{}.ron", &"a long scene name ".repeat(7)[..116]);
        assert_eq!(long.len(), 120);
        // The app's narrowest window, the screenshots' width, and a wide one.
        for width in [1000.0, 1440.0, 1600.0] {
            for name in ["blend-modes.ron", long.as_str()] {
                let (status, export) = status_and_export(name, width);
                // On Export's line it ends before Export; at a narrow width it may go to the next.
                let beside = status.top() < export.bottom();
                assert!(
                    !beside || status.right() < export.left(),
                    "at {width}, {name}: the status {status:?} runs into Export {export:?}"
                );
                if width >= 1440.0 {
                    assert!(beside, "at {width} the toolbar is one line: {status:?}, Export {export:?}");
                }
            }
        }
    }

    #[test]
    fn the_narrowest_inspector_keeps_its_value_boxes_inside() {
        let mut studio = Studio::headless();
        studio.selection.set(ThingId(2));
        studio.inspector_width = *settings::INSPECTOR_RANGE.start();
        let mut h = Harness::builder()
            .with_size(egui::vec2(1000.0, 2400.0))
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                |ui, (studio, width): &mut (Studio, f32)| {
                    *width = studio.inspector_panel(ui);
                    egui::CentralPanel::default().show(ui, |_| {});
                },
                (studio, 0.0),
            );
        h.run();
        let panel = h.state().1;
        assert!((panel - 320.0).abs() <= 1.0, "the panel is at its minimum: {panel}");
        let glow = h.get_all_by_role_and_label(egui::accesskit::Role::Slider, "Size").next().unwrap().rect();
        // The number box sits right of the slider, level with it.
        let value = h
            .root()
            .children_recursive()
            .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::SpinButton)
            .map(|n| n.rect())
            .find(|r| r.left() > glow.right() - 1.0 && (r.center().y - glow.center().y).abs() < 4.0)
            .expect("Size's number box");
        assert!(value.right() <= 1000.0 - 10.0, "Size's number box ends at {}, past the panel", value.right());
        // An x and y pair that can't fit side by side goes onto two lines, inside its row.
        let speed: Vec<egui::Rect> =
            h.get_all_by_role_and_label(egui::accesskit::Role::SpinButton, "Launch speed").map(|n| n.rect()).collect();
        assert_eq!(speed.len(), 2);
        let labels: Vec<egui::Rect> = h
            .root()
            .children_recursive()
            .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Label)
            .map(|n| n.rect())
            .collect();
        for r in &speed {
            assert!(r.right() <= 1000.0 - 10.0, "Launch speed {r:?} runs past the panel");
            assert!(!labels.iter().any(|l| l.intersects(r.shrink(1.0))), "Launch speed {r:?} runs into another row");
        }
        // Every slider row's number box fits, and so do the colours' buttons.
        for button in ["Palette", "Smooth"] {
            for node in h.query_all_by_label(button) {
                assert!(node.rect().right() <= 1000.0 - 10.0, "{button} ends at {}", node.rect().right());
            }
        }
    }

    #[test]
    fn opening_an_old_scene_reports_it_and_keeps_the_current_one() {
        let mut studio = Studio::headless();
        let old = concat!(env!("CARGO_MANIFEST_DIR"), "/../bpe-core/tests/data/old-editor-scene.ron");
        studio.open_path(Path::new(old));
        assert_eq!(studio.status, "This scene was made with the old Ball Studio");
        assert_eq!(studio.scene.things.len(), 3);
    }

    #[test]
    fn a_look_edit_keeps_the_playback_and_a_gravity_edit_restarts_it() {
        let mut studio = Studio::headless();
        studio.seek(2.0);
        let before = studio.scene.clone();
        // The test ball's Glow layer's Strength.
        let look = Address::on(ThingId(2), SlotId::Look, "2/strength");
        studio.scene.set_at(&studio.kit.registry, &look, Value::Number(1.0)).unwrap();
        studio.after_edit(before);
        assert!(studio.playback.time() >= 2.0);
        // Quick edits merge into one undo step; the frame loop ends the step once the pointer
        // is up and the edits have stopped. The next edit is a step of its own.
        studio.history.end_merging();
        let before = studio.scene.clone();
        let gravity = Address { thing: ThingId::ROOT, slot: None, key: "gravity".into() };
        studio.scene.set_at(&studio.kit.registry, &gravity, Value::Vec2([0.0, 10.0])).unwrap();
        studio.after_edit(before);
        assert_eq!(studio.playback.time(), 0.0);
        assert_eq!(studio.history.undo.len(), 2);
    }

    #[test]
    fn a_brush_stroke_or_handle_drag_is_its_own_undo_step_however_quick() {
        let mut studio = Studio::headless();
        let look = Address::on(ThingId(2), SlotId::Look, "2/strength");
        let edit = |studio: &mut Studio, v: f64, own: bool| {
            let before = studio.scene.clone();
            studio.scene.set_at(&studio.kit.registry, &look, Value::Number(v)).unwrap();
            studio.panels.inspector.own_step = own;
            studio.after_edit(before);
        };
        edit(&mut studio, 0.5, false);
        edit(&mut studio, 0.6, false);
        assert_eq!(studio.history.undo.len(), 1, "quick edits merge");
        edit(&mut studio, 0.7, true);
        assert_eq!(studio.history.undo.len(), 2, "a stroke starts its own step");
        edit(&mut studio, 0.8, false);
        assert_eq!(studio.history.undo.len(), 2, "and the drag after it joins it");
    }

    #[test]
    fn the_editor_starts_on_the_blank_project_and_new_makes_another() {
        let studio = Studio::blank();
        // A new thing's colour is picked at random, so compare the two with colours left out.
        let uncoloured = |mut scene: Scene| {
            scene.things.values_mut().for_each(|t| t.colour = None);
            scene
        };
        let blank = bpe_kit::new_studio_scene(&studio.kit.registry);
        assert_eq!(uncoloured(studio.scene.clone()), uncoloured(blank));
        assert!(!studio.unsaved());
        let mut studio = Studio::headless();
        studio.new_scene();
        let kinds: Vec<&str> = studio.scene.order.iter().map(|id| studio.scene.things[id].kind.as_str()).collect();
        assert_eq!(kinds, ["scene", "camera"], "New is blank too, with just the camera");
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
    fn scrubbing_a_huge_scene_returns_within_its_budget() {
        let mut studio = Studio::headless();
        let before = studio.scene.clone();
        // As many balls as a group may have.
        let mut block = BallsSettings { radius: 1.5, ..BallsSettings::default() };
        block.block(70, 70, 4.0);
        studio.scene.things.get_mut(&ThingId(2)).unwrap().params = block.to_params();
        studio.after_edit(before);
        let started = Instant::now();
        studio.seek(20.0);
        assert!(started.elapsed() < Duration::from_millis(500), "a scrub blocked for {:?}", started.elapsed());
        assert_eq!(studio.time, 20.0);
        assert!(studio.playback.time() < 20.0, "the preview catches the rest up a budget per frame");
    }
}
