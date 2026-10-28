//! Synthesised pitched instruments. Each function renders one note at MIDI pitch `note` and
//! strength `velocity` (0 to 1) as a mono wave that starts at the note's onset.

use super::dsp::{self, Noise, SR, Saw, Svf, add_partials, adsr, attack, midi_to_hz};
use std::f32::consts::TAU;

/// How much shorter notes ring the higher they are: 1 at middle C, halving about every two octaves.
fn pitch_decay(note: u8) -> f32 {
    2f32.powf((60.0 - note as f32) / 24.0).clamp(0.25, 3.0)
}

/// Room for a note, never longer than seven seconds.
fn buffer(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds.min(7.0) * SR) as usize]
}

/// The original soft plucked tone: a few harmonics that fade faster the higher they are.
/// Kept sample for sample as it always was, so older scenes sound the same.
pub fn pluck(note: u8, decay: f32, brightness: f32) -> Vec<f32> {
    let freq = midi_to_hz(note as f32);
    let sr = SR;
    let len = (decay * sr) as usize;
    let attack = 0.003 * sr;
    let partials: Vec<(f32, f32, f32)> = (1..=8)
        .map(|k| {
            let k = k as f32;
            let amp = brightness.powf(k - 1.0) / k;
            // -60 dB (ln 1000 = 6.9) over `decay`, faster for higher partials.
            let rate = 6.9 * (1.0 + (k - 1.0) * 0.8) / decay;
            (freq * k, amp, rate)
        })
        .filter(|(f, amp, _)| *f < sr * 0.45 && *amp > 1e-3)
        .collect();
    let norm: f32 = partials.iter().map(|p| p.1).sum();
    (0..len)
        .map(|i| {
            let t = i as f32 / sr;
            let onset = (i as f32 / attack).min(1.0);
            let s: f32 = partials
                .iter()
                .map(|(f, amp, rate)| amp * (-rate * t).exp() * (std::f32::consts::TAU * f * t).sin())
                .sum();
            s * onset / norm
        })
        .collect()
}

/// A Rhodes-like electric piano: two FM pairs, a warm body and a bright tine that fades fast.
pub fn electric_piano(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let length = 2.6 * pitch_decay(note).sqrt();
    let mut out = buffer(length);
    let body_fade = dsp::decay_factor(length);
    let tine_fade = dsp::decay_factor(0.25);
    let index_fade = dsp::decay_factor(0.6);
    let (mut body, mut tine, mut index) = (1.0f32, 1.0f32, 1.0f32);
    let depth = 0.6 + 1.6 * velocity;
    let (w, w_tine) = (TAU * f / SR, TAU * f * 14.0 / SR);
    for (i, s) in out.iter_mut().enumerate() {
        let n = i as f32;
        let modulator = (w * n).sin() * depth * index;
        let tone = (w * n + modulator).sin();
        let bell = (w * n + (w_tine * n).sin() * 1.2 * index).sin() - (w * n).sin();
        *s = tone * body + bell * tine * 0.35 * velocity;
        body *= body_fade;
        tine *= tine_fade;
        index = 0.15 + (index - 0.15) * index_fade;
    }
    attack(&mut out, 0.002);
    out
}

/// A grand piano approximation: slightly stretched partials from two detuned strings, a fast
/// first fade then a long ring, and a short thump of hammer noise.
pub fn grand_piano(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let ring = 5.0 * pitch_decay(note);
    let mut out = buffer(ring.min(7.0));
    // Stiff strings play their upper partials a little sharp.
    let stiffness = 0.0004 * (f / 261.6).powf(0.5);
    let brightness = 0.25 + 0.6 * velocity;
    let mut partials = Vec::new();
    for n in 1..=24 {
        let k = n as f32;
        let freq = f * k * (1.0 + stiffness * k * k).sqrt();
        let level = brightness.powf(k - 1.0) / k.powf(0.7);
        let fade = ring / (1.0 + 0.25 * (k - 1.0));
        for detune in [0.9995f32, 1.0005] {
            // A quick "prompt" fade and a long "aftersound".
            partials.push((freq * detune, level * 0.35, fade * 0.25));
            partials.push((freq * detune, level * 0.15, fade));
        }
    }
    add_partials(&mut out, &partials);
    let mut noise = Noise::new(note as u32 * 31 + 7);
    let mut svf = Svf::default();
    let hammer = (0.012 * SR) as usize;
    let hammer_level = 0.2 * velocity;
    for (i, s) in out.iter_mut().take(hammer).enumerate() {
        let env = 1.0 - i as f32 / hammer as f32;
        *s += svf.run(noise.next(), f * 4.0 + 800.0, 0.7).band * env * hammer_level;
    }
    attack(&mut out, 0.001);
    out
}

