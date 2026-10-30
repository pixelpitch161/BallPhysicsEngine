//! The "Batch" window: render many seeds of the scene in a folder and keep the best, on a thread,
//! with a card per seed showing how it went.

use crate::export_window::{open_file, safe_file_name, show_in_folder, videos_dir};
use crate::theme;
use bpe_cli::batch::{self, BatchSpec, Manifest, Phase, Progress, Quality, Rejected, Status};
use bpe_cli::find_ffmpeg;
use bpe_core::Scene;
use bpe_gfx::headless_device;
use bpe_kit::Kit;
use bpe_kit::score::Parts;
use eframe::egui::{self, Color32, DragValue, RichText, Ui};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the running batch tells the window where it is.
const TELL_EVERY: Duration = Duration::from_millis(200);
const CARD_WIDTH: f32 = 112.0;

pub struct BatchWindow {
    pub open: bool,
    pub spec: BatchSpec,
    /// Where the videos go.
    pub folder: PathBuf,
    /// The user picked the folder with "Choose…", so opening the window again keeps it.
    chosen_folder: bool,
    /// The scene the batch varies.
    base: Scene,
    /// The batch in the folder, as last loaded or as the run left it.
    manifest: Option<Manifest>,
    /// The folder `manifest` came from.
    looked_at: Option<PathBuf>,
    job: Option<Running>,
    /// A line under the buttons, and whether it is bad news.
    message: Option<(String, bool)>,
    pub ffmpeg: Option<PathBuf>,
}

/// A batch on its thread.
struct Running {
    shared: Arc<Mutex<Shared>>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<anyhow::Result<()>>,
}

struct Shared {
    manifest: Manifest,
    progress: Progress,
}

impl Default for BatchWindow {
    fn default() -> Self {
        Self {
            open: false,
            spec: BatchSpec::default(),
            folder: PathBuf::new(),
            chosen_folder: false,
            base: Scene::default(),
            manifest: None,
            looked_at: None,
            job: None,
            message: None,
            ffmpeg: find_ffmpeg(),
        }
    }
}

impl BatchWindow {
    /// Opens the window on `scene`, varying it by `amount`. The folder is `<Videos>/<scene name>
    /// batch` unless the user chose one.
    pub fn open_with(&mut self, scene: &Scene, amount: f32) {
        if self.job.is_none() {
            self.base = scene.clone();
            self.spec.amount = amount;
            if !self.chosen_folder {
                self.folder = videos_dir().join(format!("{} batch", safe_file_name(&scene.name)));
            }
            self.look_again();
        }
        self.open = true;
    }

    /// Reads the batch in the folder again, if there is one.
    pub fn look_again(&mut self) {
        self.looked_at = None;
        self.message = None;
    }

