//! Where each ball has been lately, for trails and particles. Recorded `SAMPLE_RATE` times a
//! second, kept for as many seconds as the thing's longest trail asks for (or for good, for a
//! Line that stays), and thrown away as new samples arrive. Between samples it also keeps each
//! ball's **corners**: where it was at the moment it bounced, so a trail turns where the ball
//! really turned instead of cutting straight across from the sample before to the one after.

use glam::Vec2;

use crate::track::{Same, Track};

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

/// The recent positions of every ball of a thing, newest last, one list per ball. A ball's list
/// starts at its first sample after it came, never before. The lists are [`Track`]s: a copy of the
/// world (a jump point) shares their older parts, and a ball standing still (gone, or at rest) adds
/// nothing to them but a count.
#[derive(Clone, Debug, PartialEq)]
pub struct History {
    /// Steps between samples.
    every: u64,
    /// Samples kept per ball; `usize::MAX` keeps every one.
    cap: usize,
    balls: Vec<Track<Vec2>>,
    /// Where each ball bounced between its samples: (the moment, in steps; where it was), oldest
    /// first, none older than its oldest sample.
    corners: Vec<Track<(f64, Vec2)>>,
    /// Each ball's newest place, sample or corner: (the moment, in steps; where it was).
    tips: Vec<(f64, Vec2)>,
    /// How far each ball has gone from its first sample to its newest place (sample or corner).
    travelled: Vec<f32>,
    /// The step each ball was first there at (the end of the step it came in).
    born: Vec<u64>,
    /// The most samples any ball has, up to `cap`.
    count: usize,
    /// The step the newest sample was taken at.
    last: Option<u64>,
}