/// Tuned wooden bars: the overtones of a marimba bar sit about 4 and 10 times the note.
pub fn marimba(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let ring = 1.3 * pitch_decay(note);
    let mut out = buffer(ring);
    let hard = 0.15 + 0.35 * velocity;
    add_partials(&mut out, &[(f, 1.0, ring), (f * 3.99, hard, ring * 0.2), (f * 9.9, hard * 0.25, ring * 0.06)]);
    mallet(&mut out, f * 6.0, 0.08 * velocity, note as u32);
    attack(&mut out, 0.0015);
    out
}

/// Metal bars with a long ring and a gentle tremolo, like a vibraphone with its motor on.
pub fn vibraphone(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let ring = 4.0 * pitch_decay(note).sqrt();
    let mut out = buffer(ring);
    let hard = 0.1 + 0.25 * velocity;
    add_partials(&mut out, &[(f, 1.0, ring), (f * 4.0, hard, ring * 0.25), (f * 10.0, hard * 0.3, ring * 0.06)]);
    for (i, s) in out.iter_mut().enumerate() {
        *s *= 1.0 - 0.22 * (0.5 - 0.5 * (TAU * 5.5 * i as f32 / SR).cos());
    }
    mallet(&mut out, f * 8.0, 0.04 * velocity, note as u32);
    attack(&mut out, 0.002);
    out
}

/// A rubbed wine glass: pure, slightly inharmonic tones that swell in.
pub fn glass(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let ring = 3.0 * pitch_decay(note).sqrt();
    let mut out = buffer(ring);
    let bright = 0.2 + 0.3 * velocity;
    add_partials(
        &mut out,
        &[(f, 1.0, ring), (f * 2.32, bright, ring * 0.5), (f * 4.25, bright * 0.4, ring * 0.3), (f * 6.63, bright * 0.2, ring * 0.15)],
    );
    attack(&mut out, 0.03);
    out
}

/// A plucked harp string (Karplus-Strong): a burst of noise circulating in a tuned delay line.
pub fn harp(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let ring = 3.5 * pitch_decay(note).sqrt();
    let len = (ring.min(7.0) * SR) as usize;
    let period = SR / f;
    let size = period.ceil() as usize + 2;
    let mut line = vec![0.0f32; size];
    let mut noise = Noise::new(note as u32 * 977 + 3);
    // Softer plucks start with a duller burst.
    let mut svf = Svf::default();
    let cutoff = f * (2.0 + 10.0 * velocity);
    for v in line.iter_mut() {
        *v = svf.run(noise.next(), cutoff, 0.6).low;
    }
    // Loss per trip round the loop so the string fades by 60 dB over `ring`.
    let loss = (-6.9 * period / (ring * SR)).exp();
    let mut out = Vec::with_capacity(len);
    let (mut write, mut previous) = (0usize, 0.0f32);
    for _ in 0..len {
        // Read a period behind the write head, less the half sample the averaging filter adds.
        let read = write as f32 + size as f32 - (period - 0.5);
        let i = read.floor() as usize;
        let frac = read - read.floor();
        let a = line[i % size];
        let b = line[(i + 1) % size];
        let delayed = a + (b - a) * frac;
        let filtered = (delayed + previous) * 0.5 * loss;
        previous = delayed;
        line[write] = filtered;
        write = (write + 1) % size;
        out.push(delayed);
    }
    dsp::normalize(&mut out, 1.0);
    attack(&mut out, 0.001);
    out
}

