use super::*;

/// A click track: a short burst of noise on every beat, the first of each bar louder.
fn clicks(bpm: f64, seconds: f64, start: f64) -> (Pcm, Vec<f64>) {
    let rate = SAMPLE_RATE as f64;
    let mut samples = vec![0.0f32; (seconds * rate) as usize];
    let mut seed = 1u32;
    let mut times = Vec::new();
    let mut beat = 0;
    loop {
        let time = start + beat as f64 * 60.0 / bpm;
        if time >= seconds - 0.05 {
            break;
        }
        times.push(time);
        let gain = if beat % 4 == 0 { 0.9 } else { 0.5 };
        let at = (time * rate).round() as usize;
        for i in 0..(0.012 * rate) as usize {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0;
            let envelope = (-(i as f32) / (0.002 * rate as f32)).exp();
            samples[at + i] += noise * envelope * gain;
        }
        beat += 1;
    }
    (Pcm { rate: SAMPLE_RATE, channels: 1, samples }, times)
}

fn check_clicks(bpm: f64) {
    let (pcm, truth) = clicks(bpm, 30.0, 0.31);
    let features = analyze(&pcm);
    assert!((features.bpm as f64 - bpm).abs() < 0.5, "{bpm} BPM read as {}", features.bpm);
    let mut worst = 0.0f64;
    for &beat in &features.beats {
        let nearest = truth.iter().map(|t| (t - beat).abs()).fold(f64::MAX, f64::min);
        worst = worst.max(nearest);
    }
    assert!(worst < 0.010, "{bpm} BPM: a beat was {:.1} ms off", worst * 1000.0);
    assert!(features.beats.len() as f64 >= truth.len() as f64 * 0.9, "{bpm}: {} of {} beats", features.beats.len(), truth.len());
    // The loud first click of each bar is a downbeat.
    let first = features.downbeats[0];
    let index = truth.iter().position(|t| (t - first).abs() < 0.01).unwrap();
    assert_eq!(index % 4, 0, "{bpm}: downbeat on beat {index}");
}

#[test]
fn click_tracks_give_their_tempo_and_beats() {
    for bpm in [90.0, 120.0, 128.0, 174.0] {
        check_clicks(bpm);
    }
}

#[test]
fn levels_follow_the_frequency_of_a_sweep() {
    // A sine sweeping up from 40 Hz to 12 kHz over 12 seconds.
    let rate = SAMPLE_RATE as f64;
    let seconds = 12.0;
    let (f0, f1) = (40.0f64, 12_000.0f64);
    let k = (f1 / f0).ln() / seconds;
    let samples: Vec<f32> = (0..(seconds * rate) as usize)
        .map(|i| {
            let t = i as f64 / rate;
            (0.5 * (std::f64::consts::TAU * f0 * ((k * t).exp() - 1.0) / k).sin()) as f32
        })
        .collect();
    let features = analyze(&Pcm { rate: SAMPLE_RATE, channels: 1, samples });
    let freq_time = |hz: f64| (hz / f0).ln() / k;
    let low = features.at(freq_time(100.0));
    let middle = features.at(freq_time(1000.0));
    let high = features.at(freq_time(8000.0));
    assert!(low.bass > 0.5 && low.mid < 0.2 && low.treble < 0.2, "{low:?}");
    assert!(middle.mid > 0.5 && middle.bass < 0.2 && middle.treble < 0.2, "{middle:?}");
    assert!(high.treble > 0.5 && high.bass < 0.2 && high.mid < 0.2, "{high:?}");
    // A half-scale sine has level about one half.
    assert!((middle.level - 0.5).abs() < 0.05, "level {}", middle.level);
}

#[test]
fn silence_and_tiny_inputs_are_quiet_with_no_beats() {
    for samples in [Vec::new(), vec![0.0; 10], vec![0.0; SAMPLE_RATE as usize * 5]] {
        let features = analyze(&Pcm { rate: SAMPLE_RATE, channels: 2, samples });
        assert!(features.beats.is_empty());
        assert_eq!(features.bpm, 0.0);
        assert!(features.level.iter().all(|&l| l == 0.0));
        assert_eq!(features.beat_phase(1.0), 0.0);
        assert_eq!(features.at(-1.0), Sample::default());
        assert_eq!(features.at(1e9), Sample::default());
        assert_eq!(features.at(f64::NAN), Sample::default());
    }
}

#[test]
fn beat_phase_rises_through_each_beat() {
    let features = AudioFeatures { beats: vec![1.0, 1.5, 2.0], ..Default::default() };
    assert_eq!(features.beat_phase(0.5), 0.0);
    assert_eq!(features.beat_phase(1.0), 0.0);
    assert!((features.beat_phase(1.25) - 0.5).abs() < 1e-6);
    assert!((features.beat_phase(1.999) - 0.998).abs() < 1e-3);
    assert_eq!(features.beat_phase(2.5), 0.0);
    assert_eq!(features.beat_index(1.7), Some(1));
    assert_eq!(features.beat_index(0.2), None);
}

#[test]
fn wav_files_decode_to_48k_stereo_and_cache() {
    let dir = std::env::temp_dir().join(format!("bpe-music-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("clicks.wav");
    let (pcm, _) = clicks(120.0, 8.0, 0.2);
    let spec = hound::WavSpec { channels: 1, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    // Written at 44.1 kHz by taking every sample, so it plays a little slower: 120 * 44.1 / 48.
    for s in &pcm.samples {
        writer.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
    }
    writer.finalize().unwrap();
    let decoded = decode(&path).unwrap();
    assert_eq!((decoded.rate, decoded.channels), (SAMPLE_RATE, 2));
    assert!((decoded.duration() - pcm.samples.len() as f64 / 44_100.0).abs() < 0.01);
    let cache = dir.join("cache");
    let first = cache::analyze_file_in(&path, Some(&cache)).unwrap();
    assert!((first.bpm - 120.0 * 44.1 / 48.0).abs() < 1.0, "bpm {}", first.bpm);
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1);
    let second = cache::analyze_file_in(&path, Some(&cache)).unwrap();
    assert_eq!(first, second);
    assert!(decode(dir.join("missing.mp3")).is_err());
    std::fs::write(dir.join("junk.mp3"), b"not a song").unwrap();
    assert!(decode(dir.join("junk.mp3")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn three_minutes_analyse_quickly() {
    let (pcm, _) = clicks(128.0, 180.0, 0.1);
    let stereo = decode::to_stereo_48k(pcm);
    let started = std::time::Instant::now();
    let features = analyze(&stereo);
    let took = started.elapsed();
    eprintln!("3 minutes analysed in {took:?}");
    assert!((features.bpm - 128.0).abs() < 0.5);
    let limit = if cfg!(debug_assertions) { 10.0 } else { 2.0 };
    assert!(took.as_secs_f64() < limit, "took {took:?}");
}