    /// A batch is running.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Stops a running batch before the editor closes; it can be carried on later.
    pub fn shutdown(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
            let _ = job.receiver.recv_timeout(Duration::from_secs(15));
        }
    }

    /// The batch in the folder has other settings than the column.
    fn different_batch(&self) -> bool {
        self.manifest.as_ref().is_some_and(|m| m.spec != self.spec)
    }

    fn resumable(&self) -> bool {
        self.manifest.as_ref().is_some_and(|m| m.spec == self.spec && m.phase != Phase::Done)
    }

    fn finished(&self) -> bool {
        self.manifest.as_ref().is_some_and(|m| m.spec == self.spec && m.phase == Phase::Done)
    }

    fn load_manifest(&mut self) {
        if self.looked_at.as_ref() == Some(&self.folder) {
            return;
        }
        self.looked_at = Some(self.folder.clone());
        self.manifest = if self.folder.join(batch::MANIFEST).is_file() {
            match Manifest::load(&self.folder) {
                Ok(manifest) => Some(manifest),
                Err(error) => {
                    self.message = Some((format!("Couldn't read the batch in this folder: {error:#}"), true));
                    None
                }
            }
        } else {
            None
        };
    }

    /// Takes the result of a finished run.
    fn poll(&mut self) {
        let Some(job) = &self.job else { return };
        let Ok(result) = job.receiver.try_recv() else { return };
        let job = self.job.take().expect("checked above");
        let shared = job.shared.lock().unwrap();
        self.manifest = Some(shared.manifest.clone());
        self.looked_at = Some(self.folder.clone());
        self.message = Some(match result {
            Ok(()) if job.cancel.load(Ordering::Relaxed) => ("Stopped. Start again to carry on.".to_owned(), false),
            Ok(()) => (summary(&shared.manifest), false),
            Err(error) => (format!("{error:#}"), true),
        });
    }

    fn start(&mut self, ctx: &egui::Context, kit: &Arc<Kit>, weights: Parts) {
        let Some(ffmpeg) = self.ffmpeg.clone() else { return };
        let manifest = match batch::open(&self.folder, &self.spec, &self.base) {
            Ok(manifest) => manifest,
            Err(error) => {
                self.message = Some((format!("{error:#}"), true));
                return;
            }
        };
        self.message = None;
        let shared = Arc::new(Mutex::new(Shared { manifest: manifest.clone(), progress: Progress::default() }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = channel();
        let (folder, kit, ctx, told, stop) =
            (self.folder.clone(), kit.clone(), ctx.clone(), shared.clone(), cancel.clone());
        std::thread::spawn(move || {
            let mut manifest = manifest;
            let result = (|| -> anyhow::Result<()> {
                let (device, queue) = headless_device()?;
                let mut last = Instant::now();
                let mut tell = |manifest: &Manifest, progress: Progress| -> bool {
                    if last.elapsed() >= TELL_EVERY || progress.phase == Phase::Done {
                        let mut shared = told.lock().unwrap();
                        shared.manifest = manifest.clone();
                        shared.progress = progress;
                        last = Instant::now();
                        ctx.request_repaint();
                    }
                    !stop.load(Ordering::Relaxed)
                };
                batch::run(&folder, &mut manifest, &kit, &device, &queue, &ffmpeg, &weights, &mut tell)
            })();
            told.lock().unwrap().manifest = manifest;
            ctx.request_repaint();
            let _ = sender.send(result);
        });
        self.job = Some(Running { shared, cancel, receiver });
    }
}

/// Draws the Batch window when it is open.
pub fn show(ctx: &egui::Context, win: &mut BatchWindow, kit: &Arc<Kit>, weights: Parts) {
    win.poll();
    if !win.open {
        return;
    }
    win.load_manifest();
    let mut open = true;
    let mut start = false;
    egui::Window::new("Batch")
        .open(&mut open)
        .resizable(true)
        .collapsible(false)
        .default_pos(egui::pos2(12.0, theme::BELOW_TOOLBAR))
        .default_size(egui::vec2(860.0, 560.0))
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(300.0);
                    settings_column(ui, win);
                });
                ui.separator();
                ui.vertical(|ui| {
                    start = queue(ui, ctx, win);
                    cards(ui, win);
                });
            });
        });
    if !open {
        win.open = false;
    }
    if start {
        win.start(ctx, kit, weights);
    }
}

