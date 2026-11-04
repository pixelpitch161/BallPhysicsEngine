//! The spectrum analyser behind the EQ's curve: a 2048-point FFT with a Hann window every 512
//! samples, read out as 256 bars spaced evenly in pitch from 20 Hz to 20 kHz.

/// How many bars the analyser shows.
pub const BINS: usize = 256;
/// Samples in one FFT.
const SIZE: usize = 2048;
/// Samples between FFTs (75 % overlap).
const HOP: usize = 512;
/// The lowest and highest bars' frequencies.
const LOW_HZ: f64 = 20.0;
const HIGH_HZ: f64 = 20_000.0;
/// The quietest a bar reads, in dB.
const FLOOR_DB: f32 = -120.0;
/// How long a bar takes to fall 63 % of the way (in dB) to a quieter reading, in ms.
const FALL_MS: f32 = 300.0;

/// A complex FFT of a fixed power-of-two size, iterative radix 2, in place.
pub(crate) struct Fft {
    /// exp(-2πik/n) for k below n/2.
    twiddles: Box<[(f32, f32)]>,
    /// Where each sample goes before the butterflies.
    reversed: Box<[u32]>,
}

impl Fft {
    /// Prepares an FFT of `n` points; `n` must be a power of two.
    pub(crate) fn new(n: usize) -> Self {
        assert!(n.is_power_of_two() && n >= 2, "an FFT needs a power of two");
        let bits = n.trailing_zeros();
        let twiddles = (0..n / 2)
            .map(|k| {
                let ph = -std::f64::consts::TAU * k as f64 / n as f64;
                (ph.cos() as f32, ph.sin() as f32)
            })
            .collect();
        let reversed = (0..n as u32).map(|i| i.reverse_bits() >> (32 - bits)).collect();
        Self { twiddles, reversed }
    }

    /// Transforms `re` + i·`im` (each `n` long) in place.
    pub(crate) fn run(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.reversed.len();
        debug_assert!(re.len() == n && im.len() == n);
        for (i, &j) in self.reversed.iter().enumerate() {
            let j = j as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut size = 2;
        while size <= n {
            let (half, stride) = (size / 2, n / size);
            for start in (0..n).step_by(size) {
                for k in 0..half {
                    let (wr, wi) = self.twiddles[k * stride];
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            size *= 2;
        }
    }
}

/// Which FFT bins one bar covers.
#[derive(Clone, Copy, Debug)]
struct Span {
    /// The whole FFT bins inside the bar (may be empty).
    first: usize,
    end: usize,
    /// The bar's middle, in FFT bins.
    middle: f32,
}

/// The spectrum analyser. Bars rise at once and fall smoothly, about 300 ms per 63 % in dB.
/// A full-scale sine reads about 0 dB in its bar, and silence reads -120 dB.
pub struct Analyser {
    fft: Fft,
    window: Box<[f32]>,
    ring: Box<[f32]>,
    at: usize,
    since: usize,
    re: Box<[f32]>,
    im: Box<[f32]>,
    /// The latest FFT's levels in dB, bins 0..=SIZE/2.
    levels: Box<[f32]>,
    spans: Box<[Span]>,
    frame: [f32; BINS],
    bins: [f32; BINS],
    fall: f32,
}

impl Default for Analyser {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyser {
    /// An analyser reading silence, with all its buffers made.
    pub fn new() -> Self {
        let window = (0..SIZE).map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / SIZE as f64).cos()) as f32);
        let per_bin = crate::SAMPLE_RATE as f64 / SIZE as f64;
        let half_step = (HIGH_HZ / LOW_HZ).powf(0.5 / (BINS - 1) as f64);
        let spans = (0..BINS)
            .map(|i| {
                let middle = Self::bin_hz(i) as f64 / per_bin;
                let (lo, hi) = (middle / half_step, middle * half_step);
                let first = (lo.ceil() as usize).min(SIZE / 2 + 1);
                let end = ((hi.ceil() as usize).min(SIZE / 2 + 1)).max(first);
                Span { first, end, middle: middle as f32 }
            })
            .collect();
        Self {
            fft: Fft::new(SIZE),
            window: window.collect(),
            ring: vec![0.0; SIZE].into_boxed_slice(),
            at: 0,
            since: 0,
            re: vec![0.0; SIZE].into_boxed_slice(),
            im: vec![0.0; SIZE].into_boxed_slice(),
            levels: vec![FLOOR_DB; SIZE / 2 + 1].into_boxed_slice(),
            spans,
            frame: [FLOOR_DB; BINS],
            bins: [FLOOR_DB; BINS],
            fall: (-(HOP as f32) / (FALL_MS / 1_000.0 * crate::SAMPLE_RATE)).exp(),
        }
    }

    /// Forgets the sound so far: every bar reads silence.
    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.at = 0;
        self.since = 0;
        self.bins = [FLOOR_DB; BINS];
    }

