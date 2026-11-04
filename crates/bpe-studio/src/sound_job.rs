//! The sound thread: it owns the sound [`Engine`], keeps the speakers' ring filled a little ahead
//! and takes the editor's commands. The editor's playhead follows the sound the speakers have
//! played ([`SoundJob::clock`]).

use crate::audio_out::{AudioOut, RATE, Shared};
use bpe_core::{Resolved, Scene, ThingId};
use bpe_kit::Kit;
use bpe_sound::{BLOCK, Engine, Meters, SoundPatch};
use rtrb::Producer;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
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
    /// Stop and clear the ring (clear number `.0`).
    Pause(u64),
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
            Command::Pause(_) => "pause",
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
            .spawn(move || run(rx, ring, thread_shared, kit, scene, resolved, thread_meters))
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

    pub fn pause(&mut self) {
        self.parked = Some(self.clock());
        self.playing = false;
        self.clear += 1;
        let clear = self.clear;
        self.send(Command::Pause(clear));
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

    /// The time the speakers are at: where the last seek landed plus what has been played since.
    /// Holds still until the speakers have dropped what they held from before the seek.
    pub fn clock(&self) -> f64 {
        if !self.playing || self.shared.cleared() != self.clear {
            return self.base;
        }
        self.base + self.shared.played() as f64 / RATE as f64
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
}

impl Worker {
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
            Command::Play => self.playing = true,
            Command::Pause(number) => {
                self.playing = false;
                self.held = false;
                // Stopping stops the sound; a key clicked afterwards starts from quiet.
                self.engine.stop_notes();
                self.tail = 0;
                if !comes(&["seek", "pause"]) {
                    self.clear(number);
                }
            }
            Command::Seek(time, number) => {
                // The old sound stops first (a long pre-roll would otherwise let it play on);
                // nothing new is written until the seek is done.
                if !comes(&["seek", "pause"]) {
                    self.clear(number);
                }
                if !comes(&["seek", "rebuild"]) {
                    self.engine.seek(time);
                    self.held = false;
                }
            }
            Command::Patch(patch) => {
                if !comes(&["patch", "rebuild"]) {
                    self.engine.swap_patch(*patch);
                }
            }
            Command::Rebuild(scene, resolved) => {
                if !comes(&["rebuild"]) {
                    let patch = SoundPatch::build_ready(&scene, &resolved, &self.kit);
                    self.engine = Engine::with_patch(&scene, &resolved, &self.kit, patch);
                    self.engine.share_meters(self.meters.clone());
                    // It starts at 0; the editor's seek to the playhead follows.
                    self.held = true;
                }
            }
            Command::Audition(a) => {
                if a.on {
                    self.engine.audition(a.thing, a.note);
                    self.tail = TAIL_BLOCKS;
                }
            }
            Command::Quit => return false,
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
    let mut w = Worker { engine, kit, meters, shared, ring, playing: false, held: false, tail: 0 };
    let mut batch = Vec::new();
    loop {
        let first = if w.playing || w.tail > 0 {
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
        batch.extend(rx.try_iter());
        let names: Vec<&'static str> = batch.iter().map(Command::name).collect();
        for (i, c) in batch.drain(..).enumerate() {
            if !w.handle(c, &names[i + 1..]) {
                return;
            }
        }
        if w.playing {
            w.fill();
        } else {
            w.fill_paused();
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
    fn a_rebuild_waits_for_its_seek() {
        let kit = Arc::new(bpe_kit::kit());
        let (scene, resolved) = toned(&kit, "test-tone");
        let (ring_in, ring_out) = ring();
        let shared = Arc::new(Shared::default());
        let meters = Arc::new(Meters::default());
        let engine = Engine::new(&scene, &resolved, &kit);
        let mut w =
            Worker { engine, kit, meters, shared: shared.clone(), ring: ring_in, playing: true, held: false, tail: 0 };
        w.handle(Command::Rebuild(Box::new(scene.clone()), Box::new(resolved.clone())), &[]);
        w.fill();
        assert_eq!(ring_out.slots(), 0, "nothing from the new world's start before the seek lands");
        shared.cleared.store(1, std::sync::atomic::Ordering::Release);
        w.handle(Command::Seek(2.0, 1), &[]);
        w.fill();
        assert!(ring_out.slots() > 0);
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
        // Past the 2 ms fade-in, frame for frame the sound at 3 s.
        assert!(expected[96..].iter().any(|f| f[0].abs() > 0.01), "the tone rings at 3 s");
        let worst = after[96..].iter().zip(&expected[96..]).fold(0.0f32, |m, (a, b)| m.max((a[0] - b[0]).abs()));
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
        job.pause();
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