fn settings_column(ui: &mut Ui, win: &mut BatchWindow) {
    let running = win.busy();
    ui.add_enabled_ui(!running, |ui| {
        egui::Grid::new("batch-settings").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            let make = ui.label("Videos to make");
            ui.add(DragValue::new(&mut win.spec.count).range(1..=10_000))
                .labelled_by(make.id)
                .on_hover_text("How many videos to render. Each takes about as long as exporting the scene once.");
            ui.end_row();
            let keep = ui.label("Videos to keep");
            win.spec.keep = win.spec.keep.min(win.spec.count);
            ui.add(DragValue::new(&mut win.spec.keep).range(1..=win.spec.count)).labelled_by(keep.id).on_hover_text(
                "How many of the rendered videos to keep, by score; the rest are moved aside or deleted",
            );
            ui.end_row();
            let first = ui.label("First seed");
            ui.add(DragValue::new(&mut win.spec.first_seed))
                .labelled_by(first.id)
                .on_hover_text("The same scene, settings and seeds always make the same videos");
            ui.end_row();
            let candidates = ui.label("Candidates per video");
            ui.add(DragValue::new(&mut win.spec.prescreen).range(1..=50)).labelled_by(candidates.id).on_hover_text(
                "Simulating is much faster than rendering, so the batch simulates this many seeds for every video \
                 and only renders the most promising ones",
            );
            ui.end_row();
            let amount = ui.label("How much to change");
            ui.add(egui::Slider::new(&mut win.spec.amount, 0.0..=1.0).show_value(false))
                .labelled_by(amount.id)
                .on_hover_text(
                    "Left keeps the videos close to your scene; shapes and arrangements only change near the right end",
                );
            ui.end_row();
            let quality = ui.label("Quality");
            egui::ComboBox::from_id_salt("batch-quality")
                .width(110.0)
                .selected_text(win.spec.quality.name())
                .show_ui(ui, |ui| {
                    for q in Quality::ALL {
                        ui.selectable_value(&mut win.spec.quality, q, q.name());
                    }
                })
                .response
                .labelled_by(quality.id)
                .on_hover_text(
                    "Draft is half size and quick, for checking a batch; Good is full size; Best is the slowest and \
                     sharpest",
                );
            ui.end_row();
            let rejected = ui.label("Rejected videos");
            egui::ComboBox::from_id_salt("batch-rejected")
                .width(110.0)
                .selected_text(rejected_name(win.spec.rejected))
                .show_ui(ui, |ui| {
                    for r in [Rejected::Move, Rejected::Delete] {
                        ui.selectable_value(&mut win.spec.rejected, r, rejected_name(r));
                    }
                })
                .response
                .labelled_by(rejected.id)
                .on_hover_text("Move to a folder puts them in a folder called \"rejected\" inside the batch folder");
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut win.spec.extras.midi, "MIDI file")
                .on_hover_text("Also save each video's notes and chords as a .mid next to it");
            ui.checkbox(&mut win.spec.extras.stems, "Stems")
                .on_hover_text("Also save each sound, bus and the song as its own WAV in a folder per video");
            if win.spec.extras.stems {
                ui.checkbox(&mut win.spec.extras.wet, "Stems include sends")
                    .on_hover_text("Each sound's stem includes its reverb and delay sends");
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Folder").strong());
        ui.add(
            egui::Label::new(RichText::new(win.folder.display().to_string()).small().color(theme::MUTED)).truncate(),
        )
        .on_hover_text(win.folder.display().to_string());
        ui.horizontal(|ui| {
            if ui.button("Choose…").on_hover_text("The folder the videos go in").clicked()
                && let Some(folder) = rfd::FileDialog::new().set_title("Folder for the batch").pick_folder()
            {
                win.folder = folder;
                win.chosen_folder = true;
                win.look_again();
            }
            if ui.button("Show in folder").on_hover_text("Open the batch folder").clicked() {
                let _ = std::fs::create_dir_all(&win.folder);
                open_file(&win.folder);
            }
        });
    });
    ui.add_space(8.0);
    ui.label(
        RichText::new(
            "Every seed is a roll of the dice on the scene, honouring the locks. The batch simulates the \
             candidates first, renders the most promising ones and keeps the best by score. A stopped batch \
             carries on where it left off.",
        )
        .small()
        .color(theme::MUTED),
    );
}

fn rejected_name(rejected: Rejected) -> &'static str {
    match rejected {
        Rejected::Move => "Move to a folder",
        Rejected::Delete => "Delete",
    }
}

/// A warning beside the start button, wrapped so it never widens the window.
fn warning(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(theme::WARNING)).wrap());
}