/// Tubular bells: a low hum, the strike tone, a minor third above and bright upper partials.
pub fn bells(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let ring = 5.0 * pitch_decay(note).sqrt();
    let mut out = buffer(ring);
    let bright = 0.4 + 0.5 * velocity;
    let partials = [
        (0.5, 0.45, 1.0),
        (1.0, 1.0, 0.85),
        (1.19, 0.55, 0.7),
        (1.5, 0.4, 0.5),
        (2.0, 0.35 * bright, 0.4),
        (2.51, 0.25 * bright, 0.3),
        (3.01, 0.2 * bright, 0.22),
        (4.2, 0.12 * bright, 0.15),
    ]
    .map(|(ratio, level, fade)| (f * ratio, level, ring * fade));
    add_partials(&mut out, &partials);
    attack(&mut out, 0.001);
    out
}

/// A thumb piano: a clear tone with a quick metallic tick.
pub fn kalimba(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let ring = 1.8 * pitch_decay(note);
    let mut out = buffer(ring);
    let tick = 0.15 + 0.3 * velocity;
    add_partials(&mut out, &[(f, 1.0, ring), (f * 5.9, tick, 0.12), (f * 16.7, tick * 0.3, 0.04)]);
    attack(&mut out, 0.001);
    out
}

/// A big trance lead: seven detuned saws through a filter that closes after the attack.
pub fn supersaw(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let (hold, release) = (0.35, 0.3);
    let mut out = buffer(hold + release);
    let detune = [-0.11f32, -0.062, -0.02, 0.0, 0.021, 0.063, 0.109];
    let mut saws: Vec<(Saw, f32)> =
        detune.iter().enumerate().map(|(i, d)| (Saw::new(i as f32 * 0.37), f * (1.0 + d * 0.15))).collect();
    let mut svf = Svf::default();
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let raw: f32 = saws.iter_mut().map(|(saw, freq)| saw.next(*freq)).sum::<f32>() / 7.0;
        let cutoff = f * (3.0 + (6.0 + 8.0 * velocity) * (-t / 0.25).exp());
        *s = svf.run(raw, cutoff, 0.8).low * adsr(t, 0.004, 0.2, 0.75, hold, release);
    }
    out
}

/// A soft, slowly swelling pad of detuned saws behind a low filter.
pub fn pad(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let (hold, release) = (1.4, 1.2);
    let mut out = buffer(hold + release);
    let mut saws: Vec<(Saw, f32)> =
        [-0.08f32, -0.03, 0.03, 0.08].iter().enumerate().map(|(i, d)| (Saw::new(i as f32 * 0.29), f * (1.0 + d * 0.12))).collect();
    let mut svf = Svf::default();
    let cutoff = f * (2.0 + 1.5 * velocity);
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        let raw: f32 = saws.iter_mut().map(|(saw, freq)| saw.next(*freq)).sum::<f32>() / 4.0;
        *s = svf.run(raw, cutoff, 0.6).low * adsr(t, 0.35, 0.4, 0.8, hold, release);
    }
    out
}

/// A punchy synth bass: a saw and a square one octave down, with a snappy filter.
pub fn synth_bass(note: u8, velocity: f32) -> Vec<f32> {
    let f = midi_to_hz(note as f32);
    let (hold, release) = (0.3, 0.1);
    let mut out = buffer(hold + release);
    let mut saw = Saw::new(0.0);
    let mut svf = Svf::default();
    let mut sub_phase = 0.0f32;
    for (i, s) in out.iter_mut().enumerate() {
        let t = i as f32 / SR;
        sub_phase = (sub_phase + f * 0.5 / SR).fract();
        let square = if sub_phase < 0.5 { 1.0 } else { -1.0 };
        let raw = saw.next(f) * 0.6 + square * 0.4;
        let cutoff = f * (1.5 + (6.0 + 10.0 * velocity) * (-t / 0.08).exp());
        *s = svf.run(raw, cutoff, 1.4).low * adsr(t, 0.002, 0.15, 0.8, hold, release);
    }
    out
}

/// The click of a mallet: a few milliseconds of filtered noise.
fn mallet(out: &mut [f32], centre: f32, level: f32, seed: u32) {
    let mut noise = Noise::new(seed * 131 + 17);
    let mut svf = Svf::default();
    let n = ((0.004 * SR) as usize).min(out.len());
    for (i, s) in out.iter_mut().take(n).enumerate() {
        let env = 1.0 - i as f32 / n as f32;
        *s += svf.run(noise.next(), centre.min(SR * 0.4), 1.0).band * level * env;
    }
}
