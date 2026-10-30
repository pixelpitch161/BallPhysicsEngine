//! The master stage at the end of every mix: one gain to the loudness target (real
//! integrated loudness, ITU-R BS.1770), an optional clipper at four times the rate, a
//! true-peak limiter, and a short fade at the very end so effect tails don't stop dead.

use crate::SAMPLE_RATE;
use bpe_synth::blocks::{Clipper, OVERSAMPLE_DELAY, TruePeakLimiter, true_peak};

/// The quietest a loudness reading goes: silence reads this.
pub const SILENCE_LUFS: f32 = -70.0;
/// The most the loudness match ever turns a mix up, so a lone click isn't blown up.
pub const MAX_GAIN_DB: f32 = 24.0;
/// How long the soundtrack fades out over at the very end.
pub const FADE_SECONDS: f32 = 0.5;
/// How often the report's meters read, in seconds.
pub const METER_STEP: f32 = 0.1;

/// The master clipper's shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ClipMode {
    #[default]
    Off,
    Soft,
    Hard,
}

/// How the master stage finishes a mix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MasterSettings {
    /// Integrated loudness target in LUFS.
    pub loudness: f32,
    pub clipper: ClipMode,
    pub drive_db: f32,
    /// True-peak ceiling in dBTP.
    pub ceiling_db: f32,
    /// The limiter's release in ms, or `None` for automatic.
    pub release_ms: Option<f32>,
}

impl Default for MasterSettings {
    fn default() -> Self {
        Self { loudness: -14.0, clipper: ClipMode::Off, drive_db: 0.0, ceiling_db: -1.0, release_ms: None }
    }
}

/// What the master stage measured, for the meters and the export window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MasterReport {
    /// The finished mix's integrated loudness, LUFS.
    pub integrated: f32,
    /// Its true peak, dBTP.
    pub true_peak: f32,
    /// Short-term loudness (the last 3 s) every [`METER_STEP`] seconds, LUFS.
    pub short_term: Vec<f32>,
    /// The limiter's gain reduction every [`METER_STEP`] seconds (the most in each step), dB.
    pub reduction: Vec<f32>,
}

impl MasterReport {
    /// The meter readings at `seconds` into the track: (short-term LUFS, gain reduction dB).
    pub fn at(&self, seconds: f64) -> (f32, f32) {
        // A hair over, so a time on a step (1.0 s, at a step of 0.1 s) reads that step.
        let i = (seconds.max(0.0) / METER_STEP as f64 + 1e-6) as usize;
        let pick = |v: &[f32], none: f32| v.get(i.min(v.len().saturating_sub(1))).copied().unwrap_or(none);
        (pick(&self.short_term, SILENCE_LUFS), pick(&self.reduction, 0.0))
    }
}

