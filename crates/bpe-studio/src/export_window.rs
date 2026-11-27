//! The "Export video" window: pick a file, the quality and the size, then render the mp4 on a
//! thread.

use crate::style;
use bpe_cli::water::WaterSource;
use bpe_cli::{
    Encoder, ExportOptions, export_arranged, export_video, find_ffmpeg, frame_count, gpu_encoders, output_size,
};
use bpe_core::Scene;
use bpe_core::video::Video;
use bpe_kit::{Kit, SceneSettings};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the window exports: the scene, or the video Video mode arranges.
pub enum Subject<'a> {
    Scene { scene: &'a Scene, settings: &'a SceneSettings },
    Video { video: &'a Video, name: String },
}

impl Subject<'_> {
    /// What the mp4 is named after.
    fn name(&self) -> String {
        match self {
            Subject::Scene { scene, .. } => scene.name.clone(),
            Subject::Video { name, .. } => name.clone(),
        }
    }

    fn is_video(&self) -> bool {
        matches!(self, Subject::Video { .. })
    }

    /// The width, height and frame rate.
    fn picture(&self) -> (u32, u32, u32) {
        match self {
            Subject::Scene { settings, .. } => (settings.width.max(1), settings.height.max(1), settings.fps),
            Subject::Video { video, .. } => (video.settings.width, video.settings.height, video.settings.fps),
        }
    }

    /// Its length in seconds.
    fn seconds(&self) -> f64 {
        match self {
            Subject::Scene { settings, .. } => f64::from(settings.duration),
            Subject::Video { video, .. } => video.end(),
        }
    }

    fn frames(&self) -> u32 {
        match self {
            Subject::Scene { settings, .. } => frame_count(settings),
            Subject::Video { video, .. } => video.frame_count(),
        }
    }
}

/// The graphics card encoders that work, and the ffmpeg they were checked with.
type CheckedEncoders = (PathBuf, Vec<Encoder>);

/// A Water scene's export (Water mode part 8): what its final is, as the editor finds it each
/// frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaterExport {
    /// The finals folder.
    pub finals: PathBuf,
    /// Whether the final covers the whole video.
    pub complete: bool,
    /// What the final is now, and what making it takes, in words.
    pub words: String,
    pub making_words: String,
    /// While a make for the export runs: how far it is (0 to 1) and about how long it has left.
    pub making: Option<(f32, Option<f32>)>,
    /// The water seconds, and the video's seconds and frames with slow motion.
    pub water_seconds: f64,
    pub video_seconds: f64,
    pub frames: u32,
}

pub struct ExportWindow {
    pub open: bool,
    /// The file chosen with "Change…". `None` saves next to the scene, named after it. It is
    /// the open scene's: opening or starting another forgets it.
    pub(crate) path: Option<PathBuf>,
    /// The file chosen with "Change…" for the video in Video mode (forgotten when another video opens).
    pub(crate) video_path: Option<PathBuf>,
    /// H.264 quality: lower is sharper and bigger.
    crf: u8,
    /// Output size as a fraction of the video size (Preview final frame draws at it too).
    pub(crate) scale: f32,
    encoder: Encoder,
    /// Also save each bus and the master as WAVs beside the video.
    stems: bool,
    midi: bool,
    /// Bake the lighting or draw it live (lighting part 3); `None` as the Lighting card's On
    /// export says.
    bake: Option<bool>,
    /// "Test a frame" was pressed: the size and the bake choice (the app opens the final frame).
    pub test_frame: Option<(f32, Option<bool>)>,
    ffmpeg: Option<PathBuf>,
    gpu: Arc<Mutex<Option<CheckedEncoders>>>,
    /// The ffmpeg a check was started for.
    checking: Option<PathBuf>,
    job: Option<Job>,
    finished: Option<Result<Finished, Failure>>,
    /// How the last export ended, for the editor's status line: taken once.
    news: Option<ExportNews>,
    /// The settings folder the choices are remembered in; `None` (tests) remembers nothing.
    remember_in: Option<PathBuf>,
    /// The choices as last written there.
    remembered: Option<Remembered>,
    /// Whether a text box had the keyboard as the last frame ended (Escape then leaves the box, no more).
    typing: bool,
    /// A Water scene's final, while one is open (the editor sets it each frame).
    pub(crate) water: Option<WaterExport>,
    /// Make final and export was pressed, or Cancel while it makes (the editor takes them).
    pub(crate) make_and_export: bool,
    pub(crate) stop_making: bool,
    /// The make for Make final and export is done: the window exports from the final.
    pub(crate) export_final: bool,
}

/// How an export ended, for the status line.
#[derive(Clone, Debug, PartialEq)]
pub enum ExportNews {
    Saved { name: String, seconds: f32 },
    Failed { headline: String },
    Cancelled,
}

/// Why an export didn't make a video.
#[derive(Clone, Debug, PartialEq)]
enum Failure {
    Cancelled,
    Failed(Explained),
}

/// A failed export in plain words, with the technical detail behind it.
#[derive(Clone, Debug, PartialEq)]
pub struct Explained {
    pub headline: String,
    pub details: String,
}

/// The export choices remembered between runs (not the file, which is the scene's, nor the
/// Bake choice, which overrides one scene's Lighting card).
#[derive(Clone, Debug, PartialEq)]
struct Remembered {
    scale: f32,
    crf: u8,
    encoder: Encoder,
    stems: bool,
    midi: bool,
    ffmpeg: Option<PathBuf>,
}

/// The file in the settings folder the export choices are remembered in.
const EXPORT_CHOICES: &str = "export-next.txt";
/// The sizes the window offers.
const SCALES: [f32; 3] = [1.0, 0.5, 0.25];
/// The compression the slider allows.
const CRF: std::ops::RangeInclusive<u8> = 14..=28;

