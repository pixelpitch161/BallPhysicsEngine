//! The Scenes window's pictures: each built-in scene's still poster and its short looping preview clip, drawn on
//! one background thread with its own graphics device the way export draws them, and handed to the window as
//! textures.
//!
//! Posters are kept for the rest of the session. Clips are kept for the last [`CLIPS_KEPT`] scenes asked for; a
//! newer clip ask goes first, and one cut short starts again from the beginning when it is asked for again.
//! Closing the window ends the thread (it has nothing to draw while the editor is in use) and the next ask starts
//! a new one.

use bpe_gfx::wgpu;
use bpe_kit::Kit;
use bpe_kit::library::{self, Entry};
use eframe::egui;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Condvar, Mutex};

/// A poster's share of the video's size (1080 x 1920 → 180 x 320).
pub const POSTER_SCALE: f32 = 1.0 / 6.0;
/// A clip frame's share of the video's size (1080 x 1920 → 216 x 384).
pub const CLIP_SCALE: f32 = 0.2;
/// A clip's frames a second.
pub const CLIP_FPS: u32 = 24;
/// How many scenes' clips are kept, the oldest asked for dropped first.
pub const CLIPS_KEPT: usize = 3;

/// A scene's preview clip, as far as it has got.
pub enum Clip<'a> {
    NotAsked,
    Making {
        done: u32,
        of: u32,
    },
    /// Its frames, [`CLIP_FPS`] a second.
    Ready(&'a [egui::TextureHandle]),
    Failed(&'a str),
}

/// How many frames a scene's clip has.
pub fn clip_frames(entry: &Entry) -> u32 {
    (entry.info.preview_seconds.max(0.0) * CLIP_FPS as f64).ceil() as u32
}

enum Poster {
    Asked,
    Ready(egui::TextureHandle),
    Failed,
}

enum ClipState {
    Making { ticket: u64, frames: Vec<egui::TextureHandle>, of: u32 },
    Ready(Vec<egui::TextureHandle>),
    Failed(String),
}

enum Job {
    Poster(&'static str),
    Clip(&'static str, u64),
}

#[derive(Default)]
struct Queue {
    /// The clip asked for last, until the thread starts it.
    clip: Option<(&'static str, u64)>,
    posters: VecDeque<&'static str>,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    /// Stops the clip being drawn (a newer one was asked for, or the window closed).
    cut: AtomicBool,
    /// Ends the thread at its next frame.
    quit: AtomicBool,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// What the thread sends back.
enum Done {
    /// It has no graphics device.
    NoDevice(String),
    Poster(&'static str, Result<(Vec<u8>, (u32, u32)), String>),
    Frame {
        id: &'static str,
        ticket: u64,
        rgba: Vec<u8>,
        size: (u32, u32),
    },
    ClipEnd {
        id: &'static str,
        ticket: u64,
        result: Result<(), String>,
    },
}

struct Worker {
    shared: Arc<Shared>,
    done: Receiver<Done>,
}

/// Where the pictures come from.
#[cfg_attr(not(test), allow(dead_code))]
enum Source {
    /// A device of its own, made on the thread at the first ask.
    Own,
    /// A device given to it (tests).
    Given(wgpu::Device, wgpu::Queue),
    /// None: nothing is drawn (no graphics device on this computer, or tests that don't draw).
    Missing,
}

pub struct Pictures {
    source: Source,
    /// Why there is no device, once the thread found none.
    missing: Option<String>,
    worker: Option<Worker>,
    posters: HashMap<&'static str, Poster>,
    clips: HashMap<&'static str, ClipState>,
    /// The clips asked for, oldest first.
    clip_order: VecDeque<&'static str>,
    tickets: u64,
}

impl Default for Pictures {
    fn default() -> Self {
        Pictures::with_source(Source::Own)
    }
}

impl Pictures {
    fn with_source(source: Source) -> Self {
        Pictures {
            source,
            missing: None,
            worker: None,
            posters: HashMap::new(),
            clips: HashMap::new(),
            clip_order: VecDeque::new(),
            tickets: 0,
        }
    }

    /// Pictures that never come: every ask is ignored and [`Pictures::device_missing`] is true.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn none() -> Self {
        Pictures::with_source(Source::Missing)
    }

    /// Pictures drawn with this device (tests).
    #[cfg(test)]
    pub fn with_device(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Pictures::with_source(Source::Given(device, queue))
    }

    /// No picture can be drawn on this computer.
    pub fn device_missing(&self) -> bool {
        matches!(self.source, Source::Missing) || self.missing.is_some()
    }

    fn worker(&mut self, kit: &Arc<Kit>, ctx: &egui::Context) -> Option<&Worker> {
        if self.device_missing() {
            return None;
        }
        if self.worker.is_none() {
            let shared = Arc::new(Shared::default());
            let (send, done) = channel();
            let device = match &self.source {
                Source::Given(device, queue) => Some((device.clone(), queue.clone())),
                _ => None,
            };
            let (thread_shared, kit, ctx) = (shared.clone(), kit.clone(), ctx.clone());
            let started = std::thread::Builder::new()
                .name("ball-studio scene pictures".into())
                .spawn(move || work(thread_shared, kit, device, send, ctx));
            if let Err(error) = started {
                self.missing = Some(error.to_string());
                return None;
            }
            self.worker = Some(Worker { shared, done });
        }
        self.worker.as_ref()
    }

    /// Asks for the scene's poster, after the ones asked for before it.
    pub fn ask_poster(&mut self, id: &'static str, kit: &Arc<Kit>, ctx: &egui::Context) {
        if self.posters.contains_key(id) || library::find(id).is_none() {
            return;
        }
        let Some(worker) = self.worker(kit, ctx) else { return };
        let shared = worker.shared.clone();
        shared.lock().posters.push_back(id);
        shared.wake.notify_one();
        self.posters.insert(id, Poster::Asked);
    }

    /// Draws the posters of `ids` (the tiles shown) before the others still waiting, in this order.
    pub fn put_first(&mut self, ids: &[&'static str]) {
        let Some(worker) = &self.worker else { return };
        let mut queue = worker.shared.lock();
        let (mut first, rest): (VecDeque<_>, VecDeque<_>) = queue.posters.drain(..).partition(|id| ids.contains(id));
        first.make_contiguous().sort_by_key(|id| ids.iter().position(|x| x == id));
        first.extend(rest);
        queue.posters = first;
    }

    /// Asks for the scene's preview clip, before anything else; a clip being drawn for another scene stops.
    pub fn ask_clip(&mut self, id: &'static str, kit: &Arc<Kit>, ctx: &egui::Context) {
        let Some(entry) = library::find(id) else { return };
        if self.clips.contains_key(id) {
            self.clip_order.retain(|x| *x != id);
            self.clip_order.push_back(id);
            return;
        }
        let Some(worker) = self.worker(kit, ctx) else { return };
        let shared = worker.shared.clone();
        self.tickets += 1;
        let ticket = self.tickets;
        {
            let mut queue = shared.lock();
            // Any clip still being made is either drawing (cut short) or waiting (replaced): it is dropped, to be
            // made again from the start when it is asked for again.
            shared.cut.store(true, Ordering::Relaxed);
            queue.clip = Some((id, ticket));
        }
        shared.wake.notify_one();
        self.clips.retain(|_, c| !matches!(c, ClipState::Making { .. }));
        self.clip_order.retain(|x| self.clips.contains_key(x));
        self.clips.insert(id, ClipState::Making { ticket, frames: Vec::new(), of: clip_frames(entry) });
        self.clip_order.push_back(id);
        self.drop_old_clips();
    }

    /// Keeps the newest [`CLIPS_KEPT`] clips.
    fn drop_old_clips(&mut self) {
        while self.clip_order.len() > CLIPS_KEPT {
            if let Some(old) = self.clip_order.pop_front() {
                self.clips.remove(old);
            }
        }
    }

    /// Turns what the thread drew into textures.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some(worker) = &self.worker else { return };
        let mut ended = false;
        let mut arrived = Vec::new();
        loop {
            match worker.done.try_recv() {
                Ok(done) => arrived.push(done),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    ended = true;
                    break;
                }
            }
        }
        for done in arrived {
            self.take(ctx, done);
        }
        if ended {
            // The thread stopped (it panicked, or found no device): what it was making won't come.
            self.worker = None;
            self.forget_waiting();
        }
    }

    fn take(&mut self, ctx: &egui::Context, done: Done) {
        let texture = |name: String, rgba: &[u8], size: (u32, u32)| {
            let image = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], rgba); // scene colour
            ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
        };
        match done {
            Done::NoDevice(why) => self.missing = Some(why),
            Done::Poster(id, Ok((rgba, size))) => {
                self.posters.insert(id, Poster::Ready(texture(format!("scene poster {id}"), &rgba, size)));
            }
            Done::Poster(id, Err(_)) => {
                self.posters.insert(id, Poster::Failed);
            }
            Done::Frame { id, ticket, rgba, size } => {
                if let Some(ClipState::Making { ticket: t, frames, .. }) = self.clips.get_mut(id)
                    && *t == ticket
                {
                    frames.push(texture(format!("scene clip {id} {}", frames.len()), &rgba, size));
                }
            }
            Done::ClipEnd { id, ticket, result } => {
                if let Some(ClipState::Making { ticket: t, frames, .. }) = self.clips.get_mut(id)
                    && *t == ticket
                {
                    let state = match result {
                        Ok(()) if !frames.is_empty() => ClipState::Ready(std::mem::take(frames)),
                        Ok(()) => ClipState::Failed("the clip has no frames".into()),
                        Err(why) => ClipState::Failed(why),
                    };
                    self.clips.insert(id, state);
                }
            }
        }
    }

    /// Asks still waiting are forgotten, so they are asked again.
    fn forget_waiting(&mut self) {
        self.posters.retain(|_, p| !matches!(p, Poster::Asked));
        self.clips.retain(|_, c| !matches!(c, ClipState::Making { .. }));
        self.clip_order.retain(|x| self.clips.contains_key(x));
    }

    pub fn poster(&self, id: &str) -> Option<&egui::TextureHandle> {
        match self.posters.get(id) {
            Some(Poster::Ready(texture)) => Some(texture),
            _ => None,
        }
    }

    pub fn clip(&self, id: &str) -> Clip<'_> {
        match self.clips.get(id) {
            None => Clip::NotAsked,
            Some(ClipState::Making { frames, of, .. }) => Clip::Making { done: frames.len() as u32, of: *of },
            Some(ClipState::Ready(frames)) => Clip::Ready(frames),
            Some(ClipState::Failed(why)) => Clip::Failed(why),
        }
    }

    /// Whether the thread is drawing or has something waiting.
    pub fn busy(&self) -> bool {
        self.worker.is_some()
            && (self.posters.values().any(|p| matches!(p, Poster::Asked))
                || self.clips.values().any(|c| matches!(c, ClipState::Making { .. })))
    }

    /// Stops drawing and ends the thread; what was waiting is asked for again next time. What was drawn stays.
    pub fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.shared.quit.store(true, Ordering::Relaxed);
            worker.shared.cut.store(true, Ordering::Relaxed);
            worker.shared.wake.notify_all();
        }
        self.forget_waiting();
    }

    /// The thread is running.
    #[cfg(test)]
    pub fn running(&self) -> bool {
        self.worker.is_some()
    }
}

