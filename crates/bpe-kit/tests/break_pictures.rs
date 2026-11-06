//! Rules part 3 in pictures: a shape broken by a rule fading out, popping and shattering, its
//! pieces wearing its own look. Measured on the cloud's software GPU; the checks compare pixels
//! with each other, so they hold on any GPU. Skipped without a graphics adapter.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/break-*.png`.

use bpe_core::{Color, Gradient, Scene, Settings, SlotId, Stop, ThingId, Value, Who, layered_key};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::{ShapeFill, ShapeSettings, add_thing, kit, new_studio_scene};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;
/// The rule breaks the shape at 0.1 s: the action applies on step 25.
const BROKE: f64 = 25.0 / 240.0;

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
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
    frame
}

fn draw(gpu: &mut Gpu, scene: &Scene, t: f64, name: &str) -> Vec<u8> {
    let frame = frame_of(scene, t);
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    if gpu.renderer.programs_waiting() {
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("break-{name}.png"))).unwrap();
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

/// How far a pixel's colour is from `from`'s.
fn off(p: [u8; 4], from: [u8; 4]) -> u32 {
    (0..3).map(|k| (p[k] as i32 - from[k] as i32).unsigned_abs()).sum()
}

/// Every pixel's world place and colour.
fn pixels_of(pixels: &[u8]) -> impl Iterator<Item = (f32, f32, [u8; 4])> + '_ {
    (0..(W * H) as usize).map(move |i| {
        let (px, py) = ((i % W as usize) as f32 + 0.5, (i / W as usize) as f32 + 0.5);
        let x = (px / W as f32 - 0.5) * 1080.0;
        let y = (py / H as f32 - 0.5) * 1920.0;
        (x, y, [pixels[i * 4], pixels[i * 4 + 1], pixels[i * 4 + 2], pixels[i * 4 + 3]])
    })
}

/// A blank project with a filled 400 × 400 square in the middle, red on its left half and blue on
/// its right, broken at 0.1 s by a rule with `look` and its settings.
fn square_broken(look: &str, more: &[(&str, Value)]) -> Scene {
    let reg = kit().registry;
    let mut scene = new_studio_scene(&reg);
    let shape = add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
    let mut s = ShapeSettings::rectangle(400.0, 400.0, 3.0);
    s.fill = ShapeFill::Filled;
    scene.things.get_mut(&shape).unwrap().params = s.to_params();
    let paint = scene.add_layer(&reg, shape, SlotId::Look, bpe_kit::modules::look::shape::PAINT).unwrap();
    let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
    let halves = Gradient {
        stops: vec![Stop::new(0.0, red), Stop::new(0.5, red), Stop::new(0.5, blue), Stop::new(1.0, blue)],
        ..Gradient::solid(red)
    };
    let card = scene.card_mut(shape, SlotId::Look).unwrap();
    card.params.set(&layered_key(paint, "colours"), Value::Gradient(halves));
    card.params.set(&layered_key(paint, "direction"), Value::Number(0.0));
    let rule = scene.add_rule("Break it");
    let when = scene.add_block(&reg, rule, "when.time", None).unwrap();
    scene.set_block_param(&reg, rule, when, "at", Value::Number(0.1)).unwrap();
    let brk = scene.add_block(&reg, rule, "do.break", Some(when)).unwrap();
    let mut set =
        vec![("who", Value::Who(Who::Thing { thing: shape, ball: None })), ("look", Value::Choice(look.into()))];
    set.extend(more.iter().map(|(k, v)| (*k, v.clone())));
    for (k, v) in set {
        scene.set_block_param(&reg, rule, brk, k, v).unwrap();
    }
    scene
}

/// How many pixels inside the square's old place show it (not the background).
fn covered(pixels: &[u8], background: [u8; 4]) -> usize {
    pixels_of(pixels).filter(|(x, y, p)| x.abs() < 190.0 && y.abs() < 190.0 && off(*p, background) > 60).count()
}

#[test]
fn a_faded_square_is_dimmer_half_way_then_gone() {
    let Some(mut gpu) = gpu() else { return };
    let scene = square_broken("fade_out", &[("fade_time", Value::Number(1.0))]);
    let whole = draw(&mut gpu, &scene, 0.05, "fade-whole");
    let half = draw(&mut gpu, &scene, BROKE + 0.5, "fade-half");
    let gone = draw(&mut gpu, &scene, BROKE + 1.1, "fade-gone");
    let background = at(&gone, 0.0, 600.0);
    for (x, y) in [(-100.0, 0.0), (100.0, 50.0)] {
        let (w, h, g) =
            (off(at(&whole, x, y), background), off(at(&half, x, y), background), off(at(&gone, x, y), background));
        assert!(w > 200, "the square shows: {w}");
        assert!(h > 40 && h + 40 < w, "half faded: {h} of {w}");
        assert!(g <= 2, "then gone: {g}");
    }
}

#[test]
fn a_popped_square_swells_past_where_it_was() {
    let Some(mut gpu) = gpu() else { return };
    let scene = square_broken("pop", &[("pop_time", Value::Number(1.0)), ("swell", Value::Number(1.6))]);
    let whole = draw(&mut gpu, &scene, 0.05, "pop-whole");
    let mid = draw(&mut gpu, &scene, BROKE + 0.4, "pop-mid");
    let outside = (-250.0, 0.0);
    let background = at(&whole, 0.0, 600.0);
    assert!(off(at(&whole, outside.0, outside.1), background) <= 2, "not there before");
    assert!(off(at(&mid, outside.0, outside.1), background) > 30, "swelled out past its edge");
}

