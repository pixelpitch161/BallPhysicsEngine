//! The particle playback: when the scene has Particles things, a thread runs its own playback of
//! the whole scene (the physics and the particles) and follows the playhead, while the editor's
//! own playback runs the physics alone (`bpe_runtime::main_part`). After each stretch it works out
//! it publishes the particles as they stand ([`Snapshot`]); the preview lends them to its world
//! to draw ([`bpe_runtime::Playback::lend_states`]). Both are built from the same scene and step
//! the balls the same way, so the particles drawn sit with the balls exactly once it has caught
//! up. A particle edit starts this thread over and never the physics or the sound.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::Thread;
use std::time::Duration;

use bpe_core::{Resolved, Scene, ThingId};
use bpe_kit::things::particles::state::ParticlesState;
use bpe_runtime::{Playback, Sims, ThingState};

/// The particle playback's own checkpoint budget, beside the main playback's.
const BUDGET: usize = 512 * 1024 * 1024;

/// How long the thread works before publishing what it has.
const STRETCH: Duration = Duration::from_millis(40);

/// The particles at one moment of the run.
#[derive(Clone)]
pub struct Snapshot {
    /// The world step they are at.
    pub step: u64,
    /// The moment, in seconds.
    pub time: f64,
    /// Each Particles thing's state.
    pub states: Vec<(ThingId, Box<dyn ThingState>)>,
    /// How many particles there are in all.
    pub count: usize,
    /// How long their last particle step took, in milliseconds, all things together.
    pub step_ms: f32,
}

impl Snapshot {
    fn of(playback: &Playback) -> Snapshot {
        let world = playback.world();
        let mut states = Vec::new();
        let (mut count, mut step_ms) = (0, 0.0);
        // The air too, for Show air (and to draw with the particles).
        let air = world.states.get(&ThingId::ROOT).filter(|s| (***s).as_any().is::<bpe_kit::air::Air>());
        if let Some(air) = air {
            states.push((ThingId::ROOT, air.clone()));
        }
        for id in &world.beside {
            if let Some(state) = world.states.get(id) {
                if let Some(p) = (**state).as_any().downcast_ref::<ParticlesState>() {
                    count += p.specks.len();
                    step_ms += p.last_ms;
                }
                states.push((*id, state.clone()));
            }
        }
        Snapshot { step: world.step, time: world.time(), states, count, step_ms }
    }
}

/// What the thread and the editor share.
struct Shared {
    /// The moment (seconds, as `f64` bits) the editor wants.
    target: AtomicU64,
    stop: AtomicBool,
    latest: Mutex<Option<Arc<Snapshot>>>,
}

/// A background playback of the whole scene, for its particles.
pub struct ParticleJob {
    shared: Arc<Shared>,
    thread: Thread,
}

impl ParticleJob {
    /// Starts one for `scene` when it has things stepped beside the physics, at `time`.
    pub fn start_if_wanted(scene: &Scene, resolved: &Resolved, sims: Arc<Sims>, time: f64) -> Option<ParticleJob> {
        if !bpe_runtime::has_beside(resolved, &sims) {
            return None;
        }
        let shared = Arc::new(Shared {
            target: AtomicU64::new(time.to_bits()),
            stop: AtomicBool::new(false),
            latest: Mutex::new(None),
        });
        let (scene, resolved, theirs) = (scene.clone(), resolved.clone(), shared.clone());
        let handle = std::thread::Builder::new()
            .name("particles".into())
            .spawn(move || {
                let mut playback = Playback::with_budget(&scene, &resolved, sims, BUDGET);
                let mut published = None;
                while !theirs.stop.load(Ordering::Relaxed) {
                    let target = f64::from_bits(theirs.target.load(Ordering::Acquire));
                    let there = playback.seek_within(target, Some(STRETCH));
                    let step = playback.world().step;
                    if published != Some(step) {
                        published = Some(step);
                        let snapshot = Arc::new(Snapshot::of(&playback));
                        if let Ok(mut latest) = theirs.latest.lock() {
                            *latest = Some(snapshot);
                        }
                    }
                    if there {
                        // Woken when the editor wants another moment.
                        std::thread::park_timeout(Duration::from_millis(100));
                    }
                }
            })
            .ok()?;
        Some(ParticleJob { shared, thread: handle.thread().clone() })
    }

    /// Asks for the particles at `time` (seconds).
    pub fn follow(&self, time: f64) {
        let old = self.shared.target.swap(time.to_bits(), Ordering::AcqRel);
        if old != time.to_bits() {
            self.thread.unpark();
        }
    }

    /// The particles as the thread last published them.
    pub fn latest(&self) -> Option<Arc<Snapshot>> {
        self.shared.latest.lock().ok().and_then(|l| l.clone())
    }

    /// The moment (seconds) the thread has got to, `None` before it has published anything.
    pub fn reached(&self) -> Option<f64> {
        self.latest().map(|s| s.time)
    }
}

impl Drop for ParticleJob {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.thread.unpark();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_kit::things::particles::looks::{Look, add_particles};

    fn wait_for(job: &ParticleJob, time: f64) -> Arc<Snapshot> {
        let started = std::time::Instant::now();
        loop {
            if let Some(s) = job.latest().filter(|s| (s.time - time).abs() < 1e-6) {
                return s;
            }
            assert!(started.elapsed().as_secs() < 60, "it gets there");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn it_follows_the_playhead_and_publishes_the_particles() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        assert!(
            ParticleJob::start_if_wanted(&scene, &bpe_kit::resolve_scene(&scene, &kit.registry), kit.sims.clone(), 0.0)
                .is_none(),
            "no particles, no thread"
        );
        let id = add_particles(&mut scene, &kit.registry, Look::Snow, ThingId::ROOT).unwrap();
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        let job = ParticleJob::start_if_wanted(&scene, &resolved, kit.sims.clone(), 1.0).expect("wanted");
        let at_one = wait_for(&job, 1.0);
        assert_eq!(at_one.states.len(), 1);
        assert_eq!(at_one.states[0].0, id);
        assert!(at_one.count > 0);
        job.follow(2.0);
        let at_two = wait_for(&job, 2.0);
        assert!(at_two.step > at_one.step);
        // Back again: the same particles as the first time.
        job.follow(1.0);
        let again = wait_for(&job, 1.0);
        let hash = |s: &Snapshot| {
            let st = (*s.states[0].1).as_any().downcast_ref::<ParticlesState>().unwrap();
            bpe_kit::things::particles::step::hash_of(st)
        };
        assert_eq!(hash(&again), hash(&at_one));
    }
}