/// The progress bar or the start button; returns whether Start was clicked.
fn queue(ui: &mut Ui, ctx: &egui::Context, win: &mut BatchWindow) -> bool {
    let mut start = false;
    if let Some(job) = &win.job {
        let progress = job.shared.lock().unwrap().progress;
        let (fraction, line) = describe_progress(&progress);
        ui.add(egui::ProgressBar::new(fraction).text(line).desired_width(ui.available_width()));
        ui.horizontal(|ui| {
            if ui.button("Stop").on_hover_text("Stop after this step. Start again later to carry on.").clicked() {
                job.cancel.store(true, Ordering::Relaxed);
            }
            if job.cancel.load(Ordering::Relaxed) {
                ui.label(RichText::new("Stopping…").color(theme::MUTED));
            }
            ui.label(RichText::new("You can keep editing; the batch carries on.").small().color(theme::MUTED));
        });
        ctx.request_repaint_after(TELL_EVERY);
    } else {
        ui.horizontal(|ui| {
            let (resumable, finished, different) = (win.resumable(), win.finished(), win.different_batch());
            let label = if resumable {
                "Carry on"
            } else if finished {
                "Finished"
            } else {
                "Start"
            };
            let button = egui::Button::new(RichText::new(label).strong().color(Color32::WHITE))
                .fill(theme::ACCENT)
                .min_size(egui::vec2(110.0, 30.0));
            let can = win.ffmpeg.is_some() && !different && !finished;
            let response = ui.add_enabled(can, button).on_disabled_hover_text(if finished {
                "This batch is finished. Change a setting or the folder to make another."
            } else if different {
                "Choose another folder, or use the settings of the batch in this one"
            } else {
                "Ball Studio needs ffmpeg to make videos"
            });
            let response = response.on_hover_text(if resumable {
                "Carry on with the stopped batch in this folder"
            } else {
                "Start the batch"
            });
            if response.clicked() {
                start = true;
            }
            if win.ffmpeg.is_none() {
                warning(
                    ui,
                    "Ball Studio needs ffmpeg to make videos and couldn't find it. Point to it in Export video…",
                );
            } else if resumable && win.manifest.as_ref().is_some_and(|m| m.base != win.base) {
                warning(
                    ui,
                    "This batch was started from an earlier version of the scene, and carrying on uses that \
                     version. Choose another folder to batch the scene as it is now.",
                );
            } else if different {
                if ui
                    .button("Use its settings")
                    .on_hover_text("Set everything on the left to the settings of the batch in this folder")
                    .clicked()
                    && let Some(manifest) = &win.manifest
                {
                    win.spec = manifest.spec.clone();
                }
                warning(
                    ui,
                    "This folder holds a batch with other settings. Choose another folder, or use its settings.",
                );
            }
        });
    }
    if let Some((message, bad)) = &win.message {
        ui.label(RichText::new(message).color(if *bad { theme::ERROR } else { theme::TEXT }));
    } else if let Some(manifest) = &win.manifest
        && win.job.is_none()
        && manifest.spec == win.spec
    {
        ui.label(RichText::new(summary(manifest)).color(theme::TEXT));
    }
    start
}

/// A card per seed, in seed order, in a scrolling grid.
fn cards(ui: &mut Ui, win: &BatchWindow) {
    let shown: Option<Manifest> = match &win.job {
        Some(job) => Some(job.shared.lock().unwrap().manifest.clone()),
        None => win.manifest.clone(),
    };
    let Some(manifest) = shown else {
        ui.add_space(8.0);
        ui.label(RichText::new("No batch in this folder yet.").color(theme::MUTED));
        return;
    };
    ui.add_space(6.0);
    let per_row = ((ui.available_width() / (CARD_WIDTH + 8.0)).floor() as usize).max(1);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for row in manifest.jobs.chunks(per_row) {
            ui.horizontal_top(|ui| {
                for job in row {
                    card(ui, &win.folder, job);
                }
            });
        }
    });
}

