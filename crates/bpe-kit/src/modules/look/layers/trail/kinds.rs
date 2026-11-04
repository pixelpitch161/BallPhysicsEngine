//! The maths under the trail kinds, apart from the frame: where a ball's trail runs (cut by its
//! length, its speed and its last bounce), how wide it is along it, how it wobbles, and where
//! a Ribbon twists, Dots sit, Smoke puffs and Ghosts stand.

use bpe_runtime::History;
use glam::Vec2;

use super::{Curve, FAST, TrailLayer};

/// Where a ball's trail runs, newest (the ball itself) first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub points: Vec<Vec2>,
    /// Seconds before now each point was there.
    pub ago: Vec<f32>,
    /// How far along the trail each point is from the ball, in world units.
    pub arc: Vec<f32>,
}

impl Path {
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// How long it is along the way, in world units.
    pub fn length(&self) -> f32 {
        self.arc.last().copied().unwrap_or(0.0)
    }

    /// How far back in time it reaches, in seconds.
    pub fn reach(&self) -> f32 {
        self.ago.last().copied().unwrap_or(0.0)
    }

    /// 0 at the ball to 1 at the tail, for point `k`: by its place in the list, as samples are
    /// even in time.
    pub fn along(&self, k: usize) -> f32 {
        if self.len() < 2 { 0.0 } else { k as f32 / (self.len() - 1) as f32 }
    }

    /// The place `arc` world units back along it, and the index of the point at or before it.
    pub fn at_arc(&self, arc: f32) -> Option<(Vec2, usize)> {
        let n = self.len();
        if n == 0 {
            return None;
        }
        let k = self.arc.partition_point(|&a| a <= arc).clamp(1, n) - 1;
        if k + 1 >= n {
            return Some((self.points[n - 1], n - 1));
        }
        let span = self.arc[k + 1] - self.arc[k];
        let f = if span > 0.0 { (arc - self.arc[k]) / span } else { 0.0 };
        Some((self.points[k].lerp(self.points[k + 1], f.clamp(0.0, 1.0)), k))
    }

    /// The place `ago` seconds back along it, while it reaches that far.
    pub fn at_ago(&self, ago: f32) -> Option<Vec2> {
        let n = self.len();
        if n == 0 || ago > self.reach() + 1e-6 {
            return None;
        }
        let k = self.ago.partition_point(|&a| a <= ago).clamp(1, n) - 1;
        if k + 1 >= n {
            return Some(self.points[n - 1]);
        }
        let span = self.ago[k + 1] - self.ago[k];
        let f = if span > 0.0 { (ago - self.ago[k]) / span } else { 0.0 };
        Some(self.points[k].lerp(self.points[k + 1], f.clamp(0.0, 1.0)))
    }

    /// The way the trail runs at point `k`, from older to newer (unit; `None` where it stands).
    pub fn heading(&self, k: usize) -> Option<Vec2> {
        let n = self.len();
        if n < 2 {
            return None;
        }
        let (a, b) = (k.saturating_sub(1), (k + 1).min(n - 1));
        (self.points[a] - self.points[b]).try_normalize()
    }
}

/// How far back a ball's trail may reach.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cut {
    /// At most this many seconds.
    pub seconds: Option<f32>,
    /// At most this far, in world units.
    pub distance: Option<f32>,
    /// Not before this many seconds ago (the ball's last bounce).
    pub since: Option<f32>,
}

impl Cut {
    /// The cut a trail layer asks for, for a ball at `speed`: its Length (longer when fast),
    /// everything for a Line that stays, and nothing before `last_hit` seconds ago when it
    /// restarts at each bounce.
    pub fn of(layer: &TrailLayer, speed: f32, last_hit: Option<f32>) -> Cut {
        let factor = 1.0 + layer.faster.max(0.0) * speed / FAST;
        let mut cut = Cut::default();
        if !layer.stays() {
            match layer.length_by {
                super::LengthBy::Time => cut.seconds = Some(layer.seconds.max(0.0) * factor),
                super::LengthBy::Distance => cut.distance = Some(layer.pixels.max(0.0) * factor),
            }
        }
        if layer.restart {
            cut.since = last_hit;
        }
        cut
    }

