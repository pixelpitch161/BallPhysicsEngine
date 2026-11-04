//! Hits placed on their exact samples.

use bpe_runtime::{Event, EventKind, Log};
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

/// Reads a physics log as it grows and hands out each block's hits.
#[derive(Clone, Debug)]
pub struct HitCursor {
    samples_per_step: f64,
    /// How much of the log has been read.
    read: usize,
    /// Hits read but not yet handed out, by sample.
    waiting: VecDeque<(u64, Event)>,
}

impl HitCursor {
    pub fn new(steps_per_second: u32) -> Self {
        Self {
            samples_per_step: crate::SAMPLE_RATE as f64 / steps_per_second.max(1) as f64,
            read: 0,
            waiting: VecDeque::with_capacity(1024),
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

    /// Takes the hits `log` gained since the last call.
    pub fn read(&mut self, log: &Log) {
        let events = log.events();
        if self.read > events.len() {
            self.read = events.len();
        }
        for e in &events[self.read..] {
            if e.kind == EventKind::Hit {
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
    /// ones are passed over.
    pub fn seek(&mut self, log: &Log, from: u64) {
        let events = log.events();
        // Step k's hits land in ((k - 1) × sps, k × sps], so steps before this one end earlier.
        let first_step = (from as f64 / self.samples_per_step).floor() as u64;
        self.read = events.partition_point(|e| e.step < first_step);
        self.waiting.clear();
        self.read(log);
        while self.waiting.front().is_some_and(|(at, _)| *at < from) {
            self.waiting.pop_front();
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
    use bpe_core::ThingId;
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
        c.read(&log);
        let mut out = Vec::with_capacity(16);
        c.take_block(0, 64, &mut out);
        assert!(out.is_empty());
        c.take_block(64, 64, &mut out);
        assert_eq!(out.iter().map(|h| h.offset).collect::<Vec<_>>(), [16]);
        c.take_block(128, 128, &mut out);
        assert_eq!(out.iter().map(|h| h.offset).collect::<Vec<_>>(), [82, 92], "sorted by sample");
    }
}
