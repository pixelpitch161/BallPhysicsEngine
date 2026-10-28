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
    render_with_bass(device, queue, scene, graphs, 0.0)
}

fn render_with_bass(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &Scene,
    graphs: &[super::Graph],
    bass: f32,
) -> Vec<u8> {
    let mut renderer = Renderer::new(device, 216, 384);
    let scene = &Scene { shader_graphs: graphs.to_vec(), ..scene.clone() };
    let mut playback = Playback::new(scene);
    let mut frame = FrameData::default();
    // Several frames in a row, so feedback effects build up their history.
    for i in 0..12 {
        let time = 4.0 + i as f64 / 60.0;
        playback.seek(time);
        frame.fill(scene, &playback, renderer.pixel_size(scene));
        frame.pulse = 0.8;
        frame.audio[0] = bass;
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
        let b =
            render(&device, &queue, &Scene { effects: vec![rebuilt], ..base.clone() }, std::slice::from_ref(&graph));
        let (mean, off) = difference(&a, &b);
        let (changed, _) = difference(&a, &plain);
        report.push(format!(
            "{id}: mean difference {mean:.3}, pixels off {:.2}%, effect strength {changed:.2}",
            off * 100.0
        ));
        assert!(changed > 0.05 || id == "glitch", "{id} did nothing, so the comparison proves nothing");
        assert!(mean < 0.6 && off < 0.01, "{id} node version differs: mean {mean:.3}, {:.2}% of pixels", off * 100.0);
        // With "Pulse with music" up and the bass playing. Glitch's node version scales the
        // bass by its react setting too, so it is only close.
        if original_info.params.iter().any(|p| p.name == "music") && id != "glitch" {
            let original = Effect { params: [("music".to_string(), 1.0)].into(), ..Effect::new(id) };
            let rebuilt = Effect { shader: super::effect_id(&graph.name), ..original.clone() };
            let scene = |e: Effect| Scene { effects: vec![e], ..base.clone() };
            let a = render_with_bass(&device, &queue, &scene(original), &[], 0.8);
            let b = render_with_bass(&device, &queue, &scene(rebuilt), std::slice::from_ref(&graph), 0.8);
            let (mean, off) = difference(&a, &b);
            report.push(format!("{id} with the bass: mean difference {mean:.3}, pixels off {:.2}%", off * 100.0));
            assert!(
                mean < 0.6 && off < 0.01,
                "{id} with the bass differs: mean {mean:.3}, {:.2}% of pixels",
                off * 100.0
            );
        }
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
    let scene = Scene {
        effects: vec![Effect::new(super::effect_id("Previews"))],
        shader_graphs: vec![graph.clone()],
        ..Scene::default()
    };
    let mut renderer = Renderer::new(&device, 108, 192);
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

/// About 40 blocks: warps, patterns, music and colour steps.
pub(super) fn big_graph() -> super::Graph {
    let mut b = super::Builder::new("Forty blocks");
    let mut at = b.add("position");
    for (i, kind) in ["twirl", "ripple", "flow", "kaleidoscope", "mirror", "zoom", "spin", "offset"].iter().enumerate()
    {
        let warp = b.add(kind);
        b.wire(at, "out", warp, "at");
        at = warp;
        if i == 3 {
            let music = b.add("music");
            let k = b.add("multiply");
            b.wire(music, "bass", k, "a").wire(k, "out", warp, "zoom");
        }
    }
    let pic = b.add("picture");
    b.wire(at, "out", pic, "at");
    let clouds = b.add("fbm");
    b.wire(at, "out", clouds, "at");
    let cells = b.add("voronoi");
    b.wire(at, "out", cells, "at");
    let mix = b.add("mix");
    b.wire(clouds, "out", mix, "a").wire(cells, "distance", mix, "b");
    let ramp = b.add("ramp");
    b.wire(mix, "out", ramp, "t");
    let mut colour = b.add("blend_screen");
    b.wire(pic, "out", colour, "base").wire(ramp, "out", colour, "layer");
    for i in 0..18 {
        let kind = ["hue_shift", "saturation", "vignette", "brightness_contrast", "grain", "blend_overlay"][i % 6];
        let step = b.add(kind);
        let input = if kind == "blend_overlay" { "base" } else { "color" };
        b.wire(colour, "out", step, input);
        colour = step;
    }
    let star = b.add("star");
    let mask = b.add("mask");
    b.wire(colour, "out", mask, "a").wire(star, "fill", mask, "mask");
    let glow = b.add("hit_glow");
    let add = b.add("blend_add");
    b.wire(mask, "out", add, "base").wire(glow, "out", add, "layer");
    let out = b.add("output");
    b.wire(add, "out", out, "color");
    b.finish()
}

/// Prints how long a 40-block node effect takes on the GPU next to a built-in effect.
/// Run with `cargo test -p bpe-render --release graph_gpu_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn graph_gpu_cost() {
    let Some((device, queue)) = gpu() else { return };
    let graph = big_graph();
    assert!(graph.nodes.len() >= 40, "{}", graph.nodes.len());
    let (width, height) = match std::env::var("BPE_BENCH_SIZE").ok().as_deref() {
        Some("full") => (1080, 1920),
        _ => (540, 960),
    };
    let time_frames = |scene: &Scene| {
        let mut renderer = Renderer::new(&device, width, height);
        let scene = &Scene { shader_graphs: vec![graph.clone()], ..scene.clone() };
        let mut playback = Playback::new(scene);
        playback.seek(4.0);
        let frame = FrameData::build(scene, &playback, renderer.pixel_size(scene));
        renderer.render(&device, &queue, scene, 4.0, &frame);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let start = std::time::Instant::now();
        for i in 0..20 {
            renderer.render(&device, &queue, scene, 4.0 + i as f64 / 60.0, &frame);
        }
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        start.elapsed().as_secs_f64() * 1000.0 / 20.0
    };
    let plain = time_frames(&Scene::default());
    let builtin = time_frames(&Scene { effects: vec![Effect::new("kaleidoscope")], ..Scene::default() });
    let nodes = time_frames(&Scene { effects: vec![Effect::new(super::effect_id(&graph.name))], ..Scene::default() });
    let compile = std::time::Instant::now();
    let info = crate::effects::EffectInfo::from_graph(&graph, false);
    let to_wgsl = compile.elapsed();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(info.compiled.as_ref().unwrap().as_str().into()),
    });
    drop(module);
    let to_module = compile.elapsed();
    println!(
        "{width}x{height}, per frame: no effects {plain:.2} ms, kaleidoscope {:.2} ms extra, 40-block graph {:.2} ms extra",
        builtin - plain,
        nodes - plain
    );
    println!("graph to checked WGSL {to_wgsl:?}, to a GPU shader module {to_module:?}");
}

