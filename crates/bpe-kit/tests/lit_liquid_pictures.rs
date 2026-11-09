//! W15 in pictures: in a scene with a Lighting card a Liquid and Soft smoke are shaded by the
//! real lights (a lamp lights the side facing it, in its colour; a Liquid shines and shows what is
//! behind it through its Surface, and can cast a shadow), and Lit 0 keeps their own look. The
//! checks compare pixels with each other, so they hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/lit-liquid-*.png`.

use bpe_core::{Color, Gradient, LayerId, Params, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::{Frame, Renderer, build_frame, test_device};
use bpe_kit::modules::look::surface::SurfaceKind;
use bpe_kit::paint::Paint;
use bpe_kit::things::light::{LightSettings, LightType, add_light};
use bpe_kit::things::particles::liquid::{LIQUID, LiquidLayer, params_of};
use bpe_kit::things::particles::look::{SMOKE, SmokeLayer};
use bpe_kit::things::particles::looks::{Look, add_particles};
use bpe_kit::things::particles::{BornFrom, Emit, ParticlesSettings};
use bpe_kit::{SceneSettings, kit, new_studio_scene};
use bpe_runtime::Playback;
use glam::Vec2;

const W: u32 = 270;
const H: u32 = 480;

struct Gpu {
    device: bpe_gfx::wgpu::Device,
    queue: bpe_gfx::wgpu::Queue,
    renderer: Renderer,
}

fn gpu() -> Gpu {
    let (device, queue) = test_device().expect("a graphics device");
    let renderer = Renderer::new(&device, W, H);
    Gpu { device, queue, renderer }
}

fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    let t = 0.5;
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    if gpu.renderer.programs_waiting() {
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("lit-liquid-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// The pixel at world point (x, y): the video is 1080 × 1920 round the origin.
fn at(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
    let px = (((x / 1080.0 + 0.5) * W as f32) as usize).min(W as usize - 1);
    let py = (((y / 1920.0 + 0.5) * H as f32) as usize).min(H as usize - 1);
    let i = (py * W as usize + px) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn bright(p: [u8; 4]) -> f32 {
    (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0
}

/// The mean brightness of the box from (x0, y0) to (x1, y1) (world), sampled every 4 px.
fn mean(pixels: &[u8], (x0, y0): (f32, f32), (x1, y1): (f32, f32)) -> f32 {
    let (mut sum, mut n) = (0.0, 0.0);
    let mut y = y0;
    while y <= y1 {
        let mut x = x0;
        while x <= x1 {
            sum += bright(at(pixels, x, y));
            n += 1.0;
            x += 4.0;
        }
        y += 4.0;
    }
    sum / n
}

/// How much the brightness changes along the row at `y` from `x0` to `x1`: the mean step between
/// pixels 4 px apart.
fn busy(pixels: &[u8], y: f32, x0: f32, x1: f32) -> f32 {
    let (mut sum, mut n) = (0.0, 0.0);
    let mut x = x0;
    while x + 4.0 <= x1 {
        sum += (bright(at(pixels, x, y)) - bright(at(pixels, x + 4.0, y))).abs();
        n += 1.0;
        x += 4.0;
    }
    sum / n
}

fn set_background(scene: &mut Scene, paint: Paint) {
    let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
    let mut s = SceneSettings::from_params(&root.params);
    s.background = paint;
    root.params = s.to_params();
}

/// Black and white stripes running up the picture, 60 px wide.
fn stripes() -> Paint {
    let (k, w) = (Color::rgb(0x10, 0x10, 0x10), Color::rgb(0xf0, 0xf0, 0xf0));
    let colours: Vec<Color> = (0..19).map(|i| if i % 2 == 0 { k } else { w }).collect();
    Paint { colours: Gradient::even(&colours), angle: 0.0, span: 1080.0, ..Paint::default() }
}

/// A blank lit project: one Point light at `lamp` (low over the picture), coloured `colour`, and
/// a Particles thing whose only layer is `kind` with `params`, a still crowd round the middle.
fn lit_scene(lamp: Option<(Vec2, Color)>, kind: &str, params: Params) -> (Scene, ThingId, LayerId) {
    let kit = kit();
    let reg = &kit.registry;
    let mut scene = new_studio_scene(reg);
    if let Some((place, colour)) = lamp {
        let light = add_light(&mut scene, reg, LightType::Point, ThingId::ROOT).unwrap();
        let mut s = LightSettings::of_kind(LightType::Point);
        s.position = place;
        s.colour = colour;
        s.height = 40.0;
        s.reach = 2000.0;
        s.size = 6.0;
        s.strength = 1.0;
        scene.things.get_mut(&light).unwrap().params = s.to_params();
    }
    let id = add_particles(&mut scene, reg, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::AnArea,
        emit: Emit::OnceAtTheStart,
        count: 600,
        speed: 0.0,
        life: 100.0,
        size: 6.0,
        size_spread: 0.0,
        width: 240.0,
        height: 240.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = source.to_params();
    let old: Vec<LayerId> = scene.things[&id].slots[&SlotId::Look].layers.iter().map(|l| l.id).collect();
    let layer = scene.add_layer(reg, id, SlotId::Look, kind).unwrap();
    for l in old {
        scene.remove_layer(reg, id, SlotId::Look, l).unwrap();
    }
    set_layer(&mut scene, id, layer, params);
    (scene, id, layer)
}

fn set_layer(scene: &mut Scene, id: ThingId, layer: LayerId, params: Params) {
    let card = scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    for (k, v) in params.0 {
        card.params.0.insert(layered_key(layer, &k), v);
    }
}

fn liquid(surface: SurfaceKind) -> LiquidLayer {
    LiquidLayer {
        surface,
        roundness: 40.0,
        colour: Paint::solid(Color::rgb(0x90, 0x90, 0x90)),
        ..LiquidLayer::default()
    }
}

const WHITE: Color = Color::rgb(255, 255, 255);
const LEFT: Vec2 = Vec2::new(-420.0, 0.0);
const RIGHT: Vec2 = Vec2::new(420.0, 0.0);

/// The liquid's left and right edges (inside its rounded rim) and its middle.
const LEFT_EDGE: ((f32, f32), (f32, f32)) = ((-150.0, -40.0), (-110.0, 40.0));
const RIGHT_EDGE: ((f32, f32), (f32, f32)) = ((110.0, -40.0), (150.0, 40.0));

#[test]
fn a_lamp_lights_the_side_of_a_liquid_facing_it() {
    let mut gpu = gpu();
    let params = params_of(&liquid(SurfaceKind::Matte));
    let (left, ..) = lit_scene(Some((LEFT, WHITE)), LIQUID, params.clone());
    let (right, ..) = lit_scene(Some((RIGHT, WHITE)), LIQUID, params);
    let a = draw(&mut gpu, &left, "lamp-left");
    let b = draw(&mut gpu, &right, "lamp-right");
    let (al, ar) = (mean(&a, LEFT_EDGE.0, LEFT_EDGE.1), mean(&a, RIGHT_EDGE.0, RIGHT_EDGE.1));
    let (bl, br) = (mean(&b, LEFT_EDGE.0, LEFT_EDGE.1), mean(&b, RIGHT_EDGE.0, RIGHT_EDGE.1));
    assert!(al > ar + 10.0, "lamp on the left: the left edge is brighter ({al} against {ar})");
    assert!(br > bl + 10.0, "lamp on the right: the right edge is brighter ({br} against {bl})");
}

#[test]
fn a_liquid_takes_the_lamps_colour() {
    let mut gpu = gpu();
    let (scene, ..) = lit_scene(Some((LEFT, Color::rgb(255, 40, 40))), LIQUID, params_of(&liquid(SurfaceKind::Matte)));
    let pixels = draw(&mut gpu, &scene, "red-lamp");
    let p = at(&pixels, -60.0, 0.0);
    assert!(p[0] as i32 > p[1] as i32 + 30, "a white liquid under a red lamp is red: {p:?}");
}

#[test]
fn lit_0_keeps_a_liquids_own_look_whatever_the_lamps_do() {
    let mut gpu = gpu();
    let mut params = params_of(&liquid(SurfaceKind::Plastic));
    params.set("lit", Value::Number(0.0));
    let (left, ..) = lit_scene(Some((LEFT, WHITE)), LIQUID, params.clone());
    let (right, ..) = lit_scene(Some((RIGHT, WHITE)), LIQUID, params);
    let a = draw(&mut gpu, &left, "lit0-left");
    let b = draw(&mut gpu, &right, "lit0-right");
    for (x, y) in [(-70.0, 0.0), (70.0, 0.0), (0.0, 0.0), (0.0, -70.0)] {
        let (p, q) = (at(&a, x, y), at(&b, x, y));
        assert!((0..3).all(|c| (p[c] as i32 - q[c] as i32).abs() <= 1), "the same at ({x}, {y}): {p:?} {q:?}");
    }
}

#[test]
fn plastic_shines_where_matte_doesnt() {
    let mut gpu = gpu();
    let (matte, ..) = lit_scene(Some((LEFT, WHITE)), LIQUID, params_of(&liquid(SurfaceKind::Matte)));
    let (plastic, ..) = lit_scene(Some((LEFT, WHITE)), LIQUID, params_of(&liquid(SurfaceKind::Plastic)));
    let a = draw(&mut gpu, &matte, "matte");
    let b = draw(&mut gpu, &plastic, "plastic");
    let top = |pixels: &[u8]| {
        let mut most: f32 = 0.0;
        let mut y = -150.0;
        while y <= 150.0 {
            let mut x = -150.0;
            while x <= 150.0 {
                most = most.max(bright(at(pixels, x, y)));
                x += 4.0;
            }
            y += 4.0;
        }
        most
    };
    let (m, p) = (top(&a), top(&b));
    assert!(p > m + 15.0, "Plastic's highlight ({p}) is brighter than anything on Matte ({m})");
}

#[test]
fn lit_glass_liquid_shows_the_stripes_behind_it() {
    let mut gpu = gpu();
    let (mut glass, ..) = lit_scene(Some((LEFT, WHITE)), LIQUID, params_of(&liquid(SurfaceKind::Glass)));
    set_background(&mut glass, stripes());
    let (mut plastic, ..) = lit_scene(Some((LEFT, WHITE)), LIQUID, params_of(&liquid(SurfaceKind::Plastic)));
    set_background(&mut plastic, stripes());
    let a = draw(&mut gpu, &glass, "glass-stripes");
    let b = draw(&mut gpu, &plastic, "plastic-stripes");
    let (g, p) = (busy(&a, 20.0, -80.0, 80.0), busy(&b, 20.0, -80.0, 80.0));
    assert!(g > p * 2.0 + 2.0, "the stripes show through glass ({g}) and not through plastic ({p})");
}

#[test]
fn a_liquid_that_blocks_light_casts_a_shadow() {
    let mut gpu = gpu();
    let grey = Paint::solid(Color::rgb(0x90, 0x90, 0x90));
    let (mut open, id, layer) = lit_scene(Some((LEFT, WHITE)), LIQUID, params_of(&liquid(SurfaceKind::Matte)));
    set_background(&mut open, grey.clone());
    let mut blocking = open.clone();
    let mut p = Params::default();
    p.set("blocks_light", Value::Bool(true));
    set_layer(&mut blocking, id, layer, p);
    let a = draw(&mut gpu, &open, "no-shadow");
    let b = draw(&mut gpu, &blocking, "shadow");
    let behind = ((200.0, -30.0), (300.0, 30.0));
    let (lit, shaded) = (mean(&a, behind.0, behind.1), mean(&b, behind.0, behind.1));
    assert!(shaded + 10.0 < lit, "behind the liquid from the lamp is in its shadow ({shaded} against {lit})");
}

#[test]
fn a_lamp_lights_the_side_of_smoke_facing_it() {
    let mut gpu = gpu();
    let smoke = SmokeLayer {
        shading: 1.0,
        detail: 0.0,
        dark_inside: 0.0,
        opacity: 1.0,
        colour: Paint::solid(Color::rgb(0xd0, 0xd0, 0xd0)),
        ..SmokeLayer::default()
    };
    let (left, ..) = lit_scene(Some((LEFT, WHITE)), SMOKE, smoke.to_params());
    let (right, ..) = lit_scene(Some((RIGHT, WHITE)), SMOKE, smoke.to_params());
    let a = draw(&mut gpu, &left, "smoke-left");
    let b = draw(&mut gpu, &right, "smoke-right");
    let edge = |x0: f32, x1: f32| ((x0, -40.0), (x1, 40.0));
    let (al, ar) = (
        mean(&a, edge(-170.0, -120.0).0, edge(-170.0, -120.0).1),
        mean(&a, edge(120.0, 170.0).0, edge(120.0, 170.0).1),
    );
    let (bl, br) = (
        mean(&b, edge(-170.0, -120.0).0, edge(-170.0, -120.0).1),
        mean(&b, edge(120.0, 170.0).0, edge(120.0, 170.0).1),
    );
    // Its own light comes from the upper left; the real lamp wins either way.
    assert!(al - ar > (bl - br) + 10.0, "the lamp's side is brighter: left lamp {al} / {ar}, right lamp {bl} / {br}");
    assert!(br > bl, "lamp on the right: the right side is brighter ({br} against {bl})");
}
