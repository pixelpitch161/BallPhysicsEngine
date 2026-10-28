//! Captions with live counters look the same in the editor's preview as in the exported video,
//! and the same at a moment however the playhead got there.

use bpe_export::ExportOptions;
use bpe_render::{FrameData, Renderer};
use bpe_scene::{Anim, Caption, CaptionStyle, React, Scene};
use bpe_sim::Playback;

fn gpu() -> Option<(bpe_render::wgpu::Device, bpe_render::wgpu::Queue)> {
    match bpe_render::headless_device() {
        Ok(device) => Some(device),
        Err(error) if std::env::var_os("BPE_REQUIRE_GPU").is_some() => panic!("no GPU adapter: {error}"),
        Err(_) => {
            eprintln!("no GPU adapter available, skipping");
            None
        }
    }
}

/// A preset with a hook, a hit counter that pulses and a countdown.
fn scene() -> Scene {
    let mut scene = Scene::presets().into_iter().find(|p| p.name == "Escape rings").unwrap().scene;
    scene.video.duration = 3.0;
    let mut hook = Caption::new("Can it escape?", CaptionStyle::Hook);
    hook.anim_in = Anim::Pop;
    let mut counter = Caption::new("{hits} bounces", CaptionStyle::Counter);
    counter.react = React::PulseOnHit;
    let timer = Caption::new("{countdown}", CaptionStyle::Timer);
    scene.captions = vec![hook, counter, timer];
    scene
}

/// The picture at `time` the way the editor draws it: seek the playback, fill the frame, render.
fn preview(
    device: &bpe_render::wgpu::Device,
    queue: &bpe_render::wgpu::Queue,
    scene: &Scene,
    playback: &mut Playback,
    renderer: &mut Renderer,
    time: f64,
) -> Vec<u8> {
    playback.seek(time);
    let frame = FrameData::build(scene, playback, renderer.pixel_size(scene));
    renderer.render(device, queue, scene, playback.simulation().time(), &frame);
    renderer.read_output(device, queue).unwrap()
}

#[test]
fn preview_and_export_draw_captions_the_same() {
    let Some((device, queue)) = gpu() else { return };
    let Some(ffmpeg) = bpe_export::find_ffmpeg() else {
        assert!(std::env::var_os("BPE_REQUIRE_GPU").is_none(), "ffmpeg is needed");
        return;
    };
    let scene = scene();
    let out = std::env::temp_dir().join(format!("bpe-captions-{}.mp4", std::process::id()));
    let mut options = ExportOptions::new(&out);
    options.ffmpeg = ffmpeg;
    options.scale = 0.25;
    options.encoder = bpe_export::Encoder::Cpu;
    let checked = [0u32, 20, 45, 89];
    let mut exported = Vec::new();
    bpe_export::export_video_with(
        &device,
        &queue,
        &scene,
        &options,
        None,
        |_, _| true,
        |index, pixels| {
            if checked.contains(&index) {
                exported.push((index, pixels.to_vec()));
            }
        },
    )
    .unwrap();
    let _ = std::fs::remove_file(&out);
    assert_eq!(exported.len(), checked.len());
    let (width, height) = bpe_export::output_size(&scene, options.scale);
    let mut renderer = Renderer::new(&device, width, height);
    let mut playback = Playback::new(&scene);
    let fps = scene.video.fps as f64;
    // Backwards on purpose: the preview seeks wherever the playhead goes.
    for (index, pixels) in exported.iter().rev() {
        let shown = preview(&device, &queue, &scene, &mut playback, &mut renderer, *index as f64 / fps);
        let different = shown.iter().zip(pixels).filter(|(a, b)| a.abs_diff(**b) > 2).count();
        assert!(different * 1000 <= shown.len(), "frame {index}: {different} of {} bytes differ", shown.len());
    }
}

#[test]
fn counters_read_the_same_after_seeking_back() {
    let Some((device, queue)) = gpu() else { return };
    let scene = scene();
    let mut renderer = Renderer::new(&device, 270, 480);
    let mut straight = Playback::new(&scene);
    let first = preview(&device, &queue, &scene, &mut straight, &mut renderer, 1.5);
    let mut wandering = Playback::new(&scene);
    preview(&device, &queue, &scene, &mut wandering, &mut renderer, 2.9);
    let again = preview(&device, &queue, &scene, &mut wandering, &mut renderer, 1.5);
    assert!(first == again, "the same moment drew differently after seeking back");
    let values = bpe_render::text::text_values(&scene, &wandering);
    let fresh = bpe_render::text::text_values(&scene, &straight);
    assert_eq!(values.hits, fresh.hits);
    assert!(values.hits > 0, "the counter never moved");
}
