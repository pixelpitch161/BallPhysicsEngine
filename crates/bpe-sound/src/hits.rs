//! Hits placed on their exact samples.

use bpe_core::{Scene, ThingId};
use bpe_runtime::{Event, EventKind, Log, Party, World};
use std::collections::VecDeque;

/// One hit in the block being rendered.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockHit {
    /// The sample within the block it lands on.
    pub offset: u32,
    pub event: Event,
}

/// The sample (from the start, at 48 kHz) a hit lands on, for steps `samples_per_step` long:
/// the start of its step plus how far through the step it happened.
pub fn hit_sample(event: &Event, samples_per_step: f64) -> u64 {
    let step_start = event.step.saturating_sub(1) as f64 * samples_per_step;
    (step_start + event.sample_in_step(samples_per_step)).round().max(0.0) as u64
}

/// Whether events of `kind` reach the voices: hits, the notes rules play, the moments a
/// Particles thing's sound plays, and the fires, catches and breaks a Balls or Emitter thing's
/// sound plays (W10). Each voice's [`crate::voices::HitFilter`] takes only its own kinds.
pub fn plays(kind: &EventKind) -> bool {
    touches(kind) || matches!(kind, EventKind::Spawn | EventKind::Enter | EventKind::Break)
}

/// Whether events of `kind` are touches on a thing: hits, rules' notes and particles' moments
/// (what a Hits sidechain keys on).
pub fn touches(kind: &EventKind) -> bool {
    matches!(kind, EventKind::Hit | EventKind::Note | EventKind::Moment)
}

/// Whether `event` is on (or by) one of the things in `heard` (sorted): only those can sound,
/// so only those wait in the queue (a crowd's ball-on-ball hits would otherwise fill it).
pub fn is_heard(event: &Event, heard: &[ThingId]) -> bool {
    heard.binary_search(&event.a.thing).is_ok() || event.b.is_some_and(|b| heard.binary_search(&b.thing).is_ok())
}

/// The balls each emitter of `scene` fires at 0 s, as `Spawn` events on sound sample 0 (W10).
/// They are there from the first frame, so no step logs them; an emitter's sound that plays
/// its fires hears them from here. `world` is the scene's world as built.
pub fn start_fires(scene: &Scene, world: &World) -> Vec<Event> {
    let mut out = Vec::new();
    for (thing, b) in &world.bodies {
        if scene.things.get(thing).is_none_or(|t| t.kind != bpe_kit::things::emitter::KIND) {
            continue;
        }
        for (i, (&position, &velocity)) in b.position.iter().zip(&b.velocity).enumerate() {
            if !b.alive.get(i).copied().unwrap_or(false) {
                continue;
            }
            out.push(Event {
                step: 0,
                kind: EventKind::Spawn,
                a: Party { thing: *thing, item: i as u32 },
                b: Some(Party { thing: *thing, item: 0 }),
                point: position,
                normal: velocity.normalize_or_zero(),
                speed: velocity.length(),
                at: 0.0,
            });
        }
    }
    out
}

/// Reads a physics log as it grows and hands out each block's hits.
#[derive(Clone, Debug)]
pub struct HitCursor {
    samples_per_step: f64,
    /// How much of the log has been read.
    read: usize,
    /// Hits read but not yet handed out, by sample.
    waiting: VecDeque<(u64, Event)>,
    /// What happened before the log starts, on sample 0 ([`start_fires`]).
    start: Vec<Event>,
}

impl HitCursor {
    pub fn new(steps_per_second: u32) -> Self {
        Self {
            samples_per_step: crate::SAMPLE_RATE as f64 / steps_per_second.max(1) as f64,
            read: 0,
            waiting: VecDeque::with_capacity(1024),
            start: Vec::new(),
        }
    }

    /// Sets what happened before the log starts (on sample 0). When nothing has been read yet,
    /// those on things in `heard` (sorted) wait now; a seek back to the start takes them again.
    pub fn start_with(&mut self, start: Vec<Event>, heard: &[ThingId]) {
        self.start = start;
        if self.read == 0 && self.waiting.is_empty() {
            self.wait_for_start(heard);
        }
    }

    /// What happened before the log starts.
    pub fn start(&self) -> &[Event] {
        &self.start
    }

    fn wait_for_start(&mut self, heard: &[ThingId]) {
        let first = self.waiting.partition_point(|(s, _)| *s == 0);
        for (k, e) in self.start.iter().filter(|e| is_heard(e, heard)).enumerate() {
            self.waiting.insert(first + k, (0, e.clone()));
        }
    }

