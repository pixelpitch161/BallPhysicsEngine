//! Export checks: a short clip has picture and sound; failures leave no file behind.

use bpe_cli::{Encoder, ExportOptions, export_video, find_ffmpeg};
use bpe_gfx::headless_device;
use bpe_kit::{kit, new_scene};
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bpe-cli-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ready() -> Option<(bpe_gfx::wgpu::Device, bpe_gfx::wgpu::Queue)> {
    let Ok(pair) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    if find_ffmpeg().is_none() {
        eprintln!("no ffmpeg available, skipping");
        return None;
    }
    Some(pair)
}

#[test]
fn exports_a_short_clip_with_sound() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let dir = temp_dir("clip");
    let mut options = ExportOptions::new(dir.join("one-ball.mp4"));
    options.max_frames = Some(30);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    let mut calls = 0;
    let used = export_video(&device, &queue, &scene, &kit, &options, |done, total| {
        calls += 1;
        assert!(done <= total && total == 30);
        true
    })
    .unwrap();
    assert_eq!(used, Encoder::Cpu);
    assert_eq!(calls, 30);
    let probe = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type,width,height:format=duration", "-of", "csv"])
        .arg(dir.join("one-ball.mp4"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(text.lines().any(|l| l.contains("video") && l.contains("270") && l.contains("480")), "{text}");
    assert!(text.lines().any(|l| l.contains("audio")), "{text}");
    let duration: f64 = text
        .lines()
        .find_map(|l| l.strip_prefix("format,"))
        .and_then(|d| d.trim().parse().ok())
        .unwrap_or_else(|| panic!("no duration in {text}"));
    assert!((duration - 0.5).abs() < 0.1, "{duration}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exports_a_stage3_scene_with_an_obstacle() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let (name, scene) = bpe_kit::scenes::stage3(&kit.registry).swap_remove(0);
    assert_eq!(name, "Peg board");
    let dir = temp_dir("pegs");
    let mut options = ExportOptions::new(dir.join("peg-board.mp4"));
    options.max_frames = Some(30);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    export_video(&device, &queue, &scene, &kit, &options, |_, _| true).unwrap();
    let probe = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type:format=duration", "-of", "csv"])
        .arg(dir.join("peg-board.mp4"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(text.lines().any(|l| l.contains("video")) && text.lines().any(|l| l.contains("audio")), "{text}");
    let duration: f64 = text
        .lines()
        .find_map(|l| l.strip_prefix("format,"))
        .and_then(|d| d.trim().parse().ok())
        .unwrap_or_else(|| panic!("no duration in {text}"));
    assert!((duration - 0.5).abs() < 0.1, "{duration}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_without_ffmpeg_fails_clearly_and_leaves_no_file() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let dir = temp_dir("noffmpeg");
    let mut options = ExportOptions::new(dir.join("out.mp4"));
    options.ffmpeg = "/no/such/ffmpeg".into();
    options.max_frames = Some(5);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    let error = export_video(&device, &queue, &scene, &kit, &options, |_, _| true).unwrap_err();
    assert!(error.to_string().starts_with("could not start ffmpeg"), "{error:#}");
    assert!(!dir.join("out.mp4").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cancelled_export_leaves_no_file() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let scene = new_scene(&kit.registry);
    let dir = temp_dir("cancel");
    let mut options = ExportOptions::new(dir.join("out.mp4"));
    options.max_frames = Some(60);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    let error = export_video(&device, &queue, &scene, &kit, &options, |done, _| done < 5).unwrap_err();
    assert!(error.to_string().contains("export cancelled"), "{error:#}");
    assert!(!dir.join("out.mp4").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn output_sizes_are_even_and_encoders_have_names() {
    assert_eq!(bpe_cli::output_size(1080, 1920, 0.25), (270, 480));
    assert_eq!(bpe_cli::output_size(1081, 1919, 1.0), (1080, 1918));
    assert_eq!(bpe_cli::output_size(10, 10, 0.0), (2, 2));
    for name in ["auto", "cpu", "nvidia", "amd", "intel"] {
        let e = Encoder::from_name(name).unwrap();
        assert!(!e.name().is_empty());
    }
    assert_eq!(Encoder::from_name("quantum"), None);
}

#[test]
fn an_exported_clip_with_beat_sync_has_sound_and_the_right_length() {
    let Some((device, queue)) = ready() else { return };
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let root = scene.things.get_mut(&bpe_core::ThingId::ROOT).unwrap();
    root.params.set("duration", bpe_core::Value::Number(2.0));
    root.params.set("bpm", bpe_core::Value::Number(120.0));
    root.params.set("beat_sync", bpe_core::Value::Choice("warp".into()));
    let dir = temp_dir("beat-sync");
    let mut options = ExportOptions::new(dir.join("beat.mp4"));
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    export_video(&device, &queue, &scene, &kit, &options, |_, _| true).unwrap();
    let size = std::fs::metadata(dir.join("beat.mp4")).unwrap().len();
    assert!(size > 10_000, "{size} bytes");
    let probe = match std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type:format=duration", "-of", "csv"])
        .arg(dir.join("beat.mp4"))
        .output()
    {
        Ok(probe) => probe,
        Err(error) => {
            eprintln!("ffprobe isn't installed ({error}); the clip's streams and length go unchecked");
            return;
        }
    };
    let text = String::from_utf8_lossy(&probe.stdout);
    assert!(text.lines().any(|l| l.contains("audio")), "{text}");
    let duration: f64 = text
        .lines()
        .find_map(|l| l.strip_prefix("format,"))
        .and_then(|d| d.trim().parse().ok())
        .unwrap_or_else(|| panic!("no duration in {text}"));
    assert!((duration - 2.0).abs() < 0.15, "{duration}");
    let _ = std::fs::remove_dir_all(&dir);
}
