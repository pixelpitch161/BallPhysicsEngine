//! The preview in Video mode (part 2): the still frame at the playhead, drawn by
//! [`bpe_cli::arranged::still_frame`] on a thread with a graphics device of its own (made once and
//! kept), so the editor never waits for a clip's run-up. A newer ask stops the one being drawn;
//! the last frame that came back stays on show, with a spinner, until the new one is ready.

use crate::app::Studio;
use crate::style;
use bpe_core::video::Video;
use bpe_kit::Kit;
use eframe::egui::{self, RichText, Ui};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};

/// The share of the video's size the still frame is drawn at.
pub const STILL_SCALE: f32 = 0.5;

struct Job {
    video: Box<Video>,
    frame: u32,
    asked: u64,
    stop: Arc<AtomicBool>,
}

/// What the worker sends back: which ask, and the pixels and their size or why not.
type Drawn = (u64, Result<(Vec<u8>, (u32, u32)), String>);

struct Worker {
    jobs: Sender<Job>,
    results: Receiver<Drawn>,
}

/// The still frame and the thread that draws it.
#[derive(Default)]
pub struct Still {
    /// Draw at all (the windowed editor, and tests that ask for it): off, the preview shows the
    /// words only.
    pub enabled: bool,
    worker: Option<Worker>,
    /// What was asked last: the video's revision and the frame, and its number.
    asked: Option<(u64, u32)>,
    asks: u64,
    /// Stops the ask being drawn.
    stop: Arc<AtomicBool>,
    /// The frame on show and the ask it answered.
    shown: Option<(egui::TextureHandle, u64)>,
    problem: Option<String>,
}

impl Still {
    /// Whether a newer frame than the one on show is being drawn.
    pub fn drawing(&self) -> bool {
        self.asked.is_some() && self.shown.as_ref().is_none_or(|(_, n)| *n != self.asks) && self.problem.is_none()
    }

    /// Whether a frame has come back and is on show.
    #[cfg(test)]
    pub fn has_frame(&self) -> bool {
        self.shown.is_some()
    }

    fn worker(&mut self, kit: &Arc<Kit>, ctx: &egui::Context) -> &Worker {
        self.worker.get_or_insert_with(|| start(kit.clone(), ctx.clone()))
    }

    /// Asks for frame `frame` of `video` at `revision`, unless that is what was asked last.
    pub fn ask(&mut self, video: &Video, revision: u64, frame: u32, kit: &Arc<Kit>, ctx: &egui::Context) {
        if self.asked == Some((revision, frame)) {
            return;
        }
        self.stop.store(true, Ordering::Relaxed);
        self.stop = Arc::new(AtomicBool::new(false));
        self.asks += 1;
        self.asked = Some((revision, frame));
        self.problem = None;
        let job = Job { video: Box::new(video.clone()), frame, asked: self.asks, stop: self.stop.clone() };
        if self.worker(kit, ctx).jobs.send(job).is_err() {
            self.worker = None;
            self.problem = Some("drawing the frame stopped unexpectedly".into());
        }
    }

    /// Takes what came back.
    pub fn take(&mut self, ctx: &egui::Context) {
        let Some(worker) = &self.worker else { return };
        loop {
            match worker.results.try_recv() {
                Ok((asked, Ok((pixels, size)))) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], &pixels); // scene colour
                    match &mut self.shown {
                        Some((texture, n)) => {
                            texture.set(image, egui::TextureOptions::LINEAR);
                            *n = asked;
                        }
                        None => {
                            let texture = ctx.load_texture("video still", image, egui::TextureOptions::LINEAR);
                            self.shown = Some((texture, asked));
                        }
                    }
                }
                Ok((asked, Err(problem))) => {
                    if asked == self.asks && problem != "stopped" {
                        self.problem = Some(problem);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.problem = Some("drawing the frame stopped unexpectedly".into());
                    break;
                }
            }
        }
    }
}

