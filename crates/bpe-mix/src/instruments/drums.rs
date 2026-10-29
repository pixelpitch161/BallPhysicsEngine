//! A synthesised drum kit. Keys follow the General MIDI drum map, so drum tracks from MIDI
//! files play the right pieces; other keys pick a piece by their place in the octave.

use std::f32::consts::TAU;

use super::dsp::{Noise, SR, Svf, attack};

/// One drum sound for MIDI `key`, hit at `velocity` (0 to 1): 35/36 kick, 37 rim, 38/40
/// snare, 39 clap, 42/44 closed hat, 46 open hat, 41/43/45/47/48/50 toms pitched by key,
/// 49/57 crash, 51/59 ride, 76/77 woodblock, and a piece by the octave for anything else.
pub fn render(key: u8, velocity: f32) -> Vec<f32> {
    let v = velocity.clamp(0.0, 1.0);
    let mut out = match key {
        35 | 36 => kick(v),
        37 => rim(),
        38 | 40 => snare(v),
        39 => clap(),
        41 | 43 => tom(85.0),
        42 | 44 => hat(0.06, v),
        45 | 47 => tom(125.0),
        46 => hat(0.45, v),
        48 | 50 => tom(175.0),
        49 | 52 | 55 | 57 => cymbal(1.8, 5000.0),
        51 | 53 | 59 => cymbal(1.2, 7000.0),
        76 | 77 => woodblock(),
        _ => match key % 12 {
            0 => kick(v),
            1 => rim(),
            2 => snare(v),
            3 => clap(),
            4 | 6 => hat(0.06, v),
            5 => tom(85.0),
            7 => tom(125.0),
            8 => hat(0.45, v),
            9 => tom(175.0),
            10 => cymbal(1.2, 7000.0),
            _ => woodblock(),
        },
    };
    attack(&mut out, 0.0005);
    out
}

fn buffer(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * SR) as usize]
}

fn kick(velocity: f32) -> Vec<f32> {
    let mut out = buffer(0.45);
    let mut phase = 0.0f32;
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let freq = 45.0 + (110.0 + 60.0 * velocity) * (-t / 0.035).exp();
        phase += TAU * freq / SR;
        let body = phase.sin() * (-t / 0.22).exp();
        let click = if t < 0.002 { (1.0 - t / 0.002) * 0.4 * velocity } else { 0.0 };
        *s = body + click;
    }
    out
}

fn snare(velocity: f32) -> Vec<f32> {
    let mut out = buffer(0.3);
    let mut noise = Noise::new(38);
    let mut svf = Svf::default();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let tone = ((TAU * 185.0 * t).sin() + 0.5 * (TAU * 330.0 * t).sin()) * (-t / 0.05).exp();
        let rattle = svf.run(noise.next(), 4000.0, 0.5).high * (-t / (0.08 + 0.05 * velocity)).exp();
        *s = tone * 0.5 + rattle * 0.8;
    }
    out
}

fn clap() -> Vec<f32> {
    let mut out = buffer(0.3);
    let mut noise = Noise::new(39);
    let mut svf = Svf::default();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        // Three quick slaps, then the tail.
        let burst =
            (0..3).map(|k| t - k as f32 * 0.011).filter(|&d| d >= 0.0).map(|d| (-d / 0.006).exp()).fold(0.0, f32::max);
        let env = burst.max(if t > 0.022 { (-(t - 0.022) / 0.09).exp() * 0.7 } else { 0.0 });
        *s = svf.run(noise.next(), 1200.0, 1.5).band * env * 2.0;
    }
    out
}

/// Six square waves at clashing, unrelated pitches: the classic drum machine metal.
fn metal(t: f32) -> f32 {
    [205.3f32, 304.4, 369.6, 522.7, 540.0, 800.0]
        .iter()
        .map(|f| if (f * 1.7 * t).fract() < 0.5 { 1.0 } else { -1.0 })
        .sum::<f32>()
        / 6.0
}

fn hat(length: f32, velocity: f32) -> Vec<f32> {
    let mut out = buffer(length * 4.0);
    let mut noise = Noise::new(42);
    let mut svf = Svf::default();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let raw = metal(t) * 0.6 + noise.next() * 0.4;
        *s = svf.run(raw, 8000.0 + 2000.0 * velocity, 0.7).high * (-t * 6.9 / length).exp() * 0.7;
    }
    out
}

fn tom(freq: f32) -> Vec<f32> {
    let mut out = buffer(0.6);
    let mut phase = 0.0f32;
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let f = freq * (1.0 + 0.6 * (-t / 0.04).exp());
        phase += TAU * f / SR;
        *s = phase.sin() * (-t / 0.18).exp();
    }
    out
}

fn cymbal(length: f32, cutoff: f32) -> Vec<f32> {
    let mut out = buffer(length);
    let mut noise = Noise::new(49);
    let mut svf = Svf::default();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let raw = metal(t * 1.3) * 0.5 + noise.next() * 0.5;
        *s = svf.run(raw, cutoff, 0.6).high * (-t * 6.9 / length).exp() * 0.6;
    }
    out
}

fn rim() -> Vec<f32> {
    let mut out = buffer(0.08);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        *s = ((TAU * 1700.0 * t).sin() * 0.7 + (TAU * 480.0 * t).sin() * 0.3) * (-t / 0.012).exp();
    }
    out
}

fn woodblock() -> Vec<f32> {
    let mut out = buffer(0.15);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        *s = ((TAU * 820.0 * t).sin() + 0.4 * (TAU * 2240.0 * t).sin()) * (-t / 0.03).exp();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_renders_a_short_finite_sound() {
        for key in [35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 57, 59, 76, 0, 61, 127] {
            let wave = render(key, 0.8);
            let peak = wave.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(!wave.is_empty() && wave.iter().all(|s| s.is_finite()), "key {key}");
            assert!(peak > 0.05, "key {key} peak {peak}");
            assert!(wave.len() <= 2 * SR as usize, "key {key} is {} samples", wave.len());
        }
        assert!(render(36, 1.0) != render(42, 1.0));
        assert_eq!(render(36, 0.5), render(36, 0.5), "the same drum sounds the same every time");
    }
}
