//! Where each ball has been lately, for trails. Recorded a few times per frame's worth of
//! steps, kept for the last couple of seconds, and thrown away as new samples arrive.

use glam::Vec2;

/// Samples kept per second.
pub const SAMPLE_RATE: u32 = 60;
/// Samples kept per ball: two seconds' worth.
pub const SAMPLES: usize = 120;

/// The last `SAMPLES` positions of every ball of a thing, one ring per ball.
#[derive(Clone, Debug, PartialEq)]
pub struct History {
    /// Steps between samples.
    every: u64,
    /// `SAMPLES` slots per ball, ball-major.
    rings: Vec<Vec2>,
    /// The slot the newest sample is in.
    head: usize,
    /// How many samples each ball has, up to `SAMPLES`.
    count: usize,
}

impl History {
    /// A history that samples `SAMPLE_RATE` times a second of `steps_per_second` steps.
    pub fn new(steps_per_second: u32) -> Self {
        Self { every: (steps_per_second / SAMPLE_RATE).max(1) as u64, rings: Vec::new(), head: 0, count: 0 }
    }

    /// Seconds between samples.
    pub fn interval(&self, dt: f32) -> f32 {
        self.every as f32 * dt
    }

    /// Records `positions` when `step` is a sampling step. A ball that is new since the last
    /// sample starts with its current position in every slot.
    pub fn record(&mut self, step: u64, positions: &[Vec2]) {
        if step % self.every != 0 {
            return;
        }
        let balls = self.rings.len() / SAMPLES;
        if positions.len() > balls {
            for &p in &positions[balls..] {
                self.rings.extend(std::iter::repeat_n(p, SAMPLES));
            }
        }
        self.head = (self.head + 1) % SAMPLES;
        self.count = (self.count + 1).min(SAMPLES);
        for (ball, &p) in positions.iter().enumerate() {
            self.rings[ball * SAMPLES + self.head] = p;
        }
    }

    /// Ball `ball`'s samples, newest first.
    pub fn points(&self, ball: usize) -> impl Iterator<Item = Vec2> + '_ {
        let start = ball * SAMPLES;
        let ring = self.rings.get(start..start + SAMPLES).unwrap_or(&[]);
        let (head, count) = (self.head, if ring.is_empty() { 0 } else { self.count });
        (0..count).map(move |i| ring[(head + SAMPLES - i) % SAMPLES])
    }

    /// How many samples each ball has so far.
    pub fn len(&self) -> usize {
        if self.rings.is_empty() { 0 } else { self.count }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes the history takes, for playback budgets.
    pub fn memory(&self) -> usize {
        self.rings.capacity() * std::mem::size_of::<Vec2>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_history_keeps_the_newest_samples_first_and_drops_the_oldest() {
        let mut h = History::new(240);
        assert_eq!(h.interval(1.0 / 240.0), 1.0 / 60.0);
        assert!(h.points(0).next().is_none());
        for step in 1..=(4 * (SAMPLES as u64 + 10)) {
            h.record(step, &[Vec2::new(step as f32, 0.0), Vec2::new(0.0, step as f32)]);
        }
        assert_eq!(h.len(), SAMPLES);
        let newest: Vec<Vec2> = h.points(0).take(3).collect();
        let last = 4.0 * (SAMPLES as f32 + 10.0);
        assert_eq!(newest, [Vec2::new(last, 0.0), Vec2::new(last - 4.0, 0.0), Vec2::new(last - 8.0, 0.0)]);
        assert_eq!(h.points(1).next(), Some(Vec2::new(0.0, last)));
        assert_eq!(h.points(0).count(), SAMPLES);
        assert_eq!(h.points(0).last(), Some(Vec2::new(last - 4.0 * (SAMPLES as f32 - 1.0), 0.0)));
        // A ball that isn't there has no history, and the memory grows with the balls.
        assert_eq!(h.points(5).count(), 0);
        assert_eq!(h.memory(), 2 * SAMPLES * 8);
    }

    #[test]
    fn a_new_ball_starts_where_it_is() {
        let mut h = History::new(60);
        h.record(1, &[Vec2::ZERO]);
        h.record(2, &[Vec2::ONE, Vec2::new(5.0, 5.0)]);
        assert_eq!(h.points(1).count(), 2);
        assert!(h.points(1).all(|p| p == Vec2::new(5.0, 5.0)));
        assert_eq!(h.points(0).collect::<Vec<_>>(), [Vec2::ONE, Vec2::ZERO]);
    }
}