impl Drop for Still {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn start(kit: Arc<Kit>, ctx: egui::Context) -> Worker {
    let (jobs, received) = channel::<Job>();
    let (told, results) = channel();
    let _ = std::thread::Builder::new().name("video still".into()).spawn(move || {
        let gpu_error: Arc<Mutex<Option<String>>> = Arc::default();
        let mut device: Option<(bpe_gfx::wgpu::Device, bpe_gfx::wgpu::Queue)> = None;
        while let Ok(mut job) = received.recv() {
            // Only the newest ask is drawn.
            while let Ok(newer) = received.try_recv() {
                job = newer;
            }
            if job.stop.load(Ordering::Relaxed) {
                continue;
            }
            if device.is_none() {
                match bpe_gfx::headless_device() {
                    Ok((d, q)) => {
                        let seen = gpu_error.clone();
                        d.on_uncaptured_error(Arc::new(move |error| {
                            seen.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(|| error.to_string());
                        }));
                        device = Some((d, q));
                    }
                    Err(error) => {
                        let _ = told.send((job.asked, Err(format!("{error:#}"))));
                        ctx.request_repaint();
                        continue;
                    }
                }
            }
            let Some((d, q)) = &device else { continue };
            let drawn = bpe_cli::arranged::still_frame(d, q, &job.video, &kit, job.frame, STILL_SCALE, &job.stop)
                .map_err(|e| format!("{e:#}"));
            let drawn = match gpu_error.lock().unwrap_or_else(|e| e.into_inner()).take() {
                Some(error) => Err(format!("the graphics card reported a problem: {error}")),
                None => drawn,
            };
            if told.send((job.asked, drawn)).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
    Worker { jobs, results }
}

/// The empty video's words and buttons: New video (the open scene as its first clip) and Open.
pub fn empty_hint(ui: &mut Ui, studio: &mut Studio) {
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("Make one video out of scenes, cut on the beat.").color(style::TEXT));
        ui.label(RichText::new("Drop a scene here, or + Add scene").small().color(style::MUTED));
        ui.add_space(8.0);
        let new = ui
            .add(style::primary_button("New video"))
            .on_hover_text("A video with the scene open in Build mode as its first clip");
        if new.clicked() {
            studio.ask_new_video(ui.ctx());
        }
        if ui.button("Open video…").clicked() {
            studio.ask_open(ui.ctx());
        }
    });
}

/// The preview in Video mode.
pub fn show(ui: &mut Ui, studio: &mut Studio) {
    let ctx = ui.ctx().clone();
    let rect = ui.available_rect_before_wrap();
    if studio.video.video.clips.is_empty() {
        ui.add_space((rect.height() * 0.4).max(0.0));
        empty_hint(ui, studio);
        return;
    }
    let s = &studio.video.video.settings;
    let (w, h) = (s.width.max(1) as f32, s.height.max(1) as f32);
    let k = ((rect.width() - 24.0) / w).min((rect.height() - 24.0) / h).max(0.01);
    let picture = egui::Rect::from_center_size(rect.center(), egui::vec2(w * k, h * k));
    let session = &mut studio.video;
    if session.still.enabled {
        let frame = session.frame();
        session.still.ask(&session.video, session.revision, frame, &studio.kit, &ctx);
        session.still.take(&ctx);
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(picture, 0.0, style::GROUND);
    if let Some((texture, _)) = &session.still.shown {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(texture.id(), picture, uv, egui::Color32::WHITE); // scene colour
    }
    if session.still.drawing() {
        let spot = egui::Rect::from_min_size(picture.left_top() + egui::vec2(8.0, 8.0), egui::vec2(18.0, 18.0));
        ui.put(spot, egui::Spinner::new());
    }
    if let Some(problem) = &session.still.problem {
        let text = format!("The frame couldn't be drawn: {problem}");
        painter.text(
            picture.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(13.0),
            style::PROBLEM,
        );
    } else if !session.still.enabled {
        let text = "The picture shows here";
        painter.text(
            picture.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(13.0),
            style::MUTED,
        );
    }
    ui.allocate_rect(rect, egui::Sense::hover());
}
