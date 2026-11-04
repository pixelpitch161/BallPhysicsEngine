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
/// How long the thread waits for the speakers to clear the ring after a seek.
const CLEAR_WAIT: Duration = Duration::from_millis(250);

/// A note played by hand from the editor (part 2's keyboard and pads).
#[allow(dead_code)]
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
    #[allow(dead_code)]
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
}

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
        SoundJob { outbox: Outbox::Thread { tx, thread }, shared, meters, clear: 0, base: 0.0, _out: None }
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
        self.send(Command::Play);
    }

    pub fn pause(&mut self) {
        self.clear += 1;
        let clear = self.clear;
        self.send(Command::Pause(clear));
    }

    /// Goes to `time`; what the speakers still held is dropped.
    pub fn seek(&mut self, time: f64) {
        self.clear += 1;
        self.base = block_floor(time);
        let clear = self.clear;
        self.send(Command::Seek(time, clear));
    }

    pub fn patch(&mut self, patch: SoundPatch) {
        self.send(Command::Patch(Box::new(patch)));
    }

    /// A new engine for a scene whose physics changed. It starts at 0; seek after it.
    pub fn rebuild(&mut self, scene: &Scene, resolved: &Resolved) {
        self.send(Command::Rebuild(Box::new(scene.clone()), Box::new(resolved.clone())));
    }

    #[allow(dead_code)]
    pub fn audition(&mut self, audition: Audition) {
        self.send(Command::Audition(audition));
    }

    /// The time the speakers are at: where the last seek landed plus what has been played since.
    /// Holds still until the speakers have dropped what they held from before the seek.
    pub fn clock(&self) -> f64 {
        if self.shared.cleared() != self.clear {
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

    /// The speakers stopped working.
    pub fn failed(&self) -> bool {
        self.shared.failed()
    }
}

impl Drop for SoundJob {
    // Without tests the outbox is always the thread.
    #[allow(irrefutable_let_patterns)]
    fn drop(&mut self) {
        if let Outbox::Thread { tx, thread } = &mut self.outbox {
            let _ = tx.send(Command::Quit);
            if let Some(thread) = thread.take() {
                let _ = thread.join();
            }
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
                if !comes(&["seek", "pause"]) {
                    self.clear(number);
                }
            }
            Command::Seek(time, number) => {
                if !comes(&["seek", "rebuild"]) {
                    self.engine.seek(time);
                }
                if !comes(&["seek", "pause"]) {
                    self.clear(number);
                }
            }
            Command::Patch(patch) => {
                if !comes(&["patch", "rebuild"]) {
                    self.engine.swap_patch(*patch);
                }
            }
            Command::Rebuild(scene, resolved) => {
                if !comes(&["rebuild"]) {
                    self.engine = Engine::new(&scene, &resolved, &self.kit);
                    self.engine.share_meters(self.meters.clone());
                }
            }
            Command::Audition(_) => {}
            Command::Quit => return false,
        }
        true
    }

    /// Renders blocks until the ring holds [`FILL_FRAMES`].
    fn fill(&mut self) {
        let mut block = [0.0f32; BLOCK * 2];
        let capacity = self.ring.buffer().capacity();
        while capacity - self.ring.slots() < FILL_FRAMES * 2 && self.ring.slots() >= BLOCK * 2 {
            self.engine.render(&mut block);
            if let Ok(mut chunk) = self.ring.write_chunk(BLOCK * 2) {
                let (a, b) = chunk.as_mut_slices();
                a.copy_from_slice(&block[..a.len()]);
                b.copy_from_slice(&block[a.len()..]);
                chunk.commit_all();
            }
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
    let mut engine = Engine::new(&scene, &resolved, &kit);
    engine.share_meters(meters.clone());
    let mut w = Worker { engine, kit, meters, shared, ring, playing: false };
    let mut batch = Vec::new();
    loop {
        let first = if w.playing {
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
        let mut after = Vec::new();
        consume_real(&mut feeder, &shared, 150, &mut after);
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
}
