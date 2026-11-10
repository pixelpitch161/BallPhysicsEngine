//! The editor's state and frame: the [`Studio`] and what it holds, built here, and its frame
//! (`eframe::App::ui`). Each job has its own file:
//!
//! - `runs.rs`: edits, undo steps and the runs (physics, aim, particles, sound) they restart.
//! - `playing.rs`: the playhead, the sound and the speakers.
//! - `files.rs`: new, open and save; `actions.rs`: undo, redo, copy, paste, duplicate, delete.
//! - `autosave.rs` and `problems.rs`: never losing work, and problems in the status line.
//! - `shortcuts.rs`, `panels.rs` (toolbar, side panels, status line) and `dialogs.rs`.

use crate::aim_job::AimJob;
use crate::audio_out::AudioOut;
use crate::export_window::ExportWindow;
use crate::grid::{self, Grid};
use crate::history::{History, UNDO_MERGE};
use crate::inspector::Inspector;
use crate::mixer::{self, Mixer};
use crate::particle_job::ParticleJob;
use crate::preview::Preview;
use crate::recovery::{self, CrashCopy, CrashSlot, Orphan};
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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod actions;
mod autosave;
mod dialogs;
mod files;
mod panels;
mod playing;
mod problems;
mod runs;
mod shortcuts;
#[cfg(test)]
mod tests;

use files::file_label;
pub(crate) use panels::side_frame;
use panels::*;
use playing::*;
use runs::*;

/// Most time spent catching the simulation up in one go. A long stretch, such as a scrub far
/// ahead on a huge scene, is worked out over several frames by the preview instead of
/// freezing the editor.
pub(crate) const CATCH_UP_BUDGET: Duration = Duration::from_millis(12);
/// How long a held drag rests before the runs it changed start over (the physics, the
/// particles, the aiming and the sound): a slider drag rebuilds them when let go or paused on.
pub(crate) const RUNS_REST: Duration = Duration::from_millis(100);

/// Where a problem in the status line came from, so each source clears only its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemKind {
    /// The speakers or the sound thread.
    Speakers,
    /// The graphics card.
    Graphics,
    /// Opening or saving a scene.
    File,
    /// Making a video.
    Export,
    /// Restoring autosaved work.
    Recovery,
    /// The autosave itself.
    Autosave,
}

/// A failure shown in the status line in the problem colour until it is dismissed (or its source
/// clears it), so a later "Undone." can't hide it.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub kind: ProblemKind,
    pub text: String,
}

/// What the graphics card has complained about, gathered from any thread.
#[derive(Default)]
pub struct GpuErrors {
    /// The first complaint, in full.
    pub first: Option<String>,
    /// How many complaints so far.
    pub count: u32,
    /// The card stopped responding.
    pub lost: bool,
}

/// Autosaved work from an editor that closed unexpectedly, offered back at the start.
pub struct Offer {
    pub orphan: Orphan,
    pub scene: Scene,
    /// The scene's name and how long ago it was saved, for the question.
    pub name: String,
    pub when: String,
    /// The scene's own file was saved after this copy was made: restoring would bring back
    /// older work.
    pub file_newer: bool,
}

/// Something that would throw away unsaved changes.
pub enum Pending {
    Close,
    Open,
    New,
    /// A recent file from the toolbar's Recent menu, or a scene dropped on the window.
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
    /// "Preview final frame" (lighting part 3).
    pub final_frame: crate::final_frame::FinalFrame,
    pub timeline: Timeline,
    pub mixer: Mixer,
    /// The Rules view in the bottom panel.
    pub rules: crate::rules_panel::RulesPanel,
}

/// What the left panel shows: the things in the scene, every light, or the Mixer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeftPanel {
    #[default]
    Things,
    Lights,
    Mixer,
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

/// How far (seconds) the particle playback may be behind the playhead before the preview says
/// it is waiting for it.
pub const PARTICLES_BEHIND: f64 = 0.25;

