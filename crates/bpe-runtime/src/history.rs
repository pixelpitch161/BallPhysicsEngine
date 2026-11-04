//! Where each ball has been lately, for trails and particles. Recorded `SAMPLE_RATE` times a
//! second, kept for as many seconds as the thing's longest trail asks for (or for good, for a
//! Line that stays), and thrown away as new samples arrive.

use std::collections::VecDeque;

use glam::Vec2;

/// Samples kept per second.
pub const SAMPLE_RATE: u32 = 60;
/// The fewest seconds a history keeps: a trail's whole Length range, so keyframing it never
/// reaches past what is kept.
pub const LEAST_SECONDS: f32 = 2.0;

/// How much of its balls' past a thing wants kept.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HistoryWant {
    /// How many seconds back, at least [`LEAST_SECONDS`].
    pub seconds: f32,
    /// Everything since the start.
    pub forever: bool,
}

impl HistoryWant {
    /// Both wants at once: the longer, and forever if either is.
    pub fn and(self, other: HistoryWant) -> HistoryWant {
        HistoryWant { seconds: self.seconds.max(other.seconds), forever: self.forever || other.forever }
    }

    /// Its seconds rounded up to a quarter second (at least [`LEAST_SECONDS`]), so a small
    /// change to a length doesn't count as a new history.
    pub fn rounded(self) -> f32 {
        let s = if self.seconds.is_finite() { self.seconds.max(LEAST_SECONDS) } else { LEAST_SECONDS };
        (s * 4.0).ceil() / 4.0
    }
}

/// The recent positions of every ball of a thing, newest last, one list per ball.
#[derive(Clone, Debug, PartialEq)]
pub struct History {
    /// Steps between samples.
    every: u64,
    /// Samples kept per ball; `usize::MAX` keeps every one.
    cap: usize,
    balls: Vec<VecDeque<Vec2>>,
    /// How far each ball has gone from its first sample to its newest.
    travelled: Vec<f32>,
    /// How many samples each ball has, up to `cap`.
    count: usize,
    /// The step the newest sample was taken at.
    last: Option<u64>,
}

impl History {
    /// A history that samples `SAMPLE_RATE` times a second of `steps_per_second` steps and keeps
    /// what `want` asks for.
    pub fn new(steps_per_second: u32, want: HistoryWant) -> Self {
        let cap =
            if want.forever { usize::MAX } else { (want.rounded() * SAMPLE_RATE as f32).ceil() as usize + 1 }.max(2);
        Self {
            every: (steps_per_second / SAMPLE_RATE).max(1) as u64,
            cap,
            balls: Vec::new(),
            travelled: Vec::new(),
            count: 0,
            last: None,
        }
    }

    /// Seconds between samples.
    pub fn interval(&self, dt: f32) -> f32 {
        self.every as f32 * dt
    }

    /// Steps between samples.
    pub fn every(&self) -> u64 {
        self.every
    }

    /// The step the newest sample was taken at.
    pub fn newest_step(&self) -> Option<u64> {
        self.last
    }

    /// Samples it keeps per ball (`usize::MAX` for every one).
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Records `positions` when `step` is a sampling step. A ball that is new since the last
    /// sample starts with its current position in every sample so far.
    pub fn record(&mut self, step: u64, positions: &[Vec2]) {
        if !step.is_multiple_of(self.every) {
            return;
        }
        for &p in positions.iter().skip(self.balls.len()) {
            self.balls.push(std::iter::repeat_n(p, self.count).collect());
            self.travelled.push(0.0);
        }
        let full = self.count >= self.cap;
        if !full {
            self.count += 1;
        }
        for ((ring, gone), &p) in self.balls.iter_mut().zip(&mut self.travelled).zip(positions) {
            if let Some(&last) = ring.back() {
                *gone += (p - last).length();
            }
            if full {
                ring.pop_front();
            }
            ring.push_back(p);
        }
        self.last = Some(step);
    }