/// One biquad in direct form I, in f64 for a steady measurement.
struct Stage {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Stage {
    fn tick(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// BS.1770's K-weighting at 48 kHz: a high shelf, then the RLB high-pass.
fn k_weighting() -> [Stage; 2] {
    [
        Stage {
            b: [1.535_124_859_586_97, -2.691_696_189_406_38, 1.198_392_810_852_85],
            a: [-1.690_659_293_182_41, 0.732_480_774_215_85],
            x: [0.0; 2],
            y: [0.0; 2],
        },
        Stage { b: [1.0, -2.0, 1.0], a: [-1.990_047_454_833_98, 0.990_072_250_366_21], x: [0.0; 2], y: [0.0; 2] },
    ]
}

/// The K-weighted mean square of each 100 ms step, summed over both sides.
fn step_powers(samples: &[f32]) -> Vec<f64> {
    let step = (SAMPLE_RATE as f32 * METER_STEP) as usize;
    let mut filters = [k_weighting(), k_weighting()];
    let mut out = Vec::new();
    let (mut sum, mut n) = (0.0f64, 0usize);
    for frame in samples.chunks_exact(2) {
        for (side, &v) in frame.iter().enumerate() {
            let [a, b] = &mut filters[side];
            let y = b.tick(a.tick(v as f64));
            sum += y * y;
        }
        n += 1;
        if n == step {
            out.push(sum / n as f64);
            (sum, n) = (0.0, 0);
        }
    }
    if n > 0 {
        out.push(sum / n as f64);
    }
    out
}

fn lufs(power: f64) -> f32 {
    if power <= 0.0 { SILENCE_LUFS } else { ((-0.691 + 10.0 * power.log10()) as f32).max(SILENCE_LUFS) }
}

/// Integrated loudness of interleaved stereo at 48 kHz (BS.1770-4): 400 ms blocks every
/// 100 ms, gated at -70 LUFS and then 10 LU under the ungated level. Silence reads -70.
pub fn integrated_lufs(samples: &[f32]) -> f32 {
    let steps = step_powers(samples);
    let blocks: Vec<f64> = if steps.len() < 4 {
        vec![steps.iter().sum::<f64>() / steps.len().max(1) as f64]
    } else {
        steps.windows(4).map(|w| w.iter().sum::<f64>() / 4.0).collect()
    };
    let loud: Vec<f64> = blocks.iter().copied().filter(|&p| lufs(p) > SILENCE_LUFS).collect();
    if loud.is_empty() {
        return SILENCE_LUFS;
    }
    let relative = lufs(loud.iter().sum::<f64>() / loud.len() as f64) - 10.0;
    let gated: Vec<f64> = loud.into_iter().filter(|&p| lufs(p) > relative).collect();
    if gated.is_empty() {
        return SILENCE_LUFS;
    }
    lufs(gated.iter().sum::<f64>() / gated.len() as f64)
}

/// Short-term loudness (the 3 s before each step's end) every [`METER_STEP`] seconds.
pub fn short_term_lufs(samples: &[f32]) -> Vec<f32> {
    let steps = step_powers(samples);
    (0..steps.len())
        .map(|i| {
            let from = (i + 1).saturating_sub(30);
            let w = &steps[from..=i];
            lufs(w.iter().sum::<f64>() / w.len() as f64)
        })
        .collect()
}

/// Finishes a mix in place: one gain to the loudness target, the clipper, the true-peak
/// limiter and the end fade. Gives what it measured.
pub fn master(samples: &mut [f32], s: &MasterSettings) -> MasterReport {
    let frames = samples.len() / 2;
    let before = integrated_lufs(samples);
    if before > SILENCE_LUFS {
        let gain = 10f32.powf((s.loudness - before).min(MAX_GAIN_DB) / 20.0);
        samples.iter_mut().for_each(|v| *v *= gain);
    }
    if s.clipper != ClipMode::Off {
        clip(samples, s.clipper == ClipMode::Hard, s.drive_db);
    }
    let (mut l, mut r): (Vec<f32>, Vec<f32>) = samples.chunks_exact(2).map(|f| (f[0], f[1])).unzip();
    let blocks = TruePeakLimiter::new(SAMPLE_RATE as f32, s.ceiling_db, s.release_ms).process(&mut l, &mut r);
    let fade = ((FADE_SECONDS * SAMPLE_RATE as f32) as usize).min(frames).max(1);
    for (i, frame) in samples.chunks_exact_mut(2).enumerate() {
        let left = frames - 1 - i;
        let g = if left < fade { left as f32 / (fade - 1).max(1) as f32 } else { 1.0 };
        frame[0] = l[i] * g;
        frame[1] = r[i] * g;
    }
    let per_step = (SAMPLE_RATE as f32 * METER_STEP) as usize / bpe_synth::BLOCK;
    MasterReport {
        integrated: integrated_lufs(samples),
        true_peak: 20.0 * true_peak(samples).max(1e-9).log10(),
        short_term: short_term_lufs(samples),
        reduction: blocks.chunks(per_step.max(1)).map(|c| c.iter().copied().fold(0.0, f32::max)).collect(),
    }
}

/// Clips at four times the rate, keeping the sound in time with where it was.
fn clip(samples: &mut [f32], hard: bool, drive_db: f32) {
    let mut clippers = [Clipper::with(hard, drive_db), Clipper::with(hard, drive_db)];
    let delay = 2 * OVERSAMPLE_DELAY;
    let frames = samples.len() / 2;
    let mut out = vec![0.0f32; samples.len()];
    for i in 0..frames + delay {
        for (side, c) in clippers.iter_mut().enumerate() {
            let x = if i < frames { samples[i * 2 + side] } else { 0.0 };
            let y = c.tick(side, x);
            if i >= delay {
                out[(i - delay) * 2 + side] = y;
            }
        }
    }
    samples.copy_from_slice(&out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SAMPLE_RATE;

    fn sine(hz: f32, amp: f32, seconds: f32) -> Vec<f32> {
        (0..(seconds * SAMPLE_RATE as f32) as usize)
            .flat_map(|i| {
                let v = amp * (std::f32::consts::TAU * hz * i as f32 / SAMPLE_RATE as f32).sin();
                [v, v]
            })
            .collect()
    }

    fn noise(seconds: f32, amp: f32) -> Vec<f32> {
        let mut x = 1u64;
        let mut last = 0.0f32;
        (0..(seconds * SAMPLE_RATE as f32) as usize * 2)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let white = (x >> 40) as f32 / (1u64 << 23) as f32 - 1.0;
                // A gentle low-pass, so it's darker than white, as music is.
                last = last * 0.7 + white * 0.3;
                last * amp
            })
            .collect()
    }

    fn true_peak_db(x: &[f32]) -> f32 {
        20.0 * bpe_synth::blocks::true_peak(x).max(1e-9).log10()
    }

    #[test]
    fn lufs_of_a_reference_sine() {
        let lufs = integrated_lufs(&sine(997.0, 0.1, 10.0));
        assert!((lufs + 20.0).abs() < 0.3, "{lufs}");
        let mut n = noise(10.0, 0.05);
        let report = master(&mut n, &MasterSettings::default());
        assert!((report.integrated + 14.0).abs() < 0.5, "{}", report.integrated);
        assert!((integrated_lufs(&n) + 14.0).abs() < 0.5);
    }

    #[test]
    fn master_hits_the_target_and_ceiling() {
        let frames = 4 * SAMPLE_RATE as usize;
        let square: Vec<f32> = (0..frames).flat_map(|i| [if (i / 240) % 2 == 0 { 0.3 } else { -0.3 }; 2]).collect();
        let intersample: Vec<f32> = (0..frames)
            .flat_map(|i| [0.5 * (std::f32::consts::FRAC_PI_2 * i as f32 + std::f32::consts::FRAC_PI_4).sin(); 2])
            .collect();
        let plucks: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let t = (i % 6_000) as f32 / SAMPLE_RATE as f32;
                [0.8 * (-t * 30.0).exp() * (std::f32::consts::TAU * 660.0 * t).sin(); 2]
            })
            .collect();
        for (name, mut x) in
            [("plucks", plucks), ("square", square), ("intersample", intersample), ("noise", noise(4.0, 0.3))]
        {
            let report = master(&mut x, &MasterSettings::default());
            let tp = true_peak_db(&x);
            assert!(tp <= -0.9, "{name}: {tp} dBTP");
            assert!((report.true_peak - tp).abs() < 0.2, "{name}: report {} vs {tp}", report.true_peak);
            let limited = report.reduction.iter().any(|&r| r > 0.5);
            assert!(limited || (report.integrated + 14.0).abs() < 0.5, "{name}: {} LUFS", report.integrated);
        }
    }

