//! The preview in Video mode (part 2): the still frame at the playhead, drawn by
//! [`bpe_cli::arranged::still_frame`] on a thread with a graphics device of its own (made once and
//! kept), so the editor never waits for a clip's run-up. A newer ask stops the one being drawn;
//! the last frame that came back stays on show, with a spinner, until the new one is ready.
//!
//! Part 3: the same thread plays the video ([`Still::play`]): a [`bpe_cli::arranged::Player`]
//! draws frame after frame into a small channel the editor empties as the playhead passes them.

use crate::app::Studio;
use crate::style;
use bpe_core::video::Video;
use bpe_kit::Kit;
use eframe::egui::{self, RichText, Ui};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError, TrySendError, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The share of the video's size the still frame is drawn at.
pub const STILL_SCALE: f32 = 0.5;

/// A played frame: its number, RGBA pixels and size.
pub type Played = (u32, Vec<u8>, (u32, u32));

/// Frames the player may draw ahead of the editor taking them.
pub const PLAYED_AHEAD: usize = 3;

/// The player steps past frames without drawing them once the playhead is more than this many
/// frames ahead of it.
pub const BEHIND: u32 = 2;

/// What the worker is asked to do.
enum Work {
    /// Draw the still frame `frame`.
    Still { frame: u32 },
    /// Play from `from`, sending each frame to `frames`; `clock` is the playhead's frame.
    Play { from: u32, frames: SyncSender<Played>, clock: Arc<AtomicU32> },
}

struct Job {
    video: Box<Video>,
    work: Work,
    asked: u64,
    stop: Arc<AtomicBool>,
}

/// What [`Still::play`] gives the editor: the frames as they are drawn, and the playhead's frame
/// for the player to keep up with. Dropping it stops the playing.
pub struct PlayFeed {
    pub frames: Receiver<Played>,
    pub clock: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
}

