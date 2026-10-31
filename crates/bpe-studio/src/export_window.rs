//! The "Export video" window: pick a file, the quality and the size, then render the mp4 with
//! its sound on a thread.

use crate::theme;
use bpe_cli::{
    Encoder, ExportOptions, ExportReport, export_video_reporting, find_ffmpeg, frame_count, gpu_encoders, output_size,
};
use bpe_core::Scene;
use bpe_kit::{Kit, SceneSettings};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The graphics card encoders that work, and the ffmpeg they were checked with.
type CheckedEncoders = (PathBuf, Vec<Encoder>);

pub struct ExportWindow {
    pub open: bool,
    /// The file chosen with "Change…". `None` saves next to the scene, named after it.
    path: Option<PathBuf>,
    /// H.264 quality: lower is sharper and bigger.
    crf: u8,
    /// Output size as a fraction of the video size.
    scale: f32,
    encoder: Encoder,
    /// MIDI and stems saved beside the video.
    extras: bpe_kit::Extras,
    ffmpeg: Option<PathBuf>,
    gpu: Arc<Mutex<Option<CheckedEncoders>>>,
    /// The ffmpeg a check was started for.
    checking: Option<PathBuf>,
    job: Option<Job>,
    finished: Option<Result<Finished, String>>,
}

struct Finished {
    path: PathBuf,
    seconds: f32,
    encoder: Encoder,
    /// What the master stage measured of the soundtrack.
    report: Option<bpe_mix::MasterReport>,
    /// The MIDI file and stems saved beside it.
    extras: Vec<PathBuf>,
}

struct Job {
    path: PathBuf,
    done: Arc<AtomicU32>,
    total: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<anyhow::Result<ExportReport>>,
    started: Instant,
}

impl Default for ExportWindow {
    fn default() -> Self {
        Self {
            open: false,
            path: None,
            crf: 18,
            scale: 1.0,
            encoder: Encoder::Auto,
            extras: bpe_kit::Extras::default(),
            ffmpeg: find_ffmpeg(),
            gpu: Arc::default(),
            checking: None,
            job: None,
            finished: None,
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

    /// Stops a running export before the editor closes, so no half-written file is left.
    pub fn shutdown(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
            // The export thread notices at its next frame, stops ffmpeg and deletes the file.
            let _ = job.receiver.recv_timeout(Duration::from_secs(15));
        }
    }

    fn output_path(&self, scene: &Scene, base_dir: Option<&Path>) -> PathBuf {
        self.path.clone().unwrap_or_else(|| {
            let folder = base_dir.map(Path::to_path_buf).unwrap_or_else(videos_dir);
            folder.join(format!("{}.mp4", safe_file_name(&scene.name)))
        })
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        scene: &Scene,
        settings: &SceneSettings,
        base_dir: Option<&Path>,
        kit: &Arc<Kit>,
    ) {
        if let Some(job) = &self.job
            && let Ok(result) = job.receiver.try_recv()
        {
            self.finished = Some(match result {
                Ok(r) => Ok(Finished {
                    path: job.path.clone(),
                    seconds: job.started.elapsed().as_secs_f32(),
                    encoder: r.encoder,
                    report: r.master,
                    extras: r.extras,
                }),
                Err(_) if job.cancel.load(Ordering::Relaxed) => Err("Export cancelled.".into()),
                Err(error) => Err(format!("{error:#}")),
            });
            self.job = None;
        }
        if !self.open {
            return;
        }
        self.check_encoders();
        let mut open = self.open;
        egui::Window::new("Export video")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(460.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(460.0);
                let busy = self.job.is_some();
                ui.add_enabled_ui(!busy, |ui| self.settings(ui, scene, settings, base_dir));
                ui.add_space(10.0);
                self.progress(ui, ctx, scene, settings, base_dir, kit);
            });
        self.open = open;
    }

