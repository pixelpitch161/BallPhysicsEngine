//! The Batch workspace: make many variations of the scene as videos, overnight if need
//! be, and keep the best ones. Settings on the left, the variations with their scores in the
//! middle, and progress along the bottom. A batch that was stopped carries on from where it got
//! to when it is started again with the same settings and folder.

use crate::explorer::{BatchRequest, score_badge};
use crate::export::{minutes, open_file, safe_file_name, videos_dir};
use crate::inspector::text::named;
use crate::theme;
use bpe_export::ExportOptions;
use bpe_export::batch::{self, BatchSpec, Job, Manifest, Phase, Progress, Quality, Rejected, Status, Vary};
use bpe_export::score::{Score, Weights};
use bpe_render::EffectLibrary;
use bpe_scene::Scene;
use eframe::egui::{self, Color32, DragValue, RichText};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Width of the settings column.
const SETTINGS_WIDTH: f32 = 270.0;
const CARD: egui::Vec2 = egui::vec2(226.0, 118.0);
/// How often the running batch hands its record to the window.
const SNAPSHOT_EVERY: Duration = Duration::from_millis(300);

pub struct BatchPanel {
    /// The scene the variations are made from, fixed when the window opens so editing the scene
    /// meanwhile doesn't change a batch half way through.
    base: Scene,
    spec: BatchSpec,
    /// Where the videos go. `None` is a folder in Videos named after the scene.
    folder: Option<PathBuf>,
    ffmpeg: Option<PathBuf>,
    effects: EffectLibrary,
    base_dir: Option<PathBuf>,
    weights: Weights,
    /// The batch record in the chosen folder, as last read or reported by the running batch.
    manifest: Option<Manifest>,
    /// The folder `manifest` was read from.
    manifest_folder: Option<PathBuf>,
    job: Option<Running>,
    /// What went wrong last, or a note on how the batch ended.
    message: Option<(String, bool)>,
    /// Thumbs given, by seed.
    thumbs: HashMap<u64, bool>,
    /// The variation picked to open in the editor.
    picked: Option<(Scene, u64)>,
    /// Start was pressed; the batch starts once the window is drawn.
    start_requested: bool,
}

/// A batch running on its own thread.
struct Running {
    cancel: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
    started: Instant,
}

#[derive(Default)]
struct Shared {
    manifest: Option<Manifest>,
    progress: Progress,
    /// Set when the thread ends.
    finished: Option<Result<(), String>>,
}

impl Default for BatchPanel {
    fn default() -> Self {
        Self {
            base: Scene::default(),
            spec: BatchSpec::default(),
            folder: None,
            ffmpeg: bpe_export::find_ffmpeg(),
            effects: EffectLibrary::new(),
            base_dir: None,
            weights: Weights::default(),
            manifest: None,
            manifest_folder: None,
            job: None,
            message: None,
            thumbs: HashMap::new(),
            picked: None,
            start_requested: false,
        }
    }
}

impl BatchPanel {
    /// Makes batches from `scene`, unless one is running.
    pub fn open(&mut self, scene: &Scene, library: &EffectLibrary, base_dir: Option<PathBuf>) {
        if self.job.is_none() {
            self.base = scene.clone();
            self.spec.base = scene.name.clone();
            self.effects = library.clone();
            self.base_dir = base_dir;
            self.weights = batch::load_weights();
            self.reload();
        }
    }

