//! A world plus its log, which can be moved to any point in time.
//!
//! Seeking backwards restarts from the latest checkpoint before the new time. The world is
//! deterministic, so this gives exactly the same result as replaying from the start.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bpe_core::{Resolved, Scene};

use crate::{Event, Log, Sims, World};

/// Seconds of simulation between checkpoints, at first.
const CHECKPOINT_SECONDS: f64 = 0.5;
/// Most memory the checkpoints of one playback may use. Past this, every other one is dropped
/// and they are taken half as often.
const CHECKPOINT_BYTES: usize = 192 << 20;
/// How often a budgeted seek looks at the clock.
const BUDGET_EVERY: u64 = 8;

/// A world and everything that has happened in it so far.
#[derive(Clone)]
pub struct Playback {
    world: World,
    log: Log,
    sims: Arc<Sims>,
    /// Snapshots in step order, each with how long the log was at the time.
    checkpoints: Vec<(World, usize)>,
    checkpoint_every: u64,
}

impl Playback {
    pub fn new(scene: &Scene, resolved: &Resolved, sims: Arc<Sims>) -> Self {
        let world = World::build(scene, resolved, &sims);
        let checkpoint_every = (CHECKPOINT_SECONDS * world.steps_per_second.max(1) as f64).ceil() as u64;
        Self {
            checkpoints: vec![(world.clone(), 0)],
            world,
            log: Log::default(),
            sims,
            checkpoint_every: checkpoint_every.max(1),
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    pub fn sims(&self) -> &Arc<Sims> {
        &self.sims
    }

    /// Seconds since the start.
    pub fn time(&self) -> f64 {
        self.world.time()
    }

    /// The events in the last `seconds` up to now.
    pub fn recent(&self, seconds: f64) -> &[Event] {
        let rate = self.world.steps_per_second as f64;
        let since = ((self.time() - seconds) * rate).floor().max(0.0) as u64;
        let start = self.log.events().partition_point(|e| e.step <= since);
        &self.log.events()[start..]
    }

    pub fn seek(&mut self, time: f64) {
        self.seek_within(time, None);
    }

    /// Like [`Playback::seek`], but stops after about `budget` of work if it hasn't got there
    /// yet, so the editor can keep drawing while a long stretch is worked out. Returns whether
    /// the playback reached `time`.
    pub fn seek_within(&mut self, time: f64, budget: Option<Duration>) -> bool {
        let target = (time * self.world.steps_per_second as f64).floor() as u64;
        if target < self.world.step {
            let index = self.checkpoints.partition_point(|(w, _)| w.step <= target).max(1) - 1;
            let (world, len) = &self.checkpoints[index];
            self.world = world.clone();
            self.log.truncate(*len);
            self.log.truncate_camera(self.world.step);
        }
        let started = budget.map(|_| Instant::now());
        while self.world.step < target {
            self.world.step(&self.sims, &mut self.log);
            if self.world.step.is_multiple_of(self.checkpoint_every)
                && self.checkpoints.last().is_some_and(|(w, _)| w.step < self.world.step)
            {
                self.save_checkpoint();
            }
            if let (Some(started), Some(budget)) = (started, budget)
                && self.world.step.is_multiple_of(BUDGET_EVERY)
                && started.elapsed() >= budget
            {
                return self.world.step >= target;
            }
        }
        true
    }

    /// Bytes the playback holds: the world, its log and every checkpoint.
    pub fn memory(&self) -> usize {
        self.world.memory() + self.log.memory() + self.checkpoints.iter().map(|(w, _)| w.memory()).sum::<usize>()
    }

    fn save_checkpoint(&mut self) {
        self.checkpoints.push((self.world.clone(), self.log.len()));
        let bytes: usize = self.checkpoints.iter().map(|(w, _)| w.memory()).sum();
        if bytes > CHECKPOINT_BYTES && self.checkpoints.len() > 2 {
            // Keep the first one and every other one after it, and take them half as often.
            self.checkpoint_every *= 2;
            let every = self.checkpoint_every;
            self.checkpoints.retain(|(w, _)| w.step.is_multiple_of(every));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{DOTS, dots_in_ring};
    use std::time::Duration;

    #[test]
    fn seeking_back_and_forth_matches_a_straight_run() {
        let (scene, resolved, sims) = dots_in_ring(20);
        let sims = Arc::new(sims);
        let mut straight = Playback::new(&scene, &resolved, sims.clone());
        straight.seek(6.0);
        let mut jumpy = Playback::new(&scene, &resolved, sims);
        for t in [2.0, 5.5, 1.2, 6.0] {
            jumpy.seek(t);
        }
        assert_eq!(straight.log().hash(), jumpy.log().hash());
        assert_eq!(straight.world().bodies[&DOTS].position, jumpy.world().bodies[&DOTS].position);
        assert_eq!(jumpy.time(), 6.0);
        assert!(!straight.recent(0.5).is_empty());
        assert!(straight.recent(0.5).len() < straight.log().len());
        assert!(straight.recent(0.5).iter().all(|e| e.time(240) > 5.5));
    }

    #[test]
    fn seeking_back_truncates_the_log() {
        let (scene, resolved, sims) = dots_in_ring(5);
        let mut p = Playback::new(&scene, &resolved, Arc::new(sims));
        p.seek(4.0);
        let at_four = p.log().len();
        p.seek(1.0);
        assert!(p.log().len() < at_four);
        assert!(p.log().events().iter().all(|e| e.step <= 240));
        p.seek(0.0);
        assert!(p.log().is_empty());
        assert_eq!(p.world().step, 0);
    }

    #[test]
    fn seek_within_stops_at_its_budget() {
        let (scene, resolved, sims) = dots_in_ring(40_000);
        let mut p = Playback::new(&scene, &resolved, Arc::new(sims));
        let done = p.seek_within(20.0, Some(Duration::from_millis(20)));
        assert!(!done && p.time() > 0.0 && p.time() < 20.0);
        assert!(p.seek_within(p.time(), Some(Duration::from_millis(20))));
        assert!(p.memory() > 0);
    }
}