struct Finished {
    path: PathBuf,
    seconds: f32,
    encoder: Encoder,
}

struct Job {
    path: PathBuf,
    done: Arc<AtomicU32>,
    total: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<anyhow::Result<Encoder>>,
    started: Instant,
}

impl Default for ExportWindow {
    fn default() -> Self {
        Self {
            open: false,
            path: None,
            video_path: None,
            crf: 18,
            scale: 1.0,
            encoder: Encoder::Auto,
            stems: false,
            midi: false,
            bake: None,
            test_frame: None,
            ffmpeg: find_ffmpeg(),
            gpu: Arc::default(),
            checking: None,
            job: None,
            finished: None,
            news: None,
            remember_in: None,
            remembered: None,
            typing: false,
            water: None,
            make_and_export: false,
            stop_making: false,
            export_final: false,
        }
    }
}

impl ExportWindow {
    /// Finds out in the background which graphics card encoders work with the chosen ffmpeg.
    fn check_encoders(&mut self) {
        let Some(ffmpeg) = self.ffmpeg.clone() else { return };
        if self.checking.as_ref() == Some(&ffmpeg) {
            return;
        }
        self.checking = Some(ffmpeg.clone());
        let found = self.gpu.clone();
        std::thread::spawn(move || {
            let encoders = gpu_encoders(&ffmpeg);
            *found.lock().unwrap() = Some((ffmpeg, encoders));
        });
    }

