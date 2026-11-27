//! Water mode part 8: a Water scene's video, drawn from its final (made first when asked), or
//! from its draft when asked for by name, and Preview final frame of it.

use std::path::PathBuf;

use bpe_cli::water::{self, FINAL_MISSING, FRAME_NOT_MADE, WaterSource};
use bpe_cli::{ExportOptions, export_video, find_ffmpeg};
use bpe_core::{Address, Scene, SlotId, Stroke, Strokes, ThingId, Value};
use bpe_kit::things::water::{LIQUID, POUR, course_of, new_water_scene};
use bpe_kit::{Kit, kit};
use glam::Vec2;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bpe-cli-water-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn gpu() -> (bpe_gfx::wgpu::Device, bpe_gfx::wgpu::Queue) {
    bpe_gfx::test_device().expect("these tests draw")
}

fn set(scene: &mut Scene, id: ThingId, slot: SlotId, key: &str, v: Value) {
    scene.things.get_mut(&id).unwrap().slots.get_mut(&slot).unwrap().params.set(key, v);
}

/// A box carved in the rock with a fill at its foot, a quarter second long at Coarse final
/// detail, 270 x 480 at 30 frames a second.
fn small(kit: &Kit) -> Scene {
    let reg = &kit.registry;
    let mut scene = new_water_scene(reg, "");
    let root = &mut scene.things.get_mut(&ThingId::ROOT).unwrap().params;
    root.set("width", Value::Int(270));
    root.set("height", Value::Int(480));
    root.set("fps", Value::Int(30));
    root.set("duration", Value::Number(0.25));
    set(&mut scene, ThingId::ROOT, SlotId::Water, "final_detail", Value::Choice("coarse".into()));
    let course = course_of(&scene).unwrap();
    let hole = Stroke { points: vec![[0.0, 0.0]], size: 200.0, strength: 1.0, square: true, ..Default::default() };
    set(&mut scene, course, SlotId::Form, "carved", Value::Strokes(Strokes { list: vec![hole] }));
    bpe_kit::add_thing(&mut scene, reg, LIQUID, ThingId::ROOT).unwrap();
    let fill = bpe_kit::add_thing(&mut scene, reg, POUR, ThingId::ROOT).unwrap();
    scene.things.get_mut(&fill).unwrap().transform.position = Vec2::new(0.0, 120.0);
    set(&mut scene, fill, SlotId::Main, "kind", Value::Choice("fill_at_the_start".into()));
    set(&mut scene, fill, SlotId::Main, "fill_size", Value::Vec2([160.0, 160.0]));
    scene
}

fn options(dir: &std::path::Path, source: WaterSource) -> ExportOptions {
    let mut options = ExportOptions::new(dir.join("water.mp4"));
    options.water = source;
    options.finals = Some(dir.join("finals"));
    options.encoder = bpe_cli::Encoder::Cpu;
    options
}

fn seconds(path: &std::path::Path) -> f64 {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0.0)
}

/// Without its final a Water scene's export says so; made first, it exports; slow motion makes it
/// longer; and Preview final frame shows the take's moment.
#[test]
fn a_water_scene_exports_from_its_final() {
    if find_ffmpeg().is_none() {
        eprintln!("no ffmpeg available, skipping");
        return;
    }
    let (device, queue) = gpu();
    let kit = kit();
    let mut scene = small(&kit);
    let dir = temp_dir("final");
    let error = export_video(&device, &queue, &scene, &kit, &options(&dir, WaterSource::Final), |_, _| true);
    assert_eq!(error.unwrap_err().to_string(), FINAL_MISSING);
    let frame = water::final_frame(&device, &queue, &scene, &kit, 0.1, 0.25, &dir.join("finals"));
    assert_eq!(frame.unwrap_err().to_string(), FRAME_NOT_MADE);
    let mut done = 0;
    export_video(&device, &queue, &scene, &kit, &options(&dir, WaterSource::MakeFinal), |d, _| {
        done = d;
        true
    })
    .unwrap();
    let out = dir.join("water.mp4");
    assert_eq!(done, 8, "a quarter second at 30 frames a second");
    assert!((seconds(&out) - 0.25).abs() < 0.05, "{}", seconds(&out));
    // (The final is made now: Export reads it, and Preview final frame shows its moment.)
    std::fs::remove_file(&out).unwrap();
    export_video(&device, &queue, &scene, &kit, &options(&dir, WaterSource::Final), |_, _| true).unwrap();
    let (pixels, size, _) = water::final_frame(&device, &queue, &scene, &kit, 0.1, 0.25, &dir.join("finals")).unwrap();
    assert_eq!(size, (68, 120));
    assert_eq!(pixels.len(), 68 * 120 * 4);
    // Half speed: the same water plays for twice as long, from the same final with more moments.
    let speed = Address { thing: ThingId::ROOT, slot: Some(SlotId::Water), key: "speed".into() };
    scene.set_key(&kit.registry, &speed, 0.0, Value::Number(0.5)).unwrap();
    export_video(&device, &queue, &scene, &kit, &options(&dir, WaterSource::MakeFinal), |_, _| true).unwrap();
    assert!((seconds(&out) - 0.5).abs() < 0.05, "{}", seconds(&out));
    std::fs::remove_dir_all(&dir).ok();
}

/// The draft exports when asked for by name.
#[test]
fn a_water_scene_exports_its_draft() {
    if find_ffmpeg().is_none() {
        eprintln!("no ffmpeg available, skipping");
        return;
    }
    let (device, queue) = gpu();
    let kit = kit();
    let scene = small(&kit);
    let dir = temp_dir("draft");
    export_video(&device, &queue, &scene, &kit, &options(&dir, WaterSource::Draft), |_, _| true).unwrap();
    assert!((seconds(&dir.join("water.mp4")) - 0.25).abs() < 0.05);
    assert!(!dir.join("finals").exists(), "the draft makes no final");
    std::fs::remove_dir_all(&dir).ok();
}