    /// Makes batches with the explorer's settings.
    pub fn open_from_explorer(&mut self, request: BatchRequest, library: &EffectLibrary, base_dir: Option<PathBuf>) {
        if self.job.is_some() {
            self.message = Some(("A batch is already running. Stop it to start another.".into(), true));
            return;
        }
        self.open(&request.base, library, base_dir);
        self.spec.vary = request.vary;
        self.spec.amount = request.amount;
        self.spec.first_seed = request.seed;
        self.reload();
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// The folder the videos go in.
    fn folder(&self) -> PathBuf {
        self.folder.clone().or_else(|| self.manifest_folder.clone()).unwrap_or_else(|| self.default_folder(1))
    }

    /// The `n`th folder in Videos named after the scene: "Escape rings batch", then
    /// "Escape rings batch 2" and so on.
    fn default_folder(&self, n: u32) -> PathBuf {
        let name = if self.base.name.trim().is_empty() { "Ball video" } else { self.base.name.as_str() };
        let suffix = if n > 1 { format!(" {n}") } else { String::new() };
        videos_dir().join(format!("{} batch{suffix}", safe_file_name(name)))
    }

    /// Reads the batch record in the chosen folder, if there is one.
    fn reload(&mut self) {
        let folder = match &self.folder {
            Some(folder) => folder.clone(),
            // Without a chosen folder, the first one free or holding this same batch, so
            // changing a setting never runs into an older batch.
            None => (1..100)
                .map(|n| self.default_folder(n))
                .find(|f| Manifest::load(f).map_or(!f.join("batch.json").exists(), |m| m.spec == self.spec))
                .unwrap_or_else(|| self.default_folder(1)),
        };
        self.manifest = Manifest::load(&folder).ok();
        self.manifest_folder = Some(folder);
    }

    /// Whether the record in the folder is this batch, so starting carries on with it.
    fn resumable(&self) -> bool {
        self.manifest.as_ref().is_some_and(|m| m.spec == self.spec && m.phase != Phase::Done)
    }

    /// Keeps a running batch's progress up to date and starts a batch when Start was pressed.
    /// Call every frame, whichever workspace is showing. Returns a variation to open in the
    /// editor.
    pub fn update(&mut self, ctx: &egui::Context, frame: &eframe::Frame) -> Option<(Scene, u64)> {
        self.poll(ctx);
        if std::mem::take(&mut self.start_requested)
            && let Some(render_state) = frame.wgpu_render_state()
        {
            self.start(&render_state.device, &render_state.queue, ctx);
        }
        self.picked.take()
    }

    /// The Batch workspace: settings on the left, the variations in the middle, progress below.
    pub(crate) fn contents(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("batch-queue").show(ui, |ui| self.queue(ui));
        egui::Panel::left("batch-settings").resizable(false).exact_size(SETTINGS_WIDTH).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.settings(ui));
        });
        egui::CentralPanel::default().show(ui, |ui| self.grid(ui));
    }

    /// Takes the running batch's latest record and notices when it has ended.
    fn poll(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        let mut shared = job.shared.lock().unwrap();
        if let Some(manifest) = shared.manifest.take() {
            self.manifest = Some(manifest);
        }
        if let Some(finished) = shared.finished.take() {
            let cancelled = job.cancel.load(Ordering::Relaxed);
            drop(shared);
            let elapsed = job.started.elapsed().as_secs_f32();
            self.message = Some(match finished {
                Err(error) => (error, true),
                Ok(()) if cancelled => ("Stopped. Start again with the same settings to carry on.".into(), false),
                Ok(()) => {
                    let kept = self.manifest.as_ref().map_or(0, |m| m.kept().len());
                    (format!("Done in {}. Kept the best {kept}.", minutes(elapsed)), false)
                }
            });
            self.job = None;
        } else {
            drop(shared);
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let running = self.job.is_some();
        let before = (self.spec.clone(), self.folder.clone());
        ui.add_enabled_ui(!running, |ui| {
            ui.label(RichText::new("What to change").strong().color(theme::TEXT));
            let vary = &mut self.spec.vary;
            let has_effects = !self.base.effects.is_empty();
            for (name, meaning) in Vary::NAMES {
                let (value, label) = match name {
                    "balls" => (&mut vary.balls, "Balls"),
                    "arena" => (&mut vary.arena, "Arena"),
                    "bounces" => (&mut vary.bounces, "Bounces"),
                    "look" => (&mut vary.look, "Look"),
                    "effects" => (&mut vary.effects, "Effects"),
                    "sound" => (&mut vary.sound, "Chords and notes"),
                    "song" => (&mut vary.song, "Song"),
                    "instrument" => (&mut vary.instrument, "Instrument"),
                    _ => (&mut vary.captions, "Captions"),
                };
                let enabled = name != "effects" || has_effects;
                let mut text = meaning.to_string();
                text[..1].make_ascii_uppercase();
                ui.add_enabled(enabled, egui::Checkbox::new(value, label))
                    .on_hover_text(text)
                    .on_disabled_hover_text("Add an effect in the Effects tab to vary it");
            }
            if !self.spec.vary.any() {
                ui.label(RichText::new("Tick at least one thing to change.").color(theme::WARNING));
            }
            ui.add_space(8.0);
            ui.label(RichText::new("How different").strong().color(theme::TEXT));
            ui.add(
                egui::Slider::new(&mut self.spec.amount, 0.0..=1.0)
                    .show_value(false)
                    .clamping(egui::SliderClamping::Edits),
            )
            .on_hover_text("Left keeps the videos close to your scene; right lets anything change");
            ui.add_space(8.0);
            if self.spec.vary.song {
                ui.label(RichText::new("Songs folder").strong().color(theme::TEXT));
                ui.horizontal(|ui| {
                    let text = self.spec.songs.as_ref().map_or("None chosen".into(), |p| p.display().to_string());
                    ui.add(egui::Label::new(RichText::new(text).small().color(theme::MUTED)).truncate());
                    if ui.button("Choose…").on_hover_text("A folder of mp3, wav, ogg or flac songs").clicked()
                        && let Some(folder) = rfd::FileDialog::new().set_title("Songs folder").pick_folder()
                    {
                        self.spec.songs = Some(folder);
                    }
                });
                ui.add_space(8.0);
            }
            egui::Grid::new("batch-numbers").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Videos to make");
                ui.add(DragValue::new(&mut self.spec.count).range(1..=10_000).clamp_existing_to_range(false))
                    .on_hover_text("How many videos to render. Each takes about as long as exporting the scene once.");
                ui.end_row();
                ui.label("Keep the best");
                ui.add(DragValue::new(&mut self.spec.keep).range(1..=self.spec.count).clamp_existing_to_range(false))
                    .on_hover_text("How many of the rendered videos to keep, by score");
                ui.end_row();
                ui.label("Seeds tried per video");
                ui.add(DragValue::new(&mut self.spec.prescreen).range(1..=50).clamp_existing_to_range(false))
                    .on_hover_text(
                        "Simulating is much faster than rendering, so the batch simulates this many seeds for \
                     every video and only renders the most promising ones",
                    );
                ui.end_row();
                ui.label("First seed");
                ui.add(DragValue::new(&mut self.spec.first_seed))
                    .on_hover_text("The same scene, settings and seeds always make the same videos");
                ui.end_row();
                ui.label("Quality");
                let quality = egui::ComboBox::from_id_salt("batch-quality")
                    .width(100.0)
                    .selected_text(self.spec.quality.name())
                    .show_ui(ui, |ui| {
                        for quality in Quality::ALL {
                            ui.selectable_value(&mut self.spec.quality, quality, quality.name());
                        }
                    });
                named(quality.response, "Quality").on_hover_text(
                    "Draft is half size and quick, for checking a batch; Good is full size; Best is the slowest \
                     and sharpest",
                );
                ui.end_row();
                ui.label("Videos not kept");
                let rejected = egui::ComboBox::from_id_salt("batch-rejected")
                    .width(100.0)
                    .selected_text(match self.spec.rejected {
                        Rejected::Move => "Move aside",
                        Rejected::Delete => "Delete",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.spec.rejected, Rejected::Move, "Move aside");
                        ui.selectable_value(&mut self.spec.rejected, Rejected::Delete, "Delete");
                    });
                named(rejected.response, "Videos not kept")
                    .on_hover_text("Move aside puts them in a folder called \"rejected\" inside the output folder");
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.label(RichText::new("Output folder").strong().color(theme::TEXT));
            ui.add(
                egui::Label::new(RichText::new(self.folder().display().to_string()).small().color(theme::MUTED))
                    .truncate(),
            )
            .on_hover_text(self.folder().display().to_string());
            ui.horizontal(|ui| {
                if ui.button("Change…").clicked()
                    && let Some(folder) = rfd::FileDialog::new().set_title("Folder for the videos").pick_folder()
                {
                    self.folder = Some(folder);
                }
                if ui.button("Open folder").clicked() {
                    let folder = self.folder();
                    let _ = std::fs::create_dir_all(&folder);
                    open_file(&folder);
                }
            });
            ui.add_space(8.0);
            let estimate = self.estimate();
            ui.label(RichText::new(estimate).small().color(theme::MUTED));
        });
        if (self.spec.clone(), self.folder.clone()) != before || self.manifest_folder.as_ref() != Some(&self.folder()) {
            self.reload();
        }
    }

    /// A rough idea of how long the batch takes.
    fn estimate(&self) -> String {
        let seconds = self.base.video.duration.min(self.spec.max_seconds.unwrap_or(f32::MAX));
        // Rendering runs at a few times real time on most computers, encoding included.
        let per_video = match self.spec.quality {
            Quality::Draft => seconds * 0.4,
            Quality::Good => seconds * 1.0,
            Quality::Best => seconds * 1.8,
        };
        let total = per_video * self.spec.count as f32;
        format!(
            "{} videos of {} each. Roughly {} on a typical computer; the computer won't sleep meanwhile.",
            self.spec.count,
            minutes(seconds),
            minutes(total)
        )
    }

    /// The variations: the kept ones first, then rendered, then the pre-screen.
    fn grid(&mut self, ui: &mut egui::Ui) {
        let Some(manifest) = &self.manifest else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No batch in this folder yet").heading().color(theme::TEXT));
                ui.label(
                    RichText::new(
                        "Choose what to change on the left and press Start. The batch tries many seeds by \
                         simulating them, renders the most promising, scores the videos and keeps the best.",
                    )
                    .color(theme::MUTED),
                );
            });
            return;
        };
        let mut jobs: Vec<&Job> = manifest.jobs.iter().filter(|j| j.prescore.is_some()).collect();
        jobs.sort_by(|a, b| rank(b).cmp(&rank(a)).then(best(b).total_cmp(&best(a))));
        let rendered = manifest.jobs.iter().filter(|j| j.score.is_some()).count();
        ui.horizontal(|ui| {
            ui.label(RichText::new(summary(manifest)).color(theme::TEXT));
            if rendered > 0 {
                ui.label(
                    RichText::new("Like and Nope teach the scorer what you like, for this and later batches.")
                        .small()
                        .color(theme::MUTED),
                );
            }
        });
        ui.add_space(6.0);
        let mut thumb = None;
        let mut open = None;
        let mut play = None;
        let columns = ((ui.available_width() / (CARD.x + 12.0)) as usize).max(1);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            egui::Grid::new("batch-jobs").spacing([8.0, 8.0]).show(ui, |ui| {
                for (i, job) in jobs.iter().take(300).enumerate() {
                    match job_card(ui, job, self.thumbs.get(&job.seed).copied(), &self.weights) {
                        Some(JobChoice::Thumb(liked)) => thumb = Some((job.seed, liked)),
                        Some(JobChoice::Open) => open = Some(job.seed),
                        Some(JobChoice::Play) => play = job.video.clone().map(|v| (v, job.status)),
                        None => {}
                    }
                    if i % columns == columns - 1 {
                        ui.end_row();
                    }
                }
            });
        });
        if let Some((seed, liked)) = thumb
            && let Some(job) = manifest.jobs.iter().find(|j| j.seed == seed)
            && let Some(score) = job.score.or(job.prescore)
        {
            learn(&mut self.weights, &score.parts, liked);
            self.thumbs.insert(seed, liked);
        }
        if let Some((video, status)) = play {
            let folder = self.folder();
            let path =
                if status == Status::Rejected { folder.join("rejected").join(&video) } else { folder.join(&video) };
            if path.is_file() {
                open_file(&path);
            } else {
                self.message = Some((format!("Couldn't find {}", path.display()), true));
            }
        }
        if let Some(seed) = open
            && let Some(manifest) = &self.manifest
        {
            match manifest.base() {
                Ok(base) => {
                    let (scene, _) = batch::variation(
                        &base,
                        &manifest.spec.vary,
                        seed,
                        manifest.spec.amount,
                        &self.effects,
                        &manifest.songs,
                    );
                    self.picked = Some((scene, seed));
                }
                Err(error) => self.message = Some((format!("{error:#}"), true)),
            }
        }
    }

    fn queue(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        if let Some(job) = &self.job {
            let progress = job.shared.lock().unwrap().progress.clone();
            let (fraction, line) = describe_progress(&progress);
            ui.add(egui::ProgressBar::new(fraction).text(line).desired_width(ui.available_width()));
            ui.horizontal(|ui| {
                if ui.button("Stop").on_hover_text("Stop after this step. Start again later to carry on.").clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                if job.cancel.load(Ordering::Relaxed) {
                    ui.label(RichText::new("Stopping…").color(theme::MUTED));
                }
                ui.label(
                    RichText::new("You can switch to another workspace and keep editing; the batch carries on.")
                        .small()
                        .color(theme::MUTED),
                );
            });
        } else {
            ui.horizontal(|ui| {
                let resumable = self.resumable();
                let finished = self.manifest.as_ref().is_some_and(|m| m.spec == self.spec && m.phase == Phase::Done);
                let label = if resumable {
                    "Carry on"
                } else if finished {
                    "Finished"
                } else {
                    "Start"
                };
                let start = egui::Button::new(RichText::new(label).strong().color(Color32::WHITE))
                    .fill(theme::ACCENT)
                    .min_size(egui::vec2(110.0, 30.0));
                let can = self.ffmpeg.is_some() && self.spec.vary.any() && !self.different_batch() && !finished;
                let response = ui.add_enabled(can, start).on_disabled_hover_text(if finished {
                    "This batch is finished. Change a setting or the folder to make another."
                } else {
                    "Tick something to change, and make sure ffmpeg was found"
                });
                let response = response.on_hover_text(if resumable {
                    "Carry on with the stopped batch in this folder"
                } else {
                    "Start the batch"
                });
                if response.clicked() {
                    self.start_requested = true;
                }
                if self.ffmpeg.is_none() {
                    ui.label(
                        RichText::new("Ball Studio needs ffmpeg to make videos and couldn't find it.").color(theme::WARNING),
                    );
                } else if self.different_batch() {
                    if ui
                        .button("Use its settings")
                        .on_hover_text("Set everything on the left to the settings of the batch in this folder")
                        .clicked()
                        && let Some(manifest) = &self.manifest
                    {
                        self.spec = manifest.spec.clone();
                    }
                    ui.label(
                        RichText::new("This folder holds a batch with other settings. Choose another folder, or use its settings.")
                            .color(theme::WARNING),
                    );
                }
            });
        }
        if let Some((message, error)) = &self.message {
            ui.label(RichText::new(message).color(if *error { theme::ERROR } else { theme::TEXT }));
        }
        ui.add_space(4.0);
    }

    /// The folder holds a batch with other settings, so starting there would fail.
    fn different_batch(&self) -> bool {
        self.manifest.as_ref().is_some_and(|m| m.spec != self.spec)
    }

    /// Starts the batch, or carries on with the one in the folder, on a thread of its own.
    pub(crate) fn start(
        &mut self,
        device: &bpe_render::wgpu::Device,
        queue: &bpe_render::wgpu::Queue,
        ctx: &egui::Context,
    ) {
        let Some(ffmpeg) = self.ffmpeg.clone() else { return };
        let folder = self.folder();
        let manifest = match batch::open(&folder, &self.spec, &self.base, self.weights) {
            Ok(manifest) => manifest,
            Err(error) => {
                self.message = Some((format!("{error:#}"), true));
                return;
            }
        };
        let mut options = ExportOptions::new(folder.join("unused.mp4"));
        options.ffmpeg = ffmpeg;
        options.base_dir = self.base_dir.clone();
        options.effects = self.effects.clone();
        let (device, queue, ctx) = (device.clone(), queue.clone(), ctx.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Shared { manifest: Some(manifest.clone()), ..Shared::default() }));
        let (c, s) = (cancel.clone(), shared.clone());
        std::thread::spawn(move || {
            let mut manifest = manifest;
            let runner = batch::Runner { device: &device, queue: &queue, folder, options, cancel: &c };
            let mut last = Instant::now();
            let mut phase = manifest.phase;
            let result = runner.run(&mut manifest, |manifest, progress| {
                let mut shared = s.lock().unwrap();
                shared.progress = progress.clone();
                // Copying the record is the costly part, so only now and then.
                if last.elapsed() > SNAPSHOT_EVERY || progress.phase != phase {
                    shared.manifest = Some(manifest.clone());
                    last = Instant::now();
                    phase = progress.phase;
                    ctx.request_repaint();
                }
            });
            let mut shared = s.lock().unwrap();
            shared.manifest = Some(manifest);
            shared.finished = Some(result.map_err(|e| format!("{e:#}")));
            ctx.request_repaint();
        });
        self.message = None;
        self.job = Some(Running { cancel, shared, started: Instant::now() });
    }
}

