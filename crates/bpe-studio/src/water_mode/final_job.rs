//! Make final in the editor (Water mode part 8): the scene's final made on a thread with a
//! graphics device of its own (so the window's frames never wait behind a final's), what the
//! scene's final is now (found in the finals folder by its print, again only when what it needs
//! changes), and the Draft | Final view.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bpe_core::{Resolved, Scene};
use bpe_kit::things::water::draft::WaterSetup;
use bpe_kit::things::water::finals::{Finals, Status, TakeHead, room_needed};
use bpe_kit::things::water::make::{MakeFinal, MakeProgress, gb, this_card};

/// What the stage shows: the draft, or the final.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WaterView {
    #[default]
    Draft,
    Final,
}

/// The shortest time between two looks in the finals folder while the scene is being edited.
const FIND_EVERY: Duration = Duration::from_millis(250);

/// What the scene's final is now.
#[derive(Clone, Debug)]
pub struct Found {
    /// The scene's revision it was found for, and what it needs (`need_of`).
    revision: u64,
    need: u64,
    pub make: MakeFinal,
    pub status: Status,
    /// The water seconds the scene plays, and the video's seconds with slow motion.
    pub length: f64,
    pub video_seconds: f64,
    /// About how many drops the final holds at its fullest, before any make.
    pub drops_about: u32,
    /// The stretches of frames (from, to) slow motion keeps more moments of.
    pub slow: Vec<(u32, u32)>,
}

impl Found {
    /// The frames of the scene's video.
    pub fn video_frames(&self, fps: u32) -> u32 {
        (self.video_seconds * f64::from(fps.max(1))).round().max(1.0) as u32
    }

    /// The room on the disk a make from where it would start takes, about.
    pub fn room(&self) -> u64 {
        let take = self.status.take.as_ref();
        let drops = take.map_or(self.drops_about, |t| t.drops.max(self.drops_about));
        let moments = self.make.moments_from(self.status.restart);
        room_needed(
            moments,
            drops,
            self.make.frames.saturating_sub(self.status.restart),
            take.and_then(|t| t.bytes_a_drop()),
        )
    }

    /// About how long a make takes from where it would start (`None` before any make on this
    /// scene measured it).
    pub fn seconds_to_make(&self) -> Option<f64> {
        let take = self.status.take.as_ref()?;
        Some(take.seconds_a_frame()? * f64::from(self.make.frames.saturating_sub(self.status.restart) + 1))
    }

    /// What making it takes, in words: "about 3 min and 6.2 GB", or "about 6.2 GB" before any
    /// make measured the time.
    pub fn making_words(&self) -> String {
        let room = gb(self.room());
        match self.seconds_to_make() {
            Some(s) => format!("about {} and {room}", crate::export_window::minutes(s as f32)),
            None => format!("about {room}"),
        }
    }
}

/// What a scene's final needs: its physics (the take's print), its frames and the moments its
/// slow motion keeps of each; and the stretches of frames it keeps more of.
fn need_of(make: &MakeFinal) -> (u64, Vec<(u32, u32)>) {
    let mut h = DefaultHasher::new();
    (make.print(), make.frames).hash(&mut h);
    let mut slow = Vec::new();
    let mut run = None;
    for k in 0..=make.frames {
        let keep = if k < make.frames && make.speeds.slowed() { make.speeds.keep_for(k) } else { 1 };
        keep.hash(&mut h);
        match (keep > 1, run) {
            (true, None) => run = Some(k),
            (false, Some(a)) => {
                slow.push((a, k));
                run = None;
            }
            _ => {}
        }
    }
    (h.finish(), slow)
}

