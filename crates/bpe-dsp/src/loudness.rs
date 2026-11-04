//! Loudness as EBU R128 measures it (ITU-R BS.1770): K-weighting, momentary (400 ms) and
//! short-term (3 s) loudness, integrated loudness with the absolute and relative gates, and the
//! true peak. Fixed memory: the integrated gate keeps a histogram of 0.01 LU bins.

use crate::oversample::PeakDetector;

/// Samples in one 100 ms piece (four make a gating block, thirty a short-term window).
const PIECE: usize = 4_800;
/// Pieces kept for the short-term window.
const SHORT: usize = 30;
/// Histogram bins, 0.01 LU each, from the absolute gate (-70 LUFS) to +10 LUFS.
const BINS: usize = 8_000;
const LOWEST: f64 = -70.0;
const BIN: f64 = 0.01;

/// One biquad, in f64 (direct form I).
#[derive(Clone, Debug, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Self { b, a, ..Self::default() }
    }

    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// The two K-weighting stages for one channel at 48 kHz, as BS.1770 gives them.
fn k_weighting() -> [Biquad; 2] {
    [
        Biquad::new(
            [1.535_124_859_586_97, -2.691_696_189_406_38, 1.198_392_810_852_85],
            [-1.690_659_293_182_41, 0.732_480_774_215_85],
        ),
        Biquad::new([1.0, -2.0, 1.0], [-1.990_047_454_833_98, 0.990_072_250_366_21]),
    ]
}

fn lufs(mean_square: f64) -> f32 {
    if mean_square <= 0.0 { f32::NEG_INFINITY } else { (-0.691 + 10.0 * mean_square.log10()) as f32 }
}

/// A stereo loudness meter.
#[derive(Clone, Debug)]
pub struct LoudnessMeter {
    filters: [[Biquad; 2]; 2],
    peaks: [PeakDetector; 2],
    /// The summed weighted power of the piece being filled, and how many samples are in it.
    piece_sum: f64,
    piece_len: usize,
    /// Each finished piece's mean square, newest at `head - 1`.
    pieces: [f64; SHORT],
    head: usize,
    finished: u64,
    /// Gating blocks above the absolute gate: their energies by loudness bin, and in all.
    bins: Vec<(f64, u32)>,
    total: (f64, u64),
    integrated: f32,
    true_peak: f32,
}

impl Default for LoudnessMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl LoudnessMeter {
    pub fn new() -> Self {
        Self {
            filters: [k_weighting(), k_weighting()],
            peaks: [PeakDetector::new(), PeakDetector::new()],
            piece_sum: 0.0,
            piece_len: 0,
            pieces: [0.0; SHORT],
            head: 0,
            finished: 0,
            bins: vec![(0.0, 0); BINS],
            total: (0.0, 0),
            integrated: f32::NEG_INFINITY,
            true_peak: 0.0,
        }
    }

    /// Starts again, as new (keeps its memory).
    pub fn reset(&mut self) {
        self.filters = [k_weighting(), k_weighting()];
        for p in &mut self.peaks {
            p.reset();
        }
        self.piece_sum = 0.0;
        self.piece_len = 0;
        self.pieces = [0.0; SHORT];
        self.head = 0;
        self.finished = 0;
        self.bins.iter_mut().for_each(|b| *b = (0.0, 0));
        self.total = (0.0, 0);
        self.integrated = f32::NEG_INFINITY;
        self.true_peak = 0.0;
    }

    /// Takes the next samples of both channels.
    pub fn process(&mut self, l: &[f32], r: &[f32]) {
        for (&a, &b) in l.iter().zip(r) {
            self.true_peak = self.true_peak.max(self.peaks[0].push(a)).max(self.peaks[1].push(b));
            let [[la, lb], [ra, rb]] = &mut self.filters;
            let za = lb.run(la.run(a as f64));
            let zb = rb.run(ra.run(b as f64));
            self.piece_sum += za * za + zb * zb;
            self.piece_len += 1;
            if self.piece_len == PIECE {
                self.finish_piece();
            }
        }
    }