/// Teaches the scorer from a Like (`liked`) or Nope. Starts from the weights saved on disk, so
/// what the explorer and the batch window each learned adds up instead of one overwriting the
/// other, then saves.
pub(crate) fn learn(weights: &mut Weights, parts: &bpe_export::score::Parts, liked: bool) {
    // Tests leave the user's learned weights alone.
    if !cfg!(test) {
        *weights = batch::load_weights();
    }
    weights.learn(parts, liked);
    if !cfg!(test) {
        batch::save_weights(weights);
    }
}

/// Sort order of the cards: kept first, then rendered, then the rest.
fn rank(job: &Job) -> u8 {
    match job.status {
        Status::Kept => 5,
        Status::Rendered => 4,
        Status::Chosen => 3,
        Status::Rejected => 2,
        Status::Screened | Status::Failed => 1,
        Status::Waiting | Status::Skipped => 0,
    }
}

/// The job's best-known score.
fn best(job: &Job) -> f32 {
    job.score.or(job.prescore).map_or(-1.0, |s| s.total)
}

fn summary(manifest: &Manifest) -> String {
    let screened = manifest.jobs.iter().filter(|j| j.prescore.is_some()).count();
    let rendered = manifest.jobs.iter().filter(|j| j.score.is_some()).count();
    let kept = manifest.count(Status::Kept);
    match manifest.phase {
        Phase::Prescreen => format!("Tried {screened} of {} seeds", manifest.jobs.len()),
        Phase::Render => format!("Rendered {rendered} of {} videos", manifest.spec.count),
        Phase::Pick => "Picking the best".into(),
        Phase::Done => format!("Finished: kept {kept} of {rendered} videos"),
    }
}