impl History {
    /// A history that samples about `SAMPLE_RATE` times a second of `steps_per_second` steps
    /// (every step when there are fewer) and keeps what `want` asks for.
    pub fn new(steps_per_second: u32, want: HistoryWant) -> Self {
        let every = (steps_per_second / SAMPLE_RATE).max(1) as u64;
        // As many samples as the seconds take at the rate they really come at.
        let per_second = steps_per_second.max(1) as f64 / every as f64;
        let cap =
            if want.forever { usize::MAX } else { (want.rounded() as f64 * per_second).ceil() as usize + 1 }.max(2);
        Self {
            every,
            cap,
            balls: Vec::new(),
            corners: Vec::new(),
            tips: Vec::new(),
            travelled: Vec::new(),
            born: Vec::new(),
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

    /// Notes that the first `rows` balls are there at `step`, for the ones not noted before.
    pub fn note(&mut self, step: u64, rows: usize) {
        while self.born.len() < rows {
            self.born.push(step);
        }
    }

    /// The step ball `ball` was first there at; `None` for a ball it hasn't seen.
    pub fn born(&self, ball: usize) -> Option<u64> {
        self.born.get(ball).copied()
    }

    /// Notes the balls new at `step`, and records `positions` when `step` is a sampling step. A
    /// ball's samples start at the first one after it came.
    pub fn record(&mut self, step: u64, positions: &[Vec2]) {
        self.note(step, positions.len());
        if !step.is_multiple_of(self.every) {
            return;
        }
        while self.balls.len() < positions.len() {
            self.balls.push(Track::new());
            self.corners.push(Track::new());
            self.tips.push((step as f64, Vec2::ZERO));
            self.travelled.push(0.0);
        }
        let now = step as f64;
        for (b, &p) in positions.iter().enumerate() {
            let ring = &mut self.balls[b];
            // A full ring of this very place, with no corner since (a ball gone, or at rest for the
            // whole stretch kept): dropping the oldest and adding it again would change nothing.
            let unchanged = ring.len() >= self.cap
                && ring.is_one_run()
                && ring.front().is_some_and(|f| f.same(&p))
                && self.tips[b].1.same(&p);
            if !unchanged {
                if !ring.is_empty() {
                    self.travelled[b] += (p - self.tips[b].1).length();
                }
                if ring.len() >= self.cap {
                    ring.pop_front();
                }
                ring.push_back(p);
            }
            self.tips[b] = (now, p);
            // Corners older than the oldest sample are past where any trail reaches.
            let oldest = now - (self.balls[b].len() - 1) as f64 * self.every as f64;
            let corners = &mut self.corners[b];
            while corners.front().is_some_and(|c| c.0 < oldest) {
                corners.pop_front();
            }
        }
        self.count = (self.count + 1).min(self.cap);
        self.last = Some(step);
    }

    /// Notes that ball `ball` bounced, and was at `place`, at the moment `at` (in steps, between
    /// whole ones). Left out for a ball with no sample yet (no trail reaches before its first
    /// one) and for a moment not after its newest place.
    pub fn corner(&mut self, ball: usize, at: f64, place: Vec2) {
        let Some(ring) = self.balls.get(ball) else { return };
        if ring.is_empty() || at <= self.tips[ball].0 {
            return;
        }
        self.travelled[ball] += (place - self.tips[ball].1).length();
        self.tips[ball] = (at, place);
        self.corners[ball].push_back((at, place));
    }

    /// Ball `ball`'s newest place, sample or corner: (the moment, in steps; where it was).
    pub fn tip(&self, ball: usize) -> Option<(f64, Vec2)> {
        self.balls.get(ball).filter(|r| !r.is_empty()).map(|_| self.tips[ball])
    }

    /// Ball `ball`'s samples and corners, newest first, back to its first sample: (the moment,
    /// in steps; where it was). A corner on a sample's own moment is the sample.
    pub fn trace(&self, ball: usize) -> impl Iterator<Item = (f64, Vec2)> + '_ {
        let last = self.last.unwrap_or(0) as f64;
        let every = self.every as f64;
        let mut samples = self.points(ball).enumerate().map(move |(j, p)| (last - j as f64 * every, p)).peekable();
        let mut corners = self.corners.get(ball).into_iter().flat_map(|c| c.iter().rev()).peekable();
        std::iter::from_fn(move || match (samples.peek().copied(), corners.peek().copied()) {
            (Some(s), Some(c)) if c.0 > s.0 => corners.next(),
            (Some(s), Some(c)) => {
                if c.0 == s.0 {
                    corners.next();
                }
                samples.next()
            }
            (Some(_), None) => samples.next(),
            // Corners are never older than the oldest sample.
            (None, _) => None,
        })
    }

    /// Ball `ball`'s samples and corners after the moment `after` (in steps), oldest first: the
    /// same places as [`History::trace`], the other way round.
    pub fn trace_after(&self, ball: usize, after: f64) -> impl Iterator<Item = (f64, Vec2)> + '_ {
        let every = self.every as f64;
        let (ring, corners) = match (self.balls.get(ball), self.corners.get(ball)) {
            (Some(r), Some(c)) => (Some(r), Some(c)),
            _ => (None, None),
        };
        let n = ring.map_or(0, Track::len);
        let oldest = self.last.unwrap_or(0) as f64 - n.saturating_sub(1) as f64 * every;
        // The first sample after `after`, and the first corner.
        let first =
            if after < oldest { 0 } else { (((after - oldest) / every).floor() as usize).saturating_add(1).min(n) };
        let mut samples = ring
            .into_iter()
            .flat_map(move |r| r.iter_from(first))
            .enumerate()
            .map(move |(j, p)| (oldest + (first + j) as f64 * every, p));
        let from = corners.map_or(0, |c| c.partition_point(|c| c.0 <= after));
        let mut corners = corners.into_iter().flat_map(move |c| c.iter_from(from));
        let mut next_sample = samples.next();
        let mut next_corner = corners.next();
        std::iter::from_fn(move || {
            loop {
                match (next_sample, next_corner) {
                    (Some(s), Some(c)) if c.0 < s.0 => {
                        next_corner = corners.next();
                        return Some(c);
                    }
                    (Some(s), Some(c)) => {
                        // A corner on a sample's own moment is the sample.
                        if c.0 == s.0 {
                            next_corner = corners.next();
                        }
                        next_sample = samples.next();
                        return Some(s);
                    }
                    (Some(s), None) => {
                        next_sample = samples.next();
                        return Some(s);
                    }
                    // Corners after the newest sample (bounces since it was taken).
                    (None, Some(c)) => {
                        next_corner = corners.next();
                        if n == 0 {
                            continue;
                        }
                        return Some(c);
                    }
                    (None, None) => return None,
                }
            }
        })
    }