fn card(ui: &mut Ui, folder: &Path, job: &batch::Job) {
    // The frame's inside follows the row's layout, so the card's lines need a column of their own.
    egui::Frame::new().fill(theme::PANEL).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.set_width(CARD_WIDTH);
            ui.label(RichText::new(format!("Seed {}", job.seed)).strong());
            let colour = match job.status {
                Status::Kept => theme::ACCENT_TEXT,
                Status::Failed => theme::ERROR,
                Status::Rejected | Status::Skipped => theme::MUTED,
                _ => theme::TEXT,
            };
            let status = ui.label(RichText::new(job.status.word()).color(colour));
            if let Some(error) = &job.error {
                status.on_hover_text(error);
            }
            match job.score {
                Some(score) => {
                    _ = ui.label(format!("Score {:.0}", score * 100.0)).on_hover_ui(|ui| {
                        if let Some(parts) = &job.parts {
                            for (name, value, meaning) in parts.list() {
                                ui.label(format!("{name}: {:.0}  {meaning}", value * 100.0));
                            }
                        }
                    })
                }
                None => _ = ui.label(RichText::new("—").color(theme::MUTED)),
            }
            if job.status == Status::Kept
                && let Some(file) = &job.file
            {
                if ui.small_button("Open").on_hover_text("Play the video").clicked() {
                    open_file(&folder.join(file));
                }
                if ui.small_button("Show in folder").clicked() {
                    show_in_folder(&folder.join(file));
                }
            }
        })
    });
}

fn summary(manifest: &Manifest) -> String {
    let screened = manifest.jobs.iter().filter(|j| j.score.is_some()).count();
    let rendered = manifest.jobs.iter().filter(|j| j.file.is_some()).count();
    let kept = manifest.count(Status::Kept);
    match manifest.phase {
        Phase::Prescreen => format!("Tried {screened} of {} seeds", manifest.jobs.len()),
        Phase::Render => format!("Rendered {rendered} of {} videos", manifest.spec.count),
        Phase::Pick => "Picking the best".into(),
        Phase::Done => format!("Finished: kept {kept} of {rendered} videos"),
    }
}

