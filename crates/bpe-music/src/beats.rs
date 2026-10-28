//! Beat tracking: tempo from the onset envelope's self-similarity, then beat times by dynamic
//! programming (Ellis, "Beat Tracking by Dynamic Programming", 2007), each beat nudged onto the
//! nearest onset peak.

use crate::FRAME_RATE;

/// Tempos are folded into this octave, in beats per minute. A beat at 87 BPM reads as 174, one
/// at 180 as 90: either way every beat still falls on the grid.
const SLOWEST: f32 = 88.0;
const FASTEST: f32 = 176.0;
/// How strongly beats keep to the tempo rather than chase every onset.
const TIGHTNESS: f32 = 400.0;

pub struct Found {
    pub beats: Vec<f64>,
    pub downbeats: Vec<f64>,
    pub bpm: f32,
}

/// Turns spectral flux into an onset envelope: the slow trend is removed and the rest scaled
/// to unit spread, so the same tightness works for quiet and loud songs.
pub fn onset_envelope(flux: &[f32]) -> Vec<f32> {
    let n = flux.len();
    if n == 0 {
        return Vec::new();
    }
    // Running mean over about a quarter second, from a prefix sum.
    let half = (FRAME_RATE * 0.125) as usize;
    let mut prefix = Vec::with_capacity(n + 1);
    prefix.push(0.0f64);
    for &f in flux {
        prefix.push(prefix.last().unwrap() + f as f64);
    }
    let mut out: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(half), (i + half + 1).min(n));
            let mean = (prefix[b] - prefix[a]) / (b - a) as f64;
            (flux[i] - mean as f32).max(0.0)
        })
        .collect();
    let mean = out.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let spread = (out.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
    if spread > 1e-9 {
        out.iter_mut().for_each(|v| *v /= spread as f32);
    }
    out
}

/// Finds beats in an onset envelope. `loudness` (same frames) picks the downbeats. `window` is the
/// analysis window in seconds, which makes onsets read a little early.
pub fn track(onset: &[f32], loudness: &[f32], window: f32) -> Found {
    let none = Found { beats: Vec::new(), downbeats: Vec::new(), bpm: 0.0 };
    let n = onset.len();
    let Some(period) = tempo(onset) else { return none };
    if n < (period * 4.0) as usize {
        return none;
    }
    // Ellis's recursion: each frame's best score is its onset plus the best earlier beat about
    // one period back, minus a penalty for straying from the period.
    let mut score = vec![0.0f32; n];
    let mut back = vec![usize::MAX; n];
    let (low, high) = ((period * 0.5).round() as usize, (period * 2.0).round() as usize);
    for t in 0..n {
        let mut best = 0.0f32;
        let mut from = usize::MAX;
        for gap in low.max(1)..=high.min(t) {
            let earlier = t - gap;
            let stray = (gap as f32 / period).ln();
            let candidate = score[earlier] - TIGHTNESS * stray * stray;
            if candidate > best || from == usize::MAX {
                best = candidate;
                from = earlier;
            }
        }
        score[t] = onset[t] + best.max(0.0);
        back[t] = if best > 0.0 { from } else { usize::MAX };
    }
    // Start from the best-scoring frame in the last period and walk back.
    let tail = n.saturating_sub(period.ceil() as usize);
    let mut t = (tail..n).max_by(|&a, &b| score[a].total_cmp(&score[b])).unwrap_or(n - 1);
    let mut frames = vec![t];
    while back[t] != usize::MAX {
        t = back[t];
        frames.push(t);
    }
    frames.reverse();

    // Drop beats at the very start or end that land on silence.
    let strengths: Vec<f32> = frames.iter().map(|&f| local_max(onset, f, period * 0.1).1).collect();
    let mut sorted = strengths.clone();
    sorted.sort_by(f32::total_cmp);
    let typical = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
    let keep = |s: f32| s > typical * 0.1 && s > 0.0;
    let first = strengths.iter().position(|&s| keep(s)).unwrap_or(frames.len());
    let last = strengths.iter().rposition(|&s| keep(s)).map_or(first, |i| i + 1);
    if last <= first + 1 {
        return none;
    }
    let lag = window / 4.0;
    let beats: Vec<f64> = frames[first..last]
        .iter()
        .map(|&f| {
            let (peak, _) = local_max(onset, f, period * 0.125);
            (peak / FRAME_RATE + lag) as f64
        })
        .collect();

    // Slope of beat time against beat number, for a tempo that isn't stuck to whole frames.
    let count = beats.len() as f64;
    let mean_i = (count - 1.0) / 2.0;
    let mean_t = beats.iter().sum::<f64>() / count;
    let (mut top, mut bottom) = (0.0, 0.0);
    for (i, &b) in beats.iter().enumerate() {
        top += (i as f64 - mean_i) * (b - mean_t);
        bottom += (i as f64 - mean_i).powi(2);
    }
    let bpm = if top > 0.0 { (60.0 * bottom / top) as f32 } else { 0.0 };

    // The bar starts where the beats are loudest (kick drums, accented clicks).
    let weight = |time: f64| {
        let f = (time * FRAME_RATE as f64) as usize;
        loudness.get(f.saturating_sub(2)..(f + 4).min(loudness.len())).map_or(0.0, |w| w.iter().copied().fold(0.0, f32::max))
    };
    let phase = (0..4.min(beats.len()))
        .max_by(|&a, &b| {
            let sum = |p: usize| beats.iter().skip(p).step_by(4).map(|&t| weight(t)).sum::<f32>();
            sum(a).total_cmp(&sum(b))
        })
        .unwrap_or(0);
    let downbeats = beats.iter().skip(phase).step_by(4).copied().collect();
    Found { beats, downbeats, bpm }
}

