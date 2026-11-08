//! The sound thread: it owns the sound [`Engine`], keeps the speakers' ring filled a little ahead
//! and takes the editor's commands. The editor's playhead follows the sound the speakers have
//! played ([`SoundJob::clock`]).

use crate::audio_out::{AudioOut, RATE, Shared};
use bpe_core::{Resolved, Scene, ThingId};
use bpe_kit::Kit;
use bpe_sound::{BLOCK, Engine, Meters, SoundPatch};
use rtrb::Producer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The thread keeps at least this many frames in the ring while playing (50 ms)...
#[cfg_attr(not(test), allow(dead_code))]
pub const AHEAD_FRAMES: usize = 2_400;
/// ...by filling it up to this many (100 ms) whenever it looks.
const FILL_FRAMES: usize = 4_800;
/// While stopped, hand-played notes are kept this few frames ahead (20 ms), so a key sounds
/// soon after it is clicked.
const PAUSED_FRAMES: usize = 1_024;
/// Blocks rendered after the last note goes quiet while stopped, for the master's tail.
const TAIL_BLOCKS: u32 = 40;
/// How long the thread waits for the speakers to clear the ring after a seek.
const CLEAR_WAIT: Duration = Duration::from_millis(250);

/// A note played by hand from the editor (the keyboard). It plays at a fixed strength with the
/// sound's own envelopes; `velocity` and `on` are kept for later parts (pads, held keys).
#[derive(Clone, Debug, PartialEq)]
pub struct Audition {
    pub thing: ThingId,
    pub note: u8,
    pub velocity: f32,
    pub on: bool,
}

/// What the editor asks of the sound thread.
pub enum Command {
    Play,
    /// Stop and clear the ring (clear number `.0`); the playhead stopped at `.1` seconds.
    Pause(u64, f64),
    /// Go to a time and clear the ring (clear number `.1`).
    Seek(f64, u64),
    /// New sound settings, same physics.
    Patch(Box<SoundPatch>),
    /// The physics changed: a new engine from the start.
    Rebuild(Box<Scene>, Box<Resolved>),
    Audition(Audition),
    Quit,
}

impl Command {
    /// The command's name, for tests.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Play => "play",
            Command::Pause(..) => "pause",
            Command::Seek(..) => "seek",
            Command::Patch(_) => "patch",
            Command::Rebuild(..) => "rebuild",
            Command::Audition(_) => "audition",
            Command::Quit => "quit",
        }
    }
}

/// Where the commands go: the thread, or (in tests) a list.
enum Outbox {
    Thread {
        tx: Sender<Command>,
        thread: Option<JoinHandle<()>>,
    },
    #[cfg(test)]
    Record(Vec<Command>),
}

/// The editor's handle on the sound thread.
pub struct SoundJob {
    outbox: Outbox,
    shared: Arc<Shared>,
    meters: Arc<Meters>,
    /// The number of the last clear asked for.
    clear: u64,
    /// The time the sound counts from since that clear.
    base: f64,
    /// The open device, kept alive while the job runs.
    _out: Option<AudioOut>,
    /// Playing (else the clock holds: what the speakers play while stopped is hand-played).
    playing: bool,
    /// Where the engine is while stopped, as far as the editor knows: keys clicked then sound
    /// as they are there. `None` while playing.
    parked: Option<f64>,
}

/// How far the engine may be from the playhead while stopped and still play a clicked key where
/// it is: a pause leaves it up to the ring's fill (100 ms) ahead.
const PARK_SLACK: f64 = 0.15;

/// `time` rounded down to a block, as the engine seeks.
fn block_floor(time: f64) -> f64 {
    let samples = (time.max(0.0) * RATE as f64) as u64 / BLOCK as u64 * BLOCK as u64;
    samples as f64 / RATE as f64
}

impl SoundJob {
    /// Starts the thread playing into `out`'s speakers.
    pub fn start(scene: &Scene, resolved: &Resolved, kit: Arc<Kit>, mut out: AudioOut) -> Option<SoundJob> {
        let ring = out.ring_writer()?;
        let mut job = Self::start_with_sink(scene, resolved, kit, ring, out.shared());
        job._out = Some(out);
        Some(job)
    }