/// A soundtrack analysis with the bass at `bass` the whole time and a beat every half second.
fn steady_music(bass: f32) -> std::sync::Arc<bpe_music::AudioFeatures> {
    let frames = 200 * 20;
    std::sync::Arc::new(bpe_music::AudioFeatures {
        rate: 200.0,
        bass: vec![bass; frames],
        mid: vec![bass * 0.5; frames],
        treble: vec![bass; frames],
        level: vec![bass; frames],
        onset: vec![0.0; frames],
        beats: (0..40).map(|i| i as f64 * 0.5).collect(),
        downbeats: Vec::new(),
        bpm: 120.0,
    })
}

fn render_with_mix(device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, bass: Option<f32>) -> Vec<u8> {
    let mut renderer = Renderer::new(device, 108, 192);
    let mut playback = Playback::new(scene);
    let mut frame = FrameData { mix: bass.map(steady_music), ..FrameData::default() };
    for i in 0..6 {
        let time = 4.0 + i as f64 / 60.0;
        playback.seek(time);
        frame.fill(scene, &playback, renderer.pixel_size(scene));
        renderer.render(device, queue, scene, time, &frame);
    }
    renderer.read_output(device, queue).unwrap()
}

#[test]
fn effects_pulse_with_the_music() {
    let Some((device, queue)) = gpu() else { return };
    // Every built-in "pulse with music" effect reacts to the bass, and only when asked to.
    for id in ["shockwave", "kaleidoscope", "color_cycle", "liquid", "echo", "glitch"] {
        let off = Scene { effects: vec![Effect::new(id)], ..Scene::default() };
        let on = Scene { effects: vec![Effect::new(id).with("music", 1.0)], ..Scene::default() };
        let quiet = render_with_mix(&device, &queue, &off, None);
        assert_eq!(quiet, render_with_mix(&device, &queue, &off, Some(1.0)), "{id} pulses with the music set to 0");
        let (moved, _) = difference(
            &render_with_mix(&device, &queue, &on, Some(0.0)),
            &render_with_mix(&device, &queue, &on, Some(1.0)),
        );
        assert!(moved > 0.02, "{id} doesn't react to the bass ({moved})");
    }
    // So do the music templates. Without music, beats rest. Lava and Rainbow flow are
    // materials that don't follow the music.
    for graph in templates::graphs().into_iter().filter(|g| !["Lava", "Rainbow flow"].contains(&g.name.as_str())) {
        let scene = Scene { effects: vec![Effect::new(super::effect_id(&graph.name))], ..Scene::default() };
        let (moved, _) = difference(
            &render_with_mix(&device, &queue, &scene, None),
            &render_with_mix(&device, &queue, &scene, Some(1.0)),
        );
        assert!(moved > 0.02, "{} doesn't react to the music ({moved})", graph.name);
    }
}