    #[test]
    fn silence_and_a_click_stay_sane() {
        let mut silence = vec![0.0f32; 96_000];
        let report = master(&mut silence, &MasterSettings::default());
        assert!(silence.iter().all(|&v| v == 0.0));
        assert_eq!(report.integrated, -70.0);
        let mut click = vec![0.0f32; 96_000];
        click[48_000] = 0.5;
        click[48_001] = 0.5;
        master(&mut click, &MasterSettings::default());
        let peak = click.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak <= 0.5 * 10f32.powf(24.0 / 20.0) + 1e-3);
        assert!(true_peak_db(&click) <= -0.9);
    }

    #[test]
    fn the_end_fades_out() {
        let mut x = noise(3.0, 0.1);
        master(&mut x, &MasterSettings::default());
        let frames = x.len() / 2;
        assert_eq!((x[x.len() - 2], x[x.len() - 1]), (0.0, 0.0));
        let rms =
            |a: usize, b: usize| (x[a * 2..b * 2].iter().map(|v| v * v).sum::<f32>() / ((b - a) * 2) as f32).sqrt();
        let quarter = SAMPLE_RATE as usize / 4;
        assert!(rms(frames - quarter, frames) < 0.5 * rms(frames - 4 * quarter, frames - 2 * quarter));
    }
}
