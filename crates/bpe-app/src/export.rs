//! The "Export video" window: pick a file and a quality, then render the mp4 on a thread.

use crate::theme;
use bpe_export::ExportOptions;
use bpe_scene::Scene;
use eframe::egui::{self, DragValue, RichText};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq)]
enum Quality {
    Best,
    Good,
    Draft,
}

impl Quality {
    fn describe(self) -> (&'static str, &'static str) {
        match self {
            Quality::Best => ("Best", "Sharpest picture. Takes the longest."),
            Quality::Good => ("Good", "Looks almost the same as Best, with a smaller file and a faster export."),
            Quality::Draft => ("Quick draft", "Half size and fast, for checking timing before a final export."),
        }
    }

    /// Size scale, H.264 quality (lower is better) and encoder speed.
    fn settings(self) -> (f32, u8, &'static str) {
        match self {
            Quality::Best => (1.0, 16, "slow"),
            Quality::Good => (1.0, 20, "medium"),
            Quality::Draft => (0.5, 24, "veryfast"),
        }
    }
}

pub struct ExportPanel {
    pub open: bool,
    /// The file chosen with "Change…". `None` saves to the Videos folder, named after the scene.
    path: Option<PathBuf>,
    quality: Quality,
    ffmpeg: Option<PathBuf>,
    job: Option<ExportJob>,
    finished: Option<Result<Finished, String>>,
}

struct Finished {
    path: PathBuf,
    seconds: f32,
}

struct ExportJob {
    path: PathBuf,
    done: Arc<AtomicU32>,
    total: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<anyhow::Result<()>>,
    started: Instant,
}

