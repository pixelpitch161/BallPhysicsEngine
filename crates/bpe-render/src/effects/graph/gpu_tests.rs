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
