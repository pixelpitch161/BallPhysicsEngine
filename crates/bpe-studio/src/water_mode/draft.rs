//! The water draft (Water mode part 2b): a thread that plays a Water scene's water on the graphics
//! card at draft detail and follows the playhead, as the particle playback does for a Balls scene.
//!
//! It runs on the window's own device (the stage draws the drops straight from the card's buffers
//! with the plain drawing), a whole frame at a time under the body's lock. The stage draws
//! whenever the thread is between frames; when it hasn't got in for a while it waits for the
//! frame the thread is on, and the thread lets it in before taking the next ([`DraftJob::body`]). Going back plays from the newest restart copy at or
//! before the playhead (one at the start and one every [`COPY_EVERY`] frames reached), and a copy
//! repeats exactly, so going back never changes the water. A new setup (a physics edit) builds a
//! new body; an edit to a look never reaches here.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;

use bpe_gfx::wgpu;
use bpe_kit::things::water::draft::WaterSetup;
use bpe_water::draw::{Plain, PlainGpu};
use bpe_water::{SolidField, Water, WaterGpu};

/// Frames between two restart copies (2 s).
pub const COPY_EVERY: u32 = 120;

/// The most the restart copies may take on the card, as estimated by [`copy_size`].
pub const COPIES_MOST: u64 = 1 << 30;

/// The playhead's water frame at `time` seconds.
pub fn frame_of(time: f64) -> u32 {
    if time.is_finite() && time > 0.0 { (time * 60.0).floor().min(f64::from(u32::MAX - 1)) as u32 } else { 0 }
}

/// A Water scene's draft as the editor holds it: the newest setup, the thread playing it (started
/// by the stage once the window's graphics card is there) and the stage's picture of it.
#[derive(Default)]
pub struct WaterRun {
    /// What the draft plays, or is starting over with. `None` in a Balls scene.
    pub setup: Option<Arc<WaterSetup>>,
    pub job: Option<DraftJob>,
    pub picture: super::picture::StagePicture,
}

impl WaterRun {
    /// The setup for `scene`: `None` for a Balls scene.
    pub fn setup_of(scene: &bpe_core::Scene, resolved: &bpe_core::Resolved) -> Option<WaterSetup> {
        (scene.sim == bpe_core::SimKind::Water).then(|| WaterSetup::of(scene, resolved))
    }

    /// Whether `new` plays other water than the setup held.
    pub fn changes(&self, new: Option<&WaterSetup>) -> bool {
        match (new, &self.setup) {
            (Some(new), Some(old)) => new.print != old.print,
            (None, None) => false,
            _ => true,
        }
    }

    /// Plays `new` from the start (`None`: stops the draft and lets its picture go).
    pub fn set(&mut self, new: Option<WaterSetup>, render: Option<&egui_wgpu::RenderState>) {
        match new {
            Some(setup) => {
                let setup = Arc::new(setup);
                // (A thread that stopped by a fault is started afresh by the stage.)
                if self.job.as_ref().is_some_and(DraftJob::failed) {
                    self.job = None;
                }
                if let Some(job) = &self.job {
                    job.restart(setup.clone());
                }
                self.setup = Some(setup);
            }
            None => {
                self.picture.free(render);
                *self = WaterRun::default();
            }
        }
    }

    /// Starts the thread on the window's card when there is a setup and no thread yet.
    pub fn start_on(&mut self, render: &egui_wgpu::RenderState) {
        if self.job.is_none()
            && let Some(setup) = &self.setup
        {
            self.job = DraftJob::start(
                &render.device,
                &render.queue,
                setup.clone(),
                bpe_kit::things::water::draft::DRAFT_MOST,
            );
        }
    }
}

/// The body the stage draws, and its drawing.
pub struct Live {
    pub water: Water,
    /// The solid field the body runs on (the drawing's own may differ while a carve stroke is
    /// being painted).
    pub field: SolidField,
    pub plain: Plain,
    /// Goes up with every new body (a new setup).
    pub generation: u64,
}

