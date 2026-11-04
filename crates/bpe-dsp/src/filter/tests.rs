use super::*;
use crate::testing::Noise;
use std::f64::consts::TAU;

/// The filter's measured gain (dB) for a steady sine at `hz`.
fn measured_db(p: &FilterParams, hz: f64) -> f64 {
    let mut f = StereoFilter::new(p.kind, p.slope24);
    let settle = 24_000;
    let n = 48_000;
    let (mut c, mut s) = (0.0f64, 0.0f64);
    let mut i = 0usize;
    for b in 0..(settle + n) / BLOCK {
        f.set(p);
        for _ in 0..BLOCK {
            let ph = TAU * hz * i as f64 / SAMPLE_RATE as f64;
            let x = (0.25 * ph.sin()) as f32;
            let (y, _) = f.tick(x, x);
            if b * BLOCK >= settle {
                c += y as f64 * ph.cos();
                s += y as f64 * ph.sin();
            }
            i += 1;
        }
    }
    let amp = 2.0 * (c * c + s * s).sqrt() / n as f64 / 0.25;
    20.0 * amp.max(1e-9).log10()
}

fn params(kind: FilterType, slope24: bool, reso: f32) -> FilterParams {
    FilterParams { kind, slope24, cutoff_hz: 1000.0, reso, gain_db: 6.0, ..FilterParams::default() }
}

#[test]
fn curves_match_textbook_responses() {
    let svf = [
        FilterType::LowPass,
        FilterType::HighPass,
        FilterType::BandPass,
        FilterType::Notch,
        FilterType::Peak,
        FilterType::LowShelf,
        FilterType::HighShelf,
    ];
    let freqs: Vec<f64> = (0..24).map(|i| 20.0 * 1000f64.powf(i as f64 / 23.0)).collect();
    for kind in svf {
        for slope24 in [false, true] {
            for reso in [0.0, 0.7] {
                let p = params(kind, slope24, reso);
                for &hz in &freqs {
                    let want = (response_db(&p, hz as f32) as f64).max(-60.0);
                    let got = measured_db(&p, hz).max(-60.0);
                    assert!(
                        (want - got).abs() < 0.5,
                        "{kind:?} 24={slope24} reso {reso} at {hz:.0} Hz: drawn {want:.2} dB, heard {got:.2} dB"
                    );
                }
            }
        }
    }
    // Textbook points: Butterworth at its cutoff, a peak's gain, the shelves' ends, the band's top.
    let at = |kind, slope24, hz: f32| response_db(&params(kind, slope24, 0.0), hz);
    for slope24 in [false, true] {
        assert!((at(FilterType::LowPass, slope24, 1000.0) + 3.01).abs() < 0.05);
        assert!((at(FilterType::HighPass, slope24, 1000.0) + 3.01).abs() < 0.05);
        assert!((at(FilterType::Peak, slope24, 1000.0) - 6.0).abs() < 0.05);
        assert!((at(FilterType::LowShelf, slope24, 20.0) - 6.0).abs() < 0.1);
        assert!((at(FilterType::HighShelf, slope24, 22_000.0) - 6.0).abs() < 0.2);
    }
    assert!(at(FilterType::BandPass, false, 1000.0).abs() < 0.05);
    assert!(at(FilterType::LowPass, false, 10_000.0) < -35.0);
    assert!(at(FilterType::LowPass, true, 10_000.0) < -75.0);
    assert!(at(FilterType::Notch, false, 1000.0) < -60.0);
}

#[test]
fn ladder_self_oscillates_at_full_reso() {
    let p = FilterParams { kind: FilterType::Ladder, cutoff_hz: 1000.0, reso: 1.0, ..FilterParams::default() };
    let mut f = StereoFilter::new(FilterType::Ladder, false);
    let mut noise = Noise(3);
    let mut out = Vec::new();
    for b in 0..(2 * 48_000 / BLOCK) {
        f.set(&p);
        for _ in 0..BLOCK {
            let x = if b == 0 { noise.next() * 0.1 } else { 0.0 };
            out.push(f.tick(x, x).0);
        }
    }
    let tail = &out[out.len() - 24_000..];
    assert!(tail.iter().all(|y| y.is_finite() && y.abs() < 4.0), "bounded");
    let rms = (tail.iter().map(|y| y * y).sum::<f32>() / tail.len() as f32).sqrt();
    assert!(rms > 0.05, "it sings on its own: rms {rms}");
    let crossings = tail.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    let hz = crossings as f64 / 0.5;
    assert!((hz - 1000.0).abs() < 150.0, "near its cutoff: {hz} Hz");
}

#[test]
fn resonance_sweep_under_fast_modulation_is_bounded() {
    for kind in FilterType::ALL {
        for slope24 in [false, true] {
            let mut f = StereoFilter::new(kind, slope24);
            let mut noise = Noise(9);
            let mut worst = 0.0f32;
            for b in 0..(10 * 48_000 / BLOCK) {
                let cutoff = if b % 2 == 0 { 20.0 } else { 20_000.0 };
                let p = FilterParams {
                    kind,
                    slope24,
                    cutoff_hz: cutoff,
                    reso: 1.0,
                    drive: 0.0,
                    gain_db: -12.0,
                    ..FilterParams::default()
                };
                f.set(&p);
                for i in 0..BLOCK {
                    // A loud saw at 110 Hz with a little noise.
                    let t = ((b * BLOCK + i) as f32 * 110.0 / SAMPLE_RATE).fract();
                    let x = 2.0 * t - 1.0 + 0.1 * noise.next();
                    let (y, z) = f.tick(x, x);
                    assert!(y.is_finite() && z.is_finite(), "{kind:?}");
                    worst = worst.max(y.abs());
                }
            }
            assert!(worst < 8.0, "{kind:?} 24={slope24}: {worst}");
        }
    }
}