    /// Listens to a stereo block (its mono sum).
    pub fn push(&mut self, l: &[f32], r: &[f32]) {
        for (a, b) in l.iter().zip(r) {
            self.ring[self.at] = 0.5 * (a + b);
            self.at = (self.at + 1) % SIZE;
            self.since += 1;
            if self.since == HOP {
                self.since = 0;
                self.analyse();
            }
        }
    }

    /// The bars, in dB (-120 at the quietest), lowest frequency first.
    pub fn bins(&self) -> &[f32; BINS] {
        &self.bins
    }

    /// Bar `i`'s middle frequency, in Hz.
    pub fn bin_hz(i: usize) -> f32 {
        (LOW_HZ * (HIGH_HZ / LOW_HZ).powf(i as f64 / (BINS - 1) as f64)) as f32
    }

    /// The bar a frequency (in FFT bins) falls in, if any.
    fn bar_of(bin: f32) -> Option<usize> {
        let hz = bin as f64 * crate::SAMPLE_RATE as f64 / SIZE as f64;
        if hz <= 0.0 {
            return None;
        }
        let i = ((hz / LOW_HZ).ln() / (HIGH_HZ / LOW_HZ).ln() * (BINS - 1) as f64).round();
        (0.0..BINS as f64).contains(&i).then_some(i as usize)
    }