impl Drop for PlayFeed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
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
        let job = Job {
            video: Box::new(video.clone()),
            work: Work::Still { frame },
            asked: self.asks,
            stop: self.stop.clone(),
        };
        if self.worker(kit, ctx).jobs.send(job).is_err() {
            self.worker = None;
            self.problem = Some("drawing the frame stopped unexpectedly".into());
        }
    }

    /// Starts playing `video` (tidied) from frame `from` on the worker, stopping any frame being
    /// drawn. Frames come back through the feed; `None` when the worker couldn't be reached.
    pub fn play(&mut self, video: &Video, from: u32, kit: &Arc<Kit>, ctx: &egui::Context) -> Option<PlayFeed> {
        self.stop.store(true, Ordering::Relaxed);
        self.stop = Arc::new(AtomicBool::new(false));
        self.asks += 1;
        // Nothing still is asked while playing: the next still ask draws afresh.
        self.asked = None;
        self.problem = None;
        let (sender, frames) = sync_channel(PLAYED_AHEAD);
        let clock = Arc::new(AtomicU32::new(from));
        let work = Work::Play { from, frames: sender, clock: clock.clone() };
        let job = Job { video: Box::new(video.clone()), work, asked: self.asks, stop: self.stop.clone() };
        if self.worker(kit, ctx).jobs.send(job).is_err() {
            self.worker = None;
            self.problem = Some("playing the video stopped unexpectedly".into());
            return None;
        }
        Some(PlayFeed { frames, clock, stop: self.stop.clone() })
    }

    /// Shows a played frame.
    pub fn show_frame(&mut self, ctx: &egui::Context, pixels: &[u8], size: (u32, u32)) {
        let image = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], pixels); // scene colour
        match &mut self.shown {
            Some((texture, n)) => {
                texture.set(image, egui::TextureOptions::LINEAR);
                *n = self.asks;
            }
            None => {
                let texture = ctx.load_texture("video still", image, egui::TextureOptions::LINEAR);
                self.shown = Some((texture, self.asks));
            }
        }
    }

    /// The playing stopped on `frame` of the video at `revision`, which is on show: the still
    /// frame there needn't be drawn again.
    pub fn shown_from_play(&mut self, revision: u64, frame: u32) {
        if self.shown.is_some() {
            self.asked = Some((revision, frame));
            if let Some((_, n)) = &mut self.shown {
                *n = self.asks;
            }
        }
    }

    /// Takes what came back.
    pub fn take(&mut self, ctx: &egui::Context) {
        let Some(worker) = &self.worker else { return };
        loop {
            match worker.results.try_recv() {
                // An older ask than the frame on show (a still that came back after playing began).
                Ok((asked, Ok(_))) if self.shown.as_ref().is_some_and(|(_, n)| *n > asked) => {}
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
            let frame = match job.work {
                Work::Still { frame } => frame,
                Work::Play { from, frames, clock } => {
                    let played = play(d, q, &job.video, &kit, from, &frames, &clock, &job.stop, &ctx);
                    let played = match gpu_error.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        Some(error) => Err(format!("the graphics card reported a problem: {error}")),
                        None => played,
                    };
                    if let Err(problem) = played {
                        if told.send((job.asked, Err(problem))).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                    }
                    continue;
                }
            };
            let drawn = bpe_cli::arranged::still_frame(d, q, &job.video, &kit, frame, STILL_SCALE, &job.stop)
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

/// The worker's playing: frames from `from` to the end into `frames`, each sent as soon as there
/// is room (the clips coming next are run up while there isn't), the physics stepped past frames
/// the playhead has already passed. Ends at the video's end, when stopped, or when the editor
/// drops the feed; gives "stopped" when stopped.
#[allow(clippy::too_many_arguments)]
fn play(
    device: &bpe_gfx::wgpu::Device,
    queue: &bpe_gfx::wgpu::Queue,
    video: &Video,
    kit: &Kit,
    from: u32,
    frames: &SyncSender<Played>,
    clock: &AtomicU32,
    stop: &AtomicBool,
    ctx: &egui::Context,
) -> Result<(), String> {
    // Huge swarms step on this card. Liquids keep the editor's detail, so the video plays in
    // time (the still frame and the export use the Export detail).
    let _card = bpe_gfx::swarm::use_for_now(Some(bpe_gfx::swarm::SwarmGpu::for_device(device, queue)));
    let scenes = bpe_cli::arranged::clip_scenes(video, kit);
    let mut player = bpe_cli::arranged::Player::new(device, video, &scenes, kit, STILL_SCALE);
    let size = player.size();
    let stopped = || stop.load(Ordering::Relaxed);
    for f in from..player.frame_count() {
        if stopped() {
            return Ok(());
        }
        if f != from && f + BEHIND < clock.load(Ordering::Relaxed) {
            player.skip(f);
            continue;
        }
        let pixels = match player.draw(device, queue, f, stop) {
            Ok(pixels) => pixels,
            Err(_) if stopped() => return Ok(()),
            Err(e) => return Err(format!("{e:#}")),
        };
        let mut item = (f, pixels, size);
        loop {
            match frames.try_send(item) {
                Ok(()) => break,
                Err(TrySendError::Disconnected(_)) => return Ok(()),
                Err(TrySendError::Full(back)) => {
                    item = back;
                    if stopped() {
                        return Ok(());
                    }
                    let began = Instant::now();
                    match player.warm_ahead(device, queue, began + Duration::from_millis(3), stop) {
                        Err(_) if stopped() => return Ok(()),
                        Err(e) => return Err(format!("{e:#}")),
                        Ok(()) => {}
                    }
                    if began.elapsed() < Duration::from_micros(500) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            }
        }
        ctx.request_repaint();
    }
    Ok(())
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
        if !session.playing.on {
            let frame = session.frame();
            session.still.ask(&session.video, session.revision, frame, &studio.kit, &ctx);
        }
        session.still.take(&ctx);
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(picture, 0.0, style::GROUND);
    if let Some((texture, _)) = &session.still.shown {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(texture.id(), picture, uv, egui::Color32::WHITE); // scene colour
    }
    let starting = session.playing.on && session.playing.starting;
    if starting || (!session.playing.on && session.still.drawing()) {
        let spot = egui::Rect::from_min_size(picture.left_top() + egui::vec2(8.0, 8.0), egui::vec2(18.0, 18.0));
        ui.put(spot, egui::Spinner::new());
    }
    if starting {
        painter.text(
            picture.left_top() + egui::vec2(32.0, 17.0),
            egui::Align2::LEFT_CENTER,
            "Getting ready…",
            egui::FontId::proportional(13.0),
            style::TEXT,
        );
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