    /// The graphics card encoders that work, or None while still checking.
    fn gpu_encoders(&self) -> Option<Vec<Encoder>> {
        let found = self.gpu.lock().unwrap();
        match (found.as_ref(), &self.ffmpeg) {
            (Some((checked, encoders)), Some(ffmpeg)) if checked == ffmpeg => Some(encoders.clone()),
            _ => None,
        }
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// How far the running export has got, 0 to 100.
    pub fn progress_percent(&self) -> Option<u32> {
        let job = self.job.as_ref()?;
        let (done, total) = (job.done.load(Ordering::Relaxed), job.total.load(Ordering::Relaxed).max(1));
        Some((done as u64 * 100 / total as u64).min(100) as u32)
    }

    /// Makes the window look mid-export at `done` of `total` frames, for tests; the job ends when
    /// the returned sender sends or is dropped.
    #[cfg(test)]
    pub(crate) fn pretend_busy(&mut self, done: u32, total: u32) -> std::sync::mpsc::Sender<anyhow::Result<Encoder>> {
        let (sender, receiver) = channel();
        self.job = Some(Job {
            path: PathBuf::from("pretend.mp4"),
            done: Arc::new(AtomicU32::new(done)),
            total: Arc::new(AtomicU32::new(total)),
            cancel: Arc::default(),
            receiver,
            started: Instant::now(),
        });
        sender
    }

    /// How the last export ended, once.
    pub fn take_news(&mut self) -> Option<ExportNews> {
        self.news.take()
    }

    /// The window with the choices remembered in the settings folder `dir`, remembering any
    /// change there from now on.
    pub fn load_in(dir: &Path) -> Self {
        let mut window = Self::default();
        let text = std::fs::read_to_string(dir.join(EXPORT_CHOICES)).unwrap_or_default();
        window.apply_remembered(&text);
        window.remember_in = Some(dir.to_path_buf());
        window.remembered = Some(window.remembered_now());
        window
    }

    /// Takes the choices in a remembered file's `key=value` lines; anything odd is left as it is.
    fn apply_remembered(&mut self, text: &str) {
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "scale" => {
                    if let Ok(scale) = value.parse::<f32>()
                        && let Some(&known) = SCALES.iter().find(|s| (**s - scale).abs() < 1e-3)
                    {
                        self.scale = known;
                    }
                }
                "crf" => {
                    if let Ok(crf) = value.parse::<u8>() {
                        self.crf = crf.clamp(*CRF.start(), *CRF.end());
                    }
                }
                "encoder" => {
                    if let Some(encoder) = Encoder::from_name(value) {
                        self.encoder = encoder;
                    }
                }
                "stems" => self.stems = value == "true",
                "midi" => self.midi = value == "true",
                // A remembered ffmpeg that has gone is looked for again.
                "ffmpeg" if !value.is_empty() && Path::new(value).is_file() => self.ffmpeg = Some(value.into()),
                _ => {}
            }
        }
    }

    fn remembered_now(&self) -> Remembered {
        Remembered {
            scale: self.scale,
            crf: self.crf,
            encoder: self.encoder,
            stems: self.stems,
            midi: self.midi,
            ffmpeg: self.ffmpeg.clone(),
        }
    }

    /// Writes the choices to the settings folder when they changed since last written.
    fn remember_if_changed(&mut self) {
        let Some(dir) = &self.remember_in else { return };
        let now = self.remembered_now();
        if self.remembered.as_ref() == Some(&now) {
            return;
        }
        let ffmpeg = now.ffmpeg.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        let text = format!(
            "scale={}\ncrf={}\nencoder={}\nstems={}\nmidi={}\nffmpeg={ffmpeg}\n",
            now.scale,
            now.crf,
            now.encoder.key(),
            now.stems,
            now.midi
        );
        let _ = std::fs::create_dir_all(dir);
        let _ = std::fs::write(dir.join(EXPORT_CHOICES), text);
        self.remembered = Some(now);
    }

    /// Records how a job ended, for the window and the status line.
    fn finish(&mut self, ended: Result<Finished, Failure>) {
        self.news = Some(match &ended {
            Ok(done) => ExportNews::Saved {
                name: done.path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                seconds: done.seconds,
            },
            Err(Failure::Cancelled) => ExportNews::Cancelled,
            Err(Failure::Failed(why)) => ExportNews::Failed { headline: why.headline.clone() },
        });
        self.finished = Some(ended);
    }

    /// Stops a running export before the editor closes, so no half-written file is left.
    pub fn shutdown(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
            // The export thread notices at its next frame, stops ffmpeg and deletes the file.
            let _ = job.receiver.recv_timeout(Duration::from_secs(15));
        }
    }

    #[cfg(test)]
    fn output_path(&self, scene: &Scene, base_dir: Option<&Path>) -> PathBuf {
        let settings = SceneSettings::default();
        self.subject_path(&Subject::Scene { scene, settings: &settings }, base_dir)
    }

    fn subject_path(&self, subject: &Subject, base_dir: Option<&Path>) -> PathBuf {
        let chosen = if subject.is_video() { &self.video_path } else { &self.path };
        chosen.clone().unwrap_or_else(|| {
            let folder = base_dir.map(Path::to_path_buf).unwrap_or_else(videos_dir);
            folder.join(format!("{}.mp4", safe_file_name(&subject.name())))
        })
    }

    /// The window for `scene` (tests).
    #[cfg(test)]
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        scene: &Scene,
        settings: &SceneSettings,
        base_dir: Option<&Path>,
        kit: &Arc<Kit>,
    ) {
        self.show_subject(ctx, &Subject::Scene { scene, settings }, base_dir, kit);
    }

    /// The window for `subject`, the scene or the video.
    pub fn show_subject(&mut self, ctx: &egui::Context, subject: &Subject, base_dir: Option<&Path>, kit: &Arc<Kit>) {
        if std::mem::take(&mut self.export_final) && self.job.is_none() && !subject.is_video() {
            self.start(ctx, subject, base_dir, kit, Some(WaterSource::Final));
        }
        if let Some(job) = &self.job {
            let result = match job.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                // Its thread stopped without a word (it panicked).
                Err(TryRecvError::Disconnected) => Some(Err(anyhow::anyhow!(STOPPED_UNEXPECTEDLY))),
            };
            if let Some(result) = result {
                let ended = match result {
                    Ok(encoder) => {
                        Ok(Finished { path: job.path.clone(), seconds: job.started.elapsed().as_secs_f32(), encoder })
                    }
                    Err(_) if job.cancel.load(Ordering::Relaxed) => Err(Failure::Cancelled),
                    Err(error) => Err(Failure::Failed(explain_export_error(&format!("{error:#}")))),
                };
                self.job = None;
                self.finish(ended);
            }
        }
        // egui takes the keyboard from a text box on Escape before the frame is drawn, so what the key was for is
        // what held the keyboard a frame ago.
        let was_typing = std::mem::replace(&mut self.typing, ctx.text_edit_focused());
        if !self.open {
            return;
        }
        self.check_encoders();
        let mut open = self.open;
        // Escape closes the window only when the key is for nothing else: not a list or menu that is open (asked
        // before the window is drawn, as the list is shut by then), a text box being left, or a question on top.
        let escape_is_free =
            !egui::Popup::is_any_open(ctx) && !was_typing && ctx.memory(|m| m.top_modal_layer().is_none());
        let window = egui::Window::new("Export video")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(460.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO);
        style::window(ctx, window, |ui| {
            ui.set_width(460.0);
            let busy = self.job.is_some();
            ui.add_enabled_ui(!busy, |ui| self.settings(ui, subject, base_dir, kit));
            ui.add_space(10.0);
            self.progress(ui, ctx, subject, base_dir, kit);
        });
        // Escape closes the window as its cross does.
        if escape_is_free && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        self.open = open;
        self.remember_if_changed();
    }

    fn settings(&mut self, ui: &mut egui::Ui, subject: &Subject, base_dir: Option<&Path>, kit: &Kit) {
        let path = self.subject_path(subject, base_dir);
        ui.label(RichText::new("SAVE AS").size(12.0).color(style::MUTED));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                ui.label(RichText::new(name).color(style::TEXT_STRONG));
                let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                ui.label(RichText::new(format!("in {folder}")).small().color(style::MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Change…").clicked()
                    && let Some(chosen) = rfd::FileDialog::new()
                        .add_filter("MP4 video", &["mp4"])
                        .set_directory(path.parent().unwrap_or(Path::new(".")))
                        .set_file_name(path.file_name().unwrap_or_default().to_string_lossy())
                        .save_file()
                {
                    if subject.is_video() {
                        self.video_path = Some(chosen);
                    } else {
                        self.path = Some(chosen);
                    }
                }
            });
        });
        if path.exists() {
            // The video this window has just made is no surprise: say what exporting again does, without the alarm.
            let just_made = matches!(&self.finished, Some(Ok(done)) if done.path == path);
            let words = if just_made {
                RichText::new("Exporting again replaces the video you just made.").small().color(style::MUTED)
            } else {
                RichText::new("A video with this name already exists and will be replaced.").small().color(style::TEXT)
            };
            ui.label(words);
        }

        ui.add_space(8.0);
        ui.label(RichText::new("SIZE AND QUALITY").size(12.0).color(style::MUTED));
        egui::Grid::new("export-settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Size").on_hover_text("A smaller draft exports much faster, for checking timing.");
            ui.horizontal(|ui| {
                for (scale, name) in SCALES.into_iter().zip(["Full", "Half", "Quarter"]) {
                    ui.radio_value(&mut self.scale, scale, name);
                }
                let (width, height, _) = subject.picture();
                let (w, h) = output_size(width, height, self.scale);
                ui.label(RichText::new(format!("{w} × {h}")).small().color(style::MUTED));
            });
            ui.end_row();
            let compression = ui
                .label("Compression")
                .on_hover_text("14 is the sharpest and biggest, 18 looks lossless, 28 is small and soft.");
            ui.add(egui::Slider::new(&mut self.crf, CRF)).labelled_by(compression.id);
            ui.end_row();
            ui.label("Length");
            let fps = subject.picture().2;
            let length = match self.water.as_ref().filter(|_| !subject.is_video()) {
                Some(w) if (w.video_seconds - w.water_seconds).abs() > 1e-3 => format!(
                    "{} s at {fps} fps · {} frames ({} s of water, in slow motion)",
                    trim_seconds(w.video_seconds),
                    w.frames,
                    trim_seconds(w.water_seconds)
                ),
                Some(w) => format!("{} s at {fps} fps · {} frames", trim_seconds(w.video_seconds), w.frames),
                None => format!("{} s at {fps} fps · {} frames", trim_seconds(subject.seconds()), subject.frames()),
            };
            ui.label(RichText::new(length).color(style::MUTED));
            ui.end_row();
            ui.label("Encoder").on_hover_text(
                "The graphics card can compress the video much faster than the CPU. The picture going in \
                 is exactly the same; files can be a little bigger.",
            );
            match self.gpu_encoders() {
                None if self.ffmpeg.is_some() => {
                    ui.label(RichText::new("Checking your graphics card…").small().color(style::MUTED));
                    // The answer comes from another thread: look again soon, or this line stays until the mouse moves.
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
                found => {
                    let mut choices = vec![Encoder::Auto, Encoder::Cpu];
                    choices.extend(found.unwrap_or_default());
                    if !choices.contains(&self.encoder) {
                        self.encoder = Encoder::Auto;
                    }
                    let list = egui::ComboBox::from_id_salt("export-encoder")
                        .selected_text(self.encoder.name())
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for encoder in choices {
                                ui.selectable_value(&mut self.encoder, encoder, encoder.name());
                            }
                        });
                    // A list has no name of its own for screen readers and tests.
                    ui.ctx().accesskit_node_builder(list.response.id, |node| node.set_label("Encoder list"));
                }
            }
            ui.end_row();
            ui.label("ffmpeg program").on_hover_text("The free program that encodes the mp4.");
            ui.horizontal(|ui| {
                let text = self.ffmpeg.as_ref().map_or("Not found".into(), |p| p.display().to_string());
                ui.add(egui::Label::new(RichText::new(text).small()).truncate());
                if ui.button("Choose…").clicked()
                    && let Some(chosen) = rfd::FileDialog::new().set_title("Find ffmpeg").pick_file()
                {
                    self.ffmpeg = Some(chosen);
                    self.check_encoders();
                }
            });
            ui.end_row();
        });
        let scene = match subject {
            Subject::Scene { scene, .. } => *scene,
            Subject::Video { .. } => {
                self.video_sound(ui);
                return;
            }
        };
        if let Some(water) = &self.water {
            water_rows(ui, water);
            self.no_ffmpeg(ui);
            return;
        }
        self.lighting(ui, scene, kit);
        ui.add_space(8.0);
        ui.label(RichText::new("SOUND").size(12.0).color(style::MUTED));
        egui::Grid::new("export-sound").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Mixed sound");
            ui.label(RichText::new("In the video, AAC 320 kbps").color(style::MUTED));
            ui.end_row();
            let label = ui.label("Also save stems").on_hover_text(
                "One WAV per bus and one of the master (48 kHz, 32-bit float), saved next to the video. The buses \
                 add up to the mix before the master's limiter.",
            );
            style::switch(ui, &mut self.stems).labelled_by(label.id);
            ui.end_row();
            let label = ui.label("Also save MIDI").on_hover_text(
                "The notes the shapes played, one track per shape, with the tempo, saved next to the video.",
            );
            style::switch(ui, &mut self.midi).labelled_by(label.id);
            ui.end_row();
        });
        ui.add_space(8.0);
        self.no_ffmpeg(ui);
    }

    /// What the window says when there is no ffmpeg.
    fn no_ffmpeg(&self, ui: &mut egui::Ui) {
        if self.ffmpeg.is_none() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Ball Studio needs the free program ffmpeg to make videos, and couldn't find it. \
                     Install it, or point to ffmpeg.exe with Choose…",
                )
                .color(style::TEXT),
            );
        }
    }

    fn progress(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        subject: &Subject,
        base_dir: Option<&Path>,
        kit: &Arc<Kit>,
    ) {
        if let Some(job) = &self.job {
            let (done, total) = (job.done.load(Ordering::Relaxed), job.total.load(Ordering::Relaxed).max(1));
            let elapsed = job.started.elapsed().as_secs_f32();
            let left = if done > 10 { elapsed / done as f32 * (total - done) as f32 } else { f32::NAN };
            let text = if left.is_finite() {
                format!("Frame {done} of {total} · about {} left", minutes(left))
            } else {
                format!("Frame {done} of {total}")
            };
            ui.add(egui::ProgressBar::new(done as f32 / total as f32).text(text));
            if ui.button("Cancel").clicked() {
                job.cancel.store(true, Ordering::Relaxed);
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        if let Some(water) = self.water.clone().filter(|_| !subject.is_video()) {
            self.water_buttons(ui, ctx, &water, subject, base_dir, kit);
        } else {
            let export = style::primary_button("Export").fill(style::TEXT).min_size(egui::vec2(120.0, 32.0));
            let empty = subject.frames() == 0 && subject.is_video();
            let button = ui.add_enabled(self.ffmpeg.is_some() && !empty, export);
            if empty {
                ui.label(RichText::new("Add a clip first").small().color(style::MUTED));
            }
            if button.clicked() {
                self.start(ctx, subject, base_dir, kit, None);
            }
        }
        match &self.finished {
            Some(Ok(done)) => {
                ui.add_space(6.0);
                let name = done.path.file_name().unwrap_or_default().to_string_lossy();
                ui.label(RichText::new(format!("Saved {name} in {}.", minutes(done.seconds))).color(style::TEXT));
                ui.label(
                    RichText::new(format!("Encoded with the {}.", done.encoder.name())).small().color(style::MUTED),
                );
                ui.horizontal(|ui| {
                    if ui.button("Play video").clicked() {
                        open_file(&done.path);
                    }
                    if ui.button("Show in folder").clicked() {
                        show_in_folder(&done.path);
                    }
                });
            }
            Some(Err(Failure::Cancelled)) => {
                ui.add_space(6.0);
                ui.label(RichText::new("Export cancelled.").color(style::MUTED));
            }
            Some(Err(Failure::Failed(why))) => {
                ui.add_space(6.0);
                ui.label(RichText::new(&why.headline).color(style::PROBLEM));
                failure_details(ui, &why.details);
            }
            None => {}
        }
    }

    /// A Water scene's buttons: Export from the final when it is made, else Make final and export
    /// and Export the draft; a progress bar while the final is made for the export.
    fn water_buttons(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        water: &WaterExport,
        subject: &Subject,
        base_dir: Option<&Path>,
        kit: &Arc<Kit>,
    ) {
        if let Some((share, left)) = water.making {
            let percent = (share * 100.0).floor() as u32;
            let text = match left {
                Some(left) => format!("Making the final: {percent} % · about {} left", minutes(left)),
                None => format!("Making the final: {percent} %"),
            };
            ui.add(egui::ProgressBar::new(share).text(text));
            ui.label(RichText::new("The video exports from it once it is made.").small().color(style::MUTED));
            if ui.button("Cancel").clicked() {
                self.stop_making = true;
            }
            ctx.request_repaint_after(Duration::from_millis(250));
            return;
        }
        let ready = self.ffmpeg.is_some();
        if water.complete {
            let export = style::primary_button("Export").fill(style::TEXT).min_size(egui::vec2(120.0, 32.0));
            let export = ui.add_enabled(ready, export).on_hover_text("Export the video from the scene's final.");
            if export.clicked() {
                self.start(ctx, subject, base_dir, kit, Some(WaterSource::Final));
            }
            return;
        }
        ui.horizontal(|ui| {
            let make =
                style::primary_button("Make final and export").fill(style::TEXT).min_size(egui::vec2(180.0, 32.0));
            let make = ui.add_enabled(ready, make).on_hover_text(format!(
                "Make the scene's final at Final detail ({}), then export the video from it.",
                water.making_words
            ));
            if make.clicked() {
                self.make_and_export = true;
                self.finished = None;
            }
            let draft = ui
                .add_enabled(ready, egui::Button::new("Export the draft").min_size(egui::vec2(0.0, 32.0)))
                .on_hover_text("Export the quick, coarse draft water now, to check the timing.");
            if draft.clicked() {
                self.start(ctx, subject, base_dir, kit, Some(WaterSource::Draft));
            }
        });
        ui.label(RichText::new(format!("Making the final takes {}.", water.making_words)).small().color(style::MUTED));
    }

    /// Says the final couldn't be made for the export.
    pub(crate) fn water_make_failed(&mut self, why: &str) {
        let headline = format!("The final couldn't be made: {why}");
        self.finish(Err(Failure::Failed(Explained { headline, details: String::new() })));
    }

    fn start(
        &mut self,
        ctx: &egui::Context,
        subject: &Subject,
        base_dir: Option<&Path>,
        kit: &Arc<Kit>,
        water: Option<WaterSource>,
    ) {
        let Some(ffmpeg) = self.ffmpeg.clone() else { return };
        let path = self.subject_path(subject, base_dir);
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        // A video open in a player can't be replaced: say so now, not after the sound is made.
        if path.exists()
            && let Err(error) = std::fs::OpenOptions::new().append(true).open(&path)
        {
            self.finish(Err(Failure::Failed(explain_export_error(&format!(
                "can't write {}: {error}",
                path.display()
            )))));
            return;
        }
        let mut options = self.options(&path, ffmpeg);
        let mut frames = subject.frames();
        if let (Some(source), Some(info)) = (water, &self.water) {
            options.water = source;
            options.finals = Some(info.finals.clone());
            (options.stems, options.midi, options.bake) = (false, false, None);
            frames = info.frames;
        }
        // Owned copies for the export's thread.
        let work = match subject {
            Subject::Scene { scene, .. } => Work::Scene(Box::new((*scene).clone())),
            Subject::Video { video, .. } => {
                // Stems, MIDI and baking aren't made for a video yet.
                (options.stems, options.midi, options.bake) = (false, false, None);
                Work::Video(Box::new((*video).clone()))
            }
        };
        let (kit, ctx) = (kit.clone(), ctx.clone());
        let (done, total, cancel) =
            (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(frames)), Arc::new(AtomicBool::new(false)));
        let (sender, receiver) = channel();
        let (d, t, c) = (done.clone(), total.clone(), cancel.clone());
        std::thread::spawn(move || {
            // A complaint from the graphics card stops the export with it, instead of panicking.
            let gpu_error: Arc<Mutex<Option<String>>> = Arc::default();
            let result = bpe_gfx::headless_device().and_then(|(device, queue)| {
                let seen = gpu_error.clone();
                device.on_uncaptured_error(Arc::new(move |error| {
                    seen.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(|| error.to_string());
                }));
                let progress = |frame, frames| {
                    d.store(frame, Ordering::Relaxed);
                    t.store(frames, Ordering::Relaxed);
                    ctx.request_repaint();
                    !c.load(Ordering::Relaxed) && gpu_error.lock().is_ok_and(|e| e.is_none())
                };
                match &work {
                    Work::Scene(scene) => export_video(&device, &queue, scene, &kit, &options, progress),
                    Work::Video(video) => export_arranged(&device, &queue, video, &kit, &options, progress),
                }
            });
            let gpu_error = gpu_error.lock().unwrap_or_else(|e| e.into_inner()).take();
            let result = match gpu_error {
                Some(error) => Err(anyhow::anyhow!("the graphics card reported a problem: {error}")),
                None => result,
            };
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        self.finished = None;
        self.job = Some(Job { path, done, total, cancel, receiver, started: Instant::now() });
    }
}

/// What an export's thread exports.
enum Work {
    Scene(Box<Scene>),
    Video(Box<Video>),
}

/// A Water scene's FINAL rows: what its final is, and its sound.
fn water_rows(ui: &mut egui::Ui, water: &WaterExport) {
    ui.add_space(8.0);
    ui.label(RichText::new("FINAL").size(12.0).color(style::MUTED));
    egui::Grid::new("export-final").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
        ui.label("Final").on_hover_text(
            "A Water scene's video is drawn from its final: the water worked out at Final detail with Make final, \
             kept on the disk so the export only reads it.",
        );
        let words = water.words.trim_start_matches("· ");
        ui.add(egui::Label::new(RichText::new(words).color(style::MUTED)).wrap());
        ui.end_row();
        ui.label("Sound");
        ui.label(RichText::new("None yet: the video has a silent track").color(style::MUTED));
        ui.end_row();
    });
}

