//! Liquid part 3 in pictures: a Liquid thing's look. Clear water shows what is behind it near its
//! top and turns to its deep colour further down; light it gives off brightens it; foam whitens
//! it; under a lamp the real lights shade it. The checks compare pixels with each other, so they
//! hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/liquid-look-*.png`.

use bpe_core::{Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::swarm::{SwarmGpu, use_on_this_thread};
use bpe_gfx::{Frame, Renderer, build_frame, test_device};
use bpe_kit::kit;
use bpe_kit::modules::look::surface::SurfaceKind;
use bpe_kit::scenes::{liquid_look_scenes, liquid_scenes};
use bpe_kit::things::liquid::look::FOAM;
use bpe_kit::things::liquid::{KIND, LookSettings};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;

struct Gpu {
    device: bpe_gfx::wgpu::Device,
    queue: bpe_gfx::wgpu::Queue,
    renderer: Renderer,
}

fn gpu() -> Gpu {
    static CARD: std::sync::OnceLock<(bpe_gfx::wgpu::Device, bpe_gfx::wgpu::Queue)> = std::sync::OnceLock::new();
    let (device, queue) = CARD.get_or_init(|| test_device().expect("a graphics device")).clone();
    let renderer = Renderer::new(&device, W, H);
    Gpu { device, queue, renderer }
}

fn draw(gpu: &mut Gpu, scene: &Scene, t: f64, name: &str) -> Vec<u8> {
    // The liquid steps on the card the pictures are drawn on (named only while drawing, so
    // nothing of it is left to drop as the test's thread ends).
    use_on_this_thread(Some(SwarmGpu::for_device(&gpu.device, &gpu.queue)));
    let pixels = draw_named(gpu, scene, t, name);
    use_on_this_thread(None);
    pixels
}

fn draw_named(gpu: &mut Gpu, scene: &Scene, t: f64, name: &str) -> Vec<u8> {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
    assert_eq!(frame.liquids.len(), 1, "the liquid draws");
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    if gpu.renderer.programs_waiting() {
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("liquid-look-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// The mean colour of the box from (x0, y0) to (x1, y1) (world), sampled every 4 px: the video is
/// 1080 × 1920 round the origin.
fn mean(pixels: &[u8], (x0, y0): (f32, f32), (x1, y1): (f32, f32)) -> [f32; 3] {
    let (mut sum, mut n) = ([0.0f32; 3], 0.0);
    let mut y = y0;
    while y <= y1 {
        let mut x = x0;
        while x <= x1 {
            let px = (((x / 1080.0 + 0.5) * W as f32) as usize).min(W as usize - 1);
            let py = (((y / 1920.0 + 0.5) * H as f32) as usize).min(H as usize - 1);
            let i = (py * W as usize + px) * 4;
            for c in 0..3 {
                sum[c] += pixels[i + c] as f32;
            }
            n += 1.0;
            x += 4.0;
        }
        y += 4.0;
    }
    sum.map(|s| s / n)
}

fn bright(c: [f32; 3]) -> f32 {
    (c[0] + c[1] + c[2]) / 3.0
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn water() -> Scene {
    liquid_scenes(&bpe_kit::registry()).into_iter().find(|(n, _)| n == "Liquid water").unwrap().1
}

fn liquid(scene: &Scene) -> ThingId {
    scene.things.values().find(|t| t.kind == KIND).map(|t| t.id).unwrap()
}

fn set_look(scene: &mut Scene, edit: impl FnOnce(&mut LookSettings)) {
    let id = liquid(scene);
    let card = scene.card_mut(id, SlotId::Look).unwrap();
    let mut l = LookSettings::from_params(&card.params);
    edit(&mut l);
    for (k, v) in l.to_params().0 {
        card.params.0.insert(k, v);
    }
}

// The ring is 508 round the middle; the water stands from y ≈ 100 down to its bottom.
const SHALLOW: ((f32, f32), (f32, f32)) = ((-200.0, 150.0), (200.0, 190.0));
const DEEP: ((f32, f32), (f32, f32)) = ((-150.0, 420.0), (150.0, 470.0));
const ABOVE: ((f32, f32), (f32, f32)) = ((-200.0, -40.0), (-120.0, 0.0));

#[test]
fn clear_water_shows_what_is_behind_it_near_the_top_and_is_deeper_further_down() {
    let mut gpu = gpu();
    // Both Plastic (a clear Glass at Clarity 0 is all see-through), with no ripples.
    let plastic = |clarity: f32| {
        let mut s = water();
        let id = liquid(&s);
        bpe_kit::scenes::set_liquid_surface(&mut s, id, SurfaceKind::Plastic);
        set_look(&mut s, |l| {
            l.clarity = clarity;
            l.ripples = 0.0;
        });
        s
    };
    let solid_px = draw(&mut gpu, &plastic(0.0), 0.5, "solid");
    let clear = plastic(300.0);
    let clear_px = draw(&mut gpu, &clear, 0.5, "clear");
    let behind = mean(&clear_px, ABOVE.0, ABOVE.1);
    let (shallow, deep) = (mean(&clear_px, SHALLOW.0, SHALLOW.1), mean(&clear_px, DEEP.0, DEEP.1));
    let solid_shallow = mean(&solid_px, SHALLOW.0, SHALLOW.1);
    assert!(
        distance(shallow, behind) < distance(solid_shallow, behind),
        "clear water near its top is nearer what is behind: {shallow:?} vs {solid_shallow:?} (behind {behind:?})"
    );
    assert!(bright(deep) < bright(shallow), "deeper is darker: {deep:?} vs {shallow:?}");
}

#[test]
fn light_it_gives_off_brightens_it() {
    let mut gpu = gpu();
    let plain = draw(&mut gpu, &water(), 0.5, "plain");
    let mut glowing = water();
    set_look(&mut glowing, |l| l.glow = 2.0);
    let glow = draw(&mut gpu, &glowing, 0.5, "glow");
    let (a, b) = (mean(&plain, DEEP.0, DEEP.1), mean(&glow, DEEP.0, DEEP.1));
    assert!(bright(b) > bright(a) + 10.0, "{b:?} vs {a:?}");
}

#[test]
fn foam_whitens_it() {
    let mut gpu = gpu();
    let plain = draw(&mut gpu, &water(), 0.5, "no-foam");
    let mut foamy = water();
    let id = liquid(&foamy);
    let reg = bpe_kit::registry();
    let layer = foamy.add_layer(&reg, id, SlotId::Look, FOAM).unwrap();
    let card = foamy.card_mut(id, SlotId::Look).unwrap();
    // Every drop foams, all the way down.
    card.params.set(&layered_key(layer, "from_speed"), Value::Number(0.0));
    card.params.set(&layered_key(layer, "on_top"), Value::Number(2000.0));
    let foam = draw(&mut gpu, &foamy, 0.5, "foam");
    let (a, b) = (mean(&plain, SHALLOW.0, SHALLOW.1), mean(&foam, SHALLOW.0, SHALLOW.1));
    assert!(bright(b) > bright(a) + 20.0, "{b:?} vs {a:?}");
    // Whiter: its channels closer together.
    let spread = |c: [f32; 3]| c.iter().cloned().fold(f32::MIN, f32::max) - c.iter().cloned().fold(f32::MAX, f32::min);
    assert!(spread(b) < spread(a), "{b:?} vs {a:?}");
}

#[test]
fn under_a_lamp_the_real_lights_shade_it_and_every_layer_draws() {
    let mut gpu = gpu();
    let scenes: Vec<(String, Scene)> = liquid_look_scenes(&bpe_kit::registry());
    let lit = scenes.iter().find(|(n, _)| n == "Liquid lit pool").unwrap().1.clone();
    let lit_px = draw(&mut gpu, &lit, 1.0, "lit");
    let mut own = lit.clone();
    set_look(&mut own, |l| l.lit = 0.0);
    let own_px = draw(&mut gpu, &own, 1.0, "lit-zero");
    let (a, b) = (mean(&lit_px, SHALLOW.0, SHALLOW.1), mean(&own_px, SHALLOW.0, SHALLOW.1));
    assert!(distance(a, b) > 3.0, "the real lights change it: {a:?} vs {b:?}");
    assert!(bright(a) > 5.0, "lit, not black: {a:?}");
    // Every layer on, its own light, and the glowing scene: drawn without trouble.
    for name in ["Liquid looks", "Liquid glow"] {
        let scene = scenes.iter().find(|(n, _)| n == name).unwrap().1.clone();
        let px = draw(&mut gpu, &scene, 1.5, &name.to_lowercase().replace(' ', "-"));
        assert!(bright(mean(&px, DEEP.0, DEEP.1)) > 5.0, "{name} draws its liquid");
    }
}
