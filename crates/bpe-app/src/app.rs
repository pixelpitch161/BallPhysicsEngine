use crate::audio_out::AudioOut;
use crate::batch::BatchPanel;
use crate::command_palette::{Action, Command, CommandPalette};
use crate::explorer::Explorer;
use crate::export::ExportPanel;
use crate::gallery::Gallery;
use crate::inspector::{self, Inspector, Tab};
use crate::keys;
use crate::preview::{Preview, PreviewQuality};
use crate::timeline::{HitSummary, Timeline};
use crate::workspace::Workspace;
use crate::{node_editor, rules_workspace, shader_workspace};
use crate::{settings, theme};
use bpe_scene::{Property, Scene, Value};
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
    /// Music workstream: beat sync's Warp clock from the latest soundtrack. The preview shows
    /// simulation time `warp.sim_time(t)` at video time `t`.
    warp: Option<bpe_sim::beat_sync::TimeWarp>,
    export: ExportPanel,
    gallery: Gallery,
    explorer: Explorer,
    batch: BatchPanel,
    /// The Batch workspace was showing last frame.
    in_batch: bool,
    timeline: Timeline,
    /// Playing was paused to scrub the timeline, and starts again when scrubbing stops.
    resume_after_scrub: bool,
    /// A toolbar button that was clicked (or Delete pressed on a keyframe). It is acted on once
    /// the panels are drawn, because while settings are animated the panels edit a copy of the
    /// scene, and so that it counts as an edit that can be undone.
    clicked: Option<ToolbarClick>,
    /// Whether the scene had unsaved changes at the start of this frame.
    was_unsaved: bool,
    /// An action waiting on "Save changes?".
    confirm: Option<Pending>,
    /// Set once the user has agreed to close, so the next close request goes through.
    closing: bool,
    /// The scene was swapped wholesale this frame (undo, redo, open), so it is not a new edit.
    swapped: bool,
    status: String,
    /// How sharp the preview is drawn.
    preview_quality: PreviewQuality,
    /// Show the performance meter over the preview.
    show_stats: bool,
    meter: Meter,
    /// (WS5) The latest soundtrack's analysis, for visuals that pulse with the music.
    mix: Option<Arc<bpe_music::AudioFeatures>>,
    workspace: Workspace,
    rules: rules_workspace::State,
    shaders: shader_workspace::State,
    palette: CommandPalette,
    /// Whether the preview floats over the Rules, Shaders and Batch workspaces.
    floating_preview: bool,
}

/// Editor frame rate and timings for the performance meter.
#[derive(Default)]
struct Meter {
    last_frame: Option<Instant>,
    /// Smoothed time between editor frames, in milliseconds.
    frame_ms: f32,
    /// Smoothed time the editor spends on one frame, in milliseconds.
    work_ms: f32,
    /// How long the last soundtrack took to make.
    sound_ms: Option<f32>,
    catching_up: bool,
}