/// What the thread and the editor share.
struct Shared {
    target: AtomicU32,
    stop: AtomicBool,
    /// The newest setup asked for, and how many have been asked for (counted under its lock).
    pending: Mutex<(Option<Arc<WaterSetup>>, u64)>,
    /// How many setups asked for the body is built from.
    built: AtomicU64,
    /// The print of the setup the body plays.
    plays: AtomicU64,
    live: Mutex<Option<Live>>,
    /// The stage is waiting for the body: the thread lets it in before its next frame.
    wanted: AtomicBool,
    /// The taps whose spouts are in the rock of the body's course (`WaterSetup::taps_in_rock`).
    in_rock: Mutex<Vec<bpe_core::ThingId>>,
    /// The body's frame (`u32::MAX`: none yet).
    reached: AtomicU32,
    count: AtomicU32,
    full: AtomicBool,
    failed: AtomicBool,
}

/// The water draft's thread.
pub struct DraftJob {
    shared: Arc<Shared>,
    /// (Taken when the job is let go, to wait for the thread to end.)
    thread: Option<JoinHandle<()>>,
}

impl DraftJob {
    /// Starts the draft of `setup` on `device` and `queue`; a body holds at most `most` drops
    /// (its pours stop there).
    pub fn start(device: &wgpu::Device, queue: &wgpu::Queue, setup: Arc<WaterSetup>, most: u32) -> Option<DraftJob> {
        let shared = Arc::new(Shared {
            target: AtomicU32::new(0),
            stop: AtomicBool::new(false),
            pending: Mutex::new((Some(setup), 1)),
            built: AtomicU64::new(0),
            plays: AtomicU64::new(0),
            live: Mutex::new(None),
            wanted: AtomicBool::new(false),
            in_rock: Mutex::new(Vec::new()),
            reached: AtomicU32::new(u32::MAX),
            count: AtomicU32::new(0),
            full: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        });
        let (device, queue, theirs) = (device.clone(), queue.clone(), shared.clone());
        let handle = std::thread::Builder::new()
            .name("water draft".into())
            .spawn(move || {
                crate::thread_priority::set_this_thread(crate::thread_priority::Priority::Low);
                let _guard = FailGuard(theirs.clone());
                let mut run = Run {
                    water_gpu: WaterGpu::new(&device, &queue),
                    plain_gpu: PlainGpu::new(&device, &queue),
                    setup: None,
                    copies: Copies::default(),
                    most,
                    generation: 0,
                };
                while !theirs.stop.load(Ordering::Relaxed) {
                    if run.work(&theirs) {
                        theirs.let_the_stage_in();
                    } else {
                        std::thread::park_timeout(Duration::from_millis(100));
                    }
                }
            })
            .ok()?;
        Some(DraftJob { shared, thread: Some(handle) })
    }

    /// Starts the draft over with `setup` (the newest one given wins).
    pub fn restart(&self, setup: Arc<WaterSetup>) {
        if let Ok(mut pending) = self.shared.pending.lock() {
            pending.0 = Some(setup);
            pending.1 += 1;
        }
        self.unpark();
    }

    fn unpark(&self) {
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }

    /// Asks for the water at frame `frame`.
    pub fn follow(&self, frame: u32) {
        if self.shared.target.swap(frame, Ordering::AcqRel) != frame {
            self.unpark();
        }
    }

    /// The body and its drawing, for tests.
    #[cfg(test)]
    pub fn live(&self) -> &Mutex<Option<Live>> {
        &self.shared.live
    }