/// How far along the batch is, from 0 to 1, and a line saying so.
fn describe_progress(progress: &Progress) -> (f32, String) {
    let eta = progress.eta.map_or(String::new(), |eta| format!(", about {} left", minutes(eta.as_secs_f32())));
    let part = |done: usize, total: usize| if total == 0 { 0.0 } else { done as f32 / total as f32 };
    match progress.phase {
        // Trying seeds is quick next to rendering, so it fills the first tenth of the bar.
        Phase::Prescreen => (
            0.1 * part(progress.done, progress.total),
            format!("Trying seeds: {} of {}", progress.done, progress.total),
        ),
        Phase::Render => {
            let frame = if progress.frame.1 > 0 { progress.frame.0 as f32 / progress.frame.1 as f32 } else { 0.0 };
            let fraction =
                0.1 + 0.9 * part(progress.done, progress.total.max(1)) + 0.9 * frame / progress.total.max(1) as f32;
            let line = match progress.seed {
                Some(seed) => format!(
                    "Rendering video {} of {} (seed {seed}){eta}",
                    (progress.done + 1).min(progress.total),
                    progress.total
                ),
                None => format!("Rendered {} of {}{eta}", progress.done, progress.total),
            };
            (fraction.min(1.0), line)
        }
        Phase::Pick => (1.0, "Picking the best".into()),
        Phase::Done => (1.0, "Done".into()),
    }
}