/// The strongest repeat time in frames, folded into one octave of tempo, or None for a song
/// with no steady pulse.
fn tempo(onset: &[f32]) -> Option<f32> {
    let n = onset.len();
    let (shortest, longest) = (60.0 * FRAME_RATE / FASTEST, 60.0 * FRAME_RATE / SLOWEST);
    let max_lag = (longest * 4.0).ceil() as usize + 2;
    if n < max_lag * 2 {
        return None;
    }
    let mean = onset.iter().sum::<f32>() / n as f32;
    let centered: Vec<f32> = onset.iter().map(|&o| o - mean).collect();
    let energy: f32 = centered.iter().map(|v| v * v).sum();
    if energy <= 1e-9 {
        return None;
    }
    let autocorrelation: Vec<f32> = (0..=max_lag)
        .map(|lag| centered[..n - lag].iter().zip(&centered[lag..]).map(|(a, b)| a * b).sum::<f32>() / energy)
        .collect();
    let at = |lag: f32| {
        let i = lag.floor() as usize;
        let t = lag - i as f32;
        autocorrelation[i] * (1.0 - t) + autocorrelation[(i + 1).min(max_lag)] * t
    };
    // A real beat repeats at one, two, three and four periods.
    let comb = |lag: f32| (1..=4).map(|m| at(lag * m as f32)).sum::<f32>();
    let mut best = (f32::MIN, shortest);
    let mut lag = shortest;
    while lag <= longest {
        let score = comb(lag);
        if score > best.0 {
            best = (score, lag);
        }
        lag += 0.25;
    }
    if best.0 <= 0.05 {
        return None;
    }
    Some(best.1)
}

/// The sub-frame position and height of the highest onset within `reach` frames of `frame`.
fn local_max(onset: &[f32], frame: usize, reach: f32) -> (f32, f32) {
    let reach = reach.max(1.0) as usize;
    let (a, b) = (frame.saturating_sub(reach), (frame + reach + 1).min(onset.len()));
    let Some(i) = (a..b).max_by(|&x, &y| onset[x].total_cmp(&onset[y])) else { return (frame as f32, 0.0) };
    let height = onset[i];
    if i == 0 || i + 1 >= onset.len() {
        return (i as f32, height);
    }
    // Fit a parabola through the peak and its neighbours.
    let (l, c, r) = (onset[i - 1], onset[i], onset[i + 1]);
    let curve = l - 2.0 * c + r;
    let shift = if curve.abs() > 1e-9 { (0.5 * (l - r) / curve).clamp(-0.5, 0.5) } else { 0.0 };
    (i as f32 + shift, height)
}