pub struct Studio {
    pub kit: Arc<Kit>,
    pub scene: Scene,
    pub resolved: Resolved,
    /// The scene as it was last saved or opened.
    pub saved: Scene,
    pub path: Option<PathBuf>,
    pub playback: Playback,
    /// Aims rounds through bounces ahead of `playback` on its own thread, while some emitter
    /// does; the playback never steps past where it has got.
    pub aim_job: Option<AimJob>,
    /// Runs the whole scene, particles and all, on its own thread while it has Particles things:
    /// `playback` runs the physics alone, and the preview draws this one's particles.
    pub particle_job: Option<ParticleJob>,
    /// What `particle_job` was built from ([`bpe_runtime::beside_fingerprint`]).
    pub particle_print: u64,
    /// The physics fingerprint `playback` was built from.
    pub fingerprint: u64,
    /// The part of it the sound hears ([`sound_fingerprint`]): the sound starts its run over
    /// only when this changes.
    pub sound_fingerprint: u64,
    /// What the sound's settings were built from ([`bpe_sound::patch_fingerprint`]): the patch is
    /// built again and sent only when this changes (not on a colour drag).
    pub patch_print: u64,
    /// The scene's sound as the picture reads it, while some setting follows sound.
    pub follow: Option<std::sync::Arc<bpe_sound::ControlSampler>>,
    /// The scene's light as the picture reads it, while something senses light (Follow light, a
    /// Light or Shadow spread, Moths, a Light modulator). Never read by the lighting itself.
    pub light_probe: Option<std::sync::Arc<dyn bpe_runtime::LightAt>>,
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
    /// The info line: what just happened ("Saved x.", "Undone.").
    pub status: String,
    /// A failure, shown before the info line until it is dismissed.
    pub problem: Option<Problem>,
    /// What is wrong with a shader module the preview skipped, shown in the status line.
    pub shader_problem: Option<String>,
    /// The preview's lighting, when it is lit: how many lights and the GPU's time on them (ms).
    pub lighting: Option<(usize, Option<f32>)>,
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
    /// The moved Stage camera the preview's picture was last seen through (lighting part 5), so
    /// the pointer and handles map through it; `None` at rest and in the World view.
    pub stage_view: Option<bpe_gfx::StageView>,
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
    /// The words of a Text being typed straight into the preview.
    pub typing: Option<crate::preview::text_typing::Typing>,
    /// This frame's change to the scene is already in the history (undo, open, delete…).
    pub recorded: bool,
    /// This frame's change to the scene is an undo step of its own (a rule or block added,
    /// removed or linked).
    pub separate_edit: bool,
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
    /// The speakers' dropouts already told of in the status line.
    underruns_seen: u64,
    /// The listening volume changed and is saved once it has rested (not on every frame of a
    /// drag).
    listen_unsaved: Option<Instant>,
    /// The speaker button and the listening volume (only what you hear).
    pub listen: ListenSettings,
    /// The left panel's face (not saved in the scene).
    pub left: LeftPanel,
    /// Build or Grade (the editor's own, not saved): Grade mode's columns take the side panels'
    /// place.
    pub mode: crate::grade_mode::EditorMode,
    /// Grade mode's own state: the compare view, folded panels, the Histogram.
    pub grade: crate::grade_mode::GradeMode,
    /// What the bottom panel shows: the timeline or the rules (the editor's own setting).
    pub bottom: settings::BottomView,
    /// The Lights tab's soloed lights: the editor's alone, never saved or exported.
    pub light_solo: std::collections::BTreeSet<crate::lights_panel::LightSource>,
    /// What a lit preview shows: the picture, or the light alone (the editor's alone).
    pub light_show: bpe_gfx::LightShow,
    /// Show air: faint arrows of the air over the preview (the editor's alone, never in the video).
    pub show_air: bool,
    /// Wavetables baked in the background when the sound was last built
    /// ([`WaveBank::baked`]); more means it is built again.
    tables_baked: u64,
    /// The scene as last recorded: each frame's edit is found by comparing with it, so the
    /// scene isn't copied every frame. `None` after an edit made outside the frame's own
    /// recording (an undo, an opened file…): the next frame copies the scene afresh.
    snapshot: Option<Scene>,
    /// Whether the scene differs from the saved one, as worked out at a revision.
    unsaved_at: std::cell::Cell<Option<(u64, bool)>>,
    /// A pointer button is held on something other than the preview's handles (a slider, a
    /// knob, a key on the timeline): edits that change the physics, the particles or the sound
    /// keep the runs they have until it is let go or rests ([`RUNS_REST`]).
    pub hold_runs: bool,
    /// When a held edit last changed what the runs were built from, while they wait to start over.
    pub runs_due: Option<Instant>,
    /// The things the runs were last built with: an edit that adds or removes one never waits,
    /// so the runs never hold a thing the scene no longer has.
    runs_things: Vec<ThingId>,
    /// This editor's autosave session (the windowed editor only).
    pub recovery: Option<recovery::Session>,
    /// The scene as last sent to the autosave, while it is unsaved.
    autosaved: Option<Scene>,
    /// When the autosave last looked; `None` looks at the next frame.
    autosaved_at: Option<Instant>,
    /// The scene was unsaved at the last look, so a recovery copy may exist.
    was_unsaved: bool,
    /// The unsaved scene a crash would save.
    crash_slot: CrashSlot,
    /// When the crash copy was last refreshed.
    crash_copied_at: Option<Instant>,
    /// Autosaved work to offer back.
    pub offer: Option<Offer>,
    /// The graphics card's complaints, filled by the device's error handler.
    pub gpu_errors: Arc<Mutex<GpuErrors>>,
    /// How many of them the status line has told of.
    gpu_told: u32,
    /// The card being lost is already in the status line.
    gpu_lost_told: bool,
    /// The editor is asking whether to stop a running export and close.
    pub ask_stop_export: bool,
    /// The window's context, to wake it for the next autosave.
    repaint: Option<egui::Context>,
    /// What was copied last, things or timeline keys (kept through New and Open).
    pub clipboard: crate::things::Clipboard,
    /// Words put on the system clipboard at the next frame's shortcuts, after a copy here: on
    /// Windows, Ctrl+V reaches the program only when the system clipboard holds something.
    pub copied_note: Option<String>,
    /// The window's title as last sent.
    title: String,
}

