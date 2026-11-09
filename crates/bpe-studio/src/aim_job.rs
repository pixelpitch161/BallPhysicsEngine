//! Background aiming: when some emitter aims Through bounces, a thread runs its own copy of the
//! scene from the start to its end, so every round's test flights are worked out there (and kept
//! for the scene, `bpe_kit::things::emitter::aim_cache`) instead of on the editor's own playback,
//! which never steps past where this thread has got to while it runs. The preview says
//! "Aiming… 37%" while it waits.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use bpe_core::{Resolved, Scene, Settings};
use bpe_kit::things::emitter::aim_cache::StopWith;
use bpe_kit::things::emitter::{Aim, EmitterSettings, KIND};
use bpe_runtime::{Log, Sims, World};

/// A background run of the scene that aims its rounds through bounces ahead of the playback.
pub struct AimJob {
    /// The moment (seconds, as `f64` bits) its world has got to.
    reached: Arc<AtomicU64>,
    /// It has run to the end (or stopped).
    done: Arc<AtomicBool>,
    /// Tells it to stop.
    stop: Arc<AtomicBool>,
}

impl AimJob {
    /// Whether some emitter in `resolved` aims through bounces.
    pub fn wanted(resolved: &Resolved) -> bool {
        resolved
            .things
            .values()
            .any(|r| r.kind == KIND && EmitterSettings::from_params(&r.params).aim == Aim::ThroughBounces)
    }

    /// Starts one for `scene` up to `duration` seconds, when it is wanted.
    pub fn start_if_wanted(scene: &Scene, resolved: &Resolved, sims: Arc<Sims>, duration: f64) -> Option<AimJob> {
        if !Self::wanted(resolved) {
            return None;
        }
        let reached = Arc::new(AtomicU64::new(0f64.to_bits()));
        let done = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (scene, resolved) = (scene.clone(), resolved.clone());
        let job = AimJob { reached: reached.clone(), done: done.clone(), stop: stop.clone() };
        let spawned = std::thread::Builder::new().name("aiming".into()).spawn(move || {
            crate::thread_priority::set_this_thread(crate::thread_priority::Priority::Low);
            // Done however it ends, a panic included, so the playback is never held for good.
            let _done = Finished(done);
            // Told to stop, its test flights give up and keep nothing.
            let _stop = StopWith::new(Some(stop.clone()));
            // Nothing draws this run, so it keeps no trail paths.
            let mut world = World::build_unseen(&scene, &resolved, &sims);
            let until = bpe_runtime::step_at(duration, world.steps_per_second);
            let mut log = Log::default();
            while world.step < until && !stop.load(Ordering::Relaxed) {
                world.step(&sims, &mut log);
                reached.store(world.time().to_bits(), Ordering::Release);
            }
        });
        // No thread: the playback aims for itself.
        spawned.ok().map(|_| job)
    }

    fn reached(&self) -> f64 {
        f64::from_bits(self.reached.load(Ordering::Acquire))
    }

    fn done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// How far the playback may go towards `time` (seconds) now: all the way once the thread is
    /// done or past it, else up to where the thread has got.
    pub fn allowed(&self, time: f64) -> f64 {
        if self.done() { time } else { time.min(self.reached()) }
    }

    /// How far (0..1) the thread has got towards `time`, while it holds the playback back.
    pub fn holding(&self, time: f64) -> Option<f32> {
        let reached = self.reached();
        (!self.done() && reached < time).then(|| (reached / time.max(1e-9)).clamp(0.0, 1.0) as f32)
    }
}

/// Marks the run done when it ends.
struct Finished(Arc<AtomicBool>);

impl Drop for Finished {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl Drop for AimJob {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_aiming_through_bounces_wants_one() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let id = bpe_kit::add_thing(&mut scene, &kit.registry, KIND, bpe_core::ThingId::ROOT).unwrap();
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        assert!(!AimJob::wanted(&resolved));
        assert!(AimJob::start_if_wanted(&scene, &resolved, kit.sims.clone(), 1.0).is_none());
        for (aim, wanted) in [(Aim::StraightIn, false), (Aim::KeepMyDirections, false), (Aim::ThroughBounces, true)] {
            let s = EmitterSettings { aim, ..EmitterSettings::default() };
            scene.things.get_mut(&id).unwrap().params = s.to_params();
            let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
            assert_eq!(AimJob::wanted(&resolved), wanted, "{aim:?}");
        }
    }

    #[test]
    fn it_holds_the_playback_back_until_it_has_got_there() {
        let kit = bpe_kit::kit();
        let mut scene = bpe_kit::new_studio_scene(&kit.registry);
        let id = bpe_kit::add_thing(&mut scene, &kit.registry, KIND, bpe_core::ThingId::ROOT).unwrap();
        let s = EmitterSettings { aim: Aim::ThroughBounces, count: 2, ..EmitterSettings::default() };
        scene.things.get_mut(&id).unwrap().params = s.to_params();
        let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
        let job = AimJob::start_if_wanted(&scene, &resolved, kit.sims.clone(), 0.5).expect("wanted");
        // Never past where it has got.
        let allowed = job.allowed(0.5);
        assert!(allowed <= 0.5);
        let started = std::time::Instant::now();
        while job.holding(0.5).is_some() {
            assert!(started.elapsed().as_secs() < 60, "it gets there");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(job.allowed(0.5), 0.5);
    }
}