/// How far along the batch is, from 0 to 1, and a line saying so. Trying seeds is the first
/// fifth of the bar, rendering the rest.
fn describe_progress(progress: &Progress) -> (f32, String) {
    let total = progress.total.max(1) as f32;
    match progress.phase {
        Phase::Prescreen => {
            (0.2 * progress.done as f32 / total, format!("Trying seeds: {} of {}", progress.done, progress.total))
        }
        Phase::Render => {
            let within = if progress.frames > 0 { progress.frame as f32 / progress.frames as f32 } else { 0.0 };
            let fraction = 0.2 + 0.8 * (progress.done as f32 + within) / total;
            let line = if progress.frames > 0 {
                format!(
                    "Rendering video {} of {} · frame {} of {}",
                    progress.done + 1,
                    progress.total,
                    progress.frame,
                    progress.frames
                )
            } else {
                format!("Rendered {} of {} videos", progress.done, progress.total)
            };
            (fraction.min(1.0), line)
        }
        Phase::Pick => (1.0, "Picking the best".into()),
        Phase::Done => (1.0, "Finished".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_cli::batch::{Job, Manifest, Phase, Status};
    use bpe_kit::{kit, new_scene};
    use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};

    fn disabled(h: &Harness<'_, BatchWindow>, label: &str) -> bool {
        h.get_by_label(label).accesskit_node().is_disabled()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bpe-batch-window-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn harness(window: BatchWindow) -> Harness<'static, BatchWindow> {
        let kit = Arc::new(kit());
        let mut h = Harness::builder()
            .with_size(egui::vec2(1100.0, 800.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(30)
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(move |ui, w: &mut BatchWindow| show(ui.ctx(), w, &kit, Parts::WEIGHTS), window);
        h.run();
        h
    }

    #[test]
    fn the_batch_window_lists_its_settings_and_a_default_folder() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.name = "One ball".into();
        let mut window = BatchWindow::default();
        window.open_with(&scene, 0.4);
        assert!(window.open);
        assert_eq!(window.spec.amount, 0.4);
        assert!(window.folder.ends_with("One ball batch"), "{}", window.folder.display());
        let h = harness(window);
        for label in [
            "Videos to make",
            "Videos to keep",
            "First seed",
            "Candidates per video",
            "How much to change",
            "Quality",
            "Rejected videos",
            "Folder",
            "Choose…",
            "Start",
        ] {
            assert!(h.query_by_label(label).is_some(), "{label}");
        }
    }

    #[test]
    fn job_cards_show_seed_status_and_score() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let dir = temp_dir("cards");
        let spec = BatchSpec { count: 2, keep: 1, ..BatchSpec::default() };
        let manifest = Manifest {
            spec: spec.clone(),
            base: scene.clone(),
            phase: Phase::Done,
            jobs: vec![
                Job {
                    seed: 1,
                    status: Status::Kept,
                    score: Some(0.8),
                    file: Some("x-seed-1.mp4".into()),
                    ..Job::default()
                },
                Job {
                    seed: 2,
                    status: Status::Rejected,
                    score: Some(0.3),
                    file: Some("x-seed-2.mp4".into()),
                    ..Job::default()
                },
            ],
        };
        manifest.save(&dir).unwrap();
        let mut window = BatchWindow { spec, ..BatchWindow::default() };
        window.open_with(&scene, 0.5);
        window.folder = dir.clone();
        let h = harness(window);
        for label in ["Seed 1", "Kept", "Score 80", "Seed 2", "Rejected", "Score 30"] {
            assert!(h.query_by_label(label).is_some(), "{label}");
        }
        assert_eq!(h.query_all_by_label("Open").count(), 1, "only the kept video can be opened");
        assert!(h.query_by_label_contains("Finished: kept 1 of 2").is_some());
        assert!(h.query_by_label("Finished").is_some(), "the start button says the batch is done");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_batch_window_opens_below_the_toolbar() {
        let kit = kit();
        let mut window = BatchWindow::default();
        window.open_with(&new_scene(&kit.registry), 0.5);
        let h = harness(window);
        let title = h.get_by_label("Batch").rect();
        assert!(title.top() >= crate::theme::BELOW_TOOLBAR, "the window covers the toolbar: {title:?}");
    }

    #[test]
    fn a_kept_card_and_a_long_warning_stay_inside_the_window() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let dir = temp_dir("narrow");
        let spec = BatchSpec { count: 2, keep: 1, ..BatchSpec::default() };
        let kept =
            Job { seed: 1, status: Status::Kept, score: Some(0.8), file: Some("x.mp4".into()), ..Job::default() };
        Manifest { spec, base: scene.clone(), phase: Phase::Done, jobs: vec![kept] }.save(&dir).unwrap();
        let mut window = BatchWindow::default();
        window.open_with(&scene, 0.5);
        window.folder = dir.clone();
        window.ffmpeg = Some("ffmpeg".into());
        let h = harness(window);
        assert!(h.query_by_label_contains("other settings").is_some(), "the folder holds a batch with other settings");
        let width = h.get_by_label("Batch").rect().width();
        assert!(width <= 900.0, "the warning widens the window to {width}");
        let card = h.get_by_label("Seed 1").rect().left();
        let buttons = h.get_all_by_label("Show in folder").map(|b| b.rect()).chain([h.get_by_label("Open").rect()]);
        for button in buttons.filter(|b| b.left() >= card) {
            assert!(button.right() <= card + CARD_WIDTH + 1.0, "{button:?} sticks out of the card at {card}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn many_cards_stack_their_lines_and_wrap_inside_the_window() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let dir = temp_dir("many");
        let spec = BatchSpec { count: 24, keep: 3, ..BatchSpec::default() };
        let jobs =
            (1..=24).map(|seed| Job { seed, status: Status::Skipped, score: Some(0.5), ..Job::default() }).collect();
        Manifest { spec: spec.clone(), base: scene.clone(), phase: Phase::Done, jobs }.save(&dir).unwrap();
        let mut window = BatchWindow { spec, ..BatchWindow::default() };
        window.open_with(&scene, 0.5);
        window.folder = dir.clone();
        let h = harness(window);
        let first = h.get_by_label("Seed 1").rect();
        let status = h.get_all_by_label("Skipped").next().expect("a status word").rect();
        assert!(status.top() >= first.bottom() - 1.0, "a card's lines stack: seed {first:?}, status {status:?}");
        assert!(h.get_by_label("Seed 24").rect().top() > first.bottom(), "the cards wrap onto more rows");
        for seed in 1..=24 {
            let card = h.get_by_label(&format!("Seed {seed}")).rect();
            assert!(card.right() <= 1100.0, "seed {seed} runs off the screen: {card:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn start_is_disabled_without_ffmpeg_or_when_the_folder_holds_another_batch() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let dir = temp_dir("other");
        let mut window = BatchWindow::default();
        window.open_with(&scene, 0.5);
        window.folder = dir.clone();
        window.ffmpeg = None;
        let mut h = harness(window);
        assert!(disabled(&h, "Start"));
        assert!(h.query_by_label_contains("needs ffmpeg").is_some());

        let other = Manifest {
            spec: BatchSpec { count: 7, keep: 3, ..BatchSpec::default() },
            base: scene.clone(),
            phase: Phase::Prescreen,
            jobs: Vec::new(),
        };
        other.save(&dir).unwrap();
        h.state_mut().ffmpeg = Some("ffmpeg".into());
        h.state_mut().look_again();
        h.run();
        assert!(disabled(&h, "Start"));
        assert!(h.query_by_label_contains("other settings").is_some());
        h.get_by_label("Use its settings").click();
        h.run();
        assert_eq!(h.state().spec.count, 7);
        assert!(!disabled(&h, "Carry on"), "the batch in the folder can be carried on");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn carrying_on_a_batch_of_an_earlier_scene_says_so() {
        let kit = kit();
        let earlier = new_scene(&kit.registry);
        let mut now = earlier.clone();
        now.name = "Edited since".into();
        let dir = temp_dir("earlier");
        let spec = BatchSpec { count: 2, keep: 1, ..BatchSpec::default() };
        batch::open(&dir, &spec, &earlier).unwrap();
        let mut window = BatchWindow { spec, ..BatchWindow::default() };
        window.open_with(&now, 0.5);
        window.folder = dir.clone();
        window.ffmpeg = Some("ffmpeg".into());
        let h = harness(window);
        assert!(!disabled(&h, "Carry on"));
        assert!(
            h.query_by_label_contains("earlier version of the scene").is_some(),
            "the window says what Carry on uses"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_small_batch_runs_from_the_window_and_reports() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.name = "Window batch".into();
        let duration = bpe_core::Address {
            owner: bpe_core::Owner::Thing(bpe_core::ThingId::ROOT),
            slot: None,
            key: "duration".into(),
        };
        scene.set_at(&kit.registry, &duration, bpe_core::Value::Number(0.5)).unwrap();
        let dir = temp_dir("run");
        let mut window = BatchWindow {
            spec: BatchSpec { count: 1, keep: 1, prescreen: 2, quality: Quality::Draft, ..BatchSpec::default() },
            ..BatchWindow::default()
        };
        if window.ffmpeg.is_none() || headless_device().is_err() {
            eprintln!("skipped: no ffmpeg or no graphics device");
            return;
        }
        window.open_with(&scene, 0.5);
        window.folder = dir.clone();
        let mut h = harness(window);
        h.get_by_label("Start").click();
        h.run();
        assert!(h.state().busy(), "the batch runs on its thread");
        let mut finished = false;
        for _ in 0..1200 {
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(50));
            if !h.state().busy() {
                finished = true;
                break;
            }
        }
        assert!(finished, "a one-video draft batch finishes within a minute");
        h.run();
        assert!(h.query_by_label_contains("Finished: kept 1 of 1").is_some());
        assert!(h.query_by_label("Finished").is_some(), "the start button says the batch is done");
        assert_eq!(h.query_all_by_label("Open").count(), 1);
        let video =
            std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.path().extension().is_some_and(|x| x == "mp4"));
        assert!(video, "a video in the folder");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn batch_offers_midi_and_stems() {
        let kit = kit();
        let scene = new_scene(&kit.registry);
        let mut window = BatchWindow::default();
        window.open_with(&scene, 0.4);
        let mut h = harness(window);
        h.get_by_label("MIDI file").click();
        h.run();
        h.get_by_label("Stems").click();
        h.run();
        h.get_by_label("Stems include sends").click();
        h.run();
        assert_eq!(h.state().spec.extras, bpe_kit::Extras { midi: true, stems: true, wet: true });
    }
}