/// A Water scene's final in the editor.
pub struct WaterFinal {
    pub finals: Finals,
    pub view: WaterView,
    pub job: Option<FinalJob>,
    pub found: Option<Found>,
    /// Look in the finals folder again (a make ended, a final was cleared).
    pub stale: bool,
    last_find: Option<Instant>,
    pub picture: super::final_view::FinalPicture,
    /// Export once the make in hand is done (Make final and export).
    pub export_after: bool,
    /// How the last make ended when it didn't finish (the status line says it).
    pub problem: Option<String>,
    /// The finals folder's size and when it was measured (the Finals menu).
    size: Option<(Instant, u64)>,
    /// The last estimate of the drops: what it was worked out from, and the drops.
    estimate: Option<(u64, u32)>,
    /// While the Final plays a scene with Un-mix: the video's time on its clock and the water time
    /// that gave the playhead (a playhead set anywhere else starts the clock again from it).
    pub clock: Option<(f64, f64)>,
}

impl WaterFinal {
    pub fn new(finals: Finals) -> WaterFinal {
        WaterFinal {
            finals,
            view: WaterView::Draft,
            job: None,
            found: None,
            stale: true,
            last_find: None,
            picture: Default::default(),
            export_after: false,
            problem: None,
            size: None,
            estimate: None,
            clock: None,
        }
    }

    /// Finds the final of `scene` as it is at `revision` (at most every [`FIND_EVERY`] while it
    /// keeps changing, and in the finals folder only when what it needs changed). `draft`: the
    /// draft's setup, for the estimate of the drops. Returns whether a find is still due.
    pub fn find(&mut self, revision: u64, scene: &Scene, resolved: &Resolved, draft: Option<&WaterSetup>) -> bool {
        if scene.sim != bpe_core::SimKind::Water {
            self.found = None;
            return false;
        }
        let current = self.found.as_ref().is_some_and(|f| f.revision == revision);
        if current && !self.stale {
            return false;
        }
        if !self.stale && self.last_find.is_some_and(|t| t.elapsed() < FIND_EVERY) {
            return true;
        }
        self.last_find = Some(Instant::now());
        let root = resolved.things.get(&bpe_core::ThingId::ROOT);
        let settings = root.map(|r| <bpe_kit::SceneSettings as bpe_core::Settings>::from_params(&r.params));
        let length = f64::from(settings.unwrap_or_default().duration.max(0.0));
        let make = MakeFinal::of(scene, resolved, length);
        let (need, slow) = need_of(&make);
        let video_seconds = make.speeds.video_length(length);
        if !self.stale
            && let Some(found) = &mut self.found
            && found.need == need
        {
            (found.revision, found.make, found.length, found.video_seconds) = (revision, make, length, video_seconds);
            found.slow = slow;
            return false;
        }
        self.stale = false;
        let status = make.status(&self.finals, this_card());
        let drops_about = match (&status.take, draft) {
            (Some(take), _) if status.own => take.drops,
            (_, Some(draft)) => {
                // (The course laid out at the draft's cell only when the draft changed.)
                let mut h = DefaultHasher::new();
                (draft.print, make.setup.cell.to_bits(), length.to_bits(), make.most).hash(&mut h);
                let key = h.finish();
                match self.estimate {
                    Some((k, drops)) if k == key => drops,
                    _ => {
                        let field = draft.field();
                        let finer = f64::from(draft.cell / make.setup.cell.max(0.5)).powi(2);
                        let about = f64::from(draft.drops_about(&field, length)) * finer;
                        let drops = about.min(f64::from(make.most)) as u32;
                        self.estimate = Some((key, drops));
                        drops
                    }
                }
            }
            _ => 0,
        };
        self.found = Some(Found { revision, need, make, status, length, video_seconds, drops_about, slow });
        false
    }

    /// Starts making the scene's final (nothing if one is being made or none is found).
    pub fn start_make(&mut self, ctx: Option<&eframe::egui::Context>) {
        if self.job.is_some() {
            return;
        }
        let Some(found) = &self.found else { return };
        self.problem = None;
        self.job = Some(FinalJob::start(found.make.clone(), self.finals.clone(), ctx.cloned()));
    }

    /// Stops the make at its next frame (its whole chunks are kept), and the export after it.
    pub fn stop_make(&mut self) {
        self.export_after = false;
        if let Some(job) = &self.job {
            job.stop();
        }
    }