/// "12", "7.5" seconds.
fn trim_seconds(seconds: f64) -> String {
    let text = format!("{seconds:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl ExportWindow {
    /// The Sound rows for a video: the mix only.
    fn video_sound(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(RichText::new("SOUND").size(12.0).color(style::MUTED));
        egui::Grid::new("export-video-sound").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Mixed sound");
            ui.label(RichText::new("The song and every clip, AAC 320 kbps").color(style::MUTED));
            ui.end_row();
        });
        ui.label(
            RichText::new("Stems, MIDI and baked lighting aren't made for a video yet.").small().color(style::MUTED),
        );
        if self.ffmpeg.is_none() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Ball Studio needs the free program ffmpeg to make videos, and couldn't find it. \
                     Install it, or point to ffmpeg.exe with Choose…",
                )
                .color(style::TEXT),
            );
        }
    }

    /// The export settings the window's choices make, for a video at `path`.
    fn options(&self, path: &Path, ffmpeg: PathBuf) -> ExportOptions {
        let mut options = ExportOptions::new(path);
        options.scale = self.scale;
        options.crf = self.crf;
        options.preset = if self.scale < 1.0 { "veryfast".into() } else { "medium".into() };
        options.ffmpeg = ffmpeg;
        options.encoder = self.encoder;
        options.stems = self.stems;
        options.midi = self.midi;
        options.bake = self.bake;
        options
    }

    /// The Lighting row, when the scene has a Lighting card: Live or Bake, what the bake does, and
    /// Test a frame.
    fn lighting(&mut self, ui: &mut egui::Ui, scene: &Scene, kit: &Kit) {
        use bpe_kit::modules::lighting::{OnExport, lighting_of};
        let Some(card) = lighting_of(scene, &kit.registry) else { return };
        let card_bakes = card.on_export == OnExport::Bake;
        ui.add_space(8.0);
        ui.label(RichText::new("LIGHTING").size(12.0).color(style::MUTED));
        egui::Grid::new("export-lighting").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Lighting").on_hover_text(
                "Live draws each frame as the editor does. Bake draws each frame many times and averages them: \
                 smoother light and edges, motion blur, light bouncing and glass caustics, but much slower. The \
                 Lighting card's On export picks which one starts here.",
            );
            ui.horizontal(|ui| {
                let mut bakes = self.bake.unwrap_or(card_bakes);
                let before = bakes;
                ui.radio_value(&mut bakes, false, "Live");
                ui.radio_value(&mut bakes, true, "Bake");
                if bakes != before {
                    self.bake = (bakes != card_bakes).then_some(bakes);
                }
                if ui
                    .button("Test a frame")
                    .on_hover_text("Draws the frame at the playhead as this export will.")
                    .clicked()
                {
                    self.test_frame = Some((self.scale, self.bake));
                }
            });
            ui.end_row();
            if self.bake.unwrap_or(card_bakes) {
                ui.label("");
                ui.label(RichText::new(bake_summary(&card)).small().color(style::MUTED));
                ui.end_row();
            }
        });
    }
}

