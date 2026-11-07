//! Particles part 6 in pictures: the Liquid layer's surface pass, Trails and Mirror on the
//! graphics card. Measured on the cloud's software GPU; the checks compare pixels with each other
//! and with the grey background, so they hold on any GPU. Without a graphics adapter they skip.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/liquid-*.png`.

use bpe_core::{Choice, Color, Gradient, LayerId, Params, Registry, Scene, Settings, SlotId, ThingId, layered_key};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::paint::Paint;
use bpe_kit::things::light::{LightType, add_light};
use bpe_kit::things::particles::liquid::{LIQUID, LiquidLayer, Surface};
use bpe_kit::things::particles::look::{SPRITES, SpritesLayer};
use bpe_kit::things::particles::looks::{Look, add_particles};
use bpe_kit::things::particles::modifiers::{Mirror, Modifiers};
use bpe_kit::things::particles::trail::{TRAILS, TrailsLayer};
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

fn gpu() -> Option<Gpu> {
    let (device, queue) = headless_device().ok()?;
    let renderer = Renderer::new(&device, W, H);
    Some(Gpu { device, queue, renderer })
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

fn draw_at(gpu: &mut Gpu, scene: &Scene, t: f64, name: &str) -> Vec<u8> {
    let frame = frame_of(scene, t);
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    if gpu.renderer.programs_waiting() {
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("liquid-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    draw_at(gpu, scene, 0.2, name)
}

/// The pixel at world point (x, y): the video is 1080 × 1920 round the origin.
fn at(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
    let px = (((x / 1080.0 + 0.5) * W as f32) as usize).min(W as usize - 1);
    let py = (((y / 1920.0 + 0.5) * H as f32) as usize).min(H as usize - 1);
    let i = (py * W as usize + px) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// How far a pixel is from the grey background.
fn off_grey(p: [u8; 4]) -> i32 {
    (0..3).map(|c| (p[c] as i32 - 0x80).abs()).max().unwrap()
}

/// The summed difference of two pictures over the square of side `half` × 2 round (x, y).
fn differ(a: &[u8], b: &[u8], x: f32, y: f32, half: f32) -> u32 {
    let mut sum = 0;
    let mut k = -half;
    while k <= half {
        let mut j = -half;
        while j <= half {
            let (p, q) = (at(a, x + j, y + k), at(b, x + j, y + k));
            sum += (0..3).map(|c| (p[c] as i32 - q[c] as i32).unsigned_abs()).sum::<u32>();
            j += 4.0;
        }
        k += 4.0;
    }
    sum
}

fn reg() -> Registry {
    kit().registry
}

fn set_background(scene: &mut Scene, paint: Paint) {
    let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
    let mut s = SceneSettings::from_params(&root.params);
    s.background = paint;
    root.params = s.to_params();
}

/// Black and white stripes running up the picture.
fn stripes() -> Paint {
    let (k, w) = (Color::rgb(0x10, 0x10, 0x10), Color::rgb(0xf0, 0xf0, 0xf0));
    Paint {
        colours: Gradient::even(&[k, w, k, w, k, w, k, w, k, w, k, w, k, w, k, w, k]),
        angle: 0.0,
        span: 1080.0,
        ..Paint::default()
    }
}

/// A blank project with a Particles thing at `place` giving `count` particles at the start,
/// moving at `speed` straight down (no forces: the Empty look has none).
fn particles_at(place: Vec2, count: u32, speed: f32) -> (Scene, ThingId) {
    let reg = reg();
    let mut scene = new_studio_scene(&reg);
    let id = add_particles(&mut scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count,
        life: 10.0,
        life_spread: 0.0,
        speed,
        speed_spread: 0.0,
        angle: 90.0,
        spread: 360.0,
        ..ParticlesSettings::default()
    };
    let thing = scene.things.get_mut(&id).unwrap();
    thing.params = source.to_params();
    thing.transform.position = place;
    (scene, id)
}

/// Sets the Look's only layer to `kind` with `params`.
fn only_layer(scene: &mut Scene, id: ThingId, kind: &str, params: Params) -> LayerId {
    let reg = reg();
    let old: Vec<LayerId> = scene.things[&id].slots[&SlotId::Look].layers.iter().map(|l| l.id).collect();
    let layer = scene.add_layer(&reg, id, SlotId::Look, kind).unwrap();
    for l in old {
        scene.remove_layer(&reg, id, SlotId::Look, l).unwrap();
    }
    let card = scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    for (k, v) in params.0 {
        card.params.0.insert(layered_key(layer, &k), v);
    }
    layer
}

/// A crowd of red particles round the middle as one liquid, shaded `surface`.
fn puddle(surface: Surface, bend: f32) -> Scene {
    let (mut scene, id) = particles_at(Vec2::ZERO, 60, 100.0);
    let layer = LiquidLayer {
        surface,
        bend,
        size: 4.0,
        colour: Paint::solid(Color::rgb(0xe0, 0x30, 0x20)),
        ..LiquidLayer::default()
    };
    only_layer(&mut scene, id, LIQUID, layer.to_params());
    scene
}

#[test]
fn a_liquid_fills_where_particles_crowd_and_leaves_the_rest() {
    let Some(mut gpu) = gpu() else { return };
    let scene = puddle(Surface::Matte, 0.0);
    let frame = frame_of(&scene, 0.2);
    assert_eq!(frame.liquids.len(), 1, "one liquid in the frame");
    let pixels = draw(&mut gpu, &scene, "matte");
    let middle = at(&pixels, 0.0, 0.0);
    assert!(off_grey(middle) > 40, "the middle is liquid: {middle:?}");
    assert!(middle[0] as i32 > middle[2] as i32 + 30, "and red: {middle:?}");
    for (x, y) in [(400.0, -800.0), (-400.0, 800.0), (450.0, 0.0)] {
        let far = at(&pixels, x, y);
        assert!(off_grey(far) <= 2, "far from it, the background: {far:?} at ({x}, {y})");
    }
}

#[test]
fn every_surface_draws_and_differs_from_matte() {
    let Some(mut gpu) = gpu() else { return };
    let matte = draw(&mut gpu, &puddle(Surface::Matte, 0.0), "surface-matte");
    for surface in [Surface::Plastic, Surface::Chrome, Surface::Metal, Surface::Glass, Surface::Toon] {
        let name = format!("surface-{}", surface.name());
        let pixels = draw(&mut gpu, &puddle(surface, 12.0), &name);
        let d = differ(&matte, &pixels, 0.0, 0.0, 60.0);
        assert!(d > 500, "{} looks like Matte ({d})", surface.name());
        let far = at(&pixels, 400.0, -800.0);
        assert!(off_grey(far) <= 2, "{} leaves the background: {far:?}", surface.name());
    }
}

#[test]
fn glass_bends_the_picture_behind_it() {
    let Some(mut gpu) = gpu() else { return };
    let mut straight = puddle(Surface::Glass, 0.0);
    set_background(&mut straight, stripes());
    let mut bent = puddle(Surface::Glass, 40.0);
    set_background(&mut bent, stripes());
    let a = draw(&mut gpu, &straight, "glass-bend-0");
    let b = draw(&mut gpu, &bent, "glass-bend-40");
    assert!(differ(&a, &b, 0.0, 0.0, 80.0) > 2000, "Bend moves the stripes seen through it");
    // Far from it both are the bare stripes.
    assert_eq!(differ(&a, &b, 400.0, -800.0, 40.0), 0);
}

#[test]
fn trails_draw_behind_moving_particles() {
    let Some(mut gpu) = gpu() else { return };
    // Particles falling straight down from above the middle.
    let (mut scene, id) = particles_at(Vec2::new(0.0, -400.0), 1, 1000.0);
    let source = ParticlesSettings::from_params(&scene.things[&id].params);
    scene.things.get_mut(&id).unwrap().params = ParticlesSettings { spread: 0.0, ..source }.to_params();
    let layer = TrailsLayer {
        length: 30,
        every: 1,
        width: 6.0,
        taper: 0.0,
        fade_along: 0.0,
        colour: Paint::solid(Color::rgb(0xff, 0xff, 0xff)),
        ..TrailsLayer::default()
    };
    only_layer(&mut scene, id, TRAILS, layer.to_params());
    // At 0.2 s the particle is 200 px down (y = −200); its last 30 places at 1/120 s reach 250 px back.
    let pixels = draw_at(&mut gpu, &scene, 0.2, "trails");
    let behind = at(&pixels, 0.0, -300.0);
    assert!(off_grey(behind) > 40, "the trail shows behind the particle: {behind:?}");
    let ahead = at(&pixels, 0.0, 100.0);
    assert!(off_grey(ahead) <= 2, "nothing ahead of it: {ahead:?}");
}

#[test]
fn mirror_four_ways_repeats_the_particles_round_the_middle() {
    let Some(mut gpu) = gpu() else { return };
    let (mut scene, id) = particles_at(Vec2::new(250.0, -350.0), 20, 0.0);
    let mut params =
        SpritesLayer { size: 8.0, colour: Paint::solid(Color::rgb(0xff, 0xff, 0x40)), ..SpritesLayer::default() }
            .to_params();
    params.0.extend(Modifiers { mirror: Mirror::FourWays, ..Modifiers::default() }.to_params().0);
    only_layer(&mut scene, id, SPRITES, params);
    let pixels = draw(&mut gpu, &scene, "mirror-four");
    // (250, −350) turned by quarter turns round the middle of the picture.
    for (x, y) in [(250.0, -350.0), (350.0, 250.0), (-250.0, 350.0), (-350.0, -250.0)] {
        let p = at(&pixels, x, y);
        assert!(off_grey(p) > 40, "a copy at ({x}, {y}): {p:?}");
    }
    let between = at(&pixels, 0.0, 0.0);
    assert!(off_grey(between) <= 2, "nothing in the middle: {between:?}");
}

#[test]
fn a_liquid_in_a_lit_scene_draws_without_trouble() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = puddle(Surface::Glass, 20.0);
    let reg = reg();
    add_light(&mut scene, &reg, LightType::Point, ThingId::ROOT).unwrap();
    assert!(scene.card(ThingId::ROOT, SlotId::Lighting).is_some(), "the scene has a Lighting card");
    let frame = frame_of(&scene, 0.2);
    assert!(frame.light.is_some(), "a lit frame");
    let pixels = draw(&mut gpu, &scene, "lit");
    let middle = at(&pixels, 0.0, 0.0);
    let far = at(&pixels, 400.0, -800.0);
    assert!((0..3).any(|c| (middle[c] as i32 - far[c] as i32).abs() > 20), "the liquid shows: {middle:?} vs {far:?}");
}