    /// Starts the thread writing into `ring`, whoever reads it.
    pub fn start_with_sink(
        scene: &Scene,
        resolved: &Resolved,
        kit: Arc<Kit>,
        ring: Producer<f32>,
        shared: Arc<Shared>,
    ) -> SoundJob {
        let (tx, rx) = channel();
        let meters = Arc::new(Meters::default());
        let (scene, resolved) = (scene.clone(), resolved.clone());
        let thread_meters = meters.clone();
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("ball-studio sound".into())
            .spawn(move || {
                crate::thread_priority::set_this_thread(crate::thread_priority::Priority::High);
                run(rx, ring, thread_shared, kit, scene, resolved, thread_meters)
            })
            .ok();
        SoundJob {
            outbox: Outbox::Thread { tx, thread },
            shared,
            meters,
            clear: 0,
            base: 0.0,
            _out: None,
            playing: false,
            parked: Some(0.0),
        }
    }

    /// A job with no thread that keeps the commands it is sent.
    #[cfg(test)]
    pub fn recording() -> SoundJob {
        SoundJob {
            outbox: Outbox::Record(Vec::new()),
            shared: Arc::new(Shared::default()),
            meters: Arc::new(Meters::default()),
            clear: 0,
            base: 0.0,
            _out: None,
            playing: false,
            parked: Some(0.0),
        }
    }

    /// The names of the commands sent so far (a recording job only).
    #[cfg(test)]
    pub fn recorded(&self) -> Vec<&'static str> {
        match &self.outbox {
            Outbox::Record(list) => list.iter().map(Command::name).collect(),
            Outbox::Thread { .. } => Vec::new(),
        }
    }

    fn send(&mut self, command: Command) {
        match &mut self.outbox {
            // Never waits: the thread passes over commands a later one makes pointless.
            Outbox::Thread { tx, .. } => {
                let _ = tx.send(command);
            }
            #[cfg(test)]
            Outbox::Record(list) => list.push(command),
        }
    }

    /// Plays from `time`.
    pub fn play(&mut self, time: f64) {
        self.seek(time);
        self.playing = true;
        self.parked = None;
        self.send(Command::Play);
    }

    /// Stops, the playhead at `at` (where Play will start again).
    pub fn pause(&mut self, at: f64) {
        self.parked = Some(at);
        self.playing = false;
        self.clear += 1;
        let clear = self.clear;
        self.send(Command::Pause(clear, at));
    }

    /// Goes to `time`; what the speakers still held is dropped.
    pub fn seek(&mut self, time: f64) {
        self.clear += 1;
        self.base = block_floor(time);
        if !self.playing {
            self.parked = Some(self.base);
        }
        let clear = self.clear;
        self.send(Command::Seek(time, clear));
    }

    /// While stopped, takes the engine to the playhead at `time` when it is somewhere else (a
    /// scrub or a rebuild moved it), so a key clicked next sounds as it does there. Scrubbing
    /// tells the sound nothing; this is called just before a key plays.
    pub fn park(&mut self, time: f64) {
        if !self.playing && self.parked.is_none_or(|at| (at - block_floor(time)).abs() > PARK_SLACK) {
            self.seek(time);
        }
    }

    pub fn patch(&mut self, patch: SoundPatch) {
        self.send(Command::Patch(Box::new(patch)));
    }

    /// A new engine for a scene whose physics changed. It starts at 0; seek after it.
    pub fn rebuild(&mut self, scene: &Scene, resolved: &Resolved) {
        if !self.playing {
            self.parked = Some(0.0);
        }
        self.send(Command::Rebuild(Box::new(scene.clone()), Box::new(resolved.clone())));
    }

    /// Plays a note on a shape now, playing or stopped.
    pub fn audition(&mut self, audition: Audition) {
        self.send(Command::Audition(audition));
    }

    /// The time the speakers are at: where the last seek landed plus what has been heard since
    /// (what was played, less the delay to the ears: the device's and the speaker delay). Holds
    /// still until the speakers have dropped what they held from before the seek.
    pub fn clock(&self) -> f64 {
        if !self.playing || self.shared.cleared() != self.clear {
            return self.base;
        }
        let heard = self.shared.played().saturating_sub(self.shared.heard_delay());
        self.base + heard as f64 / RATE as f64
    }

    pub fn meters(&self) -> Arc<Meters> {
        self.meters.clone()
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// The speakers stopped working (the sound thread may still be fine).
    pub fn device_failed(&self) -> bool {
        self.shared.failed()
    }

    /// The speakers stopped working, or the sound thread did.
    pub fn failed(&self) -> bool {
        let thread_gone = match &self.outbox {
            Outbox::Thread { thread, .. } => thread.as_ref().is_none_or(JoinHandle::is_finished),
            #[cfg(test)]
            Outbox::Record(_) => false,
        };
        self.shared.failed() || thread_gone
    }
}

