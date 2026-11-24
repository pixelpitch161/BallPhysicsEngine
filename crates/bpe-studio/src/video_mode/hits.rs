//! The Scene sound lane's hits (part 3 plan, decision 13): a thread of its own runs each clip's
//! scene copy (physics only) as far as the clip plays it and lists its hits in video time. A
//! clip whose scene copy and span were run before (by it or another clip of the same scene) is
//! not run again; a newer video stops the run in progress.

use bpe_core::Scene;
use bpe_core::video::{ClipId, Video};
use bpe_kit::Kit;
use bpe_runtime::events::{EventKind, Log};
use bpe_runtime::world::World;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

/// Runs kept for later: a few scenes' worth.
const KEEP: usize = 16;

/// What the worker sends back: a clip's hits (video seconds) at a revision, or that a revision
/// is all done.
enum Found {
    Clip(u64, ClipId, Vec<f32>),
    Done(u64),
}

struct Worker {
    asks: Sender<(u64, Box<Video>)>,
    found: Receiver<Found>,
    /// The newest revision asked for: an older run gives up.
    latest: Arc<AtomicU64>,
}

/// The hits of the video's clips, as last found.
#[derive(Default)]
pub struct Hits {
    /// Find hits at all (the windowed editor, and tests that ask for it).
    pub enabled: bool,
    worker: Option<Worker>,
    asked: Option<u64>,
    /// Each clip's hits, in video seconds.
    pub clips: BTreeMap<ClipId, Vec<f32>>,
    /// Hits are being found for the video as it is now.
    pub busy: bool,
}

impl Hits {
    /// Asks for the hits of `video` (tidied) at `revision`, unless that is what was asked last.
    pub fn ask(&mut self, video: &Video, revision: u64, kit: &Arc<Kit>) {
        if !self.enabled || self.asked == Some(revision) {
            return;
        }
        self.asked = Some(revision);
        self.busy = true;
        let worker = self.worker.get_or_insert_with(|| start(kit.clone()));
        worker.latest.store(revision, Ordering::Relaxed);
        if worker.asks.send((revision, Box::new(video.clone()))).is_err() {
            self.worker = None;
            self.busy = false;
        }
    }

    /// Takes what came back. True when something did.
    pub fn take(&mut self, video: &Video) -> bool {
        let Some(worker) = &self.worker else { return false };
        let mut any = false;
        loop {
            match worker.found.try_recv() {
                Ok(Found::Clip(revision, id, hits)) if Some(revision) == self.asked => {
                    self.clips.insert(id, hits);
                    any = true;
                }
                Ok(Found::Done(revision)) if Some(revision) == self.asked => {
                    self.busy = false;
                    // Clips that are gone lose their hits.
                    self.clips.retain(|id, _| video.clips.iter().any(|c| c.id == *id));
                    any = true;
                }
                Ok(_) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.busy = false;
                    break;
                }
            }
        }
        any
    }

    /// Waits until the hits of the last ask are all in (tests).
    #[cfg(test)]
    pub fn wait(&mut self, video: &Video) {
        let started = std::time::Instant::now();
        while self.busy && started.elapsed() < std::time::Duration::from_secs(120) {
            self.take(video);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!self.busy, "the hits never came");
    }
}

/// A scene's run: the scene copy, how far it was run (seconds) and its hits (scene seconds).
struct Run {
    scene: Scene,
    to: f64,
    hits: Vec<f32>,
}

fn start(kit: Arc<Kit>) -> Worker {
    let (asks, received) = channel::<(u64, Box<Video>)>();
    let (told, found) = channel();
    let latest = Arc::new(AtomicU64::new(0));
    let theirs = latest.clone();
    let card = bpe_gfx::swarm::on_this_thread();
    let _ = std::thread::Builder::new().name("ball-studio video hits".into()).spawn(move || {
        crate::thread_priority::set_this_thread(crate::thread_priority::Priority::Low);
        bpe_gfx::swarm::use_on_this_thread(card);
        // Liquids at their Export detail, as the export and its sound run them.
        let _export = bpe_kit::things::liquid::exporting_for_now();
        let mut runs: Vec<Run> = Vec::new();
        while let Ok(mut ask) = received.recv() {
            while let Ok(newer) = received.try_recv() {
                ask = newer;
            }
            let (revision, video) = ask;
            let stale = || theirs.load(Ordering::Relaxed) != revision;
            let mut finished = true;
            for clip in &video.clips {
                if stale() {
                    finished = false;
                    break;
                }
                let scene = bpe_kit::video::clip_scene(&video, clip, &kit.registry);
                let to = clip.offset + clip.length;
                let at = runs.iter().position(|r| r.to >= to && r.scene == scene);
                let hits = match at {
                    Some(i) => runs[i].hits.clone(),
                    None => match run(&scene, &kit, to, &stale) {
                        Some(hits) => {
                            runs.retain(|r| r.scene != scene);
                            runs.push(Run { scene, to, hits: hits.clone() });
                            if runs.len() > KEEP {
                                runs.remove(0);
                            }
                            hits
                        }
                        None => {
                            finished = false;
                            break;
                        }
                    },
                };
                let shift = clip.start - clip.offset;
                let in_clip: Vec<f32> = hits
                    .iter()
                    .map(|&h| f64::from(h))
                    .filter(|&h| h >= clip.offset && h < to)
                    .map(|h| (h + shift) as f32)
                    .collect();
                if told.send(Found::Clip(revision, clip.id, in_clip)).is_err() {
                    return;
                }
            }
            if finished && told.send(Found::Done(revision)).is_err() {
                return;
            }
        }
    });
    Worker { asks, found, latest }
}

/// The hits (scene seconds) of `scene` run from 0 to `to`, or `None` once `stale` says so.
fn run(scene: &Scene, kit: &Kit, to: f64, stale: &dyn Fn() -> bool) -> Option<Vec<f32>> {
    let resolved = bpe_kit::resolve_scene(scene, &kit.registry);
    let mut world = World::build(scene, &resolved, &kit.sims);
    let mut log = Log::default();
    let mut t = 0.0;
    while t < to {
        if stale() {
            return None;
        }
        t = (t + 1.0).min(to);
        world.advance_to(t, &kit.sims, &mut log);
    }
    let rate = world.steps_per_second;
    Some(log.events().iter().filter(|e| e.kind == EventKind::Hit).map(|e| e.time(rate) as f32).collect())
}