#[test]
fn modulators_pulse_settings_in_the_picture() {
    let Some((device, queue)) = gpu() else { return };
    let mut scene = Scene::default();
    scene.modulators.push(bpe_scene::Modulator {
        amount: 2.0,
        ..bpe_scene::Modulator::new(bpe_scene::Property::Exposure, bpe_scene::AudioBand::Bass)
    });
    let loud = render_with_mix(&device, &queue, &scene, Some(1.0));
    let quiet = render_with_mix(&device, &queue, &scene, Some(0.0));
    let plain = render_with_mix(&device, &queue, &Scene::default(), Some(1.0));
    assert_eq!(quiet, plain, "silence leaves the setting alone");
    let brighter = loud.iter().map(|&v| v as u64).sum::<u64>() > quiet.iter().map(|&v| v as u64).sum::<u64>();
    assert!(brighter, "loud bass should raise the exposure");
}

/// Prints what one editor frame costs while a node effect is being edited in the Shaders
/// workspace: rebuilding the effect, its block previews and their GPU pipelines. Times are until
/// the frame is handed to the GPU, like the editor's own frame. `BPE_BENCH_FOREGROUND=1` builds
/// changed shaders straight away, as exports do.
/// Run with `cargo test -p bpe-render --release graph_edit_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn graph_edit_cost() {
    use crate::effects::{EffectInfo, EffectLibrary, ThumbnailRequest};
    use std::sync::Arc;
    let Some((device, queue)) = gpu() else { return };
    for graph in templates::graphs().into_iter().chain([big_graph()]) {
        let library = EffectLibrary::new();
        let mut scene = Scene { effects: vec![Effect::new(super::effect_id(&graph.name))], ..Scene::default() };
        scene.shader_graphs = vec![graph.clone()];
        let background = std::env::var("BPE_BENCH_FOREGROUND").is_err();
        let mut renderer = Renderer::new(&device, 270, 480);
        renderer.set_effect_library(library.clone());
        renderer.set_effect_background_builds(background);
        let mut playback = Playback::new(&scene);
        playback.seek(2.0);
        let frame = FrameData::build(&scene, &playback, renderer.pixel_size(&scene));
        let mut previews = Renderer::new(&device, crate::effects::THUMB_SIZE.0, crate::effects::THUMB_SIZE.1);
        previews.set_effect_library(library.clone());
        previews.set_effect_background_builds(background);
        // Like the Shaders workspace: block previews are rebuilt when the graph changes.
        let mut cached: Option<(super::Graph, Arc<EffectInfo>)> = None;
        let mut frame_cost = |scene: &Scene| {
            let start = std::time::Instant::now();
            library.sync_graphs(&scene.shader_graphs);
            let graph = &scene.shader_graphs[0];
            if cached.as_ref().is_none_or(|(g, _)| g != graph) {
                cached = Some((graph.clone(), Arc::new(EffectInfo::graph_thumbnails(graph))));
            }
            let info = cached.as_ref().unwrap().1.clone();
            let views = (1..=8).collect();
            previews.set_effect_thumbnails(Some(ThumbnailRequest { effect: 0, info, views }));
            let cpu = start.elapsed();
            renderer.render(&device, &queue, scene, 2.0, &frame);
            previews.render(&device, &queue, scene, 2.0, &frame);
            let handed_over = start.elapsed();
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            (cpu.as_secs_f64() * 1000.0, handed_over.as_secs_f64() * 1000.0)
        };
        frame_cost(&scene);
        let (_, still) = frame_cost(&scene);
        scene.shader_graphs[0].nodes[0].pos[0] += 10.0;
        let (moved_cpu, moved) = frame_cost(&scene);
        scene.shader_graphs[0] = super::tests::with_setting_changed(&scene.shader_graphs[0]);
        let (slid_cpu, slid) = frame_cost(&scene);
        println!(
            "{:<22} unchanged {still:6.1} ms | block moved {moved:6.1} ms ({moved_cpu:.1} cpu) | slider moved {slid:6.1} ms ({slid_cpu:.1} cpu)",
            graph.name
        );
    }
}