    /// The body and its drawing: at once if the thread is between frames, else `None`, or with
    /// `wait` once the thread is done with the frame it is on (it then lets the stage in before it
    /// takes the next, whichever way the system hands out a lock).
    pub fn body(&self, wait: bool) -> Option<MutexGuard<'_, Option<Live>>> {
        match self.shared.live.try_lock() {
            Ok(body) => Some(body),
            Err(TryLockError::WouldBlock) if wait => {
                self.shared.wanted.store(true, Ordering::Release);
                let body = self.shared.live.lock().ok();
                self.shared.wanted.store(false, Ordering::Release);
                body
            }
            Err(_) => None,
        }
    }

    /// Whether tap `id`'s spout is in the rock of the course the body runs on.
    pub fn in_rock(&self, id: bpe_core::ThingId) -> bool {
        self.shared.in_rock.lock().is_ok_and(|ids| ids.contains(&id))
    }

    /// The frame the body is at, `None` before there is one.
    pub fn reached(&self) -> Option<u32> {
        let r = self.shared.reached.load(Ordering::Acquire);
        (r != u32::MAX).then_some(r)
    }

    /// The frame asked for.
    pub fn target(&self) -> u32 {
        self.shared.target.load(Ordering::Acquire)
    }

    /// The print of the setup the body plays (`WaterSetup::print`).
    pub fn plays(&self) -> u64 {
        self.shared.plays.load(Ordering::Acquire)
    }

    /// How many liquid drops the body holds (trapped air left out).
    pub fn count(&self) -> u32 {
        self.shared.count.load(Ordering::Acquire)
    }

    /// Whether the body holds its most drops and its pours have stopped.
    pub fn full(&self) -> bool {
        self.shared.full.load(Ordering::Acquire)
    }

    /// Whether the thread stopped by a fault.
    pub fn failed(&self) -> bool {
        self.shared.failed.load(Ordering::Acquire)
    }

    /// Whether it still has a frame or a setup to get to.
    pub fn busy(&self) -> bool {
        let asked = self.shared.pending.lock().map(|p| p.1).unwrap_or(0);
        let built = self.shared.built.load(Ordering::Acquire);
        !self.failed() && (built != asked || self.reached() != Some(self.target()))
    }
}

impl Drop for DraftJob {
    /// Stops the thread and waits for it to finish the frame it is on, so it never outlives the
    /// window's graphics card.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

impl Shared {
    /// The longest the thread waits for the stage between two frames.
    const HANDOFF_MOST: Duration = Duration::from_millis(50);

    /// Waits while the stage is waiting for the body (it gets the body the moment this thread
    /// lets it go, but a system's lock need not hand it over before this thread takes it again).
    fn let_the_stage_in(&self) {
        let since = Instant::now();
        while self.wanted.load(Ordering::Acquire) && since.elapsed() < Self::HANDOFF_MOST {
            std::thread::yield_now();
        }
    }
}

/// Marks the draft failed if its thread ends by a panic.
struct FailGuard(Arc<Shared>);

impl Drop for FailGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.failed.store(true, Ordering::Release);
        }
    }
}

/// The thread's own side.
struct Run {
    water_gpu: Arc<WaterGpu>,
    plain_gpu: Arc<PlainGpu>,
    setup: Option<Arc<WaterSetup>>,
    copies: Copies<Water>,
    most: u32,
    generation: u64,
}

