//! The master's true-peak limiter: a short lookahead, peaks found between samples (4x), and a
//! gain that is never above what any nearby peak needs, so the output's true peak stays under
//! the ceiling.

use crate::oversample::PeakDetector;

/// Frames the gain glides down over before a peak (1.5 ms at 48 kHz).
const ATTACK: usize = 72;
/// The most frames the peak detector lags the sound by.
const LAG: usize = PeakDetector::DELAY;
/// Frames either side of a peak that are also held down, for the peaks between frames.
const SPREAD: usize = 2;
/// The frames around each output frame whose peaks it must respect.
const SPAN: usize = LAG + 2 * SPREAD + 1;
/// The sliding minimum's window.
const WINDOW: usize = ATTACK + SPAN;
/// How late the sound comes out, in frames.
const DELAY: usize = ATTACK + LAG + SPREAD - 1;
/// Ring sizes (powers of two, larger than what they hold).
const RING: usize = 128;

/// A sliding minimum over the last [`WINDOW`] values, in fixed memory.
#[derive(Clone, Debug)]
struct MinWindow {
    /// (frame number, value), values rising from front to back.
    items: [(u64, f32); RING],
    head: usize,
    len: usize,
}

impl MinWindow {
    fn new() -> Self {
        Self { items: [(0, 1.0); RING], head: 0, len: 0 }
    }

    fn push(&mut self, at: u64, v: f32) -> f32 {
        while self.len > 0 && self.items[(self.head + self.len - 1) % RING].1 >= v {
            self.len -= 1;
        }
        self.items[(self.head + self.len) % RING] = (at, v);
        self.len += 1;
        while self.items[self.head].0 + (WINDOW as u64) <= at {
            self.head = (self.head + 1) % RING;
            self.len -= 1;
        }
        self.items[self.head].1
    }
}

/// A stereo true-peak limiter with its two channels linked: two stages in a row, the second
/// catching the small peaks the first one's moving gain makes between samples (up to about
/// 0.2 dB on hot noise).
#[derive(Clone, Debug)]
pub struct TruePeakLimiter {
    stages: [Stage; 2],
}

/// One limiting stage.
#[derive(Clone, Debug)]
struct Stage {
    ceiling: f32,
    release: f32,
    peaks: [PeakDetector; 2],
    mins: MinWindow,
    /// The last [`ATTACK`] sliding minimums, and their sum.
    recent: [f32; ATTACK],
    sum: f64,
    /// Frames waiting to come out.
    audio: [[f32; 2]; RING],
    frame: u64,
    gain: f32,
    /// The lowest gain of the last call, for the meter.
    lowest: f32,
}

impl Default for TruePeakLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl TruePeakLimiter {
    /// A limiter at -1 dBTP with a 60 ms release.
    pub fn new() -> Self {
        Self { stages: [Stage::new(), Stage::new()] }
    }

    /// The ceiling in dBTP (-24..0) and how long the gain takes to come back up, in ms.
    pub fn set(&mut self, ceiling_db: f32, release_ms: f32) {
        for s in &mut self.stages {
            s.set(ceiling_db, release_ms);
        }
    }

    /// Back to silence, as new.
    pub fn reset(&mut self) {
        for s in &mut self.stages {
            s.reset();
        }
    }

    /// How late the sound comes out, in frames.
    pub fn latency(&self) -> usize {
        2 * DELAY
    }

    /// How far the gain went down during the last call, in dB (0 or less).
    pub fn gain_reduction_db(&self) -> f32 {
        crate::gain_to_db(self.stages[0].lowest * self.stages[1].lowest)
    }

    /// Limits `l` and `r` in place (they come out [`TruePeakLimiter::latency`] frames late).
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for s in &mut self.stages {
            s.lowest = 1.0;
        }
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let once = self.stages[0].push([*a, *b]);
            [*a, *b] = self.stages[1].push(once);
        }
    }
}

impl Stage {
    fn new() -> Self {
        let mut l = Self {
            ceiling: 1.0,
            release: 0.0,
            peaks: [PeakDetector::new(), PeakDetector::new()],
            mins: MinWindow::new(),
            recent: [1.0; ATTACK],
            sum: ATTACK as f64,
            audio: [[0.0; 2]; RING],
            frame: 0,
            gain: 1.0,
            lowest: 1.0,
        };
        l.set(-1.0, 60.0);
        l
    }

    fn set(&mut self, ceiling_db: f32, release_ms: f32) {
        self.ceiling = crate::db_to_gain(ceiling_db.clamp(-24.0, 0.0));
        let samples = (release_ms.max(1.0) / 1_000.0 * crate::SAMPLE_RATE).max(1.0);
        self.release = 1.0 - (-1.0 / samples).exp();
    }

