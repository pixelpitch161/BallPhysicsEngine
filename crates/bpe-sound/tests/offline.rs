//! Rendering a whole scene's sound to a file.

use bpe_core::Scene;
use bpe_kit::{Kit, kit, new_scene, new_studio_scene};
use bpe_sound::{has_sound, offline, render_samples, render_to_wav};

fn toned(kit: &Kit) -> Scene {
    let mut scene = new_scene(&kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    scene.things.get_mut(&shape).unwrap().tags.insert("test-tone".into());
    scene
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bpe-sound-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("sound.wav")
}

#[test]
fn wav_matches_render_samples() {
    let kit = kit();
    let scene = toned(&kit);
    assert!(has_sound(&scene));
    assert!(!has_sound(&new_studio_scene(&kit.registry)));
    let path = temp("match");
    render_to_wav(&scene, &kit, &path, 3.5, |_| true).unwrap();
    let samples = render_samples(&scene, &kit, 3.5);
    assert_eq!(samples.len(), 3 * 48_000 * 2 + 48_000);
    let mut reader = hound::WavReader::open(&path).unwrap();
    let spec = reader.spec();
    assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (2, 48_000, 32));
    assert_eq!(spec.sample_format, hound::SampleFormat::Float);
    let back: Vec<f32> = reader.samples::<f32>().map(Result::unwrap).collect();
    assert_eq!(back.len(), samples.len());
    assert!(back.iter().zip(&samples).all(|(a, b)| a.to_bits() == b.to_bits()));
    assert!(back.iter().any(|&v| v != 0.0));
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn long_render_keeps_memory_flat() {
    let kit = kit();
    let scene = toned(&kit);
    let path = temp("long");
    let mut calls = 0;
    render_to_wav(&scene, &kit, &path, 60.0, |done| {
        calls += 1;
        assert!((0.0..=1.0).contains(&done));
        true
    })
    .unwrap();
    assert!(calls >= 60, "progress once a second at least: {calls}");
    let largest = offline::largest_chunk_bytes();
    assert!(largest > 0 && largest < 4 << 20, "chunks stay small: {largest} bytes");
    assert_eq!(hound::WavReader::open(&path).unwrap().duration(), 60 * 48_000);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn cancel_stops_early() {
    let kit = kit();
    let scene = toned(&kit);
    let path = temp("cancel");
    let mut calls = 0;
    let result = render_to_wav(&scene, &kit, &path, 30.0, |done| {
        calls += 1;
        done < 0.1
    });
    assert!(result.is_err(), "a cancelled render is an error");
    assert!(calls < 10, "it stopped soon after: {calls}");
    assert!(!path.exists(), "and left no file");
}

/// `scene` with the Master's setting `key` at `v`.
fn with_master(mut scene: Scene, kit: &Kit, key: &str, v: f64) -> Scene {
    bpe_kit::ensure_mixer(&mut scene, &kit.registry);
    let card = scene.things.get_mut(&bpe_core::ThingId::ROOT).unwrap().slots.get_mut(&bpe_core::SlotId::Mixer).unwrap();
    card.params.set(key, bpe_core::Value::Number(v));
    scene
}

/// The true peak of interleaved stereo `samples`, in dBTP.
fn true_peak(samples: &[f32]) -> f32 {
    let mut meter = bpe_dsp::LoudnessMeter::new();
    for block in samples.chunks_exact(bpe_sound::BLOCK * 2) {
        let l: [f32; bpe_sound::BLOCK] = std::array::from_fn(|i| block[2 * i]);
        let r: [f32; bpe_sound::BLOCK] = std::array::from_fn(|i| block[2 * i + 1]);
        meter.process(&l, &r);
    }
    meter.true_peak_db()
}

#[test]
fn turning_output_up_never_passes_the_ceiling() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    scene.things.get_mut(&shape).unwrap().tags.insert("test-tone-decay=1".into());
    let loud = with_master(with_master(scene.clone(), &kit, "master_input", 18.0), &kit, "master_output", 12.0);
    let peak = true_peak(&render_samples(&loud, &kit, 4.0));
    assert!(peak <= -1.0 + 0.1, "the ceiling holds with Output at +12 dB: {peak} dBTP");
    assert!(peak > -1.5, "and the limiter is working right up to it: {peak}");
    // Turned down, Output lowers the finished sound as before.
    let plain = true_peak(&render_samples(&scene, &kit, 4.0));
    let quiet = true_peak(&render_samples(&with_master(scene, &kit, "master_output", -6.0), &kit, 4.0));
    assert!((plain - quiet - 6.0).abs() < 0.2, "6 dB down: {plain} then {quiet}");
}

#[test]
fn the_end_fade_ends_in_silence() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    // A tone that rings for the whole clip.
    scene.things.get_mut(&shape).unwrap().tags.insert("test-tone-decay=60".into());
    let plain = render_samples(&scene, &kit, 4.0);
    let faded = render_samples(&with_master(scene, &kit, "master_end_fade", 1.0), &kit, 4.0);
    let frames = plain.len() / 2;
    let at = |s: f64| (s * 48_000.0) as usize * 2;
    assert!(plain[at(2.5)..at(3.0)].iter().any(|v| v.abs() > 0.01), "the tone rings at the end");
    assert!(plain[..at(3.0)].iter().zip(&faded[..at(3.0)]).all(|(a, b)| a == b), "untouched before the fade");
    let last = faded[(frames - 48) * 2..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(last < 1e-3, "silent at the very end: {last}");
    let mid = |s: &[f32]| s[at(3.4)..at(3.6)].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(mid(&faded) < mid(&plain) * 0.8 && mid(&faded) > mid(&plain) * 0.2, "half way down the fade");
}