#[test]
fn a_shattered_square_breaks_into_pieces_that_keep_its_colours_and_fall() {
    let Some(mut gpu) = gpu() else { return };
    let scene = square_broken(
        "shatter",
        &[("pieces", Value::Int(16)), ("spread", Value::Number(250.0)), ("spin", Value::Number(90.0))],
    );
    let whole = draw(&mut gpu, &scene, 0.05, "shatter-whole");
    let soon = draw(&mut gpu, &scene, BROKE + 0.04, "shatter-soon");
    let later = draw(&mut gpu, &scene, BROKE + 0.6, "shatter-later");
    let background = at(&whole, 0.0, 600.0);
    let (c_whole, c_soon, c_later) =
        (covered(&whole, background), covered(&soon, background), covered(&later, background));
    assert!(
        c_soon as f32 > 0.75 * c_whole as f32,
        "just broken, the pieces are nearly where it was: {c_soon}/{c_whole}"
    );
    assert!((c_later as f32) < 0.6 * c_whole as f32, "later they have flown apart: {c_later}/{c_whole}");
    // Each piece wears its part of the picture: the left stays red, the right blue.
    let (mut red_left, mut blue_left, mut blue_right, mut red_right) = (0, 0, 0, 0);
    for (x, y, p) in pixels_of(&soon) {
        if y.abs() > 150.0 || x.abs() < 40.0 || x.abs() > 150.0 {
            continue;
        }
        let (r, b) = (p[0] as i32, p[2] as i32);
        match (x < 0.0, r > b + 60, b > r + 60) {
            (true, true, _) => red_left += 1,
            (true, _, true) => blue_left += 1,
            (false, _, true) => blue_right += 1,
            (false, true, _) => red_right += 1,
            _ => {}
        }
    }
    assert!(
        red_left > 10 * blue_left.max(1) && blue_right > 10 * red_right.max(1),
        "{red_left} {blue_left} {blue_right} {red_right}"
    );
    // They fall: the lit pixels' middle is lower later.
    let middle = |pixels: &[u8]| {
        let lit: Vec<f32> = pixels_of(pixels).filter(|(_, _, p)| off(*p, background) > 60).map(|(_, y, _)| y).collect();
        lit.iter().sum::<f32>() / lit.len().max(1) as f32
    };
    assert!(middle(&later) > middle(&soon) + 40.0, "{} then {}", middle(&soon), middle(&later));
}

#[test]
fn a_shattered_square_draws_the_same_however_it_got_there() {
    let Some(mut gpu) = gpu() else { return };
    let scene = square_broken("shatter", &[("pieces", Value::Int(10))]);
    let a = draw(&mut gpu, &scene, BROKE + 0.3, "same-a");
    let _ = draw(&mut gpu, &scene, BROKE + 0.9, "same-b");
    let b = draw(&mut gpu, &scene, BROKE + 0.3, "same-c");
    assert_eq!(a, b);
}

#[test]
fn a_shattered_ball_breaks_into_clipped_pieces() {
    let Some(mut gpu) = gpu() else { return };
    let reg = kit().registry;
    let mut scene = new_studio_scene(&reg);
    let ball = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&ball).unwrap();
    t.params.set("radius", Value::Number(160.0));
    t.params.set("velocity", Value::Vec2([0.0, 0.0]));
    let rule = scene.add_rule("Break it");
    let when = scene.add_block(&reg, rule, "when.time", None).unwrap();
    scene.set_block_param(&reg, rule, when, "at", Value::Number(0.1)).unwrap();
    let brk = scene.add_block(&reg, rule, "do.break", Some(when)).unwrap();
    for (k, v) in [
        ("who", Value::Who(Who::Thing { thing: ball, ball: None })),
        ("look", Value::Choice("shatter".into())),
        ("pieces", Value::Int(14)),
        ("spread", Value::Number(250.0)),
    ] {
        scene.set_block_param(&reg, rule, brk, k, v).unwrap();
    }
    let whole = draw(&mut gpu, &scene, 0.1, "ball-whole");
    let soon = draw(&mut gpu, &scene, BROKE + 0.03, "ball-soon");
    let later = draw(&mut gpu, &scene, BROKE + 0.6, "ball-later");
    let background = at(&whole, 0.0, 800.0);
    let place =
        |pixels: &[u8]| pixels_of(pixels).filter(|(x, y, p)| x.hypot(*y) < 150.0 && off(*p, background) > 60).count();
    let (w, s, l) = (place(&whole), place(&soon), place(&later));
    assert!(w > 1000, "the ball shows: {w}");
    assert!(s as f32 > 0.75 * w as f32, "just broken, nearly whole: {s}/{w}");
    assert!((l as f32) < 0.5 * w as f32, "then flown apart: {l}/{w}");
    let anywhere = |pixels: &[u8]| pixels_of(pixels).filter(|(_, _, p)| off(*p, background) > 60).count();
    assert!(anywhere(&later) as f32 > 0.4 * w as f32, "the pieces are still there: {}", anywhere(&later));
}