    /// Runs one FFT over the last [`SIZE`] samples and updates the bars.
    fn analyse(&mut self) {
        // The ring's oldest sample is where the next one will be written.
        for (i, (re, im)) in self.re.iter_mut().zip(self.im.iter_mut()).enumerate() {
            *re = self.ring[(self.at + i) % SIZE] * self.window[i];
            *im = 0.0;
        }
        self.fft.run(&mut self.re, &mut self.im);
        // A Hann window keeps half a sine's height and a real sine's height splits over two
        // mirrored bins, so 4 / SIZE makes a full-scale sine read 1.
        let scale = 4.0 / SIZE as f32;
        for (k, level) in self.levels.iter_mut().enumerate() {
            let mag = (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt() * scale;
            *level = 20.0 * mag.max(1e-7).log10();
        }
        let levels = &self.levels;
        // Each bar reads the loudest FFT bin inside it, or between the bins round its middle
        // when it is narrower than one.
        for (out, span) in self.frame.iter_mut().zip(self.spans.iter()) {
            let below = (span.middle.floor() as usize).min(SIZE / 2 - 1);
            let t = span.middle - below as f32;
            let between = levels[below] + t * (levels[below + 1] - levels[below]);
            *out = levels[span.first..span.end].iter().copied().fold(between, f32::max);
        }
        // A peak between FFT bins reads its true height in the bar it falls in (a parabola
        // through the three bins round it, in dB).
        for k in 1..SIZE / 2 {
            let (a, b, c) = (levels[k - 1], levels[k], levels[k + 1]);
            if b > a && b >= c && b > FLOOR_DB {
                let bend = a - 2.0 * b + c;
                let p = if bend < 0.0 { (0.5 * (a - c) / bend).clamp(-0.5, 0.5) } else { 0.0 };
                if let Some(i) = Self::bar_of(k as f32 + p) {
                    self.frame[i] = self.frame[i].max(b - 0.25 * (a - c) * p);
                }
            }
        }
        for (bar, &new) in self.bins.iter_mut().zip(&self.frame) {
            let new = new.max(FLOOR_DB);
            *bar = if new >= *bar { new } else { new + (*bar - new) * self.fall };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BLOCK;
    use crate::testing::{Noise, sine};

    fn feed(a: &mut Analyser, x: &[f32]) {
        for c in x.chunks(BLOCK) {
            a.push(c, c);
        }
    }

    #[test]
    fn fft_matches_a_plain_dft() {
        let n = 64;
        let mut noise = Noise(5);
        let x: Vec<(f32, f32)> = (0..n).map(|_| (noise.next(), noise.next())).collect();
        let (mut re, mut im): (Vec<f32>, Vec<f32>) = x.iter().copied().unzip();
        Fft::new(n).run(&mut re, &mut im);
        for k in 0..n {
            let (mut wr, mut wi) = (0.0f64, 0.0f64);
            for (t, &(a, b)) in x.iter().enumerate() {
                let ph = -std::f64::consts::TAU * (k * t) as f64 / n as f64;
                wr += a as f64 * ph.cos() - b as f64 * ph.sin();
                wi += a as f64 * ph.sin() + b as f64 * ph.cos();
            }
            assert!((re[k] as f64 - wr).abs() < 1e-4 && (im[k] as f64 - wi).abs() < 1e-4, "bin {k}");
        }
    }

    #[test]
    fn bins_run_from_20_hz_to_20_khz() {
        assert!((Analyser::bin_hz(0) - 20.0).abs() < 1e-3);
        assert!((Analyser::bin_hz(BINS - 1) - 20_000.0).abs() < 0.1);
        let r = Analyser::bin_hz(101) / Analyser::bin_hz(100);
        assert!((r - Analyser::bin_hz(201) / Analyser::bin_hz(200)).abs() < 1e-4);
    }

    #[test]
    fn analyser_finds_a_sine() {
        for hz in [1_000.0f64, 97.0, 6_300.0] {
            let mut a = Analyser::new();
            feed(&mut a, &sine(hz, 1.0, 0.0, 48_000));
            let bins = a.bins();
            let peak = (0..BINS).max_by(|&i, &j| bins[i].total_cmp(&bins[j])).unwrap();
            let off = |i: usize| (Analyser::bin_hz(i) as f64 / hz).log2().abs();
            let nearest = (0..BINS).min_by(|&i, &j| off(i).total_cmp(&off(j))).unwrap();
            assert_eq!(peak, nearest, "{hz} Hz");
            assert!(bins[peak].abs() < 1.5, "{hz} Hz reads {}", bins[peak]);
            // Bars an octave and more than six FFT bins (140 Hz) away are well below: the
            // Hann window's skirts are about that wide.
            let far = |i: usize| off(i) > 1.0 && (Analyser::bin_hz(i) as f64 - hz).abs() > 6.0 * 48_000.0 / 2_048.0;
            for i in (0..BINS).filter(|&i| far(i)) {
                assert!(bins[i] < -60.0, "{hz} Hz: bin {i} ({} Hz) reads {}", Analyser::bin_hz(i), bins[i]);
            }
        }
        // Half as loud reads 6 dB lower.
        let mut a = Analyser::new();
        feed(&mut a, &sine(1_000.0, 0.5, 0.0, 48_000));
        let top = a.bins().iter().copied().fold(f32::MIN, f32::max);
        assert!((top + 6.02).abs() < 1.5, "{top}");
    }

    #[test]
    fn analyser_silence_is_floor() {
        let mut a = Analyser::new();
        assert!(a.bins().iter().all(|&v| v == -120.0));
        feed(&mut a, &[0.0; 24_000]);
        assert!(a.bins().iter().all(|&v| v == -120.0));
        // A sine falls away again once it stops, over a few tenths of a second.
        feed(&mut a, &sine(1_000.0, 1.0, 0.0, 24_000));
        feed(&mut a, &[0.0; 4_800]);
        let early = a.bins().iter().copied().fold(f32::MIN, f32::max);
        assert!((-45.0..-5.0).contains(&early), "still falling 100 ms on: {early}");
        feed(&mut a, &vec![0.0; 48_000 * 3]);
        assert!(a.bins().iter().all(|&v| (-120.0..-119.0).contains(&v)), "{:?}", a.bins());
        a.reset();
        assert!(a.bins().iter().all(|&v| v == -120.0));
    }
}
