//! Small building blocks for the synthesised instruments: decaying sine partials, noise,
//! filters and band-limited saws. Everything runs at [`SAMPLE_RATE`] and is deterministic.

use crate::SAMPLE_RATE;
use std::f32::consts::{PI, TAU};

pub const SR: f32 = SAMPLE_RATE as f32;

pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}

/// Per-sample multiplier that fades a level by 60 dB over `seconds`.
pub fn decay_factor(seconds: f32) -> f32 {
    (-6.9 / (seconds.max(1e-4) * SR)).exp()
}

/// A repeatable white noise source from -1 to 1 (xorshift).
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// One sine partial with an exponential fade, stepped by rotating a phasor instead of calling
/// `sin` every sample.
#[derive(Clone, Copy)]
struct Partial {
    re: f32,
    im: f32,
    cos: f32,
    sin: f32,
    fade: f32,
}

/// Adds decaying sine partials into `out`. Each partial is (frequency in Hz, level, seconds to
/// fade by 60 dB). Partials above the safe part of the spectrum are skipped.
pub fn add_partials(out: &mut [f32], partials: &[(f32, f32, f32)]) {
    let mut oscillators: Vec<Partial> = partials
        .iter()
        .filter(|(f, amp, _)| *f > 0.0 && *f < SR * 0.45 && amp.abs() > 1e-5)
        .map(|&(f, amp, seconds)| {
            let w = TAU * f / SR;
            Partial { re: 0.0, im: amp, cos: w.cos(), sin: w.sin(), fade: decay_factor(seconds) }
        })
        .collect();
    for (i, sample) in out.iter_mut().enumerate() {
        let mut s = 0.0;
        for p in &mut oscillators {
            // (re, im) rotates by w each sample; `re` is the sine of the phase times the level.
            let re = p.re * p.cos + p.im * p.sin;
            let im = p.im * p.cos - p.re * p.sin;
            p.re = re * p.fade;
            p.im = im * p.fade;
            s += p.re;
        }
        *sample += s;
        // Now and then drop partials that have faded away.
        if i % 4096 == 4095 {
            oscillators.retain(|p| p.re.abs() + p.im.abs() > 1e-6);
        }
    }
}

/// A linear attack ramp over the first `seconds`, so notes don't click.
pub fn attack(out: &mut [f32], seconds: f32) {
    let n = (seconds * SR).max(1.0);
    for (i, s) in out.iter_mut().take(n as usize).enumerate() {
        *s *= i as f32 / n;
    }
}

/// A short fade at the end so a cut-off note doesn't click.
pub fn fade_tail(out: &mut [f32], seconds: f32) {
    let n = ((seconds * SR) as usize).min(out.len()).max(1);
    let len = out.len();
    for (k, s) in out[len - n..].iter_mut().enumerate() {
        *s *= 1.0 - k as f32 / n as f32;
    }
}

/// Trims the quiet end of a note (below about -70 dB of its peak), keeping a short fade.
pub fn trim(mut wave: Vec<f32>) -> Vec<f32> {
    let peak = wave.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak == 0.0 {
        wave.clear();
        return wave;
    }
    let floor = peak * 3e-4;
    let end = wave.iter().rposition(|s| s.abs() > floor).map_or(0, |i| i + 1);
    wave.truncate((end + (0.005 * SR) as usize).min(wave.len()));
    fade_tail(&mut wave, 0.005);
    wave
}

/// Scales a wave so its peak is `level`.
pub fn normalize(wave: &mut [f32], level: f32) {
    let peak = wave.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        wave.iter_mut().for_each(|s| *s *= level / peak);
    }
}

/// Topology-preserving state variable filter (Zavalishin), usable as low, band or high pass.
#[derive(Clone, Copy, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

pub struct SvfOut {
    pub low: f32,
    pub band: f32,
    pub high: f32,
}

impl Svf {
    /// One sample through the filter at `cutoff` Hz with resonance `q` (0.5 is gentle).
    pub fn run(&mut self, x: f32, cutoff: f32, q: f32) -> SvfOut {
        let g = (PI * cutoff.clamp(10.0, SR * 0.49) / SR).tan();
        let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        SvfOut { low: v2, band: v1, high: x - k * v1 - v2 }
    }
}

/// A band-limited sawtooth (PolyBLEP), from -1 to 1.
#[derive(Clone, Copy)]
pub struct Saw {
    phase: f32,
}

impl Saw {
    pub fn new(phase: f32) -> Self {
        Self { phase: phase.rem_euclid(1.0) }
    }

    pub fn next(&mut self, freq: f32) -> f32 {
        let dt = (freq / SR).min(0.5);
        let mut value = 2.0 * self.phase - 1.0;
        value -= poly_blep(self.phase, dt);
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        value
    }
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

/// Attack, hold-at-sustain and release envelope, sampled at time `t` seconds into a note that
/// is held for `hold` seconds.
pub fn adsr(t: f32, attack: f32, decay: f32, sustain: f32, hold: f32, release: f32) -> f32 {
    let level = if t < attack {
        t / attack.max(1e-4)
    } else if t < attack + decay {
        1.0 - (1.0 - sustain) * (t - attack) / decay.max(1e-4)
    } else {
        sustain
    };
    if t <= hold {
        level
    } else {
        let at_release = adsr(hold, attack, decay, sustain, f32::INFINITY, release);
        (at_release * (1.0 - (t - hold) / release.max(1e-4))).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partials_ring_at_their_frequency_and_fade() {
        let mut out = vec![0.0; SR as usize];
        add_partials(&mut out, &[(1000.0, 1.0, 0.5)]);
        // Zero crossings in the first 0.1 s: 1000 Hz crosses 200 times.
        let crossings = out[..4800].windows(2).filter(|w| w[0].signum() != w[1].signum()).count();
        assert!((195..=205).contains(&crossings), "{crossings}");
        let early = out[..480].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let late = out[24_000..24_480].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        // 60 dB down after 0.5 s.
        assert!(late < early * 0.002, "{early} {late}");
    }

    #[test]
    fn partials_above_nyquist_are_skipped() {
        let mut out = vec![0.0; 100];
        add_partials(&mut out, &[(30_000.0, 1.0, 1.0)]);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn trimming_keeps_the_sound_and_drops_silence() {
        let mut wave = vec![0.0; 10_000];
        wave[100] = 1.0;
        let trimmed = trim(wave);
        assert!(trimmed.len() < 500 && trimmed.len() > 100);
        assert!(trim(vec![0.0; 50]).is_empty());
    }

    #[test]
    fn envelope_shape() {
        assert_eq!(adsr(0.0, 0.1, 0.1, 0.5, 1.0, 0.5), 0.0);
        assert!((adsr(0.1, 0.1, 0.1, 0.5, 1.0, 0.5) - 1.0).abs() < 1e-5);
        assert!((adsr(0.5, 0.1, 0.1, 0.5, 1.0, 0.5) - 0.5).abs() < 1e-5);
        assert!((adsr(1.25, 0.1, 0.1, 0.5, 1.0, 0.5) - 0.25).abs() < 1e-5);
        assert_eq!(adsr(2.0, 0.1, 0.1, 0.5, 1.0, 0.5), 0.0);
    }

    #[test]
    fn filters_stay_stable() {
        let mut svf = Svf::default();
        let mut noise = Noise::new(7);
        for _ in 0..48_000 {
            let out = svf.run(noise.next(), 20_000.0, 8.0);
            assert!(out.low.is_finite() && out.low.abs() < 100.0);
        }
    }
}
