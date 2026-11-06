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
    /// Whether each point is a bounce (a corner the ball turned at between two samples).
    pub corner: Vec<bool>,
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

    /// 0 at the ball to 1 at the tail, for point `k`: by how long ago it was there, as points
    /// are not evenly spaced (bounces sit between samples); by its place in the list for a path
    /// that reaches no time back.
    pub fn along(&self, k: usize) -> f32 {
        let reach = self.reach();
        if self.len() < 2 {
            0.0
        } else if reach > 0.0 {
            (self.ago[k] / reach).clamp(0.0, 1.0)
        } else {
            k as f32 / (self.len() - 1) as f32
        }
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

    /// Drops the points at its head that stand where the next one does (a caught ball's place
    /// since it stopped), so it starts at the newest of them: the last moment it moved. Gives
    /// back how many it dropped.
    pub fn drop_still_head(&mut self) -> usize {
        let still = self.points.windows(2).take_while(|w| w[0] == w[1]).count();
        if still == 0 || still + 1 >= self.len() {
            return 0;
        }
        self.points.drain(..still);
        self.ago.drain(..still);
        self.arc.drain(..still);
        self.corner.drain(..still);
        still
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
    /// Ghosts reach back as far as their copies go, whatever the (hidden) Length says. It grows
    /// up to three times Fast's worth, as far as [`TrailLayer::reach`] keeps history for.
    pub fn of(layer: &TrailLayer, speed: f32, last_hit: Option<f32>) -> Cut {
        let factor = 1.0 + layer.faster.max(0.0) * (speed / FAST).min(3.0);
        let mut cut = Cut::default();
        if layer.kind == super::TrailKind::Ghosts {
            cut.seconds = Some(layer.reach());
        } else if !layer.stays() {
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

    /// The cut for a ball that stopped dead `gone` seconds ago, having gone at `speed`: its
    /// trail runs out into where it stopped as it would had it gone on (a distance cut is used up
    /// at the speed it went), `None` once nothing is left. A trail that stays, stays.
    pub fn after_gone(mut self, gone: f32, speed: f32) -> Option<Cut> {
        if let Some(d) = self.distance {
            let left = d - speed * gone;
            if left <= 0.0 {
                return None;
            }
            self.distance = Some(left);
        }
        if self.time().is_some_and(|t| t <= gone) {
            return None;
        }
        Some(self)
    }

    /// The seconds back it reaches at most.
    fn time(&self) -> Option<f32> {
        match (self.seconds, self.since) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// Ball `ball`'s trail: from `head` (where it is now) back through its history (its samples
/// and the bounces between them), whose newest sample is `newest_ago` seconds old and whose
/// samples are `interval` seconds apart, as far as `cut` lets it.
pub fn path(head: Vec2, history: &History, ball: usize, newest_ago: f32, interval: f32, cut: Cut) -> Path {
    let mut path = Path { points: vec![head], ago: vec![0.0], arc: vec![0.0], corner: vec![false] };
    let time = cut.time();
    let newest_ago = newest_ago.max(0.0) as f64;
    // Seconds a step.
    let dt = interval as f64 / history.every() as f64;
    let newest = history.newest_step().unwrap_or(0) as f64;
    for (t, p) in history.trace(ball) {
        let ago = (newest_ago + (newest - t) * dt) as f32;
        let (last, last_ago, last_arc) = (path.points[path.len() - 1], path.ago[path.len() - 1], path.length());
        // Samples are taken on whole multiples of `every` steps; bounces are anywhere between.
        let is_corner = t % history.every() as f64 != 0.0;
        // A bounce at or after the ball's own moment, or right where the point before is, adds
        // nothing.
        if is_corner && (ago <= last_ago + 1e-6 || p == last) {
            continue;
        }
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
                path.corner.push(false);
            }
            break;
        }
        path.points.push(p);
        path.ago.push(ago);
        path.arc.push(last_arc + step);
        path.corner.push(is_corner);
    }
    path
}

/// How far, in output pixels, a Line's drawn points may stray from where its ball really went:
/// well under a pixel, so the simplified line looks the same.
pub const LINE_TOLERANCE: f32 = 0.15;

/// About how many points along a Line are always kept (see [`grid_for`]), so what changes
/// along it (fade, colours by age) has places to change at.
pub const GRID_POINTS: usize = 64;

/// The spacing, in samples, of the samples a Line always keeps: the smallest power of two that
/// leaves at most [`GRID_POINTS`] of them along a line spanning `reach` seconds (from its oldest
/// point to its newest) with samples `interval` apart (more when it wobbles: 16 a wave). A power of two keeps them
/// nested: when the line grows to need twice the spacing, every other one goes and the rest
/// stay where they were.
pub fn grid_for(reach: f32, interval: f32, wobble_waves: Option<f32>) -> u64 {
    let samples = if interval > 0.0 { (reach.max(0.0) / interval).ceil() as u64 } else { 0 };
    let wanted = wobble_waves.map_or(GRID_POINTS as u64, |w| (w.abs() * 16.0).ceil().max(GRID_POINTS as f32) as u64);
    samples.div_ceil(wanted.max(1)).max(1).next_power_of_two()
}

/// `path` (newest first) with only the points a line through them needs: from its oldest point
/// towards the ball, a point is kept where the line leaves a band `tol` wide around the straight
/// way from the last point kept, or starts coming back along it. Its oldest point, the ball and
/// every sample whose number (counted from the start of the clip; the newest is `newest`,
/// `newest_ago` seconds old, the others `interval` apart) is a multiple of `grid` are always
/// kept. Working from the oldest end and starting again at the always-kept points, what it keeps
/// of a line's older part stays the same as the line grows. Each kept point keeps its own time
/// and its place along the real way (`ago`, `arc`).
pub fn simplify(path: &Path, tol: f32, grid: u64, newest: u64, newest_ago: f32, interval: f32) -> Path {
    let n = path.len();
    if n <= 2 {
        return path.clone();
    }
    let grid = grid.max(1);
    // Oldest first: index j is the path's point n - 1 - j.
    let points: Vec<Vec2> = path.points.iter().rev().copied().collect();
    let on_grid = |j: usize| {
        let k = n - 1 - j;
        if k == 0 || path.corner[k] || interval <= 0.0 {
            return false;
        }
        let back = ((path.ago[k] - newest_ago) / interval).round().max(0.0) as u64;
        newest.checked_sub(back).is_some_and(|s| s.is_multiple_of(grid))
    };
    let mut kept = Vec::with_capacity(n / 4 + 2);
    let mut fit = Sleeve::new(tol);
    for (j, q) in points.iter().enumerate() {
        fit.push(j, *q, j + 1 < n && on_grid(j), &mut kept);
    }
    fit.finish(&mut kept);
    let mut out = Path {
        points: Vec::with_capacity(kept.len()),
        ago: Vec::with_capacity(kept.len()),
        arc: Vec::with_capacity(kept.len()),
        corner: Vec::with_capacity(kept.len()),
    };
    for &j in kept.iter().rev() {
        let k = n - 1 - j;
        out.points.push(path.points[k]);
        out.ago.push(path.ago[k]);
        out.arc.push(path.arc[k]);
        out.corner.push(path.corner[k]);
    }
    out
}

/// Picks, from points given oldest first, the ones a line through them needs within `tol`: the
/// first and last, the ones pushed as fixed, and each that the straight way from the last one
/// kept can't reach without passing further than `tol` from a point between, or that comes
/// back towards it. A sleeve fit: each point between narrows the directions the way may leave
/// in. It goes one point at a time and can be cloned part way, so a line can be fitted once and
/// carried on from where it stopped: a point it keeps is kept for good.
#[derive(Clone, Debug)]
pub struct Sleeve<T: Copy> {
    tol: f32,
    /// The last point kept, the way runs from.
    anchor: Option<Vec2>,
    /// The point before the one being looked at, kept if the next doesn't fit.
    last: Option<(T, Vec2)>,
    /// The directions the way may leave the anchor in, from `lo` round to `hi` (unit), once a
    /// point far enough from it has narrowed them.
    lo: Vec2,
    hi: Vec2,
    open: bool,
    /// How far from the anchor the furthest point so far is.
    far: f32,
}

impl<T: Copy> Sleeve<T> {
    pub fn new(tol: f32) -> Self {
        Self { tol: tol.max(0.0), anchor: None, last: None, lo: Vec2::X, hi: Vec2::X, open: true, far: 0.0 }
    }

    /// A fit that has just kept a point at `at` (as at a fixed point) and goes on from there.
    pub fn from_anchor(tol: f32, at: Vec2) -> Self {
        let mut fit = Self::new(tol);
        fit.restart(at);
        fit
    }

    /// Takes the next point, `item` at `p`; a `fixed` one is kept whatever. Points it keeps for
    /// good go to `out`.
    pub fn push(&mut self, item: T, p: Vec2, fixed: bool, out: &mut Vec<T>) {
        let Some(anchor) = self.anchor else {
            out.push(item);
            self.restart(p);
            return;
        };
        if !self.fits(p - anchor)
            && let Some((last, at)) = self.last.take()
        {
            // The point before is needed; the way starts again from it.
            out.push(last);
            self.restart(at);
        }
        let anchor = self.anchor.unwrap_or(p);
        self.narrow(p - anchor);
        self.last = Some((item, p));
        if fixed {
            out.push(item);
            self.restart(p);
        }
    }

    /// Keeps the last point taken: the line's end.
    pub fn finish(mut self, out: &mut Vec<T>) {
        if let Some((last, _)) = self.last.take() {
            out.push(last);
        }
    }

    fn restart(&mut self, at: Vec2) {
        self.anchor = Some(at);
        self.last = None;
        self.open = true;
        self.far = 0.0;
    }

    /// Whether the way from the anchor along `v` passes near every point since the anchor.
    fn fits(&self, v: Vec2) -> bool {
        let d = v.length();
        if d + self.tol < self.far {
            // Coming back towards the anchor.
            return false;
        }
        d <= self.tol || self.open || (self.lo.perp_dot(v) >= 0.0 && v.perp_dot(self.hi) >= 0.0)
    }

    /// Narrows the directions to the ones passing within `tol` of the point along `v`.
    fn narrow(&mut self, v: Vec2) {
        let d = v.length();
        self.far = self.far.max(d);
        if d <= self.tol {
            return;
        }
        let u = v / d;
        let sin = self.tol / d;
        let cos = (1.0 - sin * sin).max(0.0).sqrt();
        let lo = Vec2::new(u.x * cos + u.y * sin, u.y * cos - u.x * sin);
        let hi = Vec2::new(u.x * cos - u.y * sin, u.y * cos + u.x * sin);
        if self.open {
            (self.lo, self.hi, self.open) = (lo, hi, false);
            return;
        }
        if self.lo.perp_dot(lo) > 0.0 {
            self.lo = lo;
        }
        if hi.perp_dot(self.hi) > 0.0 {
            self.hi = hi;
        }
    }
}

/// Cuts `path` to its newest `most` points (at least two).
pub fn limit(path: &mut Path, most: usize) {
    let most = most.max(2);
    path.points.truncate(most);
    path.ago.truncate(most);
    path.arc.truncate(most);
    path.corner.truncate(most);
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
    for (k, side) in sides.iter().enumerate().skip(1) {
        let Some(side) = *side else { continue };
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
    fn ghosts_reach_back_as_far_as_their_copies_go() {
        use super::super::{LengthBy, TrailKind};
        let ghosts = TrailLayer {
            kind: TrailKind::Ghosts,
            ghosts: 10,
            ghost_every: 0.5,
            seconds: 0.5,
            faster: 1.0,
            length_by: LengthBy::Distance,
            ..TrailLayer::default()
        };
        let cut = Cut::of(&ghosts, FAST * 2.0, None);
        assert_eq!((cut.seconds, cut.distance), (Some(5.0), None));
    }

    #[test]
    fn longer_when_fast_stops_at_three_times_fast() {
        let layer = TrailLayer { seconds: 0.4, faster: 1.0, ..TrailLayer::default() };
        assert_eq!(Cut::of(&layer, FAST * 50.0, None).seconds, Some(1.6));
        assert!(Cut::of(&layer, FAST * 50.0, None).seconds.unwrap() <= layer.reach() + 1e-6);
    }

    /// A ball going round a circle of 300, with a bounce (a sharp turn) every so often, sampled
    /// `samples` times; its path from the newest sample back.
    fn long_path(samples: u64) -> Path {
        let mut h = History::new(60, bpe_runtime::HistoryWant { seconds: 0.0, forever: true });
        for step in 0..samples {
            let t = step as f32 / 60.0;
            let wobble = if step % 97 < 3 { 40.0 } else { 0.0 };
            h.record(step, &[Vec2::from_angle(t) * (300.0 + wobble)]);
        }
        path(h.points(0).next().unwrap(), &h, 0, 0.0, 1.0 / 60.0, Cut::default())
    }

    /// The distance from `q` to the segment from `a` to `b`.
    fn off_segment(q: Vec2, a: Vec2, b: Vec2) -> f32 {
        let ab = b - a;
        let f = if ab.length_squared() > 0.0 { ((q - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        q.distance(a + ab * f)
    }

    /// How far the furthest point of `whole` is from the simplified line `kept` (both newest
    /// first; each point is matched to the kept points either side of it in time).
    fn furthest_off(whole: &Path, kept: &Path) -> f32 {
        let mut worst = 0.0f32;
        for (q, ago) in whole.points.iter().zip(&whole.ago) {
            let k = kept.ago.partition_point(|a| a <= ago).clamp(1, kept.len() - 1);
            worst = worst.max(off_segment(*q, kept.points[k - 1], kept.points[k]));
        }
        worst
    }

    #[test]
    fn a_long_line_simplifies_within_its_tolerance_and_reaches_the_start() {
        let whole = long_path(60 * 120);
        let n = whole.len();
        let grid = grid_for(whole.reach() - whole.ago[0], 1.0 / 60.0, None);
        assert_eq!(grid, 128, "7200 samples, at most 64 kept on the grid");
        let p = simplify(&whole, LINE_TOLERANCE, grid, 60 * 120 - 1, 0.0, 1.0 / 60.0);
        // A round line keeps about one sample in four at this size.
        assert!(p.len() < n / 2, "{} of {n}", p.len());
        assert_eq!(p.points[0], whole.points[0], "the ball");
        assert_eq!(p.points.last(), whole.points.last(), "it still reaches the start");
        let off = furthest_off(&whole, &p);
        assert!(off <= 2.0 * LINE_TOLERANCE, "{off}");
        assert!(p.arc.windows(2).all(|w| w[0] <= w[1]) && p.ago.windows(2).all(|w| w[0] <= w[1]), "in order");
        // Its grid samples are all there: every 128th sample from the start of the clip.
        for k in 1..n {
            let back = (whole.ago[k] * 60.0).round() as u64;
            if (60 * 120 - 1 - back).is_multiple_of(128) {
                assert!(p.points.contains(&whole.points[k]), "grid sample {k}");
            }
        }
        // A sample later, the old part is kept the same.
        let later_whole = long_path(60 * 120 + 1);
        let later = simplify(&later_whole, LINE_TOLERANCE, grid, 60 * 120, 0.0, 1.0 / 60.0);
        let old = |p: &Path| -> Vec<Vec2> { (0..p.len()).filter(|&k| p.ago[k] > 5.0).map(|k| p.points[k]).collect() };
        let (a, b) = (old(&p), old(&later));
        assert!(a.len() > 20);
        assert_eq!(a[..a.len() - 2], b[..a.len() - 2], "kept points stay kept");
    }

    /// Builds a newest-first path through `points` given oldest first, a sample apart.
    fn through(points: &[Vec2]) -> Path {
        let n = points.len();
        let mut p = Path::default();
        let mut arc = 0.0;
        for (k, q) in points.iter().rev().enumerate() {
            if k > 0 {
                arc += q.distance(p.points[k - 1]);
            }
            p.points.push(*q);
            p.ago.push(k as f32 / 60.0);
            p.arc.push(arc);
            p.corner.push(false);
        }
        assert_eq!(p.len(), n);
        p
    }

    #[test]
    fn straight_runs_keep_their_ends_and_turns_their_corners() {
        let straight: Vec<Vec2> = (0..50).map(|x| Vec2::new(x as f32 * 7.0, 0.0)).collect();
        let p = simplify(&through(&straight), LINE_TOLERANCE, 1 << 20, 49, 0.0, 1.0 / 60.0);
        assert_eq!(p.points, vec![straight[49], straight[0]]);
        // Out along x, then up: the corner stays.
        let mut turn = straight.clone();
        turn.extend((1..30).map(|y| Vec2::new(343.0, y as f32 * 7.0)));
        let p = simplify(&through(&turn), LINE_TOLERANCE, 1 << 20, 78, 0.0, 1.0 / 60.0);
        assert_eq!(p.points, vec![turn[78], turn[49], turn[0]]);
        // Out and straight back the same way: the far end stays.
        let mut back = straight.clone();
        back.extend((1..30).rev().map(|x| Vec2::new(x as f32 * 7.0 + 0.5, 0.0)));
        let p = simplify(&through(&back), LINE_TOLERANCE, 1 << 20, 78, 0.0, 1.0 / 60.0);
        assert!(p.points.contains(&straight[49]), "{:?}", p.points);
        assert!(furthest_off(&through(&back), &p) <= 2.0 * LINE_TOLERANCE);
    }

    #[test]
    fn a_random_walk_stays_within_the_tolerance() {
        let mut q = Vec2::ZERO;
        let mut points = vec![q];
        let mut seed = 7u32;
        for _ in 0..3000 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let angle = (seed >> 8) as f32 / (1 << 24) as f32 * std::f32::consts::TAU;
            q += Vec2::from_angle(angle) * if seed % 5 == 0 { 0.05 } else { 3.0 };
            points.push(q);
        }
        let whole = through(&points);
        for tol in [0.15, 1.0, 5.0] {
            let p = simplify(&whole, tol, 1 << 20, 3000, 0.0, 1.0 / 60.0);
            let off = furthest_off(&whole, &p);
            assert!(off <= 2.0 * tol, "{tol}: {off}");
        }
    }

    #[test]
    fn the_grid_spacing_doubles_as_the_line_grows() {
        let i = 1.0 / 60.0;
        assert_eq!(grid_for(0.5, i, None), 1);
        assert_eq!(grid_for(64.0 * i, i, None), 1);
        assert_eq!(grid_for(65.0 * i, i, None), 2);
        assert_eq!(grid_for(60.0, i, None), 64);
        assert_eq!(grid_for(60.0, i, Some(8.0)), 32, "wobbling: 16 a wave");
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