    fn reset(&mut self) {
        let (ceiling, release) = (self.ceiling, self.release);
        *self = Self { ceiling, release, ..Self::new() };
    }

    #[inline]
    fn push(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let peak = self.peaks[0].push(frame[0]).max(self.peaks[1].push(frame[1]));
        let need = if peak > self.ceiling && peak.is_finite() { self.ceiling / peak } else { 1.0 };
        let at = self.frame;
        self.frame += 1;
        let min = self.mins.push(at, need);
        let slot = (at % ATTACK as u64) as usize;
        self.sum += min as f64 - self.recent[slot] as f64;
        self.recent[slot] = min;
        // Every minimum averaged covers the frames whose peaks the frame coming out must respect,
        // so their average is never above what any of them needs, and it slides down smoothly.
        let target = ((self.sum / ATTACK as f64) as f32).min(1.0);
        self.gain = if target < self.gain { target } else { self.gain + (target - self.gain) * self.release };
        self.lowest = self.lowest.min(self.gain);
        let ring = (at % RING as u64) as usize;
        self.audio[ring] = frame;
        let out = if at >= DELAY as u64 { self.audio[((at - DELAY as u64) % RING as u64) as usize] } else { [0.0; 2] };
        [out[0] * self.gain, out[1] * self.gain]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Noise, sine};

    fn limited(l: &[f32], r: &[f32], ceiling: f32) -> (Vec<f32>, Vec<f32>) {
        let mut lim = TruePeakLimiter::new();
        lim.set(ceiling, 60.0);
        let pad = lim.latency() + 64;
        let mut a: Vec<f32> = l.iter().copied().chain(std::iter::repeat_n(0.0, pad)).collect();
        let mut b: Vec<f32> = r.iter().copied().chain(std::iter::repeat_n(0.0, pad)).collect();
        for (ca, cb) in a.chunks_mut(crate::BLOCK).zip(b.chunks_mut(crate::BLOCK)) {
            lim.process(ca, cb);
        }
        (a, b)
    }

    fn tp_db(x: &[f32]) -> f32 {
        crate::gain_to_db(crate::true_peak(x))
    }

    #[test]
    fn limiter_never_exceeds_ceiling_as_true_peak() {
        let mut n = Noise(7);
        let l: Vec<f32> = (0..96_000).map(|_| n.next() * 4.0).collect();
        let r: Vec<f32> = (0..96_000).map(|_| n.next() * 4.0).collect();
        let (a, b) = limited(&l, &r, -1.0);
        assert!(tp_db(&a) <= -0.95 && tp_db(&b) <= -0.95, "noise: {} {}", tp_db(&a), tp_db(&b));
        // Every sample at 0 dBFS, with peaks 3 dB higher between them.
        let x = sine(12_000.0, std::f64::consts::SQRT_2, std::f64::consts::FRAC_PI_4, 48_000);
        assert!(x.iter().fold(0.0f32, |m, v| m.max(v.abs())) <= 1.0001);
        let (a, b) = limited(&x, &x, -1.0);
        assert!(tp_db(&a) <= -0.95 && tp_db(&b) <= -0.95, "inter-sample: {}", tp_db(&a));
        // And a burst that comes out of silence.
        let burst: Vec<f32> = (0..9_600).map(|i| if (4_800..4_900).contains(&i) { 3.0 } else { 0.01 }).collect();
        let (a, _) = limited(&burst, &burst, -1.0);
        assert!(tp_db(&a) <= -0.95, "burst: {}", tp_db(&a));
    }

    #[test]
    fn limiter_is_transparent_below_ceiling() {
        let x = sine(1_000.0, crate::db_to_gain(-10.0) as f64, 0.0, 48_000);
        let (a, _) = limited(&x, &x, -1.0);
        let d = 2 * DELAY;
        let worst = x.iter().zip(&a[d..]).map(|(i, o)| (i - o).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "{worst}");
    }

    #[test]
    fn the_gain_comes_back_after_a_peak() {
        let mut lim = TruePeakLimiter::new();
        let mut a = vec![0.0f32; 48_000];
        let mut b = vec![0.0f32; 48_000];
        a[100] = 4.0;
        b[100] = 4.0;
        lim.process(&mut a[..1_000], &mut b[..1_000]);
        assert!(lim.gain_reduction_db() < -12.0);
        lim.process(&mut a[1_000..], &mut b[1_000..]);
        assert!(lim.stages.iter().all(|s| s.gain > 0.999), "released");
    }
}