    /// The seconds back it reaches at most.
    fn time(&self) -> Option<f32> {
        match (self.seconds, self.since) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// Ball `ball`'s trail: from `head` (where it is now) back through its history, whose newest
/// sample is `newest_ago` seconds old and whose samples are `interval` seconds apart, as far as
/// `cut` lets it.
pub fn path(head: Vec2, history: &History, ball: usize, newest_ago: f32, interval: f32, cut: Cut) -> Path {
    let mut path = Path { points: vec![head], ago: vec![0.0], arc: vec![0.0] };
    let time = cut.time();
    for (j, p) in history.points(ball).enumerate() {
        let ago = newest_ago.max(0.0) + j as f32 * interval;
        let (last, last_ago, last_arc) = (path.points[path.len() - 1], path.ago[path.len() - 1], path.length());
        let step = (p - last).length();
        // How much of the way to this sample it may go: 1 all of it.
        let mut f = 1.0f32;
        if let Some(t) = time
            && ago > t
        {
            f = f.min(if ago > last_ago { (t - last_ago) / (ago - last_ago) } else { 0.0 });
        }
        if let Some(d) = cut.distance
            && last_arc + step > d
        {
            f = f.min(if step > 0.0 { (d - last_arc) / step } else { 0.0 });
        }
        let f = f.clamp(0.0, 1.0);
        if f < 1.0 {
            if f > 0.0 {
                path.points.push(last.lerp(p, f));
                path.ago.push(last_ago + (ago - last_ago) * f);
                path.arc.push(last_arc + step * f);
            }
            break;
        }
        path.points.push(p);
        path.ago.push(ago);
        path.arc.push(last_arc + step);
    }
    path
}

/// The half width at 0 (the ball) to 1 (the tail) along a trail `full` wide at the ball,
/// narrowing to `tail` of that by `curve`.
pub fn width_at(full: f32, tail: f32, curve: Curve, s: f32) -> f32 {
    full * (1.0 - curve.at(s) * (1.0 - tail.clamp(0.0, 1.0)))
}

/// Moves the trail's points sideways in waves: `amount` world units, `waves` along the whole of
/// it, moving along at `speed` waves a second at `time`. The ball itself stays where it is, the
/// wave growing over the first fifth.
pub fn wobble(path: &mut Path, amount: f32, waves: f32, speed: f32, time: f32) {
    let n = path.len();
    if amount == 0.0 || n < 3 {
        return;
    }
    let length = path.length().max(1e-3);
    let sides: Vec<Option<Vec2>> = (0..n).map(|k| path.heading(k).map(|h| h.perp())).collect();
    for k in 1..n {
        let Some(side) = sides[k] else { continue };
        let s = path.arc[k] / length;
        let grow = (s / 0.2).min(1.0);
        let wave = (std::f32::consts::TAU * (waves * s + speed * time)).sin();
        path.points[k] += side * amount * grow * wave;
    }
}

/// A Ribbon's width share and side at each point: `|cos θ|`, and whether its back shows
/// (`cos θ < 0`), θ being how far the ball had turned (radians) when it was there.
pub fn twist(angle: f32) -> (f32, bool) {
    let c = angle.cos();
    (c.abs(), c < 0.0)
}

/// Where Dots sit along `path`: one every `every` world units of the ball's way, counted from
/// where it started (`travelled` is how far it had gone when at the path's first point), so
/// each dot stays put as the ball moves on. Each is its distance back along the path.
pub fn dots(path: &Path, travelled: f32, every: f32) -> Vec<f32> {
    let every = every.max(1.0);
    let length = path.length();
    if path.len() < 2 || length <= 0.0 {
        return Vec::new();
    }
    // The newest dot at or behind the ball, then one every `every` further back.
    let first = travelled - (travelled / every).floor() * every;
    let mut out = Vec::new();
    let mut arc = first;
    while arc <= length {
        out.push(arc);
        arc += every;
    }
    out
}

/// The times Smoke puffs were let off, as seconds ago: on every multiple of `1 / per_second`
/// seconds into the clip, as far back as `reach` seconds, so a puff is the same puff whenever
/// the frame is drawn.
pub fn puffs(now: f64, per_second: f32, reach: f32) -> Vec<f32> {
    let per = per_second.max(0.1) as f64;
    let newest = (now * per).floor();
    let mut out = Vec::new();
    let mut k = newest;
    loop {
        let ago = (now - k / per) as f32;
        if ago > reach || k < 0.0 {
            break;
        }
        out.push(ago);
        k -= 1.0;
    }
    out
}

/// When each of `count` Ghosts stands, as seconds ago, `every` seconds apart, and how much of
/// the way back it is (for its fade): the k-th of n at `k / (n + 1)`.
pub fn ghosts(count: i32, every: f32) -> Vec<(f32, f32)> {
    let n = count.max(0);
    (1..=n).map(|k| (k as f32 * every.max(0.0), k as f32 / (n + 1) as f32)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_runtime::HistoryWant;

    /// A ball that went along x at 60 px a sample (3600 px/s), sampled 60 times a second.
    fn moving(samples: u64) -> History {
        let mut h = History::new(60, HistoryWant { seconds: 5.0, forever: false });
        for step in 0..samples {
            h.record(step, &[Vec2::new(step as f32 * 60.0, 0.0)]);
        }
        h
    }

    fn head(h: &History) -> Vec2 {
        h.points(0).next().unwrap() + Vec2::new(30.0, 0.0)
    }

    #[test]
    fn a_path_is_as_long_as_its_time_or_its_distance() {
        let h = moving(200);
        let i = 1.0 / 60.0;
        let all = path(head(&h), &h, 0, 0.5 * i, i, Cut::default());
        assert_eq!(all.len(), 201, "the ball, then every sample");
        let half = path(head(&h), &h, 0, 0.5 * i, i, Cut { seconds: Some(0.5), ..Cut::default() });
        assert!((half.reach() - 0.5).abs() < 1e-4, "{}", half.reach());
        assert_eq!(half.len(), 32, "30 samples and a bit, plus the ball");
        assert!((half.length() - 0.5 * 3600.0).abs() < 0.5);
        let far = path(head(&h), &h, 0, 0.5 * i, i, Cut { distance: Some(1000.0), ..Cut::default() });
        assert!((far.length() - 1000.0).abs() < 1e-2);
        assert!(far.points.windows(2).all(|w| w[0].x > w[1].x), "newest first");
        // A bounce 0.1 s ago cuts it there.
        let since = path(head(&h), &h, 0, 0.5 * i, i, Cut { seconds: Some(0.5), since: Some(0.1), ..Cut::default() });
        assert!((since.reach() - 0.1).abs() < 1e-4);
        // Places along it.
        assert_eq!(far.at_arc(0.0).unwrap().0, head(&h));
        assert!((far.at_arc(100.0).unwrap().0.x - (head(&h).x - 100.0)).abs() < 1e-3);
        assert!((half.at_ago(0.25).unwrap().x - (head(&h).x - 900.0)).abs() < 1e-2);
        assert!(half.at_ago(0.6).is_none());
    }

    #[test]
    fn a_trail_layer_s_cut_follows_its_settings() {
        let layer = TrailLayer { seconds: 0.4, faster: 1.0, restart: true, ..TrailLayer::default() };
        let cut = Cut::of(&layer, FAST, Some(0.2));
        assert_eq!(cut.seconds, Some(0.8), "twice as long at the fast speed");
        assert_eq!(cut.since, Some(0.2));
        let line = TrailLayer { kind: super::super::TrailKind::Line, forever: true, ..TrailLayer::default() };
        assert_eq!(Cut::of(&line, 0.0, Some(0.1)), Cut::default(), "a line that stays keeps it all");
        let far = TrailLayer { length_by: super::super::LengthBy::Distance, pixels: 200.0, ..TrailLayer::default() };
        assert_eq!(Cut::of(&far, 0.0, None).distance, Some(200.0));
    }

    #[test]
    fn widths_narrow_by_their_curve() {
        assert_eq!(width_at(10.0, 0.0, Curve::Even, 0.0), 10.0);
        assert_eq!(width_at(10.0, 0.0, Curve::Even, 0.5), 5.0);
        assert_eq!(width_at(10.0, 0.5, Curve::Even, 1.0), 5.0);
        assert_eq!(width_at(10.0, 1.0, Curve::In, 0.7), 10.0);
        assert!(width_at(10.0, 0.0, Curve::In, 0.5) > width_at(10.0, 0.0, Curve::Out, 0.5));
    }

    #[test]
    fn a_wobble_moves_points_sideways_but_not_the_ball() {
        let h = moving(100);
        let i = 1.0 / 60.0;
        let mut p = path(head(&h), &h, 0, 0.0, i, Cut { seconds: Some(0.5), ..Cut::default() });
        let before = p.clone();
        wobble(&mut p, 20.0, 2.0, 0.0, 0.3);
        assert_eq!(p.points[0], before.points[0]);
        assert!(p.points.iter().zip(&before.points).all(|(a, b)| a.x == b.x), "only sideways");
        assert!(p.points.iter().any(|a| a.y.abs() > 15.0));
        assert!(p.points.iter().all(|a| a.y.abs() <= 20.0 + 1e-3));
    }

    #[test]
    fn ribbons_twist_by_cos() {
        assert_eq!(twist(0.0), (1.0, false));
        let (w, back) = twist(std::f32::consts::PI);
        assert!((w - 1.0).abs() < 1e-6 && back);
        assert!(twist(std::f32::consts::FRAC_PI_2).0 < 1e-6);
    }

    #[test]
    fn dots_stay_put_as_the_ball_moves() {
        let h = moving(100);
        let i = 1.0 / 60.0;
        let p = path(head(&h), &h, 0, 0.0, i, Cut { seconds: Some(0.5), ..Cut::default() });
        let travelled = h.travelled(0) + 30.0;
        let d = dots(&p, travelled, 100.0);
        assert!(d.windows(2).all(|w| (w[1] - w[0] - 100.0).abs() < 1e-3), "evenly spaced");
        // Each dot sits on a multiple of 100 px of the way from the start.
        for arc in &d {
            let gone = travelled - arc;
            assert!((gone / 100.0 - (gone / 100.0).round()).abs() < 1e-3, "{gone}");
        }
    }

    #[test]
    fn puffs_and_ghosts_stand_at_fixed_times() {
        let a = puffs(1.0, 10.0, 0.35);
        assert_eq!(a.len(), 4);
        assert!((a[0] - 0.0).abs() < 1e-5 && (a[3] - 0.3).abs() < 1e-5);
        // A frame a little later sees the same puffs, older.
        let b = puffs(1.05, 10.0, 0.35);
        assert!((b[0] - 0.05).abs() < 1e-5 && (b[1] - 0.15).abs() < 1e-5);
        assert_eq!(ghosts(3, 0.1), [(0.1, 0.25), (0.2, 0.5), (0.3, 0.75)].map(|(a, b): (f32, f32)| (a, b)));
    }
}
