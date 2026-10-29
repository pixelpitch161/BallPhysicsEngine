//! A whole batch, end to end: the command line renders two short videos and keeps the better
//! one, and a stopped batch carries on from its record without rendering twice.

use bpe_cli::batch::{self, BatchSpec, Phase, Quality, Status};
use bpe_cli::find_ffmpeg;
use bpe_core::{Address, Owner, ThingId, Value};
use bpe_gfx::headless_device;
use bpe_kit::{kit, new_scene};
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bpe-batch-e2e-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn ready() -> Option<(bpe_gfx::wgpu::Device, bpe_gfx::wgpu::Queue, PathBuf)> {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    let Some(ffmpeg) = find_ffmpeg() else {
        eprintln!("no ffmpeg available, skipping");
        return None;
    };
    Some((device, queue, ffmpeg))
}

/// The new scene, half a second long.
fn short_scene() -> bpe_core::Scene {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let duration = Address { owner: Owner::Thing(ThingId::ROOT), slot: None, key: "duration".into() };
    scene.set_at(&kit.registry, &duration, Value::Number(0.5)).unwrap();
    scene
}

#[test]
fn the_command_line_renders_a_batch_and_keeps_the_best() {
    let Some((_, _, ffmpeg)) = ready() else { return };
    let dir = temp_dir("cli");
    let scene_file = dir.join("short.ron");
    std::fs::write(&scene_file, bpe_core::save(&short_scene())).unwrap();
    let folder = dir.join("out");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bpe-render-next"))
        .arg("batch")
        .arg(&scene_file)
        .arg(&folder)
        .args(["--count", "2", "--keep", "1", "--candidates", "1", "--quality", "draft", "--seed", "5"])
        .arg("--ffmpeg")
        .arg(&ffmpeg)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout).to_string() + &String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{text}");
    assert!(text.contains("kept 1 video(s)"), "{text}");
    let manifest = batch::Manifest::load(&folder).unwrap();
    assert_eq!(manifest.phase, Phase::Done);
    assert_eq!(manifest.count(Status::Kept), 1);
    assert_eq!(manifest.count(Status::Rejected), 1);
    let kept = manifest.kept()[0];
    let video = folder.join(kept.file.as_ref().unwrap());
    assert!(video.is_file(), "{}", video.display());
    assert!(std::fs::metadata(&video).unwrap().len() > 1000);
    assert!(video.with_extension("ron").is_file(), "the kept video's scene is saved beside it");
    let rejected: Vec<_> = std::fs::read_dir(folder.join("rejected")).unwrap().flatten().collect();
    assert_eq!(rejected.len(), 2, "the rejected video and its scene");
    assert!(!folder.join("batch.lock").exists(), "the lock goes when the batch ends");
}

#[test]
fn a_stopped_batch_carries_on_without_rendering_twice() {
    let Some((device, queue, ffmpeg)) = ready() else { return };
    let kit = kit();
    let base = short_scene();
    let folder = temp_dir("resume");
    let spec = BatchSpec { count: 2, keep: 2, prescreen: 1, quality: Quality::Draft, ..BatchSpec::default() };
    let weights = bpe_kit::score::Parts::WEIGHTS;

    // Stop as soon as the first video is rendered.
    let mut manifest = batch::open(&folder, &spec, &base).unwrap();
    let mut rendered = 0;
    batch::run(&folder, &mut manifest, &kit, &device, &queue, &ffmpeg, &weights, &mut |m, p| {
        rendered = m.count(Status::Rendered);
        !(p.phase == Phase::Render && rendered >= 1)
    })
    .unwrap();
    assert_eq!(manifest.phase, Phase::Render);
    assert_eq!(manifest.count(Status::Rendered), 1);
    assert_eq!(manifest.count(Status::Chosen), 1);
    let first = folder.join(manifest.jobs.iter().find(|j| j.status == Status::Rendered).unwrap().file.clone().unwrap());
    let stamp = std::fs::metadata(&first).unwrap().modified().unwrap();

    // Carry on from the record on disk.
    let mut manifest = batch::open(&folder, &spec, &base).unwrap();
    assert_eq!(manifest.phase, Phase::Render, "the record remembers where it was");
    let mut frames_seen = 0;
    batch::run(&folder, &mut manifest, &kit, &device, &queue, &ffmpeg, &weights, &mut |_, p| {
        if p.phase == Phase::Render && p.frames > 0 {
            frames_seen += 1;
        }
        true
    })
    .unwrap();
    assert_eq!(manifest.phase, Phase::Done);
    assert_eq!(manifest.count(Status::Kept), 2);
    assert_eq!(std::fs::metadata(&first).unwrap().modified().unwrap(), stamp, "the first video was not made again");
    assert!(frames_seen > 0, "the second video was");
}