    /// Ball `ball`'s samples, newest first.
    pub fn points(&self, ball: usize) -> impl Iterator<Item = Vec2> + '_ {
        self.balls.get(ball).into_iter().flat_map(|r| r.iter().rev().copied())
    }

    /// How far ball `ball` has gone from its first sample to its newest, in world units: a
    /// place along its path that stays put as samples come and go (dots along a trail).
    pub fn travelled(&self, ball: usize) -> f32 {
        self.travelled.get(ball).copied().unwrap_or(0.0)
    }

    /// Where ball `ball` was `ago` seconds before the newest sample, read between samples
    /// `interval` seconds apart; `None` before its first sample.
    pub fn at(&self, ball: usize, ago: f32, interval: f32) -> Option<Vec2> {
        let ring = self.balls.get(ball)?;
        let n = ring.len();
        if n == 0 || interval <= 0.0 {
            return ring.back().copied();
        }
        let k = (ago.max(0.0) / interval).min((n - 1) as f32);
        let (i, f) = (k.floor() as usize, k.fract());
        let a = ring[n - 1 - i];
        let b = ring[n - 1 - (i + 1).min(n - 1)];
        Some(a.lerp(b, f))
    }

    /// How many samples each ball has so far.
    pub fn len(&self) -> usize {
        if self.balls.is_empty() { 0 } else { self.count }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes the history takes, for playback budgets.
    pub fn memory(&self) -> usize {
        self.balls.iter().map(|r| r.capacity() * std::mem::size_of::<Vec2>()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: HistoryWant = HistoryWant { seconds: 2.0, forever: false };

    #[test]
    fn a_history_keeps_the_newest_samples_first_and_drops_the_oldest() {
        let mut h = History::new(240, TWO);
        let samples = h.cap();
        assert_eq!(samples, 121);
        assert_eq!(h.interval(1.0 / 240.0), 1.0 / 60.0);
        assert!(h.points(0).next().is_none());
        for step in 1..=(4 * (samples as u64 + 10)) {
            h.record(step, &[Vec2::new(step as f32, 0.0), Vec2::new(0.0, step as f32)]);
        }
        assert_eq!(h.len(), samples);
        let newest: Vec<Vec2> = h.points(0).take(3).collect();
        let last = 4.0 * (samples as f32 + 10.0);
        assert_eq!(newest, [Vec2::new(last, 0.0), Vec2::new(last - 4.0, 0.0), Vec2::new(last - 8.0, 0.0)]);
        assert_eq!(h.points(1).next(), Some(Vec2::new(0.0, last)));
        assert_eq!(h.points(0).count(), samples);
        assert_eq!(h.points(0).last(), Some(Vec2::new(last - 4.0 * (samples as f32 - 1.0), 0.0)));
        assert_eq!(h.newest_step(), Some(last as u64));
        // Between samples, a place in between.
        assert_eq!(h.at(0, 0.5 / 60.0, 1.0 / 60.0), Some(Vec2::new(last - 2.0, 0.0)));
        // A ball that isn't there has no history, and the memory grows with the balls.
        assert_eq!(h.points(5).count(), 0);
        assert!(h.memory() >= 2 * samples * 8);
    }

    #[test]
    fn a_new_ball_starts_where_it_is() {
        let mut h = History::new(60, TWO);
        h.record(1, &[Vec2::ZERO]);
        h.record(2, &[Vec2::ONE, Vec2::new(5.0, 5.0)]);
        assert_eq!(h.points(1).count(), 2);
        assert!(h.points(1).all(|p| p == Vec2::new(5.0, 5.0)));
        assert_eq!(h.points(0).collect::<Vec<_>>(), [Vec2::ONE, Vec2::ZERO]);
        assert_eq!(h.travelled(0), std::f32::consts::SQRT_2);
        assert_eq!(h.travelled(1), 0.0);
        h.record(3, &[Vec2::ONE, Vec2::new(5.0, 8.0)]);
        assert_eq!(h.travelled(1), 3.0);
    }

    #[test]
    fn the_history_is_as_long_as_it_is_asked_for_and_forever_keeps_the_start() {
        let long = History::new(240, HistoryWant { seconds: 5.1, forever: false });
        assert_eq!(long.cap(), (5.25 * 60.0) as usize + 1, "rounded up to a quarter second");
        assert_eq!(History::new(240, HistoryWant { seconds: 0.5, forever: false }).cap(), 121, "at least 2 s");
        let mut h = History::new(240, HistoryWant { seconds: 0.5, forever: true });
        for step in 0..=(240 * 10) {
            h.record(step, &[Vec2::new(step as f32, 0.0)]);
        }
        assert_eq!(h.len(), 10 * 60 + 1);
        assert_eq!(h.points(0).last(), Some(Vec2::ZERO), "the very first sample is still there");
    }

    #[test]
    fn wants_join_and_round() {
        let a = HistoryWant { seconds: 3.1, forever: false };
        let b = HistoryWant { seconds: 1.0, forever: true };
        assert_eq!(a.and(b), HistoryWant { seconds: 3.1, forever: true });
        assert_eq!(a.rounded(), 3.25);
        assert_eq!(b.rounded(), 2.0);
    }
}
