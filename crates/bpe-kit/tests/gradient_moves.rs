//! Gradients part 3 in pictures, on the software GPU: a background gradient that moves (Cycle,
//! Step on the beat, Hue spin) drawn at moments in any order gives the same picture for the same
//! moment, as preview and export both draw through `build_frame`, and what it draws is the moved
//! gradient's own sampling.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/gradient-moves-*.png`.

use bpe_core::{Color, Gradient, Mix, Moves, Scene, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device, wgpu};
use bpe_kit::{Kit, kit, new_scene};
use bpe_runtime::Playback;

const W: u32 = 135;
const H: u32 = 240;
const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };
const GREEN: Color = Color { r: 0, g: 255, b: 0, a: 255 };
const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };

/// Draws `scene` at `time` seconds, with a renderer of its own.
fn draw(device: &wgpu::Device, queue: &wgpu::Queue, kit: &Kit, scene: &Scene, time: f64, name: &str) -> Vec<u8> {
    let resolved = bpe_kit::resolve_scene(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    let mut renderer = Renderer::new(device, W, H);
    renderer.render(device, queue, &frame);
    let pixels = renderer.read_output(device, queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("gradient-moves-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

/// The new scene with nothing in it but `colours` across the background, left to right, with
/// these companions on it.
fn across(kit: &Kit, colours: Gradient, companions: &[(&str, f64)]) -> Scene {
    let mut scene = new_scene(&kit.registry);
    scene.things.get_mut(&ThingId(1)).unwrap().enabled = false;
    scene.things.get_mut(&ThingId(2)).unwrap().enabled = false;
    set(&mut scene, "bloom", Value::Number(0.0));
    set(&mut scene, "background_angle", Value::Number(0.0));
    set(&mut scene, "background_span", Value::Number(1080.0));
    set(&mut scene, "background_colours", Value::Gradient(colours));
    for (name, x) in companions {
        set(&mut scene, &format!("background_colours~{name}"), Value::Number(*x));
    }
    scene
}

fn set(scene: &mut Scene, key: &str, value: Value) {
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set(key, value);
}

fn px(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

/// How far a drawn colour may sit from the gradient's own sampling (the output rolls bright
/// channels off a little).
const CLOSE: u8 = 12;

fn off(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap()
}

/// What `g` moved by `moves` samples at column `x`, as 8-bit sRGB.
fn sampled(g: &Gradient, moves: Moves, x: u32) -> [u8; 3] {
    let g = Gradient { moves, ..g.clone() };
    let c = Color::from_linear(g.sample((x as f32 + 0.5) / W as f32));
    [c.r, c.g, c.b]
}

fn rainbow() -> Gradient {
    Gradient::spread(&[RED, GREEN, BLUE], Mix::Even, true)
}

#[test]
fn a_cycling_gradient_is_the_same_at_the_same_moment_whatever_was_drawn_before() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let scene = across(&kit, rainbow(), &[("cycle", 0.25)]);
    let start = draw(&device, &queue, &kit, &scene, 0.0, "cycle-0");
    let first = draw(&device, &queue, &kit, &scene, 1.0, "cycle-1");
    let _later = draw(&device, &queue, &kit, &scene, 3.0, "cycle-3");
    let again = draw(&device, &queue, &kit, &scene, 1.0, "cycle-1-again");
    assert_eq!(first, again, "scrubbing back gives the same picture");
    assert_ne!(first, start, "it moves");
    // A second in, the colours have slid a quarter of the way along.
    for x in [2, W / 4, W / 2, 3 * W / 4, W - 3] {
        let want = sampled(&rainbow(), Moves { shift: 0.25, ..Moves::default() }, x);
        assert!(off(px(&first, x, H / 2), want) <= CLOSE, "column {x}: {:?} vs {want:?}", px(&first, x, H / 2));
    }
}

#[test]
fn steps_jump_on_the_beat() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    // 120 BPM: a step every half second, with no glide.
    let scene = across(&kit, rainbow(), &[("step_every", 1.0), ("step_glide", 0.0)]);
    let start = draw(&device, &queue, &kit, &scene, 0.0, "step-0");
    assert_eq!(draw(&device, &queue, &kit, &scene, 0.4, "step-0.4"), start, "no step before the first beat");
    let stepped = draw(&device, &queue, &kit, &scene, 0.75, "step-0.75");
    for x in [2, W / 2, W - 3] {
        let want = sampled(&rainbow(), Moves { shift: 1.0 / 3.0, ..Moves::default() }, x);
        assert!(off(px(&stepped, x, H / 2), want) <= CLOSE, "column {x}");
    }
}

#[test]
fn hue_spin_turns_a_single_colour() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let red = draw(&device, &queue, &kit, &across(&kit, Gradient::solid(RED), &[]), 0.0, "red");
    let turned = draw(&device, &queue, &kit, &across(&kit, Gradient::solid(RED), &[("hue_turn", 180.0)]), 0.0, "cyan");
    let (r, t) = (px(&red, W / 2, H / 2), px(&turned, W / 2, H / 2));
    assert!(r[0] > 200 && r[1] < 40, "{r:?}");
    assert!(t[0] < 120 && t[1] > 120 && t[2] > 120, "half way round from red is cyan-ish: {t:?}");
}

/// A ball dropped inside a shape, its Body (painted across the ball) a red-to-blue gradient with
/// a white Hit wave of `strength`.
fn dropped_ball(kit: &Kit, strength: f64) -> (Scene, ThingId) {
    let mut scene = new_scene(&kit.registry);
    set(&mut scene, "bloom", Value::Number(0.0));
    bpe_kit::add_thing(&mut scene, &kit.registry, "shape", ThingId::ROOT).unwrap();
    let ball = bpe_kit::add_thing(&mut scene, &kit.registry, "balls", ThingId::ROOT).unwrap();
    let look = scene.things.get_mut(&ball).unwrap().slots.get_mut(&bpe_core::SlotId::Look).unwrap();
    let body = look.layers[0].id;
    let colours = bpe_core::layered_key(body, "colours");
    look.params.set(&colours, Value::Gradient(Gradient::spread(&[RED, BLUE], Mix::Even, false)));
    look.params.set(&format!("{colours}~wave"), Value::Number(strength));
    look.params.set(&format!("{colours}~wave_width"), Value::Number(0.4));
    look.params.set(&format!("{colours}~wave_speed"), Value::Number(2.0));
    (scene, ball)
}

#[test]
fn a_ball_s_hit_sends_a_wave_along_its_colours() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let (waved, ball) = dropped_ball(&kit, 1.0);
    let (plain, _) = dropped_ball(&kit, 0.0);
    // When the ball first hits something.
    let resolved = bpe_kit::resolve_scene(&waved, &kit.registry);
    let mut playback = Playback::new(&waved, &resolved, kit.sims.clone());
    playback.seek(8.0);
    let rate = playback.world().steps_per_second;
    let hit = playback
        .log()
        .events()
        .iter()
        .find(|e| e.kind == bpe_runtime::EventKind::Hit && (e.a.thing == ball || e.b.is_some_and(|b| b.thing == ball)))
        .map(|e| e.time(rate))
        .expect("the ball hits the shape");
    let before = hit - 0.1;
    assert_eq!(
        draw(&device, &queue, &kit, &waved, before, "wave-before"),
        draw(&device, &queue, &kit, &plain, before, "plain-before"),
        "no wave before the first hit"
    );
    // A quarter of a second on, the front is half way across the ball.
    let after = hit + 0.25;
    let (w, p) = (
        draw(&device, &queue, &kit, &waved, after, "wave-after"),
        draw(&device, &queue, &kit, &plain, after, "plain-after"),
    );
    let brighter = w.chunks(4).zip(p.chunks(4)).filter(|(a, b)| a[1] as i32 > b[1] as i32 + 40).count();
    assert!(brighter > 20, "the white band shows on the ball: {brighter} pixels");
}