impl Studio {
    pub fn new(cc: &eframe::CreationContext, scene_path: Option<PathBuf>) -> Self {
        style::apply(&cc.egui_ctx);
        crate::palettes::install(&cc.egui_ctx, crate::palettes::PaletteStore::open());
        // Huge swarms (particles part 8) step on the window's own card, so the preview draws them
        // straight from their buffers.
        if let Some(rs) = &cc.wgpu_render_state {
            bpe_gfx::swarm::use_on_this_thread(Some(bpe_gfx::swarm::SwarmGpu::for_device(&rs.device, &rs.queue)));
        }
        let mut studio = Self::blank();
        if let Some(rs) = &cc.wgpu_render_state {
            studio.catch_gpu_errors(&rs.device, &cc.egui_ctx);
        }
        if let Some(dir) = settings::settings_dir() {
            studio.panels.export = crate::export_window::ExportWindow::load_in(&dir);
        }
        if let Some(dir) = recovery::recovery_dir() {
            studio.start_recovery(&dir, settings::settings_dir().map(|d| d.join("last-crash.txt")));
        }
        studio.grid = Grid::load();
        studio.inspector_width = settings::load_inspector_width();
        studio.sound_width = settings::load_sound_width();
        studio.persist_settings = true;
        studio.listen = settings::load_listen();
        studio.bottom = settings::load_bottom_view();
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
        let patch_print = bpe_sound::patch_fingerprint(&scene, &resolved);
        // Started first, so the playback's own build waits on its work instead of doing it again.
        let aim_job = aim_job_for(&scene, &resolved, &kit);
        let playback = main_playback(&scene, &resolved, &kit);
        let particle_print = bpe_runtime::beside_fingerprint(&scene, &resolved, &kit.sims);
        let particle_job = ParticleJob::start_if_wanted(&scene, &resolved, kit.sims.clone(), 0.0);
        let follow = follow_of(&scene, &resolved);
        let light_probe = light_probe_of(&scene, &resolved, &follow);
        let runs_things = scene.things.keys().copied().collect();
        Self {
            saved: scene.clone(),
            scene,
            resolved,
            path: None,
            playback,
            aim_job,
            particle_job,
            particle_print,
            fingerprint,
            sound_fingerprint,
            patch_print,
            follow,
            light_probe,
            revision: 0,
            kit,
            time: 0.0,
            playing: false,
            looping: true,
            selection: Selection::default(),
            history: History::default(),
            status: "Ready.".into(),
            shader_problem: None,
            lighting: None,
            confirm: None,
            panels: Panels::default(),
            grid: Grid::default(),
            preview_mode: Default::default(),
            world_nav: Default::default(),
            view_centre: glam::Vec2::ZERO,
            stage_view: None,
            persist_settings: false,
            inspector_width: settings::INSPECTOR_DEFAULT,
            sound_width: settings::SOUND_WIDTH_DEFAULT,
            inspector_seen: settings::INSPECTOR_DEFAULT,
            inspector_resizing: false,
            inspector_sound: false,
            tree_width: style::TREE_W,
            mixer_opens: 0,
            resume_after_scrub: false,
            typing: None,
            recorded: false,
            separate_edit: false,
            closing: false,
            last_tick: None,
            clicked: None,
            sound: None,
            speakers_retry: None,
            speakers_wait: SPEAKERS_RETRY,
            sound_broken: false,
            underruns_seen: 0,
            listen_unsaved: None,
            listen: ListenSettings::default(),
            left: LeftPanel::Things,
            mode: Default::default(),
            grade: Default::default(),
            bottom: settings::BottomView::Timeline,
            light_solo: Default::default(),
            light_show: Default::default(),
            show_air: false,
            tables_baked: WaveBank::baked() + bpe_sound::SampleBank::decoded(),
            snapshot: None,
            unsaved_at: std::cell::Cell::new(None),
            hold_runs: false,
            runs_due: None,
            runs_things,
            problem: None,
            recovery: None,
            autosaved: None,
            autosaved_at: None,
            was_unsaved: false,
            crash_slot: Arc::default(),
            crash_copied_at: None,
            offer: None,
            gpu_errors: Arc::default(),
            gpu_told: 0,
            gpu_lost_told: false,
            ask_stop_export: false,
            repaint: None,
            clipboard: Default::default(),
            copied_note: None,
            title: String::new(),
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

    /// How far the playback may step towards the playhead now: up to where the background
    /// aiming has got, while it runs.
    /// While the particle playback is behind, the playback waits where it is for it (or goes
    /// only as far as it has got), so the particles drawn are the balls' own moment.
    #[cfg(test)]
    pub fn sim_target(&self) -> f64 {
        self.sim_target_with(self.particle_job.as_ref().map(|job| job.reached()))
    }

    /// Where the playback may step to now given where the background aiming has got: the
    /// moment the particle playback is asked for.
    pub fn aim_target(&self) -> f64 {
        self.aim_job.as_ref().map_or(self.sim_time(), |job| job.allowed(self.sim_time()))
    }

    /// [`Studio::sim_target`] with the particle playback at `reached` (`None` without one,
    /// `Some(None)` before it has published anything): read once a frame, so the moment the
    /// playback steps to and the particles drawn there come from the same snapshot.
    pub fn sim_target_with(&self, reached: Option<Option<f64>>) -> f64 {
        let target = self.aim_target();
        match reached {
            Some(Some(reached)) if reached < target => {
                let now = self.playback.time();
                if now <= reached { reached } else { now.min(target) }
            }
            Some(None) => self.playback.time().min(target),
            _ => target,
        }
    }

    /// Whether the particle playback (if any) has published the step the playhead is on.
    pub fn particles_there(&self) -> bool {
        let Some(job) = &self.particle_job else { return true };
        let sps = self.playback.world().steps_per_second;
        let want = bpe_runtime::step_at(self.aim_target(), sps);
        job.latest().is_some_and(|s| s.step == want)
    }

    /// How far (0..1) the particle playback has got towards the playhead, while it is well
    /// behind ([`PARTICLES_BEHIND`]): it follows the playhead a frame late while playing, which
    /// is no wait.
    pub fn particles_waiting(&self) -> Option<f32> {
        let job = self.particle_job.as_ref()?;
        let time = self.aim_target();
        let reached = job.reached().unwrap_or(0.0);
        (reached + PARTICLES_BEHIND < time).then(|| (reached / time.max(1e-9)).clamp(0.0, 1.0) as f32)
    }

    /// How far (0..1) the background aiming has got towards the playhead, while it holds the
    /// picture back.
    pub fn aiming(&self) -> Option<f32> {
        self.aim_job.as_ref().and_then(|job| job.holding(self.sim_time()))
    }

    /// Whether the scene differs from the saved one. Worked out once a revision, not on every
    /// frame the toolbar is drawn.
    pub fn unsaved(&self) -> bool {
        if let Some((revision, unsaved)) = self.unsaved_at.get()
            && revision == self.revision
        {
            return unsaved;
        }
        let unsaved = self.scene != self.saved;
        self.unsaved_at.set(Some((self.revision, unsaved)));
        unsaved
    }
}

impl eframe::App for Studio {
    fn on_exit(&mut self) {
        // A running export would otherwise leave a half-written video behind.
        self.panels.export.shutdown();
        self.save_listen_if_rested(None);
        self.sound = None;
        // The autosaved copy goes once the work is saved or was thrown away on purpose ("Don't
        // save"); otherwise it stays, to be offered back at the next start. A panic on the way out
        // must not write the copy back either.
        *self.crash_slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if let Some(session) = &self.recovery
            && (self.closing || !self.unsaved())
        {
            session.clear();
        }
        self.recovery = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing {
            if self.panels.export.busy() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.ask_stop_export = true;
            } else if self.unsaved() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.confirm = Some(Pending::Close);
            }
        }
        if self.repaint.is_none() {
            self.repaint = Some(ctx.clone());
        }
        self.tick();
        // What the inspector's distance and angle settings snap to while dragged this frame.
        crate::setting_snap::put(&ctx, self.grid.setting_snap());
        if self.panels.inspector.render.is_none() {
            // The Look editor's ball view draws with the window's GPU.
            self.panels.inspector.render = frame.wgpu_render_state().cloned();
        }
        // The scene as the frame starts: kept from the last frame's recording, copied only after
        // an edit.
        let before = self.snapshot.take().unwrap_or_else(|| self.scene.clone());
        let revision = self.revision;
        self.recorded = false;
        let (input, pointer_down) = ctx.input(|i| (!i.events.is_empty() || i.pointer.any_down(), i.pointer.any_down()));
        // A held slider changes the physics many times a second: the runs start over when it
        // is let go or rests. A drag in the preview moves the things it shows, so it doesn't wait.
        self.hold_runs = pointer_down && !self.panels.preview.as_ref().is_some_and(Preview::dragging);
        if self.confirm.is_none() && self.offer.is_none() && !self.ask_stop_export {
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
        if self.mode == crate::grade_mode::EditorMode::Grade {
            crate::grade_mode::left(ui, self);
            crate::grade_mode::right(ui, self);
            crate::grade_mode::scopes_strip(ui, self);
        } else {
            self.left_panel(ui, &before);
            self.inspector_panel(ui);
            if std::mem::take(&mut self.panels.inspector.open_grade) {
                self.mode = crate::grade_mode::EditorMode::Grade;
            }
        }
        if std::mem::take(&mut self.panels.inspector.keyed_edit) {
            // A key landed at the playhead; a drag while playing would otherwise key every frame.
            self.set_playing(false);
        }
        let outcome = egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(style::STAGE))
            .show(ui, |ui| preview::show(ui, frame, self))
            .inner;
        if self.confirm.is_none() && self.offer.is_none() && !self.ask_stop_export {
            self.late_shortcuts(&ctx);
            self.take_drops(&ctx);
        }
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
        self.lighting = outcome.lighting;
        if outcome.catching_up {
            ctx.request_repaint();
        }
        let base_dir = self.base_dir();
        let settings = self.settings();
        self.panels.export.show(&ctx, &self.scene, &settings, base_dir.as_deref(), &self.kit);
        let frames = bpe_cli::frame_count(&settings);
        let mut ask = std::mem::take(&mut self.panels.export.test_frame);
        if std::mem::take(&mut self.panels.inspector.final_frame_wanted) {
            ask = Some((self.panels.export.scale, None));
        }
        if let Some((scale, bake)) = ask {
            let ask = crate::final_frame::Ask { time: self.time, scale, bake };
            self.panels.final_frame.start(&ctx, &self.scene, &self.kit, frames, ask);
        }
        self.panels.final_frame.show(&ctx);
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
        self.stop_export_dialog(&ctx);
        self.restore_dialog(&ctx);

        if quiet_frame(self.playing, input, self.recorded || self.separate_edit, self.revision != revision) {
            // Playing with nothing pressed or typed: nothing can have edited the scene.
            self.snapshot = Some(before);
            self.after_frame();
        } else {
            self.record_frame(before);
        }
        if let Some(wait) = self.settle_runs(pointer_down) {
            ctx.request_repaint_after(wait);
        }
        let title = self.window_title();
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
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

/// Whether a frame can skip looking for an edit: playing, with no input this frame, nothing
/// flagged as edited and the scene's revision unchanged.
pub(crate) fn quiet_frame(playing: bool, input: bool, flagged: bool, revised: bool) -> bool {
    playing && !input && !flagged && !revised
}

/// Seconds as m:ss.ff.
pub(crate) fn clock(seconds: f64) -> String {
    let hundredths = (seconds.max(0.0) * 100.0).floor() as u64;
    format!("{}:{:02}.{:02}", hundredths / 6000, hundredths / 100 % 60, hundredths % 100)
}

/// Past this many lights in a frame the editor warns that they can slow the picture (review S19):
/// each costs every pixel it reaches a shadow march.
pub const MANY_LIGHTS: usize = 64;

/// The status line's lighting readout: "· Lighting: 3 lights, 1.8 ms", with a warning past
/// [`MANY_LIGHTS`].
pub fn lighting_readout(lights: usize, ms: Option<f32>) -> String {
    let count = if lights == 1 { "1 light".to_owned() } else { format!("{lights} lights") };
    let time = ms.map(|ms| format!(", {ms:.1} ms")).unwrap_or_default();
    let warn = if lights > MANY_LIGHTS { " (that many lights is slow: see the Lights tab)" } else { "" };
    format!("· Lighting: {count}{time}{warn}")
}

/// The status line's particles readout: "· Particles: 12,400, 1.8 ms" (how long the last
/// particle step took).
pub fn particles_readout(count: usize, ms: f32) -> String {
    let digits = count.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("· Particles: {grouped}, {ms:.1} ms")
}
