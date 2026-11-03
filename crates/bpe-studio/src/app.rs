//! The editor's state and frame: files, undo, shortcuts and the panel layout.

use crate::export_window::ExportWindow;
use crate::grid::{self, Grid};
use crate::history::{History, UNDO_MERGE};
use crate::inspector::Inspector;
use crate::preview::Preview;
use crate::selection::Selection;
use crate::timeline::Timeline;
use crate::tree::Tree;
use crate::{inspector, preview, settings, style, timeline, tree};
use bpe_core::{Resolved, Scene, Settings, ThingId};
use bpe_kit::{Kit, SceneSettings, resolve_scene};
use bpe_runtime::{Playback, fingerprint};
use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText, Stroke};
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
    /// The inspector's width last frame.
    inspector_seen: f32,
    /// The inspector's edge is being dragged: its width changed while a button was down.
    inspector_resizing: bool,
    /// Scrubbing paused the playback; it starts again when the scrub ends.
    pub resume_after_scrub: bool,
    /// This frame's change to the scene is already in the history (undo, open, delete…).
    pub recorded: bool,
    closing: bool,
    last_tick: Option<Instant>,
    clicked: Option<ToolbarClick>,
}

impl Studio {
    pub fn new(cc: &eframe::CreationContext, scene_path: Option<PathBuf>) -> Self {
        style::apply(&cc.egui_ctx);
        crate::palettes::install(&cc.egui_ctx, crate::palettes::PaletteStore::open());
        let mut studio = Self::blank();
        studio.grid = Grid::load();
        studio.inspector_width = settings::load_inspector_width();
        studio.persist_settings = true;
        if let Some(path) = scene_path {
            studio.open_path(&path);
        }
        studio
    }

    /// The state without a window, open on the blank project the editor always starts with.
    pub fn blank() -> Self {
        let kit = Arc::new(bpe_kit::kit());
        let scene = bpe_kit::new_studio_scene(&kit.registry);
        Self::with_scene(kit, scene)
    }

    /// The state without a window for tests: the test scene with an arena and a ball.
    #[cfg(test)]
    pub fn headless() -> Self {
        let kit = Arc::new(bpe_kit::kit());
        let scene = bpe_kit::new_scene(&kit.registry);
        Self::with_scene(kit, scene)
    }

    fn with_scene(kit: Arc<Kit>, scene: Scene) -> Self {
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
            inspector_seen: settings::INSPECTOR_DEFAULT,
            inspector_resizing: false,
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
        if fingerprint != self.fingerprint {
            self.fingerprint = fingerprint;
            self.playback = Playback::new(&self.scene, &self.resolved, self.kit.sims.clone());
        }
        self.time = self.time.min(self.duration());
        self.revision += 1;
    }

    pub fn replace_scene(&mut self, mut scene: Scene, path: Option<PathBuf>) {
        // Every scene the editor holds has a camera; an older file gets one at the identity, so
        // it looks as it always did.
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
        self.fingerprint = fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        self.playback = Playback::new(&self.scene, &self.resolved, self.kit.sims.clone());
        self.revision += 1;
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

    // ---------- playback ----------

    /// Moves the playhead. The simulation follows within the budget; the preview catches up
    /// the rest a budget per frame.
    pub fn seek(&mut self, time: f64) {
        self.time = time.clamp(0.0, self.duration());
        self.playback.seek_within(self.sim_time(), Some(CATCH_UP_BUDGET));
    }

    pub fn set_playing(&mut self, playing: bool) {
        if playing && self.time >= self.duration() {
            self.time = 0.0;
        }
        self.playing = playing;
        self.last_tick = None;
    }

    /// Moves the playhead while playing, with the wall clock.
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
    fn inspector_panel(&mut self, ui: &mut egui::Ui) -> f32 {
        let panel = egui::Panel::right("inspector")
            .default_size(self.inspector_width)
            .min_size(*settings::INSPECTOR_RANGE.start())
            .max_size(*settings::INSPECTOR_RANGE.end())
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
        let width = panel.response.rect.width();
        // Only a drag of the edge is remembered, once the mouse lets go; a window squeezing the
        // panel is not.
        let down = ui.input(|i| i.pointer.any_down());
        if down && (width - self.inspector_seen).abs() >= 0.5 {
            self.inspector_resizing = true;
        }
        self.inspector_seen = width;
        if !down && std::mem::take(&mut self.inspector_resizing) && (width - self.inspector_width).abs() >= 0.5 {
            self.inspector_width = width;
            if self.persist_settings {
                settings::save_inspector_width(width);
            }
        }
        width
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
            .on_hover_text("Drags land on the grid: moving, resizing the arena and aiming. Hold Alt to place freely.")
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
        let tree = egui::Panel::left("tree")
            .default_size(style::TREE_W)
            .min_size(180.0)
            .max_size(480.0)
            .resizable(true)
            .frame(side_frame().inner_margin(egui::Margin::symmetric(8, 10)))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                tree::show(ui, &mut self.panels.tree, &mut self.scene, &self.kit, &self.resolved, &mut self.selection)
            })
            .inner;
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
        assert_eq!(studio.scene, bpe_kit::new_studio_scene(&studio.kit.registry));
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
