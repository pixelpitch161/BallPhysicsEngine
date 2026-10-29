//! Beat sync by stretching time: the physics is left alone and the video's clock gently speeds
//! up and slows down between hits so they land on beats.

/// Video time against simulation time, as a monotone curve through anchor points.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeWarp {
    /// (video time, simulation time) points the curve passes through, both increasing.
    anchors: Vec<(f64, f64)>,
    /// Slope of the curve (simulation seconds per video second) at each anchor.
    slopes: Vec<f64>,
}

impl Default for TimeWarp {
    fn default() -> Self {
        Self::identity()
    }
}

/// Hits closer than this to the last anchor would need too sharp a bend, so they are skipped.
const MIN_GAP: f64 = 0.06;

impl TimeWarp {
    /// Fits a warp so as many `hits` (simulation times) as possible land on `beats` (video
    /// times, sorted), never running the video more than `strength` (0 to 0.9) faster or
    /// slower than real time between hits. Past the last hit, and past `duration`, time runs
    /// at its normal speed.
    pub fn fit(hits: &[f64], beats: &[f64], strength: f64, duration: f64) -> Self {
        let k = if strength.is_finite() { strength.clamp(0.0, 0.9) } else { 0.0 };
        let duration = if duration.is_finite() { duration.max(0.0) } else { 0.0 };
        let mut times: Vec<f64> = hits.iter().copied().filter(|t| t.is_finite() && *t > 0.0).collect();
        times.sort_by(f64::total_cmp);
        let mut anchors = vec![(0.0, 0.0)];
        if k > 0.0 {
            for &hit in &times {
                let (v0, s0) = *anchors.last().unwrap();
                let gap = hit - s0;
                if gap < MIN_GAP {
                    continue;
                }
                let (earliest, latest) = (v0 + gap / (1.0 + k), v0 + gap * (1.0 + k));
                let natural = v0 + gap;
                let mut best: Option<f64> = None;
                let i = beats.partition_point(|&b| b < earliest);
                for &b in beats[i..].iter().take_while(|&&b| b <= latest) {
                    if best.is_none_or(|x| (b - natural).abs() < (x - natural).abs()) {
                        best = Some(b);
                    }
                }
                if let Some(beat) = best.filter(|&b| b < duration && b - v0 >= MIN_GAP) {
                    anchors.push((beat, hit));
                }
            }
        }
        let (v, s) = *anchors.last().unwrap();
        let end = v.max(duration) + 1.0;
        anchors.push((end, s + (end - v)));
        let slopes = monotone_slopes(&anchors);
        Self { anchors, slopes }
    }

    /// No stretching at all.
    pub fn identity() -> Self {
        Self { anchors: vec![(0.0, 0.0), (1.0, 1.0)], slopes: vec![1.0, 1.0] }
    }

    /// Whether video time and simulation time are the same everywhere.
    pub fn is_identity(&self) -> bool {
        self.anchors.iter().all(|(v, s)| v == s)
    }

    /// The simulation time to show at video time `video`.
    pub fn sim_time(&self, video: f64) -> f64 {
        let a = &self.anchors;
        if video <= 0.0 {
            return video;
        }
        let i = a.partition_point(|p| p.0 <= video);
        if i >= a.len() {
            let (v, s) = a[a.len() - 1];
            return s + (video - v);
        }
        let (v0, s0) = a[i - 1];
        let (v1, s1) = a[i];
        let h = v1 - v0;
        let t = (video - v0) / h;
        // Cubic Hermite between the two anchors.
        let (t2, t3) = (t * t, t * t * t);
        let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
        let h10 = t3 - 2.0 * t2 + t;
        let h01 = -2.0 * t3 + 3.0 * t2;
        let h11 = t3 - t2;
        h00 * s0 + h10 * h * self.slopes[i - 1] + h01 * s1 + h11 * h * self.slopes[i]
    }

    /// The video time at which simulation time `sim` is shown: the inverse of
    /// [`TimeWarp::sim_time`].
    pub fn video_time(&self, sim: f64) -> f64 {
        if sim <= 0.0 {
            return sim;
        }
        let a = &self.anchors;
        let i = a.partition_point(|p| p.1 <= sim);
        if i >= a.len() {
            let (v, s) = a[a.len() - 1];
            return v + (sim - s);
        }
        if a[i - 1].1 == sim {
            return a[i - 1].0;
        }
        let (mut lo, mut hi) = (a[i - 1].0, a[i].0);
        for _ in 0..60 {
            let mid = (lo + hi) * 0.5;
            if self.sim_time(mid) < sim {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) * 0.5
    }
}

/// Slopes at each point for a monotone cubic through them (Fritsch and Carlson), so the
/// curve never runs backwards or overshoots.
fn monotone_slopes(points: &[(f64, f64)]) -> Vec<f64> {
    let n = points.len();
    let secants: Vec<f64> = points.windows(2).map(|w| (w[1].1 - w[0].1) / (w[1].0 - w[0].0)).collect();
    let mut slopes = vec![0.0; n];
    slopes[0] = secants[0];
    slopes[n - 1] = secants[n - 2];
    for i in 1..n - 1 {
        let (a, b) = (secants[i - 1], secants[i]);
        slopes[i] = if a * b <= 0.0 { 0.0 } else { 2.0 / (1.0 / a + 1.0 / b) };
    }
    slopes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_leaves_time_alone() {
        let warp = TimeWarp::identity();
        assert!(warp.is_identity());
        for t in [0.0, 0.3, 1.0, 7.5] {
            assert!((warp.sim_time(t) - t).abs() < 1e-9);
            assert!((warp.video_time(t) - t).abs() < 1e-9);
        }
        assert_eq!(TimeWarp::default(), warp);
        // No strength, or no hits, is the identity too.
        assert!(TimeWarp::fit(&[1.1], &[1.0], 0.0, 10.0).is_identity());
        assert!(TimeWarp::fit(&[], &[1.0], 0.5, 10.0).is_identity());
    }

    #[test]
    fn a_warp_moves_a_hit_onto_a_beat_within_its_strength() {
        let beats: Vec<f64> = (0..40).map(|k| k as f64 * 0.5).collect();
        let warp = TimeWarp::fit(&[1.1], &beats, 0.5, 10.0);
        assert!(!warp.is_identity());
        assert!((warp.video_time(1.1) - 1.0).abs() < 1e-6, "{}", warp.video_time(1.1));
        assert!((warp.sim_time(1.0) - 1.1).abs() < 1e-9);
        let mut last = f64::NEG_INFINITY;
        for k in 0..=1000 {
            let s = warp.sim_time(k as f64 * 0.01);
            assert!(s >= last, "time ran backwards at {k}");
            last = s;
        }
        // Far past the last hit the clocks run together again.
        assert!((warp.sim_time(12.0) - warp.sim_time(11.0) - 1.0).abs() < 1e-9);
        // A hit too far from any beat for the strength is left alone.
        let weak = TimeWarp::fit(&[1.24], &[0.0, 2.0], 0.1, 10.0);
        assert!(weak.is_identity());
    }

    #[test]
    fn video_time_inverts_sim_time() {
        let beats: Vec<f64> = (0..80).map(|k| k as f64 * 0.25).collect();
        let warp = TimeWarp::fit(&[0.3, 0.9, 1.7, 2.6, 4.1], &beats, 0.4, 10.0);
        for k in 0..=200 {
            let video = k as f64 * 0.05;
            let sim = warp.sim_time(video);
            assert!((warp.video_time(sim) - video).abs() < 1e-6, "{video} -> {sim}");
        }
        assert_eq!(warp.video_time(-1.0), -1.0);
    }
}
