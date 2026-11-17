//! A scene with a Liquid thing exports and previews a final frame.

use bpe_cli::{Encoder, ExportOptions, export_video, final_frame, find_ffmpeg};
use bpe_gfx::test_device;
use bpe_kit::kit;

#[test]
fn a_liquid_scene_previews_a_final_frame() {
    let (device, queue) = test_device().expect("a graphics device");
    let kit = kit();
    let (_, scene) = bpe_kit::scenes::liquid_scenes(&kit.registry).swap_remove(0);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let (pixels, size, _) = final_frame(&device, &queue, &scene, &kit, 1.0, 0.25, None, &stop).unwrap();
    assert_eq!(pixels.len() as u32, size.0 * size.1 * 4);
}

#[test]
fn a_liquid_scene_exports() {
    let (device, queue) = test_device().expect("a graphics device");
    if find_ffmpeg().is_none() {
        return;
    }
    let kit = kit();
    let (_, scene) = bpe_kit::scenes::liquid_scenes(&kit.registry).swap_remove(0);
    let dir = std::env::temp_dir().join(format!("bpe-liquid-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut options = ExportOptions::new(dir.join("water.mp4"));
    options.max_frames = Some(20);
    options.scale = 0.25;
    options.encoder = Encoder::Cpu;
    export_video(&device, &queue, &scene, &kit, &options, |_, _| true).unwrap();
    assert!(dir.join("water.mp4").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The editor's blank project with a Liquid thing added at the top, free in the scene.
fn free_liquid() -> (bpe_kit::Kit, bpe_core::Scene) {
    let kit = kit();
    let mut scene = bpe_kit::new_studio_scene(&kit.registry);
    bpe_kit::add_thing(&mut scene, &kit.registry, bpe_kit::things::liquid::KIND, bpe_core::ThingId::ROOT).unwrap();
    (kit, scene)
}

/// The editor keeps its own card (its window's device) named on its thread while an export or a
/// final frame draws on a device of its own, made from another wgpu instance. Devices of different
/// instances can have equal ids, so the liquid must not step on the editor's card and be drawn
/// with the export's: the graphics card complains and the export stops.
#[test]
fn a_liquid_draws_on_a_second_device_while_the_editors_is_in_use() {
    use std::sync::{Arc, Mutex};
    let (editor_device, editor_queue) = test_device().expect("a graphics device");
    let _editor =
        bpe_gfx::swarm::use_for_now(Some(bpe_gfx::swarm::SwarmGpu::for_device(&editor_device, &editor_queue)));
    let (kit, scene) = free_liquid();
    let (device, queue) = test_device().expect("a second graphics device");
    let seen: Arc<Mutex<Option<String>>> = Arc::default();
    let s = seen.clone();
    device.on_uncaptured_error(Arc::new(move |e| {
        s.lock().unwrap().get_or_insert_with(|| e.to_string());
    }));
    let stop = std::sync::atomic::AtomicBool::new(false);
    let (pixels, size, _) = final_frame(&device, &queue, &scene, &kit, 1.0, 0.25, None, &stop).unwrap();
    assert_eq!(pixels.len() as u32, size.0 * size.1 * 4);
    if let Some(error) = seen.lock().unwrap().take() {
        panic!("the graphics card reported a problem: {error}");
    }
    // The liquid is drawn: the default water is blue, so the bottom of the picture is bluer than the grey scene.
    let (w, h) = size;
    let at = |x: u32, y: u32| &pixels[((y * w + x) * 4) as usize..][..3];
    let bottom = at(w / 2, h - 4);
    assert!(bottom[2] > bottom[0] + 30, "no liquid at the bottom: {bottom:?}");
}