    pub fn samples_per_step(&self) -> f64 {
        self.samples_per_step
    }

    /// Starts reading again from the end of `log` (after a seek), forgetting waiting hits.
    pub fn skip_to_end(&mut self, log: &Log) {
        self.read = log.len();
        self.waiting.clear();
    }

    /// Takes the hits `log` gained since the last call that are on a thing in `heard` (sorted).
    pub fn read(&mut self, log: &Log, heard: &[ThingId]) {
        let events = log.events();
        if self.read > events.len() {
            self.read = events.len();
        }
        for e in &events[self.read..] {
            // Hits, the notes rules play (rules part 6) and particles' moments (particles part 7),
            // each on its sample.
            if plays(&e.kind) && is_heard(e, heard) {
                let at = hit_sample(e, self.samples_per_step);
                // Hits come in step order but can land out of order within a step.
                let i = self.waiting.partition_point(|(s, _)| *s <= at);
                self.waiting.insert(i, (at, e.clone()));
            }
        }
        self.read = events.len();
    }

    /// Moves the hits that land before `start + len` into `out` (cleared first), with their
    /// offsets in the block that starts at `start`; any that are already past land at 0.
    /// Starts over at sound sample `from`: the hits in `log` from there on wait, the earlier
    /// ones are passed over. Only hits on things in `heard` (sorted) wait.
    pub fn seek(&mut self, log: &Log, from: u64, heard: &[ThingId]) {
        let events = log.events();
        // Step k's hits land in ((k - 1) × sps, k × sps], so steps before this one end earlier.
        let first_step = (from as f64 / self.samples_per_step).floor() as u64;
        self.read = events.partition_point(|e| e.step < first_step);
        self.waiting.clear();
        self.read(log, heard);
        while self.waiting.front().is_some_and(|(at, _)| *at < from) {
            self.waiting.pop_front();
        }
        if from == 0 {
            self.wait_for_start(heard);
        }
    }

    pub fn take_block(&mut self, start: u64, len: u64, out: &mut Vec<BlockHit>) {
        out.clear();
        while let Some((at, _)) = self.waiting.front() {
            if *at >= start + len || out.len() == out.capacity() {
                break;
            }
            let (at, event) = self.waiting.pop_front().expect("just looked");
            out.push(BlockHit { offset: at.saturating_sub(start) as u32, event });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_runtime::Party;
    use glam::Vec2;

    fn hit(step: u64, at: f32) -> Event {
        Event {
            step,
            kind: EventKind::Hit,
            a: Party { thing: ThingId(2), item: 0 },
            b: Some(Party { thing: ThingId(1), item: 0 }),
            point: Vec2::ZERO,
            normal: Vec2::Y,
            speed: 100.0,
            at,
        }
    }

    #[test]
    fn hits_land_on_their_sample_in_their_block() {
        assert_eq!(hit_sample(&hit(1, 0.0), 200.0), 0);
        assert_eq!(hit_sample(&hit(3, 0.5), 200.0), 500);
        let mut log = Log::default();
        log.push(hit(1, 0.4));
        log.push(hit(2, 0.1));
        log.push(hit(2, 0.05));
        let mut c = HitCursor::new(240);
        c.read(&log, &[ThingId(1), ThingId(2)]);
        let mut out = Vec::with_capacity(16);
        c.take_block(0, 64, &mut out);
        assert!(out.is_empty());
        c.take_block(64, 64, &mut out);
        assert_eq!(out.iter().map(|h| h.offset).collect::<Vec<_>>(), [16]);
        c.take_block(128, 128, &mut out);
        assert_eq!(out.iter().map(|h| h.offset).collect::<Vec<_>>(), [82, 92], "sorted by sample");
    }

    #[test]
    fn only_hits_on_heard_things_wait() {
        let mut log = Log::default();
        log.push(hit(1, 0.4));
        let mut ball_on_ball = hit(1, 0.5);
        ball_on_ball.b = Some(Party { thing: ThingId(3), item: 4 });
        log.push(ball_on_ball);
        let mut c = HitCursor::new(240);
        c.read(&log, &[ThingId(1)]);
        let mut out = Vec::with_capacity(16);
        c.take_block(0, 1_000, &mut out);
        assert_eq!(out.len(), 1, "the hit on the heard shape, not the balls between themselves");
        assert_eq!(out[0].event.b.unwrap().thing, ThingId(1));
        let mut c = HitCursor::new(240);
        c.read(&log, &[]);
        c.take_block(0, 1_000, &mut out);
        assert!(out.is_empty(), "nothing heard: nothing waits");
    }
}
