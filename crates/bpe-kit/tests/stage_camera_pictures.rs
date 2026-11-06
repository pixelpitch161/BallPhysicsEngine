//! Lighting part 5 in pictures: the Stage camera (Orbit, Tilt, Height, Lens) seeing the lit stage
//! again from where it moved. Measured on the cloud's software GPU; the checks compare pixels with
//! each other, so they hold on any GPU.
//!
//! `BPE_SAVE_RENDERS=1` saves the pictures into `target/pictures/camera-*.png`.

use bpe_core::{Registry, Scene, SlotId, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device};
use bpe_kit::{kit, scenes};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;

struct Gpu {
    device: bpe_gfx::wgpu::Device,
    queue: bpe_gfx::wgpu::Queue,
    renderer: Renderer,
}

fn gpu() -> Option<Gpu> {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    let renderer = Renderer::new(&device, W, H);
    Some(Gpu { device, queue, renderer })
}

fn reg() -> Registry {
    kit().registry
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

fn draw(gpu: &mut Gpu, scene: &Scene, name: &str) -> Vec<u8> {
    let frame = frame_of(scene, 0.5);
    gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    if gpu.renderer.programs_waiting() {
        gpu.renderer.render(&gpu.device, &gpu.queue, &frame);
    }
    let pixels = gpu.renderer.read_output(&gpu.device, &gpu.queue).unwrap();
    if std::env::var_os("BPE_SAVE_RENDERS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("camera-{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
    }
    pixels
}

fn set_lighting(scene: &mut Scene, key: &str, v: Value) {
    scene.things.get_mut(&ThingId::ROOT).unwrap().slots.get_mut(&SlotId::Lighting).unwrap().params.set(key, v);
}

fn set_camera(scene: &mut Scene, key: &str, v: Value) {
    let camera = bpe_kit::ensure_camera(scene, &reg());
    scene.things.get_mut(&camera).unwrap().params.set(key, v);
}

/// The stage demo with its Stage camera on (still at rest).
fn stage() -> Scene {
    let mut scene = scenes::stage_demo(&reg());
    set_camera(&mut scene, "stage_camera", Value::Bool(true));
    scene
}

/// The mean of |a − b| over every channel, in steps of 1/255.
fn mean_difference(a: &[u8], b: &[u8]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as f64).sum::<f64>() / a.len() as f64
}

/// How much of the picture is near black (every channel under 8).
fn black_share(p: &[u8]) -> f64 {
    p.chunks(4).filter(|c| c[0] < 8 && c[1] < 8 && c[2] < 8).count() as f64 / (p.len() / 4) as f64
}

#[test]
fn the_camera_reaches_the_frame_only_in_stage() {
    let mut scene = stage();
    set_camera(&mut scene, "orbit", Value::Number(20.0));
    let view = frame_of(&scene, 0.5).stage_view().expect("a Stage scene with its camera on");
    assert!(view.motion > 0.99, "{}", view.motion);
    assert!(view.fill > 1.0, "zooms in to keep the edges filled: {}", view.fill);
    set_camera(&mut scene, "stage_camera", Value::Bool(false));
    assert!(frame_of(&scene, 0.5).stage_view().is_none());
    set_camera(&mut scene, "stage_camera", Value::Bool(true));
    set_lighting(&mut scene, "depth", Value::Choice("flat".into()));
    assert!(frame_of(&scene, 0.5).stage_view().is_none(), "Flat has no depth to swing round");
}

#[test]
fn a_camera_at_rest_draws_the_front_picture_exactly() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = scenes::stage_demo(&reg());
    let off = draw(&mut gpu, &scene, "off");
    scene = stage();
    let rest = draw(&mut gpu, &scene, "rest");
    assert_eq!(off, rest);
    // A flat scene ignores the camera however far it is swung.
    set_lighting(&mut scene, "depth", Value::Choice("flat".into()));
    let flat = draw(&mut gpu, &scene, "flat");
    set_camera(&mut scene, "orbit", Value::Number(30.0));
    assert_eq!(flat, draw(&mut gpu, &scene, "flat-orbit"));
}

#[test]
fn a_tiny_move_is_close_to_the_front_picture() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = stage();
    let rest = draw(&mut gpu, &scene, "rest");
    set_camera(&mut scene, "orbit", Value::Number(0.05));
    let tiny = draw(&mut gpu, &scene, "tiny");
    let d = mean_difference(&rest, &tiny);
    assert!(d < 2.0, "a 0.05° orbit moved the picture by {d:.2} a channel on average");
}

#[test]
fn orbit_tilt_and_height_each_move_the_picture_and_leave_no_holes() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = stage();
    let rest = draw(&mut gpu, &scene, "rest");
    let holes = black_share(&rest);
    for (key, value, name) in
        [("orbit", 25.0, "orbit"), ("tilt", 18.0, "tilt"), ("stage_height", 300.0, "height"), ("tilt", -15.0, "under")]
    {
        let mut moved = scene.clone();
        set_camera(&mut moved, key, Value::Number(value));
        let pixels = draw(&mut gpu, &moved, name);
        let d = mean_difference(&rest, &pixels);
        assert!(d > 1.0, "{name}: the picture hardly moved ({d:.2})");
        let black = black_share(&pixels);
        assert!(black <= holes + 0.01, "{name}: {:.1}% black, {:.1}% at rest", black * 100.0, holes * 100.0);
    }
    // A wide lens and a long one see the same swing differently.
    set_camera(&mut scene, "orbit", Value::Number(25.0));
    set_camera(&mut scene, "lens", Value::Number(18.0));
    let wide = draw(&mut gpu, &scene, "wide");
    set_camera(&mut scene, "lens", Value::Number(150.0));
    let long = draw(&mut gpu, &scene, "long");
    assert!(mean_difference(&wide, &long) > 1.0);
}

#[test]
fn stretched_edges_keep_the_framing_and_draft_still_draws() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = stage();
    set_camera(&mut scene, "orbit", Value::Number(30.0));
    let zoomed = draw(&mut gpu, &scene, "zoomed");
    set_camera(&mut scene, "fill_edges", Value::Choice("stretch".into()));
    let stretched = draw(&mut gpu, &scene, "stretched");
    assert!(mean_difference(&zoomed, &stretched) > 1.0);
    set_lighting(&mut scene, "quality", Value::Choice("draft".into()));
    let draft = draw(&mut gpu, &scene, "draft");
    assert!(black_share(&draft) < 0.5);
}

#[test]
fn wild_settings_still_draw_a_picture() {
    let Some(mut gpu) = gpu() else { return };
    let mut scene = stage();
    for (key, value) in [("orbit", 89.0), ("tilt", -89.0), ("lens", 1.0), ("stage_height", 1e7), ("pivot", -1e6)] {
        set_camera(&mut scene, key, Value::Number(value));
    }
    let pixels = draw(&mut gpu, &scene, "wild");
    assert_eq!(pixels.len(), (W * H * 4) as usize);
    set_camera(&mut scene, "lens", Value::Number(1e9));
    let _ = draw(&mut gpu, &scene, "wild-long");
}
