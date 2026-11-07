//! Particles part 5 in pictures: each new Look layer of a Particles thing drawn from a blank
//! project. Measured on the cloud's software GPU; the checks compare pixels with each other, so
//! they hold on any GPU. Skipped without a graphics adapter.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/particles-*.png`.

use bpe_core::{Params, Scene, Settings, SlotId, ThingId, layered_key};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::things::particles::look::{
    FLAME, FlameLayer, GLITTER, GlitterLayer, MoreShape, SHAPES, SMOKE, ShapesLayer, SmokeLayer, SpritesLayer, WEB,
    WebLayer,
};
use bpe_kit::things::particles::looks::{Look, add_particles};
use bpe_kit::things::particles::{BornFrom, Emit, Heading, ParticlesSettings};
use bpe_kit::{kit, new_studio_scene};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;

struct Gpu {
    device: bpe_gfx::wgpu::Device,
    queue: bpe_gfx::wgpu::Queue,
    renderer: Renderer,
}

fn gpu() -> Option<Gpu> {
    let (device, queue) = headless_device().ok()?;
    let renderer = Renderer::new(&device, W, H);
    Some(Gpu { device, queue, renderer })
}

/// A blank project with 200 particles bursting from the middle, the Empty look's layer hidden
/// and one layer of `kind` with `params`.
fn scene_with(kind: &str, params: Params) -> Scene {
    scene_moving(kind, params, 90.0)
}

/// As [`scene_with`], the particles setting off at about `speed` px/s.
fn scene_moving(kind: &str, params: Params, speed: f32) -> Scene {
    let k = kit();
    let mut scene = new_studio_scene(&k.registry);
    let id = add_particles(&mut scene, &k.registry, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count: 200,
        life: 10.0,
        speed,
        speed_spread: 0.8,
        heading: Heading::AllRound,
        size: 6.0,
        spin: 90.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = source.to_params();
    let first = scene.things[&id].slots[&SlotId::Look].layers[0].id;
    let layer = scene.add_layer(&k.registry, id, SlotId::Look, kind).unwrap();
    let hidden = SpritesLayer { opacity: 0.0, ..SpritesLayer::default() }.to_params();
    let card = scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    for (l, p) in [(first, hidden), (layer, params)] {
        for (key, v) in p.0 {
            card.params.0.insert(layered_key(l, &key), v);
        }
    }
    scene
}

fn frame_of(scene: &Scene, t: f64) -> Frame {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
    frame
}

fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    let frame = frame_of(scene, 1.0);
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("particles-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// How far a pixel's colour is from `from`'s.
fn off(p: [u8; 4], from: [u8; 4]) -> u32 {
    (0..3).map(|k| (p[k] as i32 - from[k] as i32).unsigned_abs()).sum()
}

/// The pixel at world point (x, y): the video is 1080 × 1920 round the origin.
fn at(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
    let px = (((x / 1080.0 + 0.5) * W as f32) as usize).min(W as usize - 1);
    let py = (((y / 1920.0 + 0.5) * H as f32) as usize).min(H as usize - 1);
    let i = (py * W as usize + px) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// How many pixels within `r` of the middle differ from the far corner's colour, and how many
/// far from it do.
fn changed(pixels: &[u8]) -> (usize, usize) {
    let bg = at(pixels, 500.0, 900.0);
    let (mut near, mut far) = (0, 0);
    for py in 0..H {
        for px in 0..W {
            let x = (px as f32 + 0.5) / W as f32 * 1080.0 - 540.0;
            let y = (py as f32 + 0.5) / H as f32 * 1920.0 - 960.0;
            let i = ((py * W + px) * 4) as usize;
            let p = [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]];
            let d = (x * x + y * y).sqrt();
            if off(p, bg) > 12 {
                if d < 260.0 {
                    near += 1;
                } else if d > 420.0 {
                    far += 1;
                }
            }
        }
    }
    (near, far)
}

#[test]
fn each_new_layer_draws_round_its_particles_and_nowhere_else() {
    let Some(mut gpu) = gpu() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let cases: Vec<(&str, &str, Params)> = vec![
        ("smoke", SMOKE, SmokeLayer::default().to_params()),
        ("flame", FLAME, FlameLayer::default().to_params()),
        ("leaves", SHAPES, ShapesLayer::default().to_params()),
        ("birds", SHAPES, ShapesLayer { shape: MoreShape::Birds, ..ShapesLayer::default() }.to_params()),
        ("pixels", SHAPES, ShapesLayer { shape: MoreShape::Pixels, ..ShapesLayer::default() }.to_params()),
        ("glitter", GLITTER, GlitterLayer::default().to_params()),
        ("web", WEB, WebLayer::default().to_params()),
    ];
    for (name, kind, params) in cases {
        let px = draw(&mut gpu, &scene_with(kind, params), name);
        let (near, far) = changed(&px);
        assert!(near > 300, "{name} shows round the particles: {near}");
        assert_eq!(far, 0, "{name} shows nothing far away");
    }
}

#[test]
fn a_flame_is_brighter_in_its_heart_than_at_its_edge() {
    let Some(mut gpu) = gpu() else { return };
    let params = FlameLayer { flicker: 0.0, detail: 0.0, ..FlameLayer::default() }.to_params();
    let px = draw(&mut gpu, &scene_moving(FLAME, params, 30.0), "flame-heart");
    // The mean brightness of the pixels between `a` and `b` from the middle.
    let mean = |a: f32, b: f32| {
        let (mut sum, mut n) = (0u32, 0u32);
        for py in 0..H {
            for px_ in 0..W {
                let x = (px_ as f32 + 0.5) / W as f32 * 1080.0 - 540.0;
                let y = (py as f32 + 0.5) / H as f32 * 1920.0 - 960.0;
                let d = (x * x + y * y).sqrt();
                if d >= a && d < b {
                    let i = ((py * W + px_) * 4) as usize;
                    sum += px[i] as u32 + px[i + 1] as u32 + px[i + 2] as u32;
                    n += 1;
                }
            }
        }
        sum / n.max(1)
    };
    let (heart, edge) = (mean(0.0, 30.0), mean(60.0, 90.0));
    assert!(heart > edge + 60, "{heart} {edge}");
}