#[test]
fn comb_with_key_track_plays_the_note() {
    // Key track at 100 % gives the comb the note's pitch: A at 440 Hz.
    let p = FilterParams { kind: FilterType::Comb, cutoff_hz: 440.0, reso: 1.0, ..FilterParams::default() };
    let mut f = StereoFilter::new(FilterType::Comb, false);
    let mut noise = Noise(5);
    let mut out = Vec::new();
    for _ in 0..(48_000 / BLOCK) {
        f.set(&p);
        for _ in 0..BLOCK {
            let x = noise.next() * 0.2;
            out.push(f.tick(x, x).0);
        }
    }
    let x = &out[12_000..];
    let ac = |lag: usize| x[lag..].iter().zip(x).map(|(a, b)| (a * b) as f64).sum::<f64>();
    let best = (50..400).max_by(|&a, &b| ac(a).total_cmp(&ac(b))).unwrap();
    let period = SAMPLE_RATE as f64 / 440.0;
    assert!((best as f64 - period).abs() <= 1.0, "rings with a period of {best} samples, not {period:.1}");
    assert!(ac(best) > 0.8 * ac(0), "strongly pitched");
    assert!(response_db(&p, 440.0) > response_db(&p, 660.0) + 20.0);
}

#[test]
fn formant_vowels_peak_at_their_f1() {
    for (vowel, f1) in [(0.0, 800.0), (3.0, 450.0)] {
        let p = FilterParams { kind: FilterType::Formant, vowel, ..FilterParams::default() };
        let freqs: Vec<f64> = (0..60).map(|i| 200.0 + 20.0 * i as f64).collect();
        let heard = freqs.iter().copied().max_by(|&a, &b| measured_db(&p, a).total_cmp(&measured_db(&p, b))).unwrap();
        assert!((heard - f1).abs() < 0.1 * f1, "vowel {vowel}: peaks at {heard} Hz, not near {f1}");
        let drawn = freqs
            .iter()
            .copied()
            .max_by(|&a, &b| response_db(&p, a as f32).total_cmp(&response_db(&p, b as f32)))
            .unwrap();
        assert!((drawn - f1).abs() < 0.1 * f1, "drawn peak {drawn}");
    }
}

#[test]
fn a_moving_cutoff_glides_without_steps() {
    // From 200 Hz to 8 kHz in one block: the sound opens up over the block, never in one
    // sample (the input moves by up to 0.02 a sample; a snapped cutoff jumps by far more).
    let mut f = StereoFilter::new(FilterType::LowPass, true);
    let mut out = Vec::new();
    for b in 0..200 {
        let cutoff = if b < 100 { 200.0 } else { 8000.0 };
        f.set(&FilterParams { cutoff_hz: cutoff, slope24: true, ..FilterParams::default() });
        for i in 0..BLOCK {
            let x = (TAU * 300.0 * (b * BLOCK + i) as f64 / 48_000.0).sin() as f32 * 0.5;
            out.push(f.tick(x, x).0);
        }
    }
    let step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(step < 0.1, "{step}");
}

#[test]
fn ringing_down_reaches_zero() {
    let mut f = StereoFilter::new(FilterType::BandPass, true);
    let p = FilterParams { kind: FilterType::BandPass, slope24: true, reso: 0.9, ..FilterParams::default() };
    let mut last = 1.0f32;
    for b in 0..3000 {
        f.set(&p);
        for i in 0..BLOCK {
            let x = if b < 10 { (i as f32 * 0.3).sin() } else { 0.0 };
            last = f.tick(x, x).0;
        }
    }
    assert_eq!(last, 0.0);
}

#[test]
fn a_loud_sound_through_a_clean_low_pass_stays_clean() {
    // A 200 Hz sine peaking at 3.5 (Gain +12 dB, or a few shapes summed) through a 12 dB low
    // pass at 5 kHz with no resonance: the output is the input, not soft-clipped.
    let p = FilterParams {
        kind: FilterType::LowPass,
        slope24: false,
        cutoff_hz: 5000.0,
        reso: 0.0,
        ..FilterParams::default()
    };
    let mut f = StereoFilter::new(p.kind, p.slope24);
    let mut peak = 0.0f32;
    for i in 0..48_000usize {
        if i % BLOCK == 0 {
            f.set(&p);
        }
        let x = (3.5 * (TAU * 200.0 * i as f64 / SAMPLE_RATE as f64).sin()) as f32;
        let (y, _) = f.tick(x, x);
        if i > 4800 {
            peak = peak.max(y.abs());
        }
    }
    assert!((peak - 3.5).abs() < 0.05, "peak {peak}");
}