impl Drop for SoundJob {
    // Without tests the outbox is always the thread.
    #[allow(irrefutable_let_patterns)]
    fn drop(&mut self) {
        // The thread stops at its next look; closing the editor doesn't wait for a long seek.
        if let Outbox::Thread { tx, .. } = &self.outbox {
            let _ = tx.send(Command::Quit);
        }
    }
}

/// `time` as the sample a seek to it lands on.
fn seek_sample(time: f64) -> u64 {
    (time.max(0.0) * RATE as f64) as u64 / BLOCK as u64 * BLOCK as u64
}

/// A new engine (the physics changed) being built and warmed up on a helper thread, so the sound
/// thread keeps answering meanwhile.
struct Build {
    /// Set when a newer rebuild makes it pointless: the helper gives up at its next look.
    cancel: Arc<AtomicBool>,
    /// Where to seek once it is built: the latest seek since the rebuild.
    target: Arc<Mutex<Option<f64>>>,
    done: Receiver<Built>,
}

/// An engine the helper built, and the time it was warmed up at (`None`: no seek had come).
struct Built {
    engine: Engine,
    at: Option<f64>,
}

impl Build {
    fn start(scene: Box<Scene>, resolved: Box<Resolved>, kit: Arc<Kit>) -> Build {
        let cancel = Arc::new(AtomicBool::new(false));
        let target = Arc::new(Mutex::new(None));
        let (tx, done) = channel();
        let (c, t) = (cancel.clone(), target.clone());
        let spawned = std::thread::Builder::new()
            .name("ball-studio sound warm-up".into())
            .spawn(move || build_aside(&scene, &resolved, &kit, &c, &t, &tx));
        if let Err(e) = spawned {
            eprintln!("sound: could not start the warm-up helper: {e}");
        }
        Build { cancel, target, done }
    }
}

/// The helper's work: builds the engine, then warms it up at the latest seek (again if a newer
/// one comes while it does), and hands it over. Gives up when cancelled.
fn build_aside(
    scene: &Scene,
    resolved: &Resolved,
    kit: &Kit,
    cancel: &AtomicBool,
    target: &Mutex<Option<f64>>,
    tx: &Sender<Built>,
) {
    let patch = SoundPatch::build_ready(scene, resolved, kit);
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let mut engine = Engine::with_patch(scene, resolved, kit, patch);
    let latest = || *target.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        let Some(at) = latest() else {
            let _ = tx.send(Built { engine, at: None });
            return;
        };
        if engine.seek_until(at, || cancel.load(Ordering::Relaxed) || latest() != Some(at)) {
            let _ = tx.send(Built { engine, at: Some(at) });
            return;
        }
        if cancel.load(Ordering::Relaxed) {
            return;
        }
    }
}

/// The thread's state.
struct Worker {
    engine: Engine,
    kit: Arc<Kit>,
    meters: Arc<Meters>,
    shared: Arc<Shared>,
    ring: Producer<f32>,
    playing: bool,
    /// A new engine waits for the seek that follows it before it plays.
    held: bool,
    /// Blocks still to render while stopped (hand-played notes and their tail); 0 when quiet.
    tail: u32,
    /// The sample the engine was last sought to, while it has played nothing since: a seek
    /// there again is already done.
    warm_at: Option<u64>,
    /// Stopped at this time: once nothing else is asked, the engine warms up there, so Play
    /// starts at once.
    prewarm: Option<f64>,
    /// A new engine on its way from the helper.
    build: Option<Build>,
    /// Sound settings that came while it was built, for it.
    pending_patch: Option<Box<SoundPatch>>,
    /// A warm-up left notes and tails ringing that nobody has heard: a clicked key stops them
    /// first, so it starts from quiet.
    unheard_tails: bool,
    /// Where the playhead rests while stopped (to warm up there again after a sound edit).
    paused_at: Option<f64>,
}

impl Worker {
    fn new(engine: Engine, kit: Arc<Kit>, meters: Arc<Meters>, shared: Arc<Shared>, ring: Producer<f32>) -> Worker {
        Worker {
            engine,
            kit,
            meters,
            shared,
            ring,
            playing: false,
            held: false,
            tail: 0,
            warm_at: None,
            prewarm: None,
            build: None,
            pending_patch: None,
            unheard_tails: false,
            paused_at: None,
        }
    }

    /// Before a clicked key: tails a warm-up left ringing go, so the key sounds alone. The
    /// engine warms up again once nothing else is asked.
    fn quiet_for_keys(&mut self) {
        if self.unheard_tails {
            self.engine.stop_notes();
            self.unheard_tails = false;
            self.warm_at = None;
            self.prewarm = self.paused_at;
        }
    }