/// One line on what a bake does, from the Lighting card.
fn bake_summary(card: &bpe_kit::modules::lighting::LightingSettings) -> String {
    let mut parts = vec![format!("{} samples a pixel", card.bake_samples.max(1))];
    if card.bake_bounce == bpe_kit::modules::lighting::BakeBounce::Traced {
        parts.push(match card.light_bounces {
            0 => "no traced bounces".to_string(),
            1 => "1 traced light bounce".to_string(),
            n => format!("{n} traced light bounces"),
        });
    }
    if card.motion_blur && card.shutter > 0.0 {
        parts.push(format!("motion blur (shutter {:.2})", card.shutter));
    }
    if card.clean_up {
        parts.push("noise clean-up".into());
    }
    parts.join(" · ")
}

/// What a job whose thread stopped without a word says.
const STOPPED_UNEXPECTEDLY: &str = "the export stopped unexpectedly";

/// A failed export's error in plain words: what to do about it, with the raw text as details.
pub fn explain_export_error(raw: &str) -> Explained {
    let text = raw.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| text.contains(w));
    let headline = if any(&["could not start ffmpeg"]) {
        "Ball Studio couldn't start ffmpeg. Point to ffmpeg.exe again with Choose…"
    } else if any(&["no space left", "disk full", "not enough space", "there is not enough space"]) {
        "The disk is full. Free some space, or save the video on another drive with Change…"
    } else if any(&[
        "permission denied",
        "access is denied",
        "being used by another process",
        "resource busy",
        "os error 32",
        "can't write",
    ]) {
        "Ball Studio couldn't write the video. If it's open in a player, close it and try again; otherwise \
         choose another folder with Change…"
    } else if any(&["unknown encoder", "encoder not found", "no such encoder"]) {
        "This copy of ffmpeg can't make H.264 videos. Install a full build of ffmpeg, then point to it with Choose…"
    } else if any(&["out of memory", "outofmemory"]) {
        "The graphics card ran out of memory. Try Half size, or Live lighting instead of Bake."
    } else if any(&["did not come back from the graphics card", "device lost", "graphics card reported"]) {
        "The graphics card stopped while drawing the video. Try again; if it keeps happening, restart Ball Studio."
    } else if any(&["adapter"]) {
        "Ball Studio couldn't find a graphics card to draw the video with."
    } else if any(&[STOPPED_UNEXPECTEDLY]) {
        "The export stopped unexpectedly. Try again; if it keeps happening, restart Ball Studio."
    } else {
        "Something went wrong while making the video."
    };
    Explained { headline: headline.into(), details: raw.trim().to_string() }
}