enum JobChoice {
    Thumb(bool),
    Open,
    Play,
}

fn job_card(ui: &mut egui::Ui, job: &Job, given: Option<bool>, weights: &Weights) -> Option<JobChoice> {
    let mut choice = None;
    let (status, color) = match job.status {
        Status::Kept => ("Kept", Color32::from_rgb(90, 200, 130)),
        Status::Rendered => ("Rendered", theme::TEXT),
        Status::Chosen => ("To render", theme::ACCENT_TEXT),
        Status::Rejected => ("Not kept", theme::MUTED),
        Status::Failed => ("Failed", theme::ERROR),
        Status::Screened | Status::Waiting => ("Tried", theme::MUTED),
        Status::Skipped => ("Skipped", theme::MUTED),
    };
    // Scores shown use the latest weights, so thumbs change them straight away.
    let score: Option<Score> = job.score.or(job.prescore).map(|s| weights.score(s.parts));
    let (rect, _) = ui.allocate_exact_size(CARD, egui::Sense::hover());
    ui.painter().rect_filled(rect, 8.0, theme::CARD);
    let mut card = ui
        .new_child(egui::UiBuilder::new().max_rect(rect.shrink(8.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    let ui = &mut card;
    ui.label(RichText::new(format!("Seed {}", job.seed)).strong().color(theme::TEXT));
    ui.label(RichText::new(status).small().color(color));
    if let Some(error) = &job.error {
        ui.add(egui::Label::new(RichText::new(error).small().color(theme::ERROR)).truncate());
    } else if let Some(song) = &job.song {
        let song = std::path::Path::new(song).file_name().map_or(song.clone(), |n| n.to_string_lossy().into_owned());
        ui.add(egui::Label::new(RichText::new(song).small().color(theme::MUTED)).truncate());
    } else if let Some(score) = score {
        ui.add(egui::Label::new(RichText::new(strongest(&score)).small().color(theme::MUTED)).truncate());
    }
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.horizontal(|ui| {
            if ui.small_button("Edit").on_hover_text("Open this variation in the editor").clicked() {
                choice = Some(JobChoice::Open);
            }
            if job.video.is_some() && ui.small_button("Play").on_hover_text("Play the video").clicked() {
                choice = Some(JobChoice::Play);
            }
            let enabled = score.is_some() && given.is_none();
            let up = egui::Button::new("Like").small().selected(given == Some(true));
            if ui.add_enabled(enabled, up).on_hover_text("Score ones like this higher").clicked() {
                choice = Some(JobChoice::Thumb(true));
            }
            let down = egui::Button::new("Nope").small().selected(given == Some(false));
            if ui.add_enabled(enabled, down).on_hover_text("Score ones like this lower").clicked() {
                choice = Some(JobChoice::Thumb(false));
            }
        });
    });
    let id = ui.scope_id().with(("job score", job.seed));
    score_badge(ui, rect, score, id);
    choice
}

/// The two best parts of a score, such as "Climax 92 · Rhythm 80".
fn strongest(score: &Score) -> String {
    let mut parts: Vec<(&str, f32)> = score.parts.list().iter().filter_map(|(n, v, _)| Some((*n, (*v)?))).collect();
    parts.sort_by(|a, b| b.1.total_cmp(&a.1));
    parts.iter().take(2).map(|(n, v)| format!("{n} {:.0}", v * 100.0)).collect::<Vec<_>>().join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn progress_fills_the_bar_in_order() {
        let mut progress = Progress { phase: Phase::Prescreen, done: 5, total: 10, ..Progress::default() };
        let (screening, _) = describe_progress(&progress);
        progress = Progress { phase: Phase::Render, done: 1, total: 4, seed: Some(3), frame: (50, 100), ..progress };
        let (rendering, line) = describe_progress(&progress);
        assert!(screening < rendering && rendering < 1.0, "{screening} {rendering}");
        assert!(line.contains("video 2 of 4"), "{line}");
        progress.phase = Phase::Done;
        assert_eq!(describe_progress(&progress).0, 1.0);
    }

    #[test]
    fn kept_videos_come_first() {
        let kept = Job { status: Status::Kept, ..Job::default() };
        let tried = Job { status: Status::Screened, ..Job::default() };
        assert!(rank(&kept) > rank(&tried));
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bpe-app-batch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A panel on a small, quick batch in `folder`.
    fn panel(folder: &std::path::Path) -> BatchPanel {
        let mut panel = BatchPanel::default();
        let scene = Scene::presets().into_iter().next().unwrap().scene;
        panel.open(&scene, &EffectLibrary::new(), None);
        panel.folder = Some(folder.to_path_buf());
        panel.spec.count = 2;
        panel.spec.keep = 1;
        panel.spec.prescreen = 2;
        panel.spec.max_seconds = Some(1.0);
        panel.spec.scale = 0.1;
        panel.spec.quality = Quality::Draft;
        panel.reload();
        panel
    }

    fn harness(panel: BatchPanel) -> Harness<'static, BatchPanel> {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(1100.0, 760.0))
            .with_max_steps(240)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(|ui, panel: &mut BatchPanel| panel.contents(ui), panel);
        harness.run();
        harness
    }

    #[test]
    fn an_empty_folder_explains_what_a_batch_does() {
        let folder = temp_dir("empty");
        let mut harness = harness(panel(&folder));
        harness.get_by_label("No batch in this folder yet");
        harness.get_by_label("Start");
        // Nothing to change means nothing to start.
        for name in ["Balls", "Arena", "Bounces", "Look"] {
            harness.get_by_label(name).click();
        }
        harness.run();
        harness.get_by_label("Tick at least one thing to change.");
        harness.get_by_label("Start").click();
        harness.run();
        assert!(!harness.state().start_requested);
        harness.get_by_label("Captions").click();
        harness.get_by_label("Start").click();
        harness.run();
        assert!(harness.state().start_requested);
        assert!(harness.state().spec.vary.captions);
    }

    #[test]
    fn a_stopped_batch_offers_to_carry_on_and_another_batch_is_refused() {
        let folder = temp_dir("carry-on");
        let panel = panel(&folder);
        batch::open(&folder, &panel.spec, &panel.base, Weights::default()).unwrap();
        let mut harness = harness(self::panel(&folder));
        harness.get_by_label("Carry on");
        harness.get_by_label_contains("Tried 0 of 4 seeds");
        // Different settings in the same folder would mix two batches.
        harness.state_mut().spec.amount = 0.9;
        harness.run();
        harness.get_by_label_contains("holds a batch with other settings");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Runs a real batch from the window, on the graphics card, and checks it keeps the best.
    #[test]
    fn a_batch_runs_from_the_window_and_keeps_the_best() {
        let require = std::env::var_os("BPE_REQUIRE_GPU").is_some();
        let Ok((device, queue)) = bpe_render::headless_device() else {
            assert!(!require, "BPE_REQUIRE_GPU is set but no GPU was found");
            eprintln!("skipping: no GPU");
            return;
        };
        if bpe_export::find_ffmpeg().is_none() {
            assert!(!require, "BPE_REQUIRE_GPU is set but ffmpeg wasn't found");
            return;
        }
        let folder = temp_dir("run");
        let mut harness = harness(panel(&folder));
        harness.get_by_label("Start").click();
        harness.run();
        assert!(std::mem::take(&mut harness.state_mut().start_requested));
        let ctx = harness.ctx.clone();
        harness.state_mut().start(&device, &queue, &ctx);
        let started = Instant::now();
        while harness.state().busy() {
            assert!(started.elapsed() < Duration::from_secs(180), "the batch took too long");
            harness.state_mut().poll(&ctx);
            harness.step();
            std::thread::sleep(Duration::from_millis(50));
        }
        harness.run();
        let message = harness.state().message.clone().unwrap();
        assert!(!message.1 && message.0.contains("Kept the best 1"), "{message:?}");
        harness.get_by_label_contains("Finished: kept 1 of 2 videos");
        harness.get_by_label("Kept");
        let manifest = harness.state().manifest.clone().unwrap();
        let kept = manifest.kept();
        assert_eq!(kept.len(), 1);
        assert!(folder.join(kept[0].video.as_ref().unwrap()).is_file());
        // Thumbs teach the scorer, and only once per video.
        let weights = harness.state().weights;
        harness.get_all_by_label("Like").next().unwrap().click();
        harness.run();
        assert_ne!(harness.state().weights, weights);
        assert_eq!(harness.state().thumbs.len(), 1);
        // Edit opens that variation in the editor: the same scene the batch rendered.
        harness.get_all_by_label("Edit").next().unwrap().click();
        harness.run();
        let (scene, seed) = harness.state_mut().picked.take().unwrap();
        let saved = Scene::load(folder.join(kept[0].video.as_ref().unwrap().replace(".mp4", ".ron"))).unwrap();
        assert_eq!(scene.to_ron().unwrap(), saved.to_ron().unwrap(), "seed {seed}");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