impl Drop for Pictures {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The thread: makes its device, then draws what is asked, a clip before any poster, until told to quit.
fn work(
    shared: Arc<Shared>,
    kit: Arc<Kit>,
    given: Option<(wgpu::Device, wgpu::Queue)>,
    send: Sender<Done>,
    ctx: egui::Context,
) {
    // A complaint from the graphics card fails the picture instead of panicking the thread.
    let gpu_error: Arc<Mutex<Option<String>>> = Arc::default();
    let (device, queue) = match given.map(Ok).unwrap_or_else(bpe_gfx::headless_device) {
        Ok(pair) => pair,
        Err(error) => {
            let _ = send.send(Done::NoDevice(format!("{error:#}")));
            ctx.request_repaint();
            return;
        }
    };
    let seen = gpu_error.clone();
    device.on_uncaptured_error(Arc::new(move |error| {
        seen.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(|| error.to_string());
    }));
    let gpu_problem = || gpu_error.lock().unwrap_or_else(|e| e.into_inner()).take();
    loop {
        let job = {
            let mut queue = shared.lock();
            loop {
                if shared.quit.load(Ordering::Relaxed) {
                    return;
                }
                if let Some((id, ticket)) = queue.clip.take() {
                    shared.cut.store(false, Ordering::Relaxed);
                    break Job::Clip(id, ticket);
                }
                if let Some(id) = queue.posters.pop_front() {
                    break Job::Poster(id);
                }
                queue = shared.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
            }
        };
        match job {
            Job::Poster(id) => {
                let Some(entry) = library::find(id) else { continue };
                let scene = entry.scene(&kit.registry);
                let at = entry.info.poster_seconds;
                let drawn =
                    bpe_cli::final_frame(&device, &queue, &scene, &kit, at, POSTER_SCALE, Some(false), &shared.quit);
                let drawn = match (drawn, gpu_problem()) {
                    (_, Some(problem)) => Err(problem),
                    (Ok((rgba, size, _)), None) => Ok((rgba, size)),
                    (Err(error), None) => Err(format!("{error:#}")),
                };
                if send.send(Done::Poster(id, drawn)).is_err() {
                    return;
                }
            }
            Job::Clip(id, ticket) => {
                let Some(entry) = library::find(id) else { continue };
                let scene = entry.scene(&kit.registry);
                let seconds = entry.info.preview_seconds;
                let drawn = bpe_cli::preview_clip(
                    &device,
                    &queue,
                    &scene,
                    &kit,
                    seconds,
                    CLIP_FPS,
                    CLIP_SCALE,
                    &shared.cut,
                    |_, rgba, size| {
                        let _ = send.send(Done::Frame { id, ticket, rgba, size });
                        ctx.request_repaint();
                    },
                );
                let result = match (drawn, gpu_problem()) {
                    (_, Some(problem)) => Err(problem),
                    (Ok(_), None) => Ok(()),
                    (Err(error), None) => Err(format!("{error:#}")),
                };
                if send.send(Done::ClipEnd { id, ticket, result }).is_err() {
                    return;
                }
            }
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn kit() -> Arc<Kit> {
        Arc::new(bpe_kit::kit())
    }

    #[test]
    fn with_no_device_nothing_is_drawn_and_it_says_so() {
        let (kit, ctx) = (kit(), egui::Context::default());
        let mut pictures = Pictures::none();
        assert!(pictures.device_missing());
        pictures.ask_poster("first-bounce", &kit, &ctx);
        pictures.ask_clip("first-bounce", &kit, &ctx);
        pictures.poll(&ctx);
        assert!(!pictures.running(), "no thread is started");
        assert!(pictures.poster("first-bounce").is_none());
        assert!(matches!(pictures.clip("first-bounce"), Clip::NotAsked));
        assert!(!pictures.busy());
    }

    /// Polls until `done` or a minute passes.
    fn wait(pictures: &mut Pictures, ctx: &egui::Context, done: impl Fn(&Pictures) -> bool) {
        let started = Instant::now();
        while !done(pictures) && started.elapsed() < Duration::from_secs(60) {
            let mut out = ctx.run_ui(Default::default(), |ui| pictures.poll(ui.ctx()));
            out.textures_delta.clear();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_poster_and_a_clip_arrive_and_a_newer_clip_goes_first() {
        let Ok((device, queue)) = bpe_gfx::test_device() else { return };
        let (kit, ctx) = (kit(), egui::Context::default());
        let mut pictures = Pictures::with_device(device, queue);
        pictures.ask_poster("first-bounce", &kit, &ctx);
        wait(&mut pictures, &ctx, |p| p.poster("first-bounce").is_some());
        let poster = pictures.poster("first-bounce").expect("the poster arrived");
        assert_eq!(poster.size(), [180, 320]);

        // A clip asked for and at once replaced by another: only the newer one is made.
        pictures.ask_clip("ball-in-a-circle", &kit, &ctx);
        pictures.ask_clip("first-bounce", &kit, &ctx);
        assert!(matches!(pictures.clip("ball-in-a-circle"), Clip::NotAsked), "the older ask is dropped");
        let of = clip_frames(library::find("first-bounce").unwrap());
        assert!(matches!(pictures.clip("first-bounce"), Clip::Making { done: 0, of: o } if o == of));
        wait(&mut pictures, &ctx, |p| !matches!(p.clip("first-bounce"), Clip::Making { .. }));
        match pictures.clip("first-bounce") {
            Clip::Ready(frames) => {
                assert_eq!(frames.len() as u32, of);
                assert_eq!(frames[0].size(), [216, 384]);
            }
            Clip::Failed(why) => panic!("the clip failed: {why}"),
            _ => panic!("the clip never came"),
        }
        assert!(!pictures.busy());
        pictures.stop();
        assert!(!pictures.running());
        assert!(pictures.poster("first-bounce").is_some(), "what was drawn stays");
    }

    #[test]
    fn only_the_last_few_clips_are_kept() {
        let (kit, ctx) = (kit(), egui::Context::default());
        let mut pictures = Pictures::none();
        let ids: Vec<_> = library::all().iter().map(|e| e.info.id).take(CLIPS_KEPT + 1).collect();
        for id in &ids {
            pictures.clips.insert(id, ClipState::Ready(Vec::new()));
            pictures.clip_order.push_back(id);
        }
        // Asking for a kept clip again only makes it the newest.
        pictures.ask_clip(ids[0], &kit, &ctx);
        assert_eq!(pictures.clip_order.back(), Some(&ids[0]));
        pictures.drop_old_clips();
        assert!(matches!(pictures.clip(ids[1]), Clip::NotAsked), "the oldest asked for goes first");
        assert!(ids.iter().filter(|id| !matches!(pictures.clip(id), Clip::NotAsked)).count() == CLIPS_KEPT);
    }
}