    fn finish_piece(&mut self) {
        self.pieces[self.head] = self.piece_sum / PIECE as f64;
        self.head = (self.head + 1) % SHORT;
        self.finished += 1;
        self.piece_sum = 0.0;
        self.piece_len = 0;
        if self.finished >= 4 {
            let block = self.mean_of(4);
            let l = lufs(block) as f64;
            if l > LOWEST {
                let i = (((l - LOWEST) / BIN) as usize).min(BINS - 1);
                self.bins[i].0 += block;
                self.bins[i].1 += 1;
                self.total.0 += block;
                self.total.1 += 1;
                self.integrated = self.gated();
            }
        }
    }

    /// The mean square over the last `n` pieces (silence before the start).
    fn mean_of(&self, n: usize) -> f64 {
        (1..=n).map(|k| self.pieces[(self.head + SHORT - k) % SHORT]).sum::<f64>() / n as f64
    }

    fn gated(&self) -> f32 {
        if self.total.1 == 0 {
            return f32::NEG_INFINITY;
        }
        let relative = lufs(self.total.0 / self.total.1 as f64) as f64 - 10.0;
        let from = if relative <= LOWEST { 0 } else { (((relative - LOWEST) / BIN).ceil() as usize).min(BINS) };
        let (sum, count) = self.bins[from..].iter().fold((0.0, 0u64), |(s, c), b| (s + b.0, c + b.1 as u64));
        if count == 0 { f32::NEG_INFINITY } else { lufs(sum / count as f64) }
    }

    /// Loudness over the last 400 ms, in LUFS.
    pub fn momentary(&self) -> f32 {
        lufs(self.mean_of(4))
    }

    /// Loudness over the last 3 s, in LUFS.
    pub fn short_term(&self) -> f32 {
        lufs(self.mean_of(SHORT))
    }

    /// Loudness of everything so far, gated, in LUFS.
    pub fn integrated(&self) -> f32 {
        self.integrated
    }

    /// The highest true peak so far, in dBTP.
    pub fn true_peak_db(&self) -> f32 {
        crate::gain_to_db(self.true_peak)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::sine;

    fn feed(m: &mut LoudnessMeter, db: f32, seconds: usize) {
        let x = sine(1_000.0, crate::db_to_gain(db) as f64, 0.0, 48_000 * seconds);
        for c in x.chunks(crate::BLOCK) {
            m.process(c, c);
        }
    }

    #[test]
    fn loudness_matches_tech_3341() {
        for db in [-23.0, -20.0] {
            let mut m = LoudnessMeter::new();
            feed(&mut m, db, 10);
            for (what, v) in [("momentary", m.momentary()), ("short", m.short_term()), ("integrated", m.integrated())] {
                assert!((v - db).abs() < 0.1, "{db}: {what} {v}");
            }
            assert!((m.true_peak_db() - db).abs() < 0.1, "true peak {}", m.true_peak_db());
        }
        // The quiet part falls under the relative gate (Tech 3341 cases 3 and 4 work the same way).
        let mut m = LoudnessMeter::new();
        feed(&mut m, -36.0, 20);
        feed(&mut m, -23.0, 20);
        assert!((m.integrated() - -23.0).abs() < 0.1, "gated {}", m.integrated());
        m.reset();
        assert_eq!(m.integrated(), f32::NEG_INFINITY);
        assert_eq!(m.momentary(), f32::NEG_INFINITY);
    }

    #[test]
    fn silence_reads_minus_infinity_and_the_absolute_gate_holds() {
        let mut m = LoudnessMeter::new();
        feed(&mut m, -80.0, 5);
        assert_eq!(m.integrated(), f32::NEG_INFINITY);
        assert!(m.momentary() < -70.0);
    }
}