    /// Seeks the engine to `time`, unless it is already there and warm. False when `stop`
    /// made it give up.
    fn seek(&mut self, time: f64, stop: impl FnMut() -> bool) -> bool {
        let at = seek_sample(time);
        if self.warm_at == Some(at) && self.engine.sample_position() == at {
            return true;
        }
        let done = self.engine.seek_until(time, stop);
        // Given up, it still landed there, cold.
        self.warm_at = done.then(|| self.engine.sample_position());
        self.unheard_tails = done;
        done
    }

    /// Takes the engine the helper built, once it is ready.
    fn poll_build(&mut self) {
        let Some(b) = &self.build else { return };
        let built = match b.done.try_recv() {
            Ok(built) => built,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                // The helper failed: the old engine plays on, waiting for a seek.
                self.build = None;
                return;
            }
        };
        let target = *b.target.lock().unwrap_or_else(|e| e.into_inner());
        self.build = None;
        self.engine = built.engine;
        self.engine.share_meters(self.meters.clone());
        if let Some(patch) = self.pending_patch.take() {
            self.engine.swap_patch(*patch);
        }
        self.warm_at = None;
        match (built.at, target) {
            (Some(at), Some(t)) if at == t => {
                self.warm_at = Some(self.engine.sample_position());
                self.unheard_tails = true;
                self.held = false;
            }
            (_, Some(t)) => {
                self.seek(t, || false);
                self.held = false;
            }
            // It starts at 0; the editor's seek to the playhead follows.
            (_, None) => self.held = true,
        }
        if !self.playing && self.tail > 0 {
            // A key was ringing on the old engine: the new one's tails mustn't join it.
            self.quiet_for_keys();
        }
    }

    /// Waits for the helper's engine (tests).
    #[cfg(test)]
    fn wait_build(&mut self) {
        let started = Instant::now();
        while self.build.is_some() && started.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(1));
            self.poll_build();
        }
    }

    /// Asks the speakers to drop what they hold and waits (a little) until they have, so
    /// nothing written after this is thrown away.
    fn clear(&self, number: u64) {
        self.shared.ask_clear(number);
        let started = Instant::now();
        while self.shared.cleared() != number && started.elapsed() < CLEAR_WAIT {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Handles one command, passing over work a `later` command in the same batch makes
    /// pointless (a drag sends one a frame). False to stop.
    fn handle(&mut self, command: Command, later: &[&str]) -> bool {
        let comes = |names: &[&str]| later.iter().any(|k| names.contains(k));
        match command {
            Command::Play => {
                self.playing = true;
                self.prewarm = None;
                self.paused_at = None;
            }
            Command::Pause(number, at) => {
                self.playing = false;
                if self.build.is_none() {
                    self.held = false;
                }
                // Stopping stops the sound; a key clicked afterwards starts from quiet.
                self.engine.stop_notes();
                self.warm_at = None;
                self.unheard_tails = false;
                self.tail = 0;
                if !comes(&["seek", "pause"]) {
                    self.clear(number);
                }
                // Warmed up where it stopped while nothing else is asked, Play is instant.
                self.prewarm = Some(at);
                self.paused_at = Some(at);
            }
            Command::Seek(time, number) => {
                // The old sound stops first (a long warm-up would otherwise let it play on);
                // nothing new is written until the seek is done.
                if !comes(&["seek", "pause"]) {
                    self.clear(number);
                }
                // Seeking warms it up there; an earlier wish to warm up elsewhere is over.
                self.prewarm = None;
                if !self.playing {
                    self.paused_at = Some(time);
                }
                if !comes(&["seek", "rebuild"]) {
                    if let Some(b) = &self.build {
                        // The new engine is warmed up there as it is built.
                        *b.target.lock().unwrap_or_else(|e| e.into_inner()) = Some(time);
                    } else {
                        self.seek(time, || false);
                        self.held = false;
                    }
                }
            }
            Command::Patch(patch) => {
                if !comes(&["patch", "rebuild"]) {
                    if self.build.is_some() {
                        self.pending_patch = Some(patch);
                    } else {
                        self.engine.swap_patch(*patch);
                        if !self.playing {
                            // Its tails (a new reverb, a new voice) aren't warmed up yet: warm
                            // up again once nothing else is asked.
                            self.warm_at = None;
                            self.prewarm = self.paused_at;
                        }
                    }
                }
            }
            Command::Rebuild(scene, resolved) => {
                if !comes(&["rebuild"]) {
                    if let Some(old) = self.build.take() {
                        old.cancel.store(true, Ordering::Relaxed);
                    }
                    self.pending_patch = None;
                    self.prewarm = None;
                    self.build = Some(Build::start(scene, resolved, self.kit.clone()));
                    // Nothing plays until the new engine is in and at the playhead.
                    self.held = true;
                    self.warm_at = None;
                }
            }
            Command::Audition(a) => {
                if a.on {
                    self.quiet_for_keys();
                    self.engine.audition(a.thing, a.note);
                    self.tail = TAIL_BLOCKS;
                }
            }
            Command::Quit => {
                if let Some(b) = &self.build {
                    b.cancel.store(true, Ordering::Relaxed);
                }
                return false;
            }
        }
        true
    }

    /// While stopped: renders hand-played notes until the ring holds [`PAUSED_FRAMES`], and
    /// stops a little after they go quiet.
    fn fill_paused(&mut self) {
        let mut block = [0.0f32; BLOCK * 2];
        let capacity = self.ring.buffer().capacity();
        while self.tail > 0 && capacity - self.ring.slots() < PAUSED_FRAMES * 2 && self.ring.slots() >= BLOCK * 2 {
            self.engine.render_paused(&mut block);
            self.write(&block);
            if self.engine.is_quiet() {
                self.tail -= 1;
            } else {
                self.tail = TAIL_BLOCKS;
            }
        }
    }

    fn write(&mut self, block: &[f32; BLOCK * 2]) {
        if let Ok(mut chunk) = self.ring.write_chunk(BLOCK * 2) {
            let (a, b) = chunk.as_mut_slices();
            a.copy_from_slice(&block[..a.len()]);
            b.copy_from_slice(&block[a.len()..]);
            chunk.commit_all();
        }
    }

    /// Renders blocks until the ring holds [`FILL_FRAMES`].
    fn fill(&mut self) {
        if self.held {
            return;
        }
        let mut block = [0.0f32; BLOCK * 2];
        let capacity = self.ring.buffer().capacity();
        while capacity - self.ring.slots() < FILL_FRAMES * 2 && self.ring.slots() >= BLOCK * 2 {
            self.engine.render(&mut block);
            self.warm_at = None;
            self.unheard_tails = false;
            self.write(&block);
        }
    }
}

fn run(
    rx: Receiver<Command>,
    ring: Producer<f32>,
    shared: Arc<Shared>,
    kit: Arc<Kit>,
    scene: Scene,
    resolved: Resolved,
    meters: Arc<Meters>,
) {
    // Wavetables not yet baked start baking elsewhere; the editor builds the sound again then.
    let patch = SoundPatch::build_ready(&scene, &resolved, &kit);
    let mut engine = Engine::with_patch(&scene, &resolved, &kit, patch);
    engine.share_meters(meters.clone());
    let mut w = Worker::new(engine, kit, meters, shared, ring);
    let mut batch = Vec::new();
    loop {
        if batch.is_empty() {
            let first = if w.playing || w.tail > 0 || w.build.is_some() || w.prewarm.is_some() {
                match rx.recv_timeout(Duration::from_millis(2)) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => return,
                }
            };
            batch.extend(first);
        }
        batch.extend(rx.try_iter());
        let names: Vec<&'static str> = batch.iter().map(Command::name).collect();
        for (i, c) in batch.drain(..).enumerate() {
            if !w.handle(c, &names[i + 1..]) {
                return;
            }
        }
        w.poll_build();
        // A ring left to run dry on purpose (waiting for a new engine) is no dropout.
        w.shared.set_playing(w.playing && !w.held);
        if w.playing {
            w.fill();
        } else {
            w.fill_paused();
            if w.tail == 0
                && w.build.is_none()
                && let Some(at) = w.prewarm.take()
            {
                // Gives up as soon as a command comes (it is handled next; still wanted, the
                // warm-up is tried again after it).
                let done = w.seek(at, || match rx.try_recv() {
                    Ok(c) => {
                        batch.push(c);
                        true
                    }
                    Err(TryRecvError::Empty) => false,
                    Err(TryRecvError::Disconnected) => true,
                });
                if !done {
                    w.prewarm = Some(at);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_out::{Feeder, ring};

    fn toned(kit: &Kit, tag: &str) -> (Scene, Resolved) {
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
        scene.things.get_mut(&shape).unwrap().tags.insert(tag.into());
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        (scene, resolved)
    }

    /// Reads the ring at real speed for `ms` milliseconds, a millisecond at a time.
    fn consume(feeder: &mut Feeder, ms: u32, out: &mut Vec<[f32; 2]>) {
        let started = Instant::now();
        for k in 1..=ms {
            feeder.poll_clear();
            for _ in 0..48 {
                out.push(feeder.next_frame());
            }
            let due = Duration::from_millis(k as u64);
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                std::thread::sleep(wait);
            }
        }
    }

    /// Like [`consume`], but keeps only the frames that came from the ring (not underruns).
    fn consume_real(feeder: &mut Feeder, shared: &Shared, ms: u32, out: &mut Vec<[f32; 2]>) {
        let started = Instant::now();
        for k in 1..=ms {
            feeder.poll_clear();
            for _ in 0..48 {
                let before = shared.played();
                let frame = feeder.next_frame();
                if shared.played() > before {
                    out.push(frame);
                }
            }
            let due = Duration::from_millis(k as u64);
            if let Some(wait) = due.checked_sub(started.elapsed()) {
                std::thread::sleep(wait);
            }
        }
    }

    #[test]
    fn a_seek_stops_the_old_sound_at_once() {
        let kit = Arc::new(bpe_kit::kit());
        let (scene, resolved) = toned(&kit, "test-tone-decay=30");
        let (producer, consumer) = ring();
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(consumer, shared.clone(), RATE);
        let mut job = SoundJob::start_with_sink(&scene, &resolved, kit, producer, shared.clone());
        job.play(0.0);
        consume(&mut feeder, 80, &mut Vec::new());
        // A far seek: four seconds of pre-roll to work out before the new sound starts.
        job.seek(20.0);
        let mut old = Vec::new();
        let started = Instant::now();
        while shared.cleared() != 2 && started.elapsed() < Duration::from_secs(10) {
            consume_real(&mut feeder, &shared, 1, &mut old);
        }
        assert!(old.len() <= 150, "{} frames of the old sound played after the seek", old.len());
    }

    #[test]
    fn the_clock_is_what_was_heard() {
        let mut job = SoundJob::recording();
        job.playing = true;
        job.shared.played.store(10_000, std::sync::atomic::Ordering::Release);
        assert_eq!(job.clock(), 10_000.0 / 48_000.0);
        job.shared.set_speaker_delay_ms(100.0);
        assert_eq!(job.clock(), 5_200.0 / 48_000.0, "less the speaker delay");
        job.shared.set_speaker_delay_ms(500.0);
        assert_eq!(job.clock(), 0.0, "never before the seek");
    }

    #[test]
    fn a_rebuild_waits_for_its_seek() {
        let kit = Arc::new(bpe_kit::kit());
        let (scene, resolved) = toned(&kit, "test-tone");
        let (ring_in, ring_out) = ring();
        let shared = Arc::new(Shared::default());
        let meters = Arc::new(Meters::default());
        let engine = Engine::new(&scene, &resolved, &kit);
        let mut w = Worker::new(engine, kit, meters, shared.clone(), ring_in);
        w.playing = true;
        w.handle(Command::Rebuild(Box::new(scene.clone()), Box::new(resolved.clone())), &[]);
        w.fill();
        assert_eq!(ring_out.slots(), 0, "nothing from the new world's start before the seek lands");
        shared.cleared.store(1, std::sync::atomic::Ordering::Release);
        w.handle(Command::Seek(2.0, 1), &[]);
        w.wait_build();
        assert!(w.build.is_none(), "the new engine came from the helper");
        assert_eq!(w.engine.sample_position(), 96_000, "warmed up at the seek");
        w.fill();
        assert!(ring_out.slots() > 0);
    }

    #[test]
    fn keys_play_on_the_old_engine_while_the_new_one_is_built() {
        let kit = Arc::new(bpe_kit::kit());
        let (scene, resolved) = toned(&kit, "test-tone-decay=25");
        let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
        let (ring_in, _ring_out) = ring();
        let shared = Arc::new(Shared::default());
        let engine = Engine::new(&scene, &resolved, &kit);
        let mut w = Worker::new(engine, kit, Arc::new(Meters::default()), shared.clone(), ring_in);
        w.handle(Command::Rebuild(Box::new(scene.clone()), Box::new(resolved.clone())), &[]);
        // A long warm-up there (25 s tails at 40 s) runs on the helper...
        shared.cleared.store(1, std::sync::atomic::Ordering::Release);
        w.handle(Command::Seek(40.0, 1), &[]);
        // ...while the sound thread answers at once.
        let started = Instant::now();
        w.handle(Command::Audition(Audition { thing: shape, note: 60, velocity: 0.8, on: true }), &[]);
        assert!(started.elapsed() < Duration::from_millis(200), "the key waited for the warm-up");
        assert_eq!(w.tail, TAIL_BLOCKS);
        // A newer seek while it warms up: it lands there.
        w.handle(Command::Seek(10.0, 1), &[]);
        w.wait_build();
        assert_eq!(w.engine.sample_position(), 480_000);
    }

    #[test]
    fn play_after_pause_starts_without_another_warm_up() {
        let kit = Arc::new(bpe_kit::kit());
        let (scene, resolved) = toned(&kit, "test-tone-decay=25");
        let (ring_in, _ring_out) = ring();
        let shared = Arc::new(Shared::default());
        let engine = Engine::new(&scene, &resolved, &kit);
        let mut w = Worker::new(engine, kit, Arc::new(Meters::default()), shared.clone(), ring_in);
        shared.cleared.store(1, std::sync::atomic::Ordering::Release);
        w.handle(Command::Pause(1, 30.0), &[]);
        assert_eq!(w.prewarm, Some(30.0));
        // The idle warm-up (what the thread does once nothing else is asked).
        let at = w.prewarm.take().unwrap();
        assert!(w.seek(at, || false));
        shared.cleared.store(2, std::sync::atomic::Ordering::Release);
        let started = Instant::now();
        w.handle(Command::Seek(30.0, 2), &[]);
        w.handle(Command::Play, &[]);
        assert!(started.elapsed() < Duration::from_millis(50), "Play warmed up again");
        assert_eq!(w.engine.sample_position(), 30 * 48_000);
        // Somewhere else is a real seek.
        w.handle(Command::Seek(31.0, 2), &[]);
        assert_eq!(w.engine.sample_position(), 31 * 48_000);
        assert_eq!(w.warm_at, Some(31 * 48_000));
        w.fill();
        assert_eq!(w.warm_at, None, "played since");
    }

    #[test]
    fn the_warm_state_after_pause_gives_way_to_newer_wishes() {
        let kit = Arc::new(bpe_kit::kit());
        let (scene, resolved) = toned(&kit, "test-tone-decay=25");
        let (ring_in, _ring_out) = ring();
        let shared = Arc::new(Shared::default());
        let engine = Engine::new(&scene, &resolved, &kit);
        let mut w = Worker::new(engine, kit.clone(), Arc::new(Meters::default()), shared.clone(), ring_in);
        shared.cleared.store(1, std::sync::atomic::Ordering::Release);
        w.handle(Command::Pause(1, 20.0), &[]);
        // A command cuts the idle warm-up short: it still lands at the playhead, quiet, and tries
        // again later.
        assert!(!w.seek(20.0, || true));
        assert_eq!(w.engine.sample_position(), 20 * 48_000);
        assert!(!w.unheard_tails && w.warm_at.is_none());
        // Warmed up, a sound edit makes it warm up again.
        assert!(w.seek(20.0, || false));
        assert!(w.unheard_tails);
        w.handle(Command::Patch(Box::new(SoundPatch::build(&scene, &resolved, &kit))), &[]);
        assert_eq!((w.warm_at, w.prewarm), (None, Some(20.0)));
        // A seek elsewhere ends the wish to warm up at the pause.
        w.handle(Command::Seek(5.0, 1), &[]);
        assert_eq!(w.prewarm, None);
        assert_eq!(w.paused_at, Some(5.0));
        // A clicked key stops the tails the warm-up left, then plays alone.
        let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
        assert!(w.unheard_tails);
        w.handle(Command::Audition(Audition { thing: shape, note: 60, velocity: 0.8, on: true }), &[]);
        assert!(!w.unheard_tails);
        assert_eq!(w.prewarm, Some(5.0), "warmed up again once the key is done");
    }

    #[test]
    fn a_dead_sound_thread_lets_the_wall_clock_take_over() {
        let kit = Arc::new(bpe_kit::kit());
        let (scene, resolved) = toned(&kit, "test-tone");
        let (producer, _consumer) = ring();
        let mut job = SoundJob::start_with_sink(&scene, &resolved, kit, producer, Arc::new(Shared::default()));
        assert!(!job.failed());
        job.send(Command::Quit);
        let started = Instant::now();
        while !job.failed() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(job.failed(), "a thread that stopped counts as failed");
    }

    #[test]
    fn the_job_stays_ahead() {
        let kit = Arc::new(bpe_kit::kit());
        let (scene, resolved) = toned(&kit, "test-tone");
        let (producer, consumer) = ring();
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(consumer, shared.clone(), RATE);
        let mut job = SoundJob::start_with_sink(&scene, &resolved, kit, producer, shared);
        job.play(0.0);
        let mut heard = Vec::new();
        consume(&mut feeder, 100, &mut heard);
        assert!(feeder.buffered_frames() >= AHEAD_FRAMES, "{} frames ahead", feeder.buffered_frames());
        assert!(job.clock() > 0.05, "the clock follows what was played: {}", job.clock());
    }

    #[test]
    fn seeking_clears_old_audio() {
        let kit = Arc::new(bpe_kit::kit());
        let (scene, resolved) = toned(&kit, "test-tone-decay=30");
        let (producer, consumer) = ring();
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(consumer, shared.clone(), RATE);
        let mut job = SoundJob::start_with_sink(&scene, &resolved, kit.clone(), producer, shared.clone());
        job.play(0.5);
        let mut heard = Vec::new();
        consume(&mut feeder, 60, &mut heard);
        job.seek(3.0);
        assert_eq!(job.clock(), 3.0, "the clock waits at the seek");
        // The thread seeks, then asks the speakers to drop what they hold.
        let started = Instant::now();
        while shared.cleared() != 2 && started.elapsed() < Duration::from_secs(10) {
            consume(&mut feeder, 1, &mut Vec::new());
        }
        assert_eq!(shared.cleared(), 2, "the ring was cleared");
        // The new sound starts once the seek's pre-roll is worked out.
        let mut after = Vec::new();
        let started = Instant::now();
        while after.len() < 4_000 && started.elapsed() < Duration::from_secs(10) {
            consume_real(&mut feeder, &shared, 10, &mut after);
        }
        assert!(after.len() > 3_000, "{} frames after the seek", after.len());

        let mut reference = Engine::new(&scene, &resolved, &kit);
        reference.seek(3.0);
        let mut block = [0.0f32; BLOCK * 2];
        let mut expected = Vec::new();
        while expected.len() < after.len() {
            reference.render(&mut block);
            expected.extend(block.chunks(2).map(|f| [f[0], f[1]]));
        }
        // Heard 2 ms late (the line) and past the 2 ms fade-in, frame for frame the sound at 3 s.
        assert!(expected[96..].iter().any(|f| f[0].abs() > 0.01), "the tone rings at 3 s");
        let worst = after[192..].iter().zip(&expected[96..]).fold(0.0f32, |m, (a, b)| m.max((a[0] - b[0]).abs()));
        assert!(worst < 1e-6, "the sound after the seek is the sound at 3 s: {worst}");
    }

    #[test]
    fn audition_sounds_while_paused() {
        let kit = Arc::new(bpe_kit::kit());
        let mut scene = bpe_kit::new_scene(&kit.registry);
        let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
        let card = bpe_kit::sound::starter_voice(&kit.registry);
        scene.things.get_mut(&shape).unwrap().slots.insert(bpe_core::SlotId::Sound, card);
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        // The sound thread never bakes wavetables (the editor builds the sound again once they
        // are in), so the starter's is baked here first, as the editor does when it opens.
        bpe_dsp::WaveBank::get(bpe_dsp::WaveShape::Triangle, bpe_dsp::WarpModeId::Sync);
        let (producer, consumer) = ring();
        let shared = Arc::new(Shared::default());
        let mut feeder = Feeder::new(consumer, shared.clone(), RATE);
        let mut job = SoundJob::start_with_sink(&scene, &resolved, kit, producer, shared.clone());
        job.seek(1.5);
        job.pause(1.5);
        // The seek's pre-roll is worked out, then the pause clears the ring.
        let started = Instant::now();
        while shared.cleared() != 2 && started.elapsed() < Duration::from_secs(20) {
            consume(&mut feeder, 1, &mut Vec::new());
        }
        assert_eq!(shared.cleared(), 2);
        let quiet = |frames: &[[f32; 2]]| frames.iter().all(|f| f[0].abs() < 1e-6 && f[1].abs() < 1e-6);
        let mut before = Vec::new();
        consume_real(&mut feeder, &shared, 40, &mut before);
        assert!(quiet(&before), "stopped and nothing played: silence");
        job.audition(Audition { thing: shape, note: 60, velocity: 0.8, on: true });
        let mut heard = Vec::new();
        let started = Instant::now();
        while heard.len() < 4_800 && started.elapsed() < Duration::from_secs(10) {
            consume_real(&mut feeder, &shared, 10, &mut heard);
        }
        assert!(!quiet(&heard), "the key sounds while stopped ({} frames came)", heard.len());
        assert_eq!(job.clock(), 1.5, "the playhead stays put");
        // It rings out and the thread stops rendering.
        consume(&mut feeder, 1_500, &mut Vec::new());
        let mut after = Vec::new();
        consume_real(&mut feeder, &shared, 50, &mut after);
        assert!(quiet(&after), "quiet again once the note is over");
        assert_eq!(job.clock(), 1.5);
    }
}