    /// Where ball `ball` was at the moment `t` (in steps) exactly, when it has a sample or a
    /// corner there.
    pub fn item_at(&self, ball: usize, t: f64) -> Option<Vec2> {
        let ring = self.balls.get(ball).filter(|r| !r.is_empty())?;
        let every = self.every as f64;
        let oldest = self.last.unwrap_or(0) as f64 - (ring.len() - 1) as f64 * every;
        let i = (t - oldest) / every;
        if i >= 0.0 && i.fract() == 0.0 && (i as usize) < ring.len() {
            return ring.get(i as usize);
        }
        let corners = &self.corners[ball];
        let k = corners.partition_point(|c| c.0 < t);
        corners.get(k).filter(|c| c.0 == t).map(|c| c.1)
    }

    /// Ball `ball`'s oldest sample: (the moment, in steps; where it was).
    pub fn first(&self, ball: usize) -> Option<(f64, Vec2)> {
        let ring = self.balls.get(ball).filter(|r| !r.is_empty())?;
        let oldest = self.last.unwrap_or(0) as f64 - (ring.len() - 1) as f64 * self.every as f64;
        Some((oldest, ring.front()?))
    }

    /// Ball `ball`'s samples, newest first, back to its first.
    pub fn points(&self, ball: usize) -> impl Iterator<Item = Vec2> + '_ {
        self.balls.get(ball).into_iter().flat_map(|r| r.iter().rev())
    }

    /// How far ball `ball` has gone from its first sample to its newest place (a sample or a
    /// corner, see [`History::tip`]), in world units, through its corners: a place along its path
    /// that stays put as samples come and go (dots along a trail).
    pub fn travelled(&self, ball: usize) -> f32 {
        self.travelled.get(ball).copied().unwrap_or(0.0)
    }

    /// Where ball `ball` was at the moment `t` (in steps), when it is at `head` at the moment
    /// `head_t` (at or after its newest place): along the way through its corners and samples,
    /// where it was first sampled for further back than that, and `None` without a sample.
    pub fn place(&self, ball: usize, t: f64, head_t: f64, head: Vec2) -> Option<Vec2> {
        let ring = self.balls.get(ball).filter(|r| !r.is_empty())?;
        let corners = &self.corners[ball];
        let n = ring.len();
        let every = self.every as f64;
        let last = self.last.unwrap_or(0) as f64;
        let oldest = last - (n - 1) as f64 * every;
        let at = |i: usize| ring.get(i).unwrap_or(Vec2::ZERO);
        if t <= oldest {
            return Some(at(0));
        }
        // The two places either side of `t`: a sample, a corner or the head.
        let (mut older, mut newer) = if t >= last {
            ((last, at(n - 1)), (head_t.max(last), head))
        } else {
            let i = ((t - oldest) / every).floor() as usize;
            let i = i.min(n - 2);
            ((oldest + i as f64 * every, at(i)), (oldest + (i + 1) as f64 * every, at(i + 1)))
        };
        // Corners between them, the nearest each side of `t`.
        let from = corners.partition_point(|c| c.0 <= older.0);
        // (None between when both are the same moment: a corner there is the sample itself.)
        let to = corners.partition_point(|c| c.0 < newer.0).max(from);
        let split = corners.partition_point(|c| c.0 <= t).clamp(from, to);
        if split > from
            && let Some(c) = corners.get(split - 1)
        {
            older = c;
        }
        if split < to
            && let Some(c) = corners.get(split)
        {
            newer = c;
        }
        let span = newer.0 - older.0;
        let f = if span > 0.0 { ((t - older.0) / span).clamp(0.0, 1.0) } else { 1.0 };
        Some(older.1.lerp(newer.1, f as f32))
    }

    /// How many samples the longest-kept ball has so far.
    pub fn len(&self) -> usize {
        if self.balls.is_empty() { 0 } else { self.count }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes the history takes, for playback budgets.
    /// Parts shared with copies count their share (see [`Track::memory`]).
    pub fn memory(&self) -> usize {
        self.memory_beside(None)
    }

    /// Bytes it takes beside `earlier` (an earlier copy of it): see [`Track::memory_beside`].
    pub fn memory_beside(&self, earlier: Option<&History>) -> usize {
        let balls = self.balls.iter().enumerate().map(|(b, t)| t.memory_beside(earlier.and_then(|e| e.balls.get(b))));
        let corners =
            self.corners.iter().enumerate().map(|(b, t)| t.memory_beside(earlier.and_then(|e| e.corners.get(b))));
        balls.sum::<usize>() + corners.sum::<usize>()
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
        assert_eq!(h.place(0, last as f64 - 2.0, last as f64, Vec2::ZERO), Some(Vec2::new(last - 2.0, 0.0)));
        // A ball that isn't there has no history, and the memory grows with the balls.
        assert_eq!(h.points(5).count(), 0);
        assert!(h.memory() >= 2 * samples * 8);
    }

    #[test]
    fn a_new_ball_starts_where_it_is() {
        let mut h = History::new(60, TWO);
        h.record(1, &[Vec2::ZERO]);
        h.record(2, &[Vec2::ONE, Vec2::new(5.0, 5.0)]);
        assert_eq!(h.points(1).collect::<Vec<_>>(), [Vec2::new(5.0, 5.0)], "nothing from before it came");
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

    /// At a step rate that isn't a multiple of 60 the samples come faster, and the history keeps
    /// as many seconds all the same.
    #[test]
    fn the_history_keeps_its_seconds_at_any_step_rate() {
        for rate in [100, 240, 250, 90, 30] {
            let h = History::new(rate, TWO);
            let per_second = rate as f32 / h.every() as f32;
            let kept = (h.cap() - 1) as f32 / per_second;
            assert!((kept - 2.0).abs() < 0.05, "{rate} steps a second keeps {kept} s");
        }
    }

    /// A ball that comes later has no samples from before it was there: its history stops where
    /// it began, and it knows the step it began on.
    #[test]
    fn a_late_ball_has_no_past_before_it_began() {
        let mut h = History::new(240, TWO);
        h.note(0, 1);
        for step in 1..=40 {
            let mut positions = vec![Vec2::new(step as f32, 0.0)];
            if step >= 22 {
                positions.push(Vec2::new(0.0, step as f32));
            }
            h.record(step, &positions);
        }
        assert_eq!(h.points(0).count(), 10);
        // Ball 1 came on step 22 and was sampled on 24, 28, ..., 40.
        assert_eq!(h.points(1).collect::<Vec<_>>().len(), 5);
        assert_eq!(h.points(1).last(), Some(Vec2::new(0.0, 24.0)));
        assert_eq!(h.born(0), Some(0));
        assert_eq!(h.born(1), Some(22));
        assert_eq!(h.born(2), None);
        // Reading further back than it goes gives where it began.
        assert_eq!(h.place(1, 0.0, 40.0, Vec2::ZERO), Some(Vec2::new(0.0, 24.0)));
        // Once full, each ball drops its own oldest.
        for step in 41..=2000 {
            h.record(step, &[Vec2::new(step as f32, 0.0), Vec2::new(0.0, step as f32)]);
        }
        assert_eq!(h.points(0).count(), h.cap());
        assert_eq!(h.points(1).count(), h.cap());
        assert_eq!(h.points(1).next(), Some(Vec2::new(0.0, 2000.0)));
    }

    /// A ball going right at 1 px a step that bounces straight up 1.5 steps after its sample at
    /// step 8: its corner sits between the samples, in order, and counts in how far it went.
    #[test]
    fn corners_sit_between_the_samples() {
        let mut h = History::new(240, TWO);
        let at = |t: f64| if t <= 9.5 { Vec2::new(t as f32, 0.0) } else { Vec2::new(9.5, (t - 9.5) as f32) };
        // No sample yet: nowhere for a corner to go.
        h.corner(0, 1.5, at(1.5));
        for step in 1..=16u64 {
            if step == 10 {
                h.corner(0, 9.5, at(9.5));
            }
            h.record(step, &[at(step as f64)]);
        }
        let trace: Vec<(f64, Vec2)> = h.trace(0).collect();
        assert_eq!(trace, [(16.0, at(16.0)), (12.0, at(12.0)), (9.5, at(9.5)), (8.0, at(8.0)), (4.0, at(4.0))]);
        assert_eq!(h.tip(0), Some((16.0, at(16.0))));
        // 4 to 9.5 along, then 6.5 up: the corner counts, the straight cut across would not.
        assert!((h.travelled(0) - 12.0).abs() < 1e-5, "{}", h.travelled(0));
        // Read back through the corner, and between it and a sample.
        assert_eq!(h.place(0, 9.5, 16.0, at(16.0)), Some(at(9.5)));
        assert_eq!(h.place(0, 9.0, 16.0, at(16.0)), Some(at(9.0)));
        assert_eq!(h.place(0, 11.0, 16.0, at(16.0)), Some(at(11.0)));
        // Past the newest sample: towards the head, through a corner made since.
        h.corner(0, 17.0, Vec2::new(9.5, 7.5));
        assert_eq!(h.tip(0), Some((17.0, Vec2::new(9.5, 7.5))));
        assert_eq!(h.trace(0).next(), Some((17.0, Vec2::new(9.5, 7.5))));
        assert_eq!(h.place(0, 16.5, 18.0, Vec2::new(10.5, 7.5)), Some(Vec2::new(9.5, 7.0)));
        assert_eq!(h.place(0, 17.5, 18.0, Vec2::new(10.5, 7.5)), Some(Vec2::new(10.0, 7.5)));
        // A corner no later than the newest place is left out.
        h.corner(0, 17.0, Vec2::ZERO);
        assert_eq!(h.trace(0).count(), 6);
        // A corner on a sample's own moment is that sample.
        h.corner(0, 20.0, Vec2::new(9.5, 10.5));
        h.record(20, &[Vec2::new(9.5, 10.5)]);
        assert_eq!(h.trace(0).filter(|c| c.0 == 20.0).count(), 1);
        assert_eq!(h.place(0, 20.0, 20.0, Vec2::new(9.5, 10.5)), Some(Vec2::new(9.5, 10.5)));
        assert_eq!(h.place(0, 19.0, 20.0, Vec2::new(9.5, 10.5)), Some(Vec2::new(9.5, 9.5)));
        // Corners go with the samples they sit between.
        for step in 21..=2000u64 {
            h.record(step, &[Vec2::new(9.5, step as f32)]);
        }
        assert_eq!(h.trace(0).count(), h.cap());
        assert!(h.memory() >= h.cap() * 8);
    }

    #[test]
    fn wants_join_and_round() {
        let a = HistoryWant { seconds: 3.1, forever: false };
        let b = HistoryWant { seconds: 1.0, forever: true };
        assert_eq!(a.and(b), HistoryWant { seconds: 3.1, forever: true });
        assert_eq!(a.rounded(), 3.25);
        assert_eq!(b.rounded(), 2.0);
    }

    #[test]
    fn tracing_after_a_moment_goes_the_other_way_round() {
        let mut h = History::new(240, HistoryWant { seconds: 2.0, forever: false });
        h.note(0, 1);
        for step in 0..=1000u64 {
            let p = Vec2::new(step as f32, (step % 37) as f32);
            if step % 9 == 5 {
                h.corner(0, step as f64 - 0.5, p + Vec2::new(0.25, 3.0));
            }
            h.record(step, &[p]);
        }
        // A bounce after the newest sample.
        h.corner(0, 1002.5, Vec2::new(7.0, 7.0));
        let back: Vec<(f64, Vec2)> = h.trace(0).collect();
        let mut forth: Vec<(f64, Vec2)> = h.trace_after(0, f64::NEG_INFINITY).collect();
        forth.reverse();
        assert_eq!(back, forth);
        for after in [0.0, 500.0, 503.5, 504.0, 999.0, 1000.0, 1002.5] {
            let mut tail: Vec<(f64, Vec2)> = h.trace_after(0, after).collect();
            tail.reverse();
            let want: Vec<(f64, Vec2)> = back.iter().copied().filter(|i| i.0 > after).collect();
            assert_eq!(tail, want, "after {after}");
        }
        for (t, p) in &back {
            assert_eq!(h.item_at(0, *t), Some(*p), "at {t}");
        }
        assert_eq!(h.item_at(0, 501.25), None);
        assert_eq!(h.first(0), Some((back.last().unwrap().0, back.last().unwrap().1)));
        assert_eq!(h.trace_after(1, 0.0).count(), 0);
    }
}