impl Run {
    /// Does one piece of work: a new body, a step back or a frame on. `false` when there is
    /// nothing to do.
    fn work(&mut self, shared: &Shared) -> bool {
        let pending = shared.pending.lock().ok().and_then(|mut p| p.0.take().map(|s| (s, p.1)));
        if let Some((setup, asked)) = pending {
            self.begin(shared, setup);
            shared.built.store(asked, Ordering::Release);
            return true;
        }
        let Some(setup) = self.setup.clone() else { return false };
        let target = shared.target.load(Ordering::Acquire);
        let Ok(mut live) = shared.live.lock() else { return false };
        let Some(live) = live.as_mut() else { return false };
        let at = live.water.frames();
        if target < at {
            self.copies.drop_after(target);
            match self.copies.newest_at_or_before(target) {
                Some(copy) => live.water = copy.copy(),
                None => live.water = setup.start(self.water_gpu.clone(), &live.field, self.most),
            }
        } else if at < target {
            let inputs = setup.inputs_on(at, &live.field);
            let water = &mut live.water;
            water.set_gravity(inputs.gravity);
            water.set_air_lift(inputs.air_lift);
            if !inputs.feels.is_empty() {
                water.set_feels(&inputs.feels);
            }
            // (By the liquid: trapped air comes and goes, and would stop and start the pours.)
            let full = water.liquid() >= self.most;
            water.set_pours(if full { &[] } else { &inputs.pours });
            water.frame();
            let now = water.frames();
            if now % COPY_EVERY == 0 && !self.copies.has(now) {
                let size = copy_size(water);
                self.copies.keep(now, size, water.copy());
                self.copies.prune(COPIES_MOST);
            }
        } else {
            return false;
        }
        shared.reached.store(live.water.frames(), Ordering::Release);
        shared.count.store(live.water.liquid(), Ordering::Release);
        shared.full.store(live.water.liquid() >= self.most, Ordering::Release);
        true
    }

    /// A new body for `setup`, built with the body's lock let go, then swapped in.
    fn begin(&mut self, shared: &Shared, setup: Arc<WaterSetup>) {
        let field = setup.field();
        let water = setup.start(self.water_gpu.clone(), &field, self.most);
        let plain = Plain::new(self.plain_gpu.clone(), &field, setup.numbers().wall_off);
        self.copies = Copies::default();
        self.copies.keep(0, copy_size(&water), water.copy());
        self.generation += 1;
        let (count, generation) = (water.liquid(), self.generation);
        let in_rock = setup.taps_in_rock(&field);
        if let Ok(mut live) = shared.live.lock() {
            *live = Some(Live { water, field, plain, generation });
        }
        if let Ok(mut ids) = shared.in_rock.lock() {
            *ids = in_rock;
        }
        shared.plays.store(setup.print, Ordering::Release);
        self.setup = Some(setup);
        shared.reached.store(0, Ordering::Release);
        shared.count.store(count, Ordering::Release);
        shared.full.store(count >= self.most, Ordering::Release);
    }
}

/// About how much card memory a copy of `water` takes: its drops' buffers (as roomy as the
/// body's) and its grid's.
fn copy_size(water: &Water) -> u64 {
    u64::from(water.room().max(water.count())) * 128 + water.layout().nodes() as u64 * 64
}

/// The restart copies, oldest first, by the frame they are at.
struct Copies<T> {
    list: Vec<(u32, u64, T)>,
}

impl<T> Default for Copies<T> {
    fn default() -> Self {
        Copies { list: Vec::new() }
    }
}

impl<T> Copies<T> {
    fn has(&self, frame: u32) -> bool {
        self.list.iter().any(|c| c.0 == frame)
    }

    fn keep(&mut self, frame: u32, size: u64, copy: T) {
        let at = self.list.partition_point(|c| c.0 < frame);
        self.list.insert(at, (frame, size, copy));
    }

    fn drop_after(&mut self, frame: u32) {
        self.list.retain(|c| c.0 <= frame);
    }

    fn newest_at_or_before(&self, frame: u32) -> Option<&T> {
        self.list.iter().rev().find(|c| c.0 <= frame).map(|c| &c.2)
    }

    /// While the copies take more than `most`, lets every other one go, keeping the first (the
    /// start) and the newest.
    fn prune(&mut self, most: u64) {
        while self.list.len() > 2 && self.list.iter().map(|c| c.1).sum::<u64>() > most {
            let last = self.list.len() - 1;
            let mut i = 0;
            self.list.retain(|_| {
                let keep = i == 0 || i == last || i % 2 == 0;
                i += 1;
                keep
            });
        }
    }

    #[cfg(test)]
    fn frames(&self) -> Vec<u32> {
        self.list.iter().map(|c| c.0).collect()
    }
}

#[cfg(test)]
mod tests;