/// The technical detail of a failure, folded away: a scrolling, selectable log and a copy button.
fn failure_details(ui: &mut egui::Ui, details: &str) {
    if details.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(RichText::new("Details").small()).id_salt("export-failure-details").show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(details).small().monospace().color(style::MUTED)).selectable(true));
        });
        if ui.button("Copy details").clicked() {
            ui.ctx().copy_text(details.to_string());
        }
    });
}

pub fn minutes(seconds: f32) -> String {
    let seconds = seconds.max(0.0).round() as u32;
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{}:{:02} min", seconds / 60, seconds % 60),
        _ => format!("{} h {:02} min", seconds / 3600, seconds % 3600 / 60),
    }
}

/// The user's Videos folder, or their home folder if there is none.
pub(crate) fn videos_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    match home {
        Some(home) if home.join("Videos").is_dir() => home.join("Videos"),
        Some(home) => home,
        None => PathBuf::from("."),
    }
}

/// A scene name with the characters Windows does not allow in file names replaced.
pub fn safe_file_name(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '-' } else { c }).collect();
    let cleaned = cleaned.trim().trim_end_matches('.');
    if cleaned.is_empty() { "ball-video".into() } else { cleaned.into() }
}

pub(crate) fn open_file(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

pub(crate) fn show_in_folder(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer").raw_arg(format!("/select,\"{}\"", path.display())).spawn();
    }
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    if let Some(folder) = path.parent() {
        let _ = std::process::Command::new("xdg-open").arg(folder).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Settings, ThingId};

    #[test]
    fn file_names_are_safe() {
        assert_eq!(safe_file_name("Reel block"), "Reel block");
        assert_eq!(safe_file_name("a/b: c?"), "a-b- c-");
        assert_eq!(safe_file_name("  "), "ball-video");
    }

    #[test]
    fn the_stems_switch_reaches_the_export() {
        let mut window = ExportWindow::default();
        assert!(!window.options(Path::new("a.mp4"), "ffmpeg".into()).stems, "off by default");
        window.stems = true;
        assert!(window.options(Path::new("a.mp4"), "ffmpeg".into()).stems);
    }

    #[test]
    fn export_window_has_stem_and_midi_switches() {
        let mut window = ExportWindow::default();
        assert!(!window.options(Path::new("a.mp4"), "ffmpeg".into()).midi, "off by default");
        window.midi = true;
        assert!(window.options(Path::new("a.mp4"), "ffmpeg".into()).midi);
    }

    #[test]
    fn escape_closes_the_window_but_first_a_list_that_is_open() {
        use egui_kittest::kittest::Queryable;
        let studio = crate::app::Studio::headless();
        // No ffmpeg: nothing is run to ask about the graphics card.
        let window = ExportWindow { open: true, ffmpeg: None, ..Default::default() };
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(900.0, 900.0))
            .allow_missing_glyphs()
            .with_context_setup(style::apply)
            .build_ui_state(
                move |ui, window: &mut ExportWindow| {
                    let ctx = ui.ctx().clone();
                    window.show(&ctx, &studio.scene, &studio.settings(), None, &studio.kit);
                },
                window,
            );
        h.run();
        h.get_by_label("Encoder list").click();
        h.run();
        assert!(h.query_by_label("CPU (x264)").is_some(), "the list is open");
        h.key_press(egui::Key::Escape);
        h.run();
        assert!(h.state().open, "the first Escape shuts the list only");
        // Escape in a number box leaves the box.
        let typed = h.query_all_by_role(egui::accesskit::Role::SpinButton).next().expect("the compression number");
        typed.click();
        h.run();
        assert!(h.ctx.text_edit_focused(), "the number is being typed");
        h.key_press(egui::Key::Escape);
        h.run();
        assert!(h.state().open, "Escape left the number box only");
        h.key_press(egui::Key::Escape);
        h.run();
        assert!(!h.state().open, "the next one closes the window");
    }

    #[test]
    fn the_lighting_choice_reaches_the_export() {
        let mut window = ExportWindow::default();
        assert_eq!(window.options(Path::new("a.mp4"), "ffmpeg".into()).bake, None, "as the card says");
        window.bake = Some(true);
        assert_eq!(window.options(Path::new("a.mp4"), "ffmpeg".into()).bake, Some(true));
        let card = bpe_kit::modules::lighting::LightingSettings::default();
        let summary = bake_summary(&card);
        assert!(summary.starts_with("16 samples a pixel · motion blur"), "{summary}");
        let traced = bpe_kit::modules::lighting::LightingSettings {
            bake_bounce: bpe_kit::modules::lighting::BakeBounce::Traced,
            ..card
        };
        assert!(bake_summary(&traced).contains("2 traced light bounces"));
    }

    #[test]
    fn export_failures_read_as_plain_advice_with_the_log_kept() {
        let cases = [
            (
                "ffmpeg failed (exit status: 1): Error opening output file C:/v/a.mp4: Permission denied",
                "open in a player",
            ),
            ("can't write C:/v/a.mp4: being used by another process. (os error 32)", "open in a player"),
            ("ffmpeg failed: No space left on device", "disk is full"),
            ("could not start ffmpeg (C:/ff/ffmpeg.exe). Install ffmpeg", "start ffmpeg"),
            ("ffmpeg failed: Unknown encoder 'libx264'", "can't make H.264"),
            ("Out of memory while allocating a buffer", "ran out of memory"),
            ("the frame did not come back from the graphics card", "stopped while drawing"),
            ("the graphics card reported a problem: Validation Error", "stopped while drawing"),
            ("no suitable adapter found", "find a graphics card"),
            (STOPPED_UNEXPECTEDLY, "stopped unexpectedly"),
            ("something odd", "Something went wrong"),
        ];
        for (raw, expected) in cases {
            let explained = explain_export_error(raw);
            assert!(explained.headline.contains(expected), "{raw:?} gave {:?}", explained.headline);
            assert_eq!(explained.details, raw, "the log stays behind Details");
        }
    }

    #[test]
    fn export_choices_are_remembered_and_odd_lines_ignored() {
        let dir = std::env::temp_dir().join(format!("ball-studio-export-choices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fresh = ExportWindow::load_in(&dir);
        assert_eq!((fresh.scale, fresh.crf, fresh.encoder), (1.0, 18, Encoder::Auto), "the defaults");
        let mut window = fresh;
        window.scale = 0.5;
        window.crf = 22;
        window.encoder = Encoder::Cpu;
        window.stems = true;
        window.midi = true;
        let ffmpeg = dir.join("ffmpeg.exe");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&ffmpeg, "").unwrap();
        window.ffmpeg = Some(ffmpeg.clone());
        window.remember_if_changed();
        let back = ExportWindow::load_in(&dir);
        assert_eq!(back.remembered_now(), window.remembered_now());
        std::fs::remove_file(&ffmpeg).unwrap();
        assert_ne!(ExportWindow::load_in(&dir).ffmpeg, Some(ffmpeg), "an ffmpeg that has gone is looked for again");
        std::fs::write(dir.join(EXPORT_CHOICES), "scale=0.3\ncrf=99\nencoder=potato\nnonsense\n").unwrap();
        let odd = ExportWindow::load_in(&dir);
        assert_eq!((odd.scale, odd.crf, odd.encoder), (1.0, 28, Encoder::Auto));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_window_without_a_settings_folder_remembers_nothing() {
        let mut window = ExportWindow { crf: 20, ..Default::default() };
        window.remember_if_changed();
        assert_eq!(window.remembered, None);
    }

    #[test]
    fn a_job_whose_thread_stopped_ends_as_a_failure_with_news() {
        let mut window = ExportWindow::default();
        let (sender, receiver) = channel();
        drop(sender);
        window.job = Some(Job {
            path: PathBuf::from("a.mp4"),
            done: Arc::new(AtomicU32::new(3)),
            total: Arc::new(AtomicU32::new(12)),
            cancel: Arc::default(),
            receiver,
            started: Instant::now(),
        });
        assert_eq!(window.progress_percent(), Some(25));
        let kit = Arc::new(bpe_kit::kit());
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let settings = SceneSettings::from_params(&resolved.things[&ThingId::ROOT].params);
        window.show(&egui::Context::default(), &scene, &settings, None, &kit);
        assert!(!window.busy(), "no longer looks busy forever");
        match window.take_news() {
            Some(ExportNews::Failed { headline }) => assert!(headline.contains("stopped unexpectedly"), "{headline}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(window.take_news(), None, "told once");
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(minutes(42.4), "42 s");
        assert_eq!(minutes(75.0), "1:15 min");
        assert_eq!(minutes(125_962.0), "34 h 59 min");
    }

    #[test]
    fn a_new_scene_exports_beside_its_file_or_in_videos() {
        let window = ExportWindow::default();
        let kit = bpe_kit::kit();
        let scene = bpe_kit::new_scene(&kit.registry);
        let beside = window.output_path(&scene, Some(Path::new("/scenes")));
        assert_eq!(beside, PathBuf::from("/scenes/New scene.mp4"));
        let alone = window.output_path(&scene, None);
        assert_eq!(alone.file_name().unwrap(), "New scene.mp4");
    }

    #[test]
    fn closing_while_exporting_cancels_and_removes_the_file() {
        let mut window = ExportWindow::default();
        if window.ffmpeg.is_none() {
            eprintln!("skipped: no ffmpeg");
            return;
        }
        window.scale = 0.25;
        let kit = Arc::new(bpe_kit::kit());
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let settings = SceneSettings::from_params(&resolved.things[&ThingId::ROOT].params);
        let dir = std::env::temp_dir().join(format!("bpe-next-close-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = egui::Context::default();
        window.start(&ctx, &Subject::Scene { scene: &scene, settings: &settings }, Some(dir.as_path()), &kit, None);
        assert!(window.busy());
        let path = dir.join("New scene.mp4");
        // Let it get going (or fail for want of a graphics card), then close the editor.
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(20) {
            if window.job.as_ref().unwrap().done.load(Ordering::Relaxed) > 5 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let done = window.job.as_ref().unwrap().done.load(Ordering::Relaxed);
        window.shutdown();
        eprintln!("frames done before closing: {done}");
        assert!(!window.busy());
        assert!(!path.exists(), "{} was left behind", path.display());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
