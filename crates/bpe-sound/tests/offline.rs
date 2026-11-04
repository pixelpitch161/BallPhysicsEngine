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
