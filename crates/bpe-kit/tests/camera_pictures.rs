//! Picture tests for the camera: the world drawn through it, and things stuck to the screen.

use bpe_core::{Scene, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device, wgpu};
use bpe_kit::{Kit, ensure_camera, kit, new_scene, scenes};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;
const BALLS: ThingId = ThingId(2);

fn draw(device: &wgpu::Device, queue: &wgpu::Queue, kit: &Kit, scene: &Scene, time: f64) -> (Frame, Vec<u8>) {
    let resolved = bpe_kit::resolve_scene(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    let mut renderer = Renderer::new(device, W, H);
    renderer.render(device, queue, &frame);
    (frame, renderer.read_output(device, queue).unwrap())
}

/// One still ball of `radius` at `at`, no arena, no gravity, and the camera set by `camera`.
fn still_ball(kit: &Kit, at: [f32; 2], radius: f64, camera: &[(&str, Value)]) -> Scene {
    let mut scene = new_scene(&kit.registry);
    scene.things.get_mut(&ThingId(1)).unwrap().enabled = false;
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
    let ball = scene.things.get_mut(&BALLS).unwrap();
    ball.transform.position = at.into();
    ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
    ball.params.set("radius", Value::Number(radius));
    let cam = ensure_camera(&mut scene, &kit.registry);
    for (key, value) in camera {
        scene.things.get_mut(&cam).unwrap().params.set(key, value.clone());
    }
    scene
}

fn lit(pixels: &[u8]) -> Vec<(u32, u32)> {
    pixels
        .chunks(4)
        .enumerate()
        .filter(|(_, p)| p[0] > 120 || p[1] > 120 || p[2] > 120)
        .map(|(i, _)| (i as u32 % W, i as u32 / W))
        .collect()
}

fn middle(points: &[(u32, u32)]) -> (f32, f32) {
    let n = points.len().max(1) as f32;
    let (x, y) = points.iter().fold((0.0, 0.0), |(x, y), p| (x + p.0 as f32, y + p.1 as f32));
    (x / n, y / n)
}

#[test]
fn no_camera_draws_like_before() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    for (name, scene) in scenes::stage2(&kit.registry) {
        let mut with = scene.clone();
        ensure_camera(&mut with, &kit.registry);
        let (_, a) = draw(&device, &queue, &kit, &scene, 1.5);
        let (_, b) = draw(&device, &queue, &kit, &with, 1.5);
        assert!(a == b, "{name}: a camera at rest changed the picture");
    }
}

#[test]
fn moved_camera_moves_the_ball() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let scene = still_ball(&kit, [300.0, 0.0], 40.0, &[("position", Value::Vec2([300.0, 0.0]))]);
    let (_, pixels) = draw(&device, &queue, &kit, &scene, 0.1);
    let (x, y) = middle(&lit(&pixels));
    assert!((x - W as f32 / 2.0).abs() < 2.0 && (y - H as f32 / 2.0).abs() < 2.0, "{x}, {y}");
}

#[test]
fn zoom_two_doubles_size() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let one = draw(&device, &queue, &kit, &still_ball(&kit, [0.0, 0.0], 50.0, &[]), 0.1).1;
    let two = draw(&device, &queue, &kit, &still_ball(&kit, [0.0, 0.0], 50.0, &[("zoom", Value::Number(2.0))]), 0.1).1;
    let ratio = lit(&two).len() as f32 / lit(&one).len() as f32;
    assert!((3.6..=4.4).contains(&ratio), "{ratio}");
}

#[test]
fn stuck_text_stays() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let make = |camera: [f32; 2]| {
        let mut scene = still_ball(&kit, [5000.0, 5000.0], 10.0, &[("position", Value::Vec2(camera))]);
        let text = scene.add_thing(&kit.registry, "text", ThingId::ROOT).unwrap();
        scene.things.get_mut(&text).unwrap().params.set("text", Value::Text("HELLO".into()));
        scene
    };
    let still = draw(&device, &queue, &kit, &make([0.0, 0.0]), 0.5).1;
    let moved = draw(&device, &queue, &kit, &make([5000.0, 5000.0]), 0.5).1;
    let (a, b) = (lit(&still), lit(&moved));
    assert!(!a.is_empty());
    // The ball comes into the moved shot; the caption's pixels are where they were.
    assert!(a.iter().all(|p| b.contains(p)), "the caption moved with the camera");
}

#[test]
fn culled_ball_is_not_in_frame() {
    let kit = kit();
    let scene = still_ball(&kit, [0.0, 0.0], 20.0, &[("position", Value::Vec2([10000.0, 0.0]))]);
    let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
    let playback = Playback::new(&scene, &resolved, kit.sims.clone());
    let mut frame = Frame::default();
    build_frame(&scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
    assert!(frame.balls.is_empty() && frame.looks.is_empty(), "{:?}", frame.balls);
}