    fn settings(&mut self, ui: &mut egui::Ui, scene: &Scene, settings: &SceneSettings, base_dir: Option<&Path>) {
        let path = self.output_path(scene, base_dir);
        ui.label(RichText::new("SAVE AS").size(12.0).color(theme::MUTED));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(path.file_name().unwrap_or_default().to_string_lossy()).strong());
                let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                ui.label(RichText::new(format!("in {folder}")).small().color(theme::MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Change…").clicked()
                    && let Some(chosen) = rfd::FileDialog::new()
                        .add_filter("MP4 video", &["mp4"])
                        .set_directory(path.parent().unwrap_or(Path::new(".")))
                        .set_file_name(path.file_name().unwrap_or_default().to_string_lossy())
                        .save_file()
                {
                    self.path = Some(chosen);
                }
            });
        });
        if path.exists() {
            ui.label(
                RichText::new("A video with this name already exists and will be replaced.")
                    .small()
                    .color(theme::WARNING),
            );
        }

        ui.add_space(8.0);
        ui.label(RichText::new("SIZE AND QUALITY").size(12.0).color(theme::MUTED));
        egui::Grid::new("export-settings").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Size").on_hover_text("A smaller draft exports much faster, for checking timing.");
            ui.horizontal(|ui| {
                for (scale, name) in [(1.0, "Full"), (0.5, "Half"), (0.25, "Quarter")] {
                    ui.radio_value(&mut self.scale, scale, name);
                }
                let (w, h) = output_size(settings.width.max(1), settings.height.max(1), self.scale);
                ui.label(RichText::new(format!("{w} × {h}")).small().color(theme::MUTED));
            });
            ui.end_row();
            let compression = ui
                .label("Compression")
                .on_hover_text("14 is the sharpest and biggest, 18 looks lossless, 28 is small and soft.");
            ui.add(egui::Slider::new(&mut self.crf, 14..=28)).labelled_by(compression.id);
            ui.end_row();
            ui.label("Length");
            let frames = frame_count(settings);
            ui.label(
                RichText::new(format!("{} s at {} fps · {frames} frames", settings.duration, settings.fps))
                    .color(theme::MUTED),
            );
            ui.end_row();
            ui.label("Encoder").on_hover_text(
                "The graphics card can compress the video much faster than the CPU. The picture going in \
                 is exactly the same; files can be a little bigger.",
            );
            match self.gpu_encoders() {
                None if self.ffmpeg.is_some() => {
                    ui.label(RichText::new("Checking your graphics card…").small().color(theme::MUTED));
                }
                found => {
                    let mut choices = vec![Encoder::Auto, Encoder::Cpu];
                    choices.extend(found.unwrap_or_default());
                    if !choices.contains(&self.encoder) {
                        self.encoder = Encoder::Auto;
                    }
                    egui::ComboBox::from_id_salt("export-encoder")
                        .selected_text(self.encoder.name())
                        .width(240.0)
                        .show_ui(ui, |ui| {
                            for encoder in choices {
                                ui.selectable_value(&mut self.encoder, encoder, encoder.name());
                            }
                        });
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
        ui.add_space(8.0);
        ui.label(RichText::new("ALSO SAVE").size(12.0).color(theme::MUTED));
        ui.checkbox(&mut self.extras.midi, "MIDI file")
            .on_hover_text("Also save the notes and chords as a .mid next to the video");
        ui.checkbox(&mut self.extras.stems, "Stems")
            .on_hover_text("Also save each sound, bus and the song as its own WAV in a folder");
        if self.extras.stems {
            ui.indent("stems", |ui| {
                ui.checkbox(&mut self.extras.wet, "Stems include sends")
                    .on_hover_text("Each sound's stem includes its reverb and delay sends");
            });
        }
        if self.ffmpeg.is_none() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Ball Studio needs the free program ffmpeg to make videos, and couldn't find it. \
                     Install it, or point to ffmpeg.exe with Choose…",
                )
                .color(theme::WARNING),
            );
        }
    }

    fn progress(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        scene: &Scene,
        settings: &SceneSettings,
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
        let export = egui::Button::new(RichText::new("Export").strong().color(egui::Color32::WHITE))
            .fill(theme::ACCENT)
            .min_size(egui::vec2(120.0, 32.0));
        if ui.add_enabled(self.ffmpeg.is_some(), export).clicked() {
            self.start(ctx, scene, settings, base_dir, kit);
        }
        match &self.finished {
            Some(Ok(done)) => {
                ui.add_space(6.0);
                let name = done.path.file_name().unwrap_or_default().to_string_lossy();
                ui.label(RichText::new(format!("Saved {name} in {}.", minutes(done.seconds))).color(theme::TEXT));
                ui.label(
                    RichText::new(format!("Encoded with the {}.", done.encoder.name())).small().color(theme::MUTED),
                );
                if let Some(text) = also_saved(&done.extras) {
                    ui.label(RichText::new(text).small().color(theme::MUTED));
                }
                if let Some(r) = &done.report {
                    ui.label(
                        RichText::new(format!("Loudness {:.1} LUFS · True peak {:.1} dBTP", r.integrated, r.true_peak))
                            .small()
                            .color(theme::MUTED),
                    )
                    .on_hover_text("Measured on the finished soundtrack, after the master stage");
                }
                ui.horizontal(|ui| {
                    if ui.button("Play video").clicked() {
                        open_file(&done.path);
                    }
                    if ui.button("Show in folder").clicked() {
                        show_in_folder(&done.path);
                    }
                });
            }
            Some(Err(message)) => {
                ui.add_space(6.0);
                ui.label(RichText::new(message).color(theme::ERROR));
            }
            None => {}
        }
    }

    fn start(
        &mut self,
        ctx: &egui::Context,
        scene: &Scene,
        settings: &SceneSettings,
        base_dir: Option<&Path>,
        kit: &Arc<Kit>,
    ) {
        let Some(ffmpeg) = self.ffmpeg.clone() else { return };
        let path = self.output_path(scene, base_dir);
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let options = self.options(&path, ffmpeg);
        let (scene, kit, ctx) = (scene.clone(), kit.clone(), ctx.clone());
        let (done, total, cancel) = (
            Arc::new(AtomicU32::new(0)),
            Arc::new(AtomicU32::new(frame_count(settings))),
            Arc::new(AtomicBool::new(false)),
        );
        let (sender, receiver) = channel();
        let (d, t, c) = (done.clone(), total.clone(), cancel.clone());
        std::thread::spawn(move || {
            let result = bpe_gfx::headless_device().and_then(|(device, queue)| {
                export_video_reporting(&device, &queue, &scene, &kit, &options, |frame, frames| {
                    d.store(frame, Ordering::Relaxed);
                    t.store(frames, Ordering::Relaxed);
                    ctx.request_repaint();
                    !c.load(Ordering::Relaxed)
                })
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        self.finished = None;
        self.job = Some(Job { path, done, total, cancel, receiver, started: Instant::now() });
    }
}

impl ExportWindow {
    /// The export settings the window's choices make, for a video at `path`.
    fn options(&self, path: &Path, ffmpeg: PathBuf) -> ExportOptions {
        let mut options = ExportOptions::new(path);
        options.scale = self.scale;
        options.crf = self.crf;
        options.preset = if self.scale < 1.0 { "veryfast".into() } else { "medium".into() };
        options.ffmpeg = ffmpeg;
        options.encoder = self.encoder;
        options.extras = self.extras;
        options
    }
}

/// "Also saved MIDI and 5 stems", or `None` when nothing else was saved.
pub(crate) fn also_saved(extras: &[PathBuf]) -> Option<String> {
    let midi = extras.iter().any(|p| p.extension().is_some_and(|e| e == "mid"));
    let stems = extras.iter().filter(|p| p.extension().is_some_and(|e| e == "wav")).count();
    let stems_text = match stems {
        0 => None,
        1 => Some("1 stem".to_string()),
        n => Some(format!("{n} stems")),
    };
    match (midi, stems_text) {
        (false, None) => None,
        (true, None) => Some("Also saved a MIDI file.".into()),
        (false, Some(s)) => Some(format!("Also saved {s}.")),
        (true, Some(s)) => Some(format!("Also saved MIDI and {s}.")),
    }
}

pub fn minutes(seconds: f32) -> String {
    let seconds = seconds.max(0.0).round() as u32;
    if seconds < 60 { format!("{seconds} s") } else { format!("{}:{:02} min", seconds / 60, seconds % 60) }
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
    fn durations_read_naturally() {
        assert_eq!(minutes(42.4), "42 s");
        assert_eq!(minutes(75.0), "1:15 min");
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
    fn export_shows_loudness() {
        use egui_kittest::{Harness, kittest::Queryable};
        let kit = Arc::new(bpe_kit::kit());
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let settings = SceneSettings::from_params(&resolved.things[&ThingId::ROOT].params);
        let mut window = ExportWindow { open: true, ..Default::default() };
        let report = bpe_mix::MasterReport { integrated: -14.04, true_peak: -1.0, ..Default::default() };
        window.finished = Some(Ok(Finished {
            path: PathBuf::from("/videos/a.mp4"),
            seconds: 3.0,
            encoder: Encoder::Cpu,
            report: Some(report),
            extras: vec![
                "/videos/a.mid".into(),
                "/videos/a-stems/01 Lead.wav".into(),
                "/videos/a-stems/02 Song.wav".into(),
            ],
        }));
        let mut h = Harness::builder()
            .with_size(egui::vec2(600.0, 700.0))
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(move |ui, w: &mut ExportWindow| w.show(ui.ctx(), &scene, &settings, None, &kit), window);
        h.run();
        h.get_by_label("Loudness -14.0 LUFS · True peak -1.0 dBTP");
        h.get_by_label("Also saved MIDI and 2 stems.");
    }

    #[test]
    fn export_offers_midi_and_stems() {
        use egui_kittest::{Harness, kittest::Queryable};
        let kit = Arc::new(bpe_kit::kit());
        let scene = bpe_kit::new_scene(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let settings = SceneSettings::from_params(&resolved.things[&ThingId::ROOT].params);
        let window = ExportWindow { open: true, ..Default::default() };
        let mut h = Harness::builder()
            .with_size(egui::vec2(600.0, 900.0))
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(move |ui, w: &mut ExportWindow| w.show(ui.ctx(), &scene, &settings, None, &kit), window);
        h.run();
        assert!(h.query_by_label("Stems include sends").is_none(), "only with stems on");
        h.get_by_label("MIDI file").click();
        h.run();
        h.get_by_label("Stems").click();
        h.run();
        h.get_by_label("Stems include sends").click();
        h.run();
        let options = h.state().options(Path::new("/videos/a.mp4"), "ffmpeg".into());
        assert_eq!(options.extras, bpe_kit::Extras { midi: true, stems: true, wet: true });
        assert_eq!(also_saved(&[]), None);
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
        window.start(&ctx, &scene, &settings, Some(&dir), &kit);
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