    /// Follows the make: `Some(true)` the frame it finished, `Some(false)` the frame it stopped
    /// (whatever it had made by then is kept) or failed (the reason in `problem`).
    pub fn follow(&mut self) -> Option<bool> {
        let ended = self.job.as_mut()?.ended()?;
        let stopped = self.job.as_ref().is_some_and(FinalJob::stopping);
        self.job = None;
        self.stale = true;
        self.size = None;
        match ended {
            Ok(_) if stopped => Some(false),
            Ok(_) => Some(true),
            Err(_) if stopped => Some(false),
            Err(why) => {
                self.problem = Some(why);
                Some(false)
            }
        }
    }

    /// Removes the scene's own final.
    pub fn clear_this(&mut self) -> anyhow::Result<()> {
        let Some(found) = &self.found else { return Ok(()) };
        let print = found.make.print();
        self.finals.clear(print)?;
        self.stale = true;
        self.size = None;
        Ok(())
    }

    /// Removes every final; gives back the bytes it freed.
    pub fn clear_all(&mut self) -> anyhow::Result<u64> {
        let freed = self.finals.clear_all()?;
        self.stale = true;
        self.size = None;
        Ok(freed)
    }

    /// The finals folder's size (measured again at most every two seconds).
    pub fn size(&mut self) -> u64 {
        if let Some((at, size)) = self.size
            && at.elapsed() < Duration::from_secs(2)
        {
            return size;
        }
        let size = self.finals.size();
        self.size = Some((Instant::now(), size));
        size
    }

    /// Whether the make in hand is of the scene as it is now (an edit while it makes leaves it
    /// making the final of the scene as it was).
    pub fn making_this(&self) -> bool {
        match (&self.job, &self.found) {
            (Some(job), Some(found)) => job.print == found.make.print(),
            _ => false,
        }
    }

    /// What the status line says of the final: its words and whether they tell a problem.
    pub fn words(&self) -> Option<(String, bool)> {
        if let Some(job) = &self.job {
            let percent = (job.progress().share() * 100.0).floor() as u32;
            let left = job.seconds_left().map(|s| format!(" · about {} left", crate::export_window::minutes(s)));
            let doing = match (job.building(), self.making_this()) {
                (true, _) => "· Making the final: laying out the course",
                (false, true) => "· Making the final:",
                (false, false) => "· Making the final of the scene as it was:",
            };
            return Some(match job.building() {
                true => (doing.to_owned(), false),
                false => (format!("{doing} {percent} %{}", left.unwrap_or_default()), false),
            });
        }
        if let Some(why) = &self.problem {
            return Some((format!("· The final stopped: {why}"), true));
        }
        let found = self.found.as_ref()?;
        let status = &found.status;
        let longer = (found.video_seconds - found.length).abs() > 1e-3;
        let video = match found.make.speeds.longer_words().filter(|_| longer) {
            Some(how) => format!(" · video {} s {how}", trim(found.video_seconds)),
            None => String::new(),
        };
        Some((
            match (&status.take, status.own) {
                (Some(take), true) => {
                    let made = (f64::from(status.good.min(found.make.frames)) / 60.0).min(found.length);
                    format!(
                        "· Final: {} drops, {} s of {} made, {}",
                        millions(take.drops),
                        trim(made),
                        trim(found.length),
                        gb(take.bytes())
                    )
                }
                (Some(_), false) => format!(
                    "· Final: right up to {} s, Make final remakes the rest ({})",
                    trim(f64::from(status.good) / 60.0),
                    found.making_words()
                ),
                (None, _) => {
                    format!("· Final: about {} drops, {} (estimate)", millions(found.drops_about), found.making_words())
                }
            } + &video,
            false,
        ))
    }
}

/// "1.4M", "820k", "960".
pub fn millions(n: u32) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{}k", n / 1000),
        _ => format!("{:.1}M", f64::from(n) / 1e6),
    }
}

