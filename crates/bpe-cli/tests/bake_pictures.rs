//! Lighting part 3 in pictures: the bake (averaged samples, motion blur, the Stage path tracer's
//! bounce light) and glass caustics. Measured on the cloud's software GPU; the checks compare
//! pixels with each other, so they hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/bake-*.png`.

use bpe_cli::FrameMaker;
use bpe_core::{Choice, Color, Gradient, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_gfx::bake::Bake;
use bpe_gfx::{Renderer, headless_device};
use bpe_kit::modules::look::surface::{SurfaceKind, preset_values};
use bpe_kit::paint::Paint;
use bpe_kit::{SceneSettings, kit, scenes};

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

fn save(name: &str, pixels: &[u8]) {
    if std::env::var_os("BPE_SAVE_RENDERS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
    std::fs::create_dir_all(&dir).unwrap();
    let file = std::fs::File::create(dir.join(format!("bake-{name}.png"))).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
}

/// The scene at `t`, live (`bake` None) or baked, as the export draws it.
fn draw(gpu: &mut Gpu, scene: &Scene, t: f64, bake: Option<Bake>, name: &str) -> Vec<u8> {
    let kit = kit();
    for _ in 0..2 {
        let mut maker = FrameMaker::new(scene, &kit);
        let samples = maker.samples(t, 30, bake.as_ref());
        gpu.renderer.render_samples(&gpu.device, &gpu.queue, &samples);
        if !gpu.renderer.programs_waiting() {
            break;
        }
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    save(name, &pixels);
    pixels
}

/// The pixel at world point (x, y): the video is 1080 × 1920 round the origin.
fn at(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
    let px = (((x / 1080.0 + 0.5) * W as f32) as usize).min(W as usize - 1);
    let py = (((y / 1920.0 + 0.5) * H as f32) as usize).min(H as usize - 1);
    let i = (py * W as usize + px) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

fn brightness(p: [u8; 4]) -> i32 {
    p[0] as i32 + p[1] as i32 + p[2] as i32
}

fn mean_brightness(pixels: &[u8]) -> f64 {
    pixels.chunks(4).map(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) as f64).sum::<f64>() / (W * H) as f64
}

fn thing_of(scene: &Scene, kind: &str) -> ThingId {
    scene.things.values().find(|t| t.kind == kind).map(|t| t.id).unwrap()
}

fn set_lighting(scene: &mut Scene, key: &str, v: Value) {
    scene.card_mut(ThingId::ROOT, SlotId::Lighting).unwrap().params.set(key, v);
}

fn set_light(scene: &mut Scene, key: &str, v: Value) {
    let light = thing_of(scene, "light");
    scene.things.get_mut(&light).unwrap().params.set(key, v);
}

fn set_colour(scene: &mut Scene, ball: ThingId, colour: Color) {
    let card = scene.card_mut(ball, SlotId::Look).unwrap();
    let body = card.layers.iter().find(|l| l.kind == "layer.body").unwrap().id;
    card.params.set(&layered_key(body, "colours"), Value::Gradient(Gradient::even(&[colour, colour])));
}

fn set_surface(scene: &mut Scene, thing: ThingId, kind: SurfaceKind) {
    let card = scene.card_mut(thing, SlotId::Look).unwrap();
    card.params.set("surface", Value::Choice(kind.name().into()));
    for (k, v) in preset_values(kind) {
        card.params.set(k, v);
    }
}

/// The lit room with no gravity: one ball of radius `radius` at `at` moving at `velocity`, its
/// Body alone, a lamp up and in front, the room's shape out of the way.
fn room(stage: bool, at: [f32; 2], radius: f32, velocity: [f32; 2]) -> Scene {
    let reg = kit().registry;
    let mut scene = scenes::lit_room(&reg);
    let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
    let mut s = SceneSettings::from_params(&root.params);
    s.gravity = glam::Vec2::ZERO;
    s.fps = 30;
    root.params = s.to_params();
    set_lighting(&mut scene, "depth", Value::Choice(if stage { "stage" } else { "flat" }.into()));
    set_lighting(&mut scene, "floor", Value::Choice("none".into()));
    set_lighting(&mut scene, "ambient", Value::Number(0.3));
    set_light(&mut scene, "position", Value::Vec2([0.0, -400.0]));
    set_light(&mut scene, "height", Value::Number(300.0));
    set_light(&mut scene, "reach", Value::Number(3000.0));
    let ball = thing_of(&scene, "balls");
    let t = scene.things.get_mut(&ball).unwrap();
    t.params.set("radius", Value::Number(radius as f64));
    t.params.set("velocity", Value::Vec2(velocity));
    t.transform.position = glam::Vec2::from(at);
    let look = scene.card_mut(ball, SlotId::Look).unwrap();
    look.layers.retain(|l| l.kind == "layer.body");
    let shape = thing_of(&scene, "shape");
    scene.things.get_mut(&shape).unwrap().params.set("size", Value::Number(3000.0));
    scene
}

fn bake(samples: u32, shutter: f32) -> Option<Bake> {
    Some(Bake { samples, shutter })
}

#[test]
fn one_live_sample_is_the_live_picture() {
    let Some(mut gpu) = gpu() else { return };
    let scene = room(false, [0.0, 0.0], 120.0, [0.0, 0.0]);
    let kit = kit();
    let frame = FrameMaker::new(&scene, &kit).frame(0.0);
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    let live = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    gpu.renderer.render_samples(&gpu.device, &gpu.queue, std::slice::from_ref(&frame));
    let again = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    assert!(live == again);
}

#[test]
fn a_still_baked_frame_keeps_its_light_and_bakes_the_same_twice() {
    let Some(mut gpu) = gpu() else { return };
    let scene = room(false, [0.0, 0.0], 120.0, [0.0, 0.0]);
    let live = draw(&mut gpu, &scene, 0.0, None, "still-live");
    let baked = draw(&mut gpu, &scene, 0.0, bake(8, 0.0), "still-baked");
    let again = draw(&mut gpu, &scene, 0.0, bake(8, 0.0), "still-baked-again");
    let (a, b) = (mean_brightness(&live), mean_brightness(&baked));
    assert!((a - b).abs() / a < 0.02, "live {a}, baked {b}");
    assert!(live != baked, "the samples moved within the pixel");
    assert!(baked == again, "the same bake twice is the same picture");
}

/// How many pixels along the row (or column) through world point `c` show the pink ball (its
/// shadow and the wall left out).
fn extent(pixels: &[u8], c: [f32; 2], across: bool) -> usize {
    (-200..=200)
        .filter(|k| {
            let d = *k as f32 * 4.0;
            let p = if across { at(pixels, c[0] + d, c[1]) } else { at(pixels, c[0], c[1] + d) };
            p[0] as i32 - p[1] as i32 > 40
        })
        .count()
}

#[test]
fn motion_blur_smears_a_fast_ball_along_its_way() {
    let Some(mut gpu) = gpu() else { return };
    let scene = room(false, [-300.0, 0.0], 60.0, [3000.0, 0.0]);
    let t = 0.2;
    // The ball is at x = -300 + 3000 t at the frame's moment.
    let c = [-300.0 + 3000.0 * t as f32, 0.0];
    let sharp = draw(&mut gpu, &scene, t, bake(12, 0.0), "blur-off");
    let (w0, h0) = (extent(&sharp, c, true), extent(&sharp, c, false));
    let blurred = draw(&mut gpu, &scene, t, bake(12, 1.0), "blur-on");
    let (w1, h1) = (extent(&blurred, [c[0] - 50.0, 0.0], true), extent(&blurred, c, false));
    assert!((w0 as i32 - h0 as i32).abs() <= 3, "sharp: {w0} across, {h0} down");
    assert!(w1 > h1 + 6, "blurred: {w1} across, {h1} down");
    assert!(w1 > w0 + 6, "longer than the sharp ball: {w1} against {w0}");
}

/// The brightest pixel in the box from (x0, y0) to (x1, y1) (world).
fn brightest(pixels: &[u8], (x0, y0): (f32, f32), (x1, y1): (f32, f32)) -> i32 {
    let mut best = 0;
    let mut y = y0;
    while y <= y1 {
        let mut x = x0;
        while x <= x1 {
            best = best.max(brightness(at(pixels, x, y)));
            x += 4.0;
        }
        y += 4.0;
    }
    best
}

fn glass_room(stage: bool) -> Scene {
    let mut scene = room(stage, [0.0, 0.0], 100.0, [0.0, 0.0]);
    let ball = thing_of(&scene, "balls");
    set_colour(&mut scene, ball, Color::rgb(0xff, 0xff, 0xff));
    set_surface(&mut scene, ball, SurfaceKind::Glass);
    set_lighting(&mut scene, "ambient", Value::Number(0.15));
    set_light(&mut scene, "strength", Value::Number(1.5));
    let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint::solid(Color::rgb(0x60, 0x60, 0x60));
    root.params = s.to_params();
    scene
}

#[test]
fn a_glass_ball_focuses_light_into_a_bright_spot() {
    let Some(mut gpu) = gpu() else { return };
    for stage in [true, false] {
        let name = if stage { "stage" } else { "flat" };
        let mut scene = glass_room(stage);
        // The lamp is up and in front: the light goes down and back through the ball.
        let below = ((-200.0, 110.0), (200.0, 700.0));
        let on = draw(&mut gpu, &scene, 0.0, None, &format!("caustics-{name}"));
        set_lighting(&mut scene, "glass_caustics", Value::Number(0.0));
        let off = draw(&mut gpu, &scene, 0.0, None, &format!("caustics-{name}-off"));
        let (b_on, b_off) = (brightest(&on, below.0, below.1), brightest(&off, below.0, below.1));
        assert!(b_on > b_off + 40, "{name}: with caustics {b_on}, without {b_off}");
    }
}

#[test]
fn rainbow_split_splits_the_caustic_into_colours() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = glass_room(true);
    let ball = thing_of(&scene, "balls");
    scene.card_mut(ball, SlotId::Look).unwrap().params.set("rainbow", Value::Number(3.0));
    // Red focuses on the wall; the bluer light, bent more, crosses sooner and spreads round it.
    set_light(&mut scene, "strength", Value::Number(5.0));
    let pixels = draw(&mut gpu, &scene, 0.0, bake(8, 0.0), "rainbow");
    set_lighting(&mut scene, "glass_caustics", Value::Number(0.0));
    let without = draw(&mut gpu, &scene, 0.0, bake(8, 0.0), "rainbow-off");
    let (mut red, mut blue) = ((0, (0.0, 0.0)), (0, (0.0, 0.0)));
    let mut y = 110.0;
    while y <= 700.0 {
        let mut x = -200.0;
        while x <= 200.0 {
            let (p, q) = (at(&pixels, x, y), at(&without, x, y));
            let rb = (p[0] as i32 - q[0] as i32) - (p[2] as i32 - q[2] as i32);
            if rb > red.0 {
                red = (rb, (x, y));
            }
            if -rb > blue.0 {
                blue = (-rb, (x, y));
            }
            x += 4.0;
        }
        y += 4.0;
    }
    assert!(red.0 > 24 && blue.0 > 8, "red {red:?}, blue {blue:?}");
    let apart = (red.1.0 - blue.1.0).hypot(red.1.1 - blue.1.1);
    assert!(apart > 20.0, "red at {:?}, blue at {:?}", red.1, blue.1);
}

#[test]
fn the_stage_bake_bounces_a_red_balls_light_onto_the_wall() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = room(true, [0.0, 0.0], 160.0, [0.0, 0.0]);
    let ball = thing_of(&scene, "balls");
    set_colour(&mut scene, ball, Color::rgb(0xff, 0x10, 0x10));
    set_lighting(&mut scene, "wall_depth", Value::Number(200.0));
    set_lighting(&mut scene, "ambient", Value::Number(0.1));
    // From the right and close to the wall: the ball's lit side shines on the wall beside it.
    set_light(&mut scene, "position", Value::Vec2([700.0, 0.0]));
    set_light(&mut scene, "height", Value::Number(150.0));
    let root = scene.things.get_mut(&ThingId::ROOT).unwrap();
    let mut s = SceneSettings::from_params(&root.params);
    s.background = Paint::solid(Color::rgb(0xc0, 0xc0, 0xc0));
    root.params = s.to_params();
    let redness = |p: [u8; 4]| p[0] as i32 - (p[1] as i32 + p[2] as i32) / 2;
    let beside = |pixels: &[u8]| {
        (0..8).map(|k| redness(at(pixels, 190.0 + k as f32 * 8.0, -60.0 + k as f32 * 16.0))).sum::<i32>()
    };
    set_lighting(&mut scene, "bounce", Value::Number(0.0));
    let without = draw(&mut gpu, &scene, 0.0, bake(4, 0.0), "bounce-off");
    set_lighting(&mut scene, "bounce", Value::Number(1.0));
    let with = draw(&mut gpu, &scene, 0.0, bake(4, 0.0), "bounce-on");
    let (r0, r1) = (beside(&without), beside(&with));
    assert!(r1 > r0 + 8 * 4, "redness beside the ball: {r0} without bounce, {r1} with");
}

#[test]
fn the_export_bakes_as_the_lighting_card_says_unless_told() {
    let kit = kit();
    let demo = scenes::bake_demo(&kit.registry);
    let bake = bpe_cli::bake_for(&demo, &kit, None).expect("the demo bakes");
    assert_eq!((bake.samples, bake.shutter), (16, 0.5));
    assert_eq!(bpe_cli::bake_for(&demo, &kit, Some(false)), None, "Live in the export window");
    let plain = room(false, [0.0, 0.0], 100.0, [0.0, 0.0]);
    assert_eq!(bpe_cli::bake_for(&plain, &kit, None), None, "Same as live by default");
    assert!(bpe_cli::bake_for(&plain, &kit, Some(true)).is_some(), "Bake in the export window");
    let blank = bpe_kit::new_scene(&kit.registry);
    assert_eq!(bpe_cli::bake_for(&blank, &kit, Some(true)), None, "no Lighting card, nothing to bake");
}