impl ExportPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            path: None,
            quality: Quality::Best,
            ffmpeg: bpe_export::find_ffmpeg(),
            job: None,
            finished: None,
        }
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    fn output_path(&self, scene: &Scene) -> PathBuf {
        self.path.clone().unwrap_or_else(|| videos_dir().join(format!("{}.mp4", safe_file_name(&scene.name))))
    }

    pub fn show(&mut self, ctx: &egui::Context, frame: &eframe::Frame, scene: &mut Scene, base_dir: Option<PathBuf>) {
        if let Some(job) = &self.job
            && let Ok(result) = job.receiver.try_recv()
        {
            self.finished = Some(match result {
                Ok(()) => Ok(Finished { path: job.path.clone(), seconds: job.started.elapsed().as_secs_f32() }),
                Err(error) if job.cancel.load(Ordering::Relaxed) => Err(format!("Export cancelled. {error}")),
                Err(error) => Err(format!("{error:#}")),
            });
            self.job = None;
        }
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
                ui.add_enabled_ui(!busy, |ui| self.settings(ui, scene));
                ui.add_space(10.0);
                self.progress(ui, ctx, frame, scene, base_dir);
            });
        self.open = open;
    }

    fn settings(&mut self, ui: &mut egui::Ui, scene: &mut Scene) {
        let path = self.output_path(scene);
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
        ui.label(RichText::new("QUALITY").size(12.0).color(theme::MUTED));
        for quality in [Quality::Best, Quality::Good, Quality::Draft] {
            let (name, detail) = quality.describe();
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.quality, quality, RichText::new(name).strong());
                ui.label(RichText::new(detail).color(theme::MUTED));
            });
        }

        ui.add_space(6.0);
        if self.ffmpeg.is_none() {
            ui.label(
                RichText::new(
                    "Ball Studio needs the free program ffmpeg to make videos, and couldn't find it. \
                     Install it, or point to ffmpeg.exe under Advanced.",
                )
                .color(theme::WARNING),
            );
        }
        egui::CollapsingHeader::new(RichText::new("Advanced").color(theme::ACCENT_TEXT)).default_open(false).show(
            ui,
            |ui| {
                egui::Grid::new("export-advanced").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    let video = &mut scene.video;
                    ui.label("Video size").on_hover_text(
                        "Width and height in pixels. 1080 × 1920 is the vertical size for TikTok, Reels and Shorts.",
                    );
                    ui.horizontal(|ui| {
                        ui.add(DragValue::new(&mut video.width).range(16..=4096).suffix(" px"));
                        ui.label("×");
                        ui.add(DragValue::new(&mut video.height).range(16..=4096).suffix(" px"));
                        if ui.small_button("Vertical").on_hover_text("1080 × 1920").clicked() {
                            (video.width, video.height) = (1080, 1920);
                        }
                    });
                    ui.end_row();
                    ui.label("Frames per second").on_hover_text("60 looks smoothest. 30 exports twice as fast.");
                    ui.horizontal(|ui| {
                        for fps in [24, 30, 60] {
                            ui.radio_value(&mut video.fps, fps, fps.to_string());
                        }
                    });
                    ui.end_row();
                    ui.label("ffmpeg program");
                    ui.horizontal(|ui| {
                        let text = self.ffmpeg.as_ref().map_or("Not found".into(), |p| p.display().to_string());
                        ui.add(egui::Label::new(RichText::new(text).small()).truncate())
                            .on_hover_text("The program that encodes the mp4.");
                        if ui.button("Choose…").clicked()
                            && let Some(chosen) = rfd::FileDialog::new().set_title("Find ffmpeg").pick_file()
                        {
                            self.ffmpeg = Some(chosen);
                        }
                    });
                    ui.end_row();
                });
            },
        );
    }

    fn progress(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        scene: &Scene,
        base_dir: Option<PathBuf>,
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
            return;
        }
        let export = egui::Button::new(RichText::new("Export").strong().color(egui::Color32::WHITE))
            .fill(theme::ACCENT)
            .min_size(egui::vec2(120.0, 32.0));
        if ui.add_enabled(self.ffmpeg.is_some(), export).clicked() {
            self.start(ctx, frame, scene, base_dir);
        }
        match &self.finished {
            Some(Ok(done)) => {
                ui.add_space(6.0);
                let name = done.path.file_name().unwrap_or_default().to_string_lossy();
                ui.label(RichText::new(format!("Saved {name} in {}.", minutes(done.seconds))).color(theme::TEXT));
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

    fn start(&mut self, ctx: &egui::Context, frame: &eframe::Frame, scene: &Scene, base_dir: Option<PathBuf>) {
        let (Some(render_state), Some(ffmpeg)) = (frame.wgpu_render_state(), self.ffmpeg.clone()) else { return };
        let path = self.output_path(scene);
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let (scale, crf, preset) = self.quality.settings();
        let mut options = ExportOptions::new(&path);
        options.scale = scale;
        options.crf = crf;
        options.preset = preset.into();
        options.ffmpeg = ffmpeg;
        options.base_dir = base_dir;
        let (device, queue, scene, ctx) =
            (render_state.device.clone(), render_state.queue.clone(), scene.clone(), ctx.clone());
        let (done, total, cancel) = (
            Arc::new(AtomicU32::new(0)),
            Arc::new(AtomicU32::new(scene.frame_count())),
            Arc::new(AtomicBool::new(false)),
        );
        let (sender, receiver) = channel();
        let (d, t, c) = (done.clone(), total.clone(), cancel.clone());
        std::thread::spawn(move || {
            let result = bpe_export::export_video(&device, &queue, &scene, &options, |frame, frames| {
                d.store(frame, Ordering::Relaxed);
                t.store(frames, Ordering::Relaxed);
                ctx.request_repaint();
                !c.load(Ordering::Relaxed)
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        self.finished = None;
        self.job = Some(ExportJob { path, done, total, cancel, receiver, started: Instant::now() });
    }
}

pub fn minutes(seconds: f32) -> String {
    let seconds = seconds.max(0.0).round() as u32;
    if seconds < 60 { format!("{seconds} s") } else { format!("{}:{:02} min", seconds / 60, seconds % 60) }
}

/// The user's Videos folder, or their home folder if there is none.
fn videos_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    match home {
        Some(home) if home.join("Videos").is_dir() => home.join("Videos"),
        Some(home) => home,
        None => PathBuf::from("."),
    }
}

/// A scene name with the characters Windows does not allow in file names replaced.
fn safe_file_name(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '-' } else { c }).collect();
    let cleaned = cleaned.trim().trim_end_matches('.');
    if cleaned.is_empty() { "ball-video".into() } else { cleaned.into() }
}

fn open_file(path: &Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

fn show_in_folder(path: &Path) {
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

    #[test]
    fn file_names_are_safe() {
        assert_eq!(safe_file_name("Reference reel"), "Reference reel");
        assert_eq!(safe_file_name("a/b: c?"), "a-b- c-");
        assert_eq!(safe_file_name("  "), "ball-video");
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(minutes(42.4), "42 s");
        assert_eq!(minutes(75.0), "1:15 min");
    }
}