/// While a changed node effect builds on another thread, the preview keeps drawing the old
/// version; once it is built, the picture is exactly what a fresh renderer draws.
#[test]
fn changed_effects_build_in_the_background() {
    let Some((device, queue)) = gpu() else { return };
    let mut b = super::Builder::new("Mine");
    let hue = b.add("hue_shift");
    let out = b.add("output");
    b.wire(hue, "out", out, "color");
    let graph = b.finish();
    let mut scene = Scene { effects: vec![Effect::new(super::effect_id("Mine"))], ..Scene::default() };
    scene.shader_graphs = vec![graph];
    let mut playback = Playback::new(&scene);
    playback.seek(2.0);

    let draw = |renderer: &mut Renderer, scene: &Scene| {
        let frame = FrameData::build(scene, &playback, renderer.pixel_size(scene));
        renderer.reset_effects();
        renderer.render(&device, &queue, scene, 2.0, &frame);
        renderer.read_output(&device, &queue).unwrap()
    };
    let mut live = Renderer::new(&device, 108, 192);
    live.set_effect_background_builds(true);
    let old = draw(&mut live, &scene);
    assert!(!live.effects_building(), "a new effect is built straight away");

    scene.shader_graphs[0].nodes[0].params.insert("turns".into(), bpe_graph::Param::Number(0.6));
    let mut fresh = Renderer::new(&device, 108, 192);
    let new = draw(&mut fresh, &scene);
    assert_ne!(old, new, "the setting should change the picture");

    let mut frames = 0;
    let mut building = true;
    while building {
        let picture = draw(&mut live, &scene);
        building = live.effects_building();
        if building {
            assert_eq!(picture, old, "the old version stands in while the new one builds");
        }
        frames += 1;
        assert!(frames < 2000, "the build never finished");
    }
    assert_eq!(draw(&mut live, &scene), new);
    assert!(live.effect_gpu_errors().is_empty(), "{:?}", live.effect_gpu_errors());
}
