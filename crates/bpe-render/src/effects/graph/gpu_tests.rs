//! Pictures: node versions of the built-in effects must look like the originals.

use crate::effects::templates;
use crate::{FrameData, Renderer, headless_device};
use bpe_scene::{Effect, Scene};
use bpe_sim::Playback;

/// A GPU, or a skip. With `BPE_REQUIRE_GPU=1` a missing GPU fails the test instead, so CI-like
/// runs can't pass by silently skipping (install mesa-vulkan-drivers for software Vulkan).
fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    match headless_device() {
        Ok(gpu) => Some(gpu),
        Err(error) if std::env::var("BPE_REQUIRE_GPU").is_ok_and(|v| v == "1") => panic!("no GPU: {error}"),
        Err(_) => {
            eprintln!("no GPU adapter available, skipping");
            None
        }
    }
}

fn render(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, graphs: &[super::Graph]) -> Vec<u8> {
    let mut renderer = Renderer::new(device, 216, 384);
    renderer.effect_library().sync_graphs(graphs);
    let mut playback = Playback::new(scene);
    let mut frame = FrameData::default();
    // Several frames in a row, so feedback effects build up their history.
    for i in 0..12 {
        let time = 4.0 + i as f64 / 60.0;
        playback.seek(time);
        frame.fill(scene, &playback, renderer.pixel_size(scene));
        frame.pulse = 0.8;
        renderer.render(device, queue, scene, time, &frame);
    }
    assert!(renderer.effect_gpu_errors().is_empty(), "{:?}", renderer.effect_gpu_errors());
    renderer.read_output(device, queue).unwrap()
}

/// Average difference per colour channel (0 to 255) and the share of pixels off by more than 12.
fn difference(a: &[u8], b: &[u8]) -> (f64, f64) {
    let mut total = 0u64;
    let mut off = 0usize;
    for (p, q) in a.chunks(4).zip(b.chunks(4)) {
        let d: Vec<u64> = (0..3).map(|i| (p[i] as i32 - q[i] as i32).unsigned_abs() as u64).collect();
        total += d.iter().sum::<u64>();
        if d.iter().any(|&d| d > 12) {
            off += 1;
        }
    }
    let pixels = a.len() / 4;
    (total as f64 / (pixels * 3) as f64, off as f64 / pixels as f64)
}

#[test]
fn node_versions_match_the_built_in_effects() {
    let Some((device, queue)) = gpu() else { return };
    let base = Scene::default();
    let plain = render(&device, &queue, &base, &[]);
    let mut report = Vec::new();
    for id in templates::REBUILT {
        let graph = templates::rebuild(id).unwrap();
        let graph_id = super::effect_id(&graph.name);
        // Switch toggles on so their code paths are compared too.
        let original_info = crate::EffectLibrary::new().builtin(id).unwrap();
        let mut original = Effect::new(id);
        for param in &original_info.params {
            if param.kind == crate::effects::ParamKind::Toggle && param.name != "swap" {
                original = original.with(&param.name, 1.0);
            }
        }
        let mut rebuilt = original.clone();
        rebuilt.shader = graph_id;
        let a = render(&device, &queue, &Scene { effects: vec![original], ..base.clone() }, &[]);
        let b = render(&device, &queue, &Scene { effects: vec![rebuilt], ..base.clone() }, &[graph]);
        let (mean, off) = difference(&a, &b);
        let (changed, _) = difference(&a, &plain);
        report.push(format!("{id}: mean difference {mean:.3}, pixels off {:.2}%, effect strength {changed:.2}", off * 100.0));
        assert!(changed > 0.05 || id == "glitch", "{id} did nothing, so the comparison proves nothing");
        assert!(mean < 0.6 && off < 0.01, "{id} node version differs: mean {mean:.3}, {:.2}% of pixels", off * 100.0);
    }
    println!("{}", report.join("\n"));
}

#[test]
fn thumbnail_shaders_pass_the_check() {
    for graph in templates::graphs() {
        let info = crate::effects::EffectInfo::graph_thumbnails(&graph);
        let code = info.compiled.as_ref().unwrap();
        let module = naga::front::wgsl::parse_str(code).unwrap();
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap();
    }
}

#[test]
fn block_previews_draw_each_block() {
    let Some((device, queue)) = gpu() else { return };
    let mut b = super::Builder::new("Previews");
    let red = b.add("colour");
    b.set_color(red, "color", [1.0, 0.0, 0.0]);
    let green = b.add("make_colour");
    b.set(green, "r", 0.0).set(green, "g", 1.0).set(green, "b", 0.0);
    let out = b.add("output");
    b.wire(red, "out", out, "color");
    let graph = b.finish();
    let info = std::sync::Arc::new(crate::effects::EffectInfo::graph_thumbnails(&graph));
    let generated = info.graph.as_ref().unwrap().generated.as_ref().unwrap();
    assert_eq!(generated.thumbnails, [red, green]);
    let scene = Scene { effects: vec![Effect::new(super::effect_id("Previews"))], ..Scene::default() };
    let mut renderer = Renderer::new(&device, 108, 192);
    renderer.effect_library().sync_graphs(std::slice::from_ref(&graph));
    renderer.set_effect_thumbnails(Some(crate::effects::ThumbnailRequest { effect: 0, info, views: vec![1, 2] }));
    let playback = Playback::new(&scene);
    let frame = FrameData::build(&scene, &playback, renderer.pixel_size(&scene));
    renderer.render(&device, &queue, &scene, 0.0, &frame);
    assert!(renderer.effect_thumbnails_drawn());
    let pixels = read(&device, &queue, renderer.effect_thumbnails(&device));
    let (width, _) = crate::effects::thumbnail_atlas_size();
    let at = |tile: usize| {
        let [x, y, w, h] = crate::effects::thumbnail_tile(tile);
        let i = (((y + h / 2) * width + x + w / 2) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    assert!(at(0)[0] > 200 && at(0)[1] < 30, "{:?}", at(0));
    assert!(at(1)[1] > 200 && at(1)[0] < 30, "{:?}", at(1));
    assert_eq!(at(2), [0, 0, 0], "unused tiles stay black");
    // Switched off, nothing more is drawn.
    renderer.set_effect_thumbnails(None);
    renderer.render(&device, &queue, &scene, 0.0, &frame);
    assert!(!renderer.effect_thumbnails_drawn());
}

/// Reads an Rgba8 texture view's texture back. Only for the thumbnail atlas.
fn read(device: &wgpu::Device, queue: &wgpu::Queue, view: &wgpu::TextureView) -> Vec<u8> {
    let texture = view.texture();
    let (width, height) = (texture.width(), texture.height());
    let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = buffer.slice(..).get_mapped_range().unwrap();
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let start = (y * row) as usize;
        pixels.extend_from_slice(&data[start..start + (width * 4) as usize]);
    }
    pixels
}