/// "10", "7.5", "0.25" seconds.
fn trim(seconds: f64) -> String {
    let text = format!("{seconds:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A make on its own thread and graphics device.
pub struct FinalJob {
    /// The print of the take it makes.
    pub print: u64,
    progress: Arc<MakeProgress>,
    stop: Arc<AtomicBool>,
    ended: Receiver<Result<TakeHead, String>>,
    thread: Option<JoinHandle<()>>,
    started: Instant,
}

impl FinalJob {
    /// Starts making `make` into `finals`; `ctx` is asked to draw again as it goes.
    pub fn start(make: MakeFinal, finals: Finals, ctx: Option<eframe::egui::Context>) -> FinalJob {
        let print = make.print();
        let progress = Arc::new(MakeProgress::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, ended) = channel();
        let (theirs, halt) = (progress.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("water final".into())
            .spawn(move || {
                crate::thread_priority::set_this_thread(crate::thread_priority::Priority::Low);
                let gpu_error: Arc<std::sync::Mutex<Option<String>>> = Arc::default();
                let result = bpe_gfx::headless_device().and_then(|(device, queue)| {
                    let seen = gpu_error.clone();
                    device.on_uncaptured_error(Arc::new(move |error| {
                        seen.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(|| error.to_string());
                    }));
                    let gpu = bpe_water::WaterGpu::new(&device, &queue);
                    let repaint = ctx.clone();
                    // (Asks the window to draw again now and then while it makes.)
                    let ticker = std::thread::spawn({
                        let (progress, halt) = (theirs.clone(), halt.clone());
                        move || {
                            while progress.phase.load(Ordering::Acquire) < 2 && !halt.load(Ordering::Acquire) {
                                if let Some(ctx) = &repaint {
                                    ctx.request_repaint();
                                }
                                std::thread::sleep(Duration::from_millis(250));
                            }
                        }
                    });
                    let made = make.make(&gpu, &finals, &theirs, &halt);
                    theirs.phase.store(2, Ordering::Release);
                    let _ = ticker.join();
                    made
                });
                let result = match gpu_error.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    Some(error) => Err(format!("the graphics card reported a problem: {error}")),
                    None => result.map_err(|e| format!("{e:#}")),
                };
                let _ = sender.send(result);
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            })
            .ok();
        FinalJob { print, progress, stop, ended, thread, started: Instant::now() }
    }

    /// A job that makes nothing and stands at `at` of frames `from` to `to` of the take `print`
    /// (tests).
    #[cfg(test)]
    pub fn pretend(
        print: u64,
        from: u32,
        at: u32,
        to: u32,
    ) -> (FinalJob, std::sync::mpsc::Sender<Result<TakeHead, String>>) {
        let progress = Arc::new(MakeProgress::default());
        progress.from.store(from, Ordering::Release);
        progress.at.store(at, Ordering::Release);
        progress.to.store(to, Ordering::Release);
        progress.phase.store(1, Ordering::Release);
        let (sender, ended) = channel();
        let job = FinalJob { print, progress, stop: Arc::default(), ended, thread: None, started: Instant::now() };
        (job, sender)
    }

    pub fn progress(&self) -> &MakeProgress {
        &self.progress
    }

    /// Whether it is still laying out the course at Final detail.
    pub fn building(&self) -> bool {
        self.progress.phase.load(Ordering::Acquire) == 0
    }

    /// The frames made so far: from, at, to.
    pub fn frames(&self) -> (u32, u32, u32) {
        let p = &self.progress;
        (p.from.load(Ordering::Acquire), p.at.load(Ordering::Acquire), p.to.load(Ordering::Acquire))
    }

    /// About how many seconds are left, once enough frames are made to tell.
    pub fn seconds_left(&self) -> Option<f32> {
        let (from, at, to) = self.frames();
        let done = at.saturating_sub(from);
        (done >= 30).then(|| self.started.elapsed().as_secs_f32() / done as f32 * to.saturating_sub(at) as f32)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    /// How it ended, once it has.
    fn ended(&mut self) -> Option<Result<TakeHead, String>> {
        match self.ended.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("making the final stopped unexpectedly".into())),
        }
    }
}

impl Drop for FinalJob {
    /// Stops the make and waits for it to finish the frame it is on (it keeps its whole chunks).
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