#[derive(Clone, Copy)]
enum ToolbarClick {
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    Explore,
    /// Delete was pressed with a keyframe selected in the timeline.
    DeleteKeyframe,
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

/// A finished soundtrack: its generation, the result with the hits it was made from, and how
/// long it took in milliseconds.
/// (WS5) Also the soundtrack's analysis, for visuals that pulse with the finished mix.
type SoundResult = (u64, anyhow::Result<(bpe_audio::SceneSound, Arc<bpe_music::AudioFeatures>)>, HitSummary, f32);

/// Mixes the soundtrack on a background thread whenever the scene changes.
struct SoundJob {
    generation: u64,
    dirty_since: Option<Instant>,
    running: bool,
    sender: Sender<SoundResult>,
    receiver: Receiver<SoundResult>,
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
            playback: Playback::with_inputs(&scene, &bpe_audio::sim_inputs(&scene, None)),
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
            warp: None,
            export: ExportPanel::new(),
            gallery: Gallery::new(show_gallery),
            explorer: Explorer::default(),
            batch: BatchPanel::default(),
            in_batch: false,
            timeline: Timeline::default(),
            resume_after_scrub: false,
            clicked: None,
            was_unsaved: false,
            confirm: None,
            closing: false,
            swapped: false,
            status,
            preview_quality: PreviewQuality::default(),
            show_stats: false,
            meter: Meter::default(),
            mix: None,
            workspace: Workspace::default(),
            rules: rules_workspace::State::default(),
            shaders: shader_workspace::State::default(),
            palette: CommandPalette::default(),
            floating_preview: true,
        }
    }

    fn base_dir(&self) -> Option<PathBuf> {
        self.scene_path.as_ref().and_then(|p| p.parent()).map(PathBuf::from)
    }

    fn unsaved(&self) -> bool {
        self.scene != self.saved_scene
    }

    fn scene_changed(&mut self) {
        // Music workstream: follow the song's beats straight away when the song is already
        // loaded. Warp's clock and a song read for the first time arrive with the soundtrack.
        let base_dir = self.base_dir();
        let song = bpe_audio::cached_song(&self.scene, base_dir.as_deref());
        self.playback = Playback::with_inputs(&self.scene, &bpe_audio::sim_inputs(&self.scene, song.as_ref()));
        if self.scene.music.beat_sync.mode != bpe_scene::SyncMode::Warp {
            self.warp = None;
        }
        self.timeline.clear_hits();
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

    /// The playhead's time on the nearest video frame, where keyframes are added.
    fn key_time(&self) -> f32 {
        let fps = self.scene.video.fps.max(1) as f64;
        ((self.time * fps).round() / fps) as f32
    }

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
        while let Ok((generation, result, hits, took)) = self.sound.receiver.try_recv() {
            self.sound.running = false;
            if generation != self.sound.generation {
                continue;
            }
            self.meter.sound_ms = Some(took);
            self.timeline.set_hits(hits);
            match result {
                Ok((sound, mix)) => {
                    self.sound.error = None;
                    self.mix = Some(mix);
                    // Music workstream: the preview follows the same beats and clock as the sound.
                    self.inspector.music.report = inspector::music::Report::new(&sound);
                    let inputs = self.playback.simulation().inputs();
                    let same_music = inputs.music.as_ref().map(|m| m.duration())
                        == sound.inputs.music.as_ref().map(|m| m.duration());
                    if inputs.beats != sound.inputs.beats || !same_music {
                        self.playback = Playback::with_inputs(&self.scene, &sound.inputs);
                    }
                    self.warp = sound.warp;
                    if let Some(audio) = &self.audio {
                        audio.set_track(Arc::new(sound.mix.track.samples));
                        if self.playing {
                            audio.play_from(self.time);
                        }
                    }
                }
                Err(error) => {
                    self.sound.error = Some(format!("{error:#}"));
                    self.inspector.music.report = Default::default();
                    if self.warp.take().is_some() {
                        let song = bpe_audio::cached_song(&self.scene, self.base_dir().as_deref());
                        self.playback =
                            Playback::with_inputs(&self.scene, &bpe_audio::sim_inputs(&self.scene, song.as_ref()));
                    }
                    // WS5: don't keep pulsing to a soundtrack that is gone.
                    self.mix = None;
                }
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
                let started = Instant::now();
                let result = bpe_audio::render_scene(&scene, base_dir.as_deref()).map(|sound| {
                    // WS5: what the finished mix sounds like, for visuals that pulse with it.
                    let mix = bpe_render::effects::mix_features(&sound.mix.track.samples, bpe_audio::SAMPLE_RATE, 2);
                    (sound, mix)
                });
                let hits = match &result {
                    Ok((sound, _)) => {
                        let mut hits = HitSummary::new(&sound.video_events());
                        hits.music = Some(Arc::new(crate::timeline::MusicView::new(&scene, sound)));
                        hits
                    }
                    // The bounces still show when a sound file is missing.
                    Err(_) => HitSummary::new(&bpe_sim::collect_wall_hits(&scene)),
                };
                let took = started.elapsed().as_secs_f32() * 1000.0;
                let _ = sender.send((generation, result, hits, took));
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
        if pressed(Modifiers::COMMAND, Key::K) {
            self.palette.toggle();
        }
        for (key, workspace) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4].into_iter().zip(Workspace::ALL) {
            if pressed(Modifiers::COMMAND, key) {
                self.workspace = workspace;
            }
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
        // Over a block canvas, Space adds a block and Delete removes one.
        let canvas_keys = node_editor::take_wants_keyboard(ctx) && self.workspace != Workspace::Scene;
        if !canvas_keys && ctx.input(|i| i.key_pressed(Key::Space)) {
            self.set_playing(!self.playing);
        }
        if ctx.input(|i| i.key_pressed(Key::F3)) {
            self.show_stats = !self.show_stats;
        }
        // Arrow keys belong to a slider or button that has the keyboard focus.
        if canvas_keys || ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let fps = self.scene.video.fps.max(1) as f64;
        let (left, right, home, end, delete, shift) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::End),
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.modifiers.shift,
            )
        });
        let step = if shift { 1.0 } else { 1.0 / fps };
        if left || right {
            // Land on whole frames, so keyframes added here line up with the video's frames.
            let frame = (self.time * fps).round() / fps;
            self.seek(if left { frame - step } else { frame + step });
        }
        if home {
            self.seek(0.0);
        }
        if end {
            self.seek(self.scene.video.duration as f64);
        }
        if delete && self.timeline.selected() {
            self.clicked = Some(ToolbarClick::DeleteKeyframe);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("New").on_hover_text("Start a new scene from a preset").clicked() {
                self.gallery.open();
            }
            if ui.button("Open…").on_hover_text("Open a saved scene (Ctrl+O)").clicked() {
                self.clicked = Some(ToolbarClick::Open);
            }
            if ui.button("Save").on_hover_text("Save this scene (Ctrl+S)").clicked() {
                self.clicked = Some(ToolbarClick::Save);
            }
            if ui.button("Save as…").on_hover_text("Save this scene under a new name").clicked() {
                self.clicked = Some(ToolbarClick::SaveAs);
            }
            ui.separator();
            let undo = egui::Button::new("Undo");
            if ui.add_enabled(!self.history.undo.is_empty(), undo).on_hover_text("Undo (Ctrl+Z)").clicked() {
                self.clicked = Some(ToolbarClick::Undo);
            }
            let redo = egui::Button::new("Redo");
            if ui.add_enabled(!self.history.redo.is_empty(), redo).on_hover_text("Redo (Ctrl+Y)").clicked() {
                self.clicked = Some(ToolbarClick::Redo);
            }
            ui.separator();
            self.workspace_switch(ui);
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.scene.name).desired_width(200.0).hint_text("Scene name"))
                .on_hover_text("The scene's name. Exported videos are named after it.");
            let saved_label = match (&self.scene_path, self.was_unsaved) {
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
                    self.clicked = Some(ToolbarClick::Explore);
                }
            });
        });
    }

    /// Scene | Rules | Shaders | Batch, as one segmented control.
    fn workspace_switch(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, workspace) in Workspace::ALL.into_iter().enumerate() {
            let selected = self.workspace == workspace;
            // A dot on Batch while a batch runs, wherever the user is.
            let name = if workspace == Workspace::Batch && self.batch.busy() {
                format!("{} (running)", workspace.name())
            } else {
                workspace.name().to_string()
            };
            let text = if selected { RichText::new(name).color(Color32::WHITE) } else { RichText::new(name) };
            let button = egui::Button::selectable(selected, text);
            if ui.add(button).on_hover_text(format!("{} (Ctrl+{})", workspace.help(), i + 1)).clicked() {
                self.workspace = workspace;
            }
        }
        ui.spacing_mut().item_spacing.x = 8.0;
    }

    /// The live preview and the speed meter over it.
    fn preview(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) -> crate::preview::Outcome {
        let outcome = Preview::show(
            &mut self.preview,
            ui,
            frame,
            &mut self.scene,
            &mut self.playback,
            // Music workstream: Warp shows the simulation on its bent clock.
            self.warp.as_ref().map_or(self.time, |w| w.sim_time(self.time)),
            self.playing,
            self.inspector.selected,
            &self.inspector.effects.library,
            self.preview_quality,
        );
        if self.show_stats {
            let stats = self.preview.as_ref().map(|p| p.stats).unwrap_or_default();
            draw_meter(
                ui,
                &self.meter,
                &stats,
                self.playback.events().iter().filter(|e| e.is_hit()).count(),
                self.playing,
            );
        }
        outcome
    }

    fn run_command(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::Workspace(workspace) => self.workspace = workspace,
            Command::Tab(tab) => self.inspector.tab = tab,
            Command::Setting { label, tab, property } => {
                self.inspector.tab = tab;
                crate::command_palette::point_at(ctx, &label, tab, property);
            }
            Command::Action(action) => match action {
                Action::New => self.gallery.open(),
                Action::Open => self.clicked = Some(ToolbarClick::Open),
                Action::Save => self.clicked = Some(ToolbarClick::Save),
                Action::SaveAs => self.clicked = Some(ToolbarClick::SaveAs),
                Action::Undo => self.clicked = Some(ToolbarClick::Undo),
                Action::Redo => self.clicked = Some(ToolbarClick::Redo),
                Action::Export => self.export.open = true,
                Action::Explore => self.clicked = Some(ToolbarClick::Explore),
                Action::PlayPause => self.set_playing(!self.playing),
                Action::Restart => self.seek(0.0),
                Action::SpeedMeter => self.show_stats = !self.show_stats,
                Action::TogglePreview => self.floating_preview = !self.floating_preview,
            },
        }
    }

    /// The preview in a small window over a workspace's canvas, in the bottom right corner
    /// until moved.
    fn floating_preview(
        &mut self,
        ctx: &egui::Context,
        area: egui::Rect,
        frame: &mut eframe::Frame,
    ) -> crate::preview::Outcome {
        let mut outcome = crate::preview::Outcome::default();
        if !self.floating_preview {
            return outcome;
        }
        let mut open = true;
        egui::Window::new("Preview")
            .id(egui::Id::unique("floating preview"))
            .open(&mut open)
            .default_size([270.0, 520.0])
            .min_size([160.0, 240.0])
            .pivot(egui::Align2::RIGHT_BOTTOM)
            .default_pos(area.right_bottom() - egui::vec2(16.0, 16.0))
            .constrain_to(area)
            .collapsible(true)
            .resizable(true)
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());
                outcome = self.preview(ui, frame);
            });
        if !open {
            self.floating_preview = false;
            self.status = "Preview hidden. Ctrl+K, \"floating preview\" brings it back.".into();
        }
        outcome
    }

    fn transport(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if round_button(ui, 34.0, if self.playing { Icon::Pause } else { Icon::Play })
                .on_hover_text("Play or pause (Space)")
                .clicked()
            {
                self.set_playing(!self.playing);
            }
            if round_button(ui, 28.0, Icon::Restart).on_hover_text("Back to the start (Home)").clicked() {
                self.seek(0.0);
            }
            let duration = self.scene.video.duration as f64;
            ui.monospace(format!("{} / {}", clock(self.time), clock(duration)));
            ui.add_space(8.0);
            ui.label(RichText::new("Length").color(theme::MUTED));
            ui.add(egui::DragValue::new(&mut self.scene.video.duration).range(1.0..=600.0).speed(0.1).suffix(" s"))
                .on_hover_text(
                    "How long the video is, in seconds. Click to type, drag, or drag the end of the timeline.",
                );
            ui.checkbox(&mut self.looping, "Loop");
            if ui.checkbox(&mut self.muted, "Mute").changed()
                && let Some(audio) = &self.audio
            {
                audio.set_muted(self.muted);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut self.show_stats, "Speed meter")
                    .on_hover_text("Show the frame rate and how long each part of the editor takes (F3)");
                egui::ComboBox::from_id_salt("preview-quality")
                    .selected_text(self.preview_quality.label())
                    .width(80.0)
                    .show_ui(ui, |ui| {
                        for quality in PreviewQuality::ALL {
                            ui.selectable_value(&mut self.preview_quality, quality, quality.label());
                        }
                    })
                    .response
                    .on_hover_text(
                        "How sharp the preview is. Half or Quarter keeps big scenes and effect stacks smooth \
                         on a slower computer. Exported videos are always full quality.",
                    );
                ui.label(RichText::new("Preview").color(theme::MUTED));
                ui.add_space(8.0);
                ui.label(RichText::new("Arrow keys step a frame, Shift+arrow a second").small().color(theme::MUTED));
            });
        });
        let outcome = self.timeline.show(ui, &mut self.scene, self.time);
        match outcome.scrubbing {
            Some(true) => {
                self.resume_after_scrub = self.playing;
                self.set_playing(false);
            }
            Some(false) if std::mem::take(&mut self.resume_after_scrub) => self.set_playing(true),
            _ => {}
        }
        if let Some(time) = outcome.seek {
            self.seek(time);
        }
        if let Some(tab) = outcome.show_tab {
            self.inspector.tab = tab;
        }
        if let Some(index) = outcome.caption {
            self.inspector.text.focus = Some(index);
        }
    }

    fn status_line(&mut self, ui: &mut egui::Ui) {
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
        let frame_started = Instant::now();
        if let Some(last) = self.meter.last_frame.replace(frame_started) {
            let ms = (frame_started - last).as_secs_f32() * 1000.0;
            self.meter.frame_ms += (ms.min(1000.0) - self.meter.frame_ms) * 0.1;
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.closing && self.unsaved() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
        let tab_open = self.inspector.tab == Tab::Effects;
        self.inspector.effects.poll(&ctx, &self.scene, self.base_dir(), tab_open);
        let modal_open = self.gallery.open || self.explorer.open || self.confirm.is_some() || self.palette.open;
        if !modal_open {
            self.shortcuts(&ctx);
        }
        self.advance_clock();
        self.update_soundtrack(&ctx);

        let before = self.scene.clone();
        self.was_unsaved = self.unsaved();
        // While settings are animated, the panels show and edit them as they are at the playhead.
        let key_time = self.key_time();
        let shown = (!self.scene.animation.is_empty()).then(|| self.scene.at_time(key_time));
        if let Some(shown) = &shown {
            self.scene = shown.clone();
        }
        keys::publish(&ctx, &self.scene, key_time);
        crate::setting_menu::pulse::publish(
            &ctx,
            &self.scene,
            self.mix.clone(),
            self.playback.simulation().inputs().music.clone(),
        );
        egui::Panel::top("toolbar").frame(bar_frame()).show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(12, 4)))
            .show(ui, |ui| self.status_line(ui));
        egui::Panel::bottom("transport")
            .frame(bar_frame())
            .resizable(true)
            .default_size(300.0)
            .min_size(150.0)
            .max_size(640.0)
            .show(ui, |ui| self.transport(ui));
        egui::Panel::left("inspector")
            .default_size(460.0)
            .min_size(340.0)
            .max_size(600.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::PANEL))
            .show(ui, |ui| {
                self.inspector.music.base_dir = self.base_dir();
                inspector::show(ui, &mut self.inspector, &mut self.scene)
            });

        // Coming into the Batch workspace makes batches from the scene as it is now.
        if self.workspace == Workspace::Batch && !self.in_batch {
            let base_dir = self.base_dir();
            self.batch.open(&self.scene, &self.inspector.effects.library, base_dir);
        }
        self.in_batch = self.workspace == Workspace::Batch;
        let central = egui::Frame::new().fill(theme::BACKGROUND);
        let outcome = egui::CentralPanel::default()
            .frame(central)
            .show(ui, |ui| match self.workspace {
                Workspace::Scene => self.preview(ui, frame),
                workspace => {
                    let area = ui.max_rect();
                    egui::Frame::new().inner_margin(egui::Margin::same(8)).show(ui, |ui| match workspace {
                        Workspace::Rules => {
                            self.rules.editor.fit_inset_right = if self.floating_preview { 300.0 } else { 0.0 };
                            let live = (self.playback.simulation().rules(), self.time);
                            rules_workspace::show(ui, &mut self.scene, &mut self.rules, Some(live))
                        }
                        Workspace::Shaders => shader_workspace::show(
                            ui,
                            frame,
                            &mut self.scene,
                            &mut self.shaders,
                            &self.inspector.effects.library,
                            &self.playback,
                            self.time,
                        ),
                        _ => self.batch.contents(ui),
                    });
                    self.floating_preview(&ctx, area, frame)
                }
            })
            .inner;
        self.meter.catching_up = outcome.catching_up;
        if outcome.catching_up {
            ctx.request_repaint();
        }
        let base_dir = self.base_dir();
        if let Some(preview) = &mut self.preview {
            self.inspector.effects.gpu_errors = preview.effect_gpu_errors().to_vec();
            preview.set_mix(self.mix.clone());
            preview.set_font_folder(base_dir);
            self.inspector.text.font_errors = preview.font_errors().to_vec();
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
        if let Some(shown) = shown
            && !self.swapped
        {
            let edited = std::mem::take(&mut self.scene);
            self.scene = keys::write_back(&before, &shown, edited, key_time);
        }
        let requests = keys::take_requests(&ctx);
        if !requests.is_empty() {
            let library = &self.inspector.effects.library;
            let effects = self.scene.effects.clone();
            let rules = Scene { logic: self.scene.logic.clone(), ..Scene::default() };
            keys::apply_requests(&mut self.scene, requests, key_time, |property| match property {
                Property::EffectParam(i, name) => {
                    let effect = effects.get(*i as usize)?;
                    let info = library.get(&effect.shader)?;
                    let param = info.params.iter().find(|p| p.name == *name)?;
                    Some(Value::Number(param.default))
                }
                Property::Rule(..) => keys::rule_default(&rules, property),
                _ => None,
            });
        }
        // WS5: "Pulse with music" from the setting menus and wave icons, and its window.
        let requests = crate::setting_menu::pulse::take_requests(&ctx);
        if !requests.is_empty() {
            let library = &self.inspector.effects.library;
            let effects = self.scene.effects.clone();
            let rules = Scene { logic: self.scene.logic.clone(), ..Scene::default() };
            crate::setting_menu::pulse::apply_requests(&ctx, &mut self.scene, requests, |property| match property {
                Property::EffectParam(i, name) => {
                    let info = library.get(&effects.get(*i as usize)?.shader)?;
                    Some(Value::Number(info.params.iter().find(|p| p.name == *name)?.default))
                }
                Property::Rule(..) => keys::rule_default(&rules, property),
                _ => None,
            });
        }
        crate::setting_menu::pulse::window(&ctx, &mut self.scene, self.time);
        // WS5: "New node effect" and "Edit as nodes" open the Shaders workspace.
        if let Some(name) = self.inspector.effects.open_graph.take() {
            self.shaders.open(name);
            self.workspace = Workspace::Shaders;
        }
        match self.clicked.take() {
            Some(ToolbarClick::Open) => self.guard(Pending::Open, &ctx),
            Some(ToolbarClick::Save) => _ = self.save(false),
            Some(ToolbarClick::SaveAs) => _ = self.save(true),
            Some(ToolbarClick::Undo) => self.undo(),
            Some(ToolbarClick::Redo) => self.redo(),
            Some(ToolbarClick::Explore) => {
                self.set_playing(false);
                self.explorer.open(&self.scene, &self.inspector.effects.library);
            }
            Some(ToolbarClick::DeleteKeyframe) => self.timeline.delete_selected(&mut self.scene),
            None => {}
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
        if let Some(request) = self.explorer.make_batch.take() {
            let base_dir = self.base_dir();
            self.batch.open_from_explorer(request, &self.inspector.effects.library, base_dir);
            self.workspace = Workspace::Batch;
            self.in_batch = true;
        }
        if let Some((scene, seed)) = self.batch.update(&ctx, frame) {
            self.scene = scene;
            self.status = format!("Editing the batch's variation from seed {seed}. Ctrl+Z puts your scene back.");
            self.workspace = Workspace::Scene;
            separate = true;
        }
        if let Some(index) = self.inspector.rules.open.take() {
            self.workspace = Workspace::Rules;
            self.rules.graph = index;
            self.rules.editor.refit();
        }
        if let Some(rolled) = self.inspector.rolled.take() {
            self.status = rolled;
            separate = true;
        }
        self.confirm_dialog(&ctx);
        if let Some(command) = self.palette.show(&ctx, &self.scene) {
            self.run_command(command, &ctx);
        }

        if std::mem::take(&mut self.swapped) {
            // Already handled by undo, redo or opening a scene.
        } else if self.scene != before {
            // Effects and animated looks only change the picture, so the simulation and sound
            // can stay as they are.
            let picture_only = moving_parts(&self.scene) == moving_parts(&before);
            self.record_edit(before, separate);
            if !picture_only {
                self.scene_changed();
            }
        }
        self.end_merging(&ctx);

        if self.playing || self.export.busy() {
            ctx.request_repaint();
        }
        if self.show_stats {
            // Keep the meter's numbers fresh.
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        let work = frame_started.elapsed().as_secs_f32() * 1000.0;
        self.meter.work_ms += (work - self.meter.work_ms) * 0.1;
    }
}

/// The performance meter, in the top left corner of the preview.
fn draw_meter(ui: &egui::Ui, meter: &Meter, stats: &crate::preview::Stats, hits: usize, playing: bool) {
    let fps = if meter.frame_ms > 0.0 { 1000.0 / meter.frame_ms } else { 0.0 };
    let speed = if playing || meter.catching_up {
        format!("{fps:.0} fps · {:.1} ms a frame", meter.work_ms)
    } else {
        format!("paused, redrawn only on changes · {:.1} ms a frame", meter.work_ms)
    };
    let mut lines = vec![
        speed,
        format!("physics {:.1} ms{}", stats.simulate, if meter.catching_up { " (catching up)" } else { "" }),
    ];
    if stats.drew {
        lines.push(format!("shapes {:.1} ms · drawing {:.1} ms", stats.build, stats.draw));
    } else {
        lines.push("picture unchanged, not redrawn".into());
    }
    lines.push(format!("{} balls · {} shapes · {hits} hits so far", stats.balls, stats.shapes));
    if let Some(sound) = meter.sound_ms {
        lines.push(format!("sound rebuilt in {sound:.0} ms"));
    }
    let painter = ui.painter();
    let font = egui::FontId::monospace(12.0);
    let origin = ui.max_rect().min + egui::vec2(16.0, 16.0);
    let galleys: Vec<_> = lines.into_iter().map(|l| painter.layout_no_wrap(l, font.clone(), Color32::WHITE)).collect();
    let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
    let height: f32 = galleys.iter().map(|g| g.size().y).sum();
    let back = egui::Rect::from_min_size(origin, egui::vec2(width + 16.0, height + 12.0));
    painter.rect_filled(back, 6.0, Color32::from_black_alpha(190));
    let mut y = origin.y + 6.0;
    for galley in galleys {
        let h = galley.size().y;
        painter.galley(egui::pos2(origin.x + 8.0, y), galley, Color32::WHITE);
        y += h;
    }
}

/// The parts of a scene that decide how balls move and sound.
fn moving_parts(scene: &Scene) -> Scene {
    let mut parts = Scene { effects: Vec::new(), animation: Vec::new(), ..scene.clone() };
    // Sound settings change the soundtrack, which is worked out with the simulation.
    let moves = |p: &Property| p.affects_simulation() || p.affects_sound();
    parts.animation = scene.animation.iter().filter(|t| moves(&t.property)).cloned().collect();
    // WS5: node effects and pulses on the picture only change the picture.
    parts.shader_graphs = Vec::new();
    parts.modulators = scene.modulators.iter().filter(|m| m.property.affects_simulation()).cloned().collect();
    // 3D (WS4): the camera, lights and looks only change the picture.
    if let bpe_scene::View::Space(space) = &mut parts.view {
        **space = space.moving_parts();
    }
    parts
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

/// Seconds as m:ss.t.
pub(crate) fn clock(seconds: f64) -> String {
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
