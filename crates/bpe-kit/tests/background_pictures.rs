//! Picture tests for the scene's background paint on the software GPU.

use bpe_core::{Color, Gradient, Scene, ThingId, Value};
use bpe_gfx::{Frame, Renderer, build_frame, headless_device, wgpu};
use bpe_kit::{Kit, kit, new_scene};
use bpe_runtime::Playback;

const W: u32 = 270;
const H: u32 = 480;

fn draw(device: &wgpu::Device, queue: &wgpu::Queue, kit: &Kit, scene: &Scene, time: f64) -> Vec<u8> {
    let resolved = bpe_kit::resolve_scene(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    let mut renderer = Renderer::new(device, W, H);
    renderer.render(device, queue, &frame);
    renderer.read_output(device, queue).unwrap()
}

/// The new scene with nothing in it but the background.
fn empty(kit: &Kit) -> Scene {
    let mut scene = new_scene(&kit.registry);
    scene.things.get_mut(&ThingId(1)).unwrap().enabled = false;
    scene.things.get_mut(&ThingId(2)).unwrap().enabled = false;
    scene
}

fn write(name: &str, pixels: &[u8]) {
    let file = std::fs::File::create(name).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
}

fn saved(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name);
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height, info.color_type), (W, H, png::ColorType::Rgba));
    pixels.truncate(info.buffer_size());
    pixels
}

#[test]
fn default_background_matches_old_top_bottom() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let pixels = draw(&device, &queue, &kit, &empty(&kit), 0.0);
    if let Some(path) = std::env::var_os("BPE_SAVE_BACKGROUND") {
        write(path.to_str().unwrap(), &pixels);
        return;
    }
    // Drawn by the commit before the background became a paint (top and bottom colours).
    let before = saved("default-background.png");
    let worst = pixels.iter().zip(&before).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    println!(
        "worst difference {worst}, {} of {} bytes differ",
        pixels.iter().zip(&before).filter(|(a, b)| a != b).count(),
        pixels.len()
    );
    assert!(worst <= 1, "the default background is {worst}/255 off the old picture");
}

fn px(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * W + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

fn set(scene: &mut Scene, key: &str, value: Value) {
    scene.things.get_mut(&ThingId::ROOT).unwrap().params.set(key, value);
}

fn red_blue() -> Value {
    Value::Gradient(Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]))
}

#[test]
fn the_old_picture_is_not_flat() {
    let before = saved("default-background.png");
    assert!(px(&before, 135, 0)[2] > px(&before, 135, H - 1)[2] + 10, "bluer at the top");
}

#[test]
fn the_background_runs_at_any_angle_or_from_the_centre() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut scene = empty(&kit);
    set(&mut scene, "bloom", Value::Number(0.0));
    set(&mut scene, "background_colours", red_blue());
    // Across the picture from left to right: red on the left, blue on the right.
    set(&mut scene, "background_angle", Value::Number(0.0));
    set(&mut scene, "background_span", Value::Number(1080.0));
    let sideways = draw(&device, &queue, &kit, &scene, 0.0);
    let (left, right) = (px(&sideways, 2, 240), px(&sideways, W - 3, 240));
    assert!(left[0] > left[2] + 60 && right[2] > right[0] + 60, "{left:?} {right:?}");
    assert_eq!(px(&sideways, 135, 5), px(&sideways, 135, H - 5), "nothing changes top to bottom");
    // From the centre outward: red in the middle, blue at the corners.
    set(&mut scene, "background_shape", Value::Choice("centre".into()));
    set(&mut scene, "background_span", Value::Number(400.0));
    let rings = draw(&device, &queue, &kit, &scene, 0.0);
    let (middle, corner) = (px(&rings, 135, 240), px(&rings, 2, 2));
    assert!(middle[0] > middle[2] + 60 && corner[2] > corner[0] + 60, "{middle:?} {corner:?}");
}

#[test]
fn the_background_follows_time_and_the_world_when_asked() {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return;
    };
    let kit = kit();
    let mut scene = empty(&kit);
    set(&mut scene, "bloom", Value::Number(0.0));
    set(&mut scene, "background_colours", red_blue());
    set(&mut scene, "background_follow", Value::Choice("time".into()));
    set(&mut scene, "background_scroll", Value::Number(0.5));
    set(&mut scene, "background_repeat", Value::Choice("clamp".into()));
    // One flat colour that turns from red to blue as the video runs.
    let early = px(&draw(&device, &queue, &kit, &scene, 0.0), 135, 240);
    let late = px(&draw(&device, &queue, &kit, &scene, 2.0), 135, 240);
    assert!(early[0] > early[2] + 60 && late[2] > late[0] + 60, "{early:?} {late:?}");

    // On the screen the background stays put while the camera moves; in the world it goes.
    let mut scene = empty(&kit);
    set(&mut scene, "bloom", Value::Number(0.0));
    set(&mut scene, "background_colours", red_blue());
    set(&mut scene, "background_angle", Value::Number(0.0));
    set(&mut scene, "background_span", Value::Number(1080.0));
    let camera = bpe_kit::ensure_camera(&mut scene, &kit.registry);
    scene.things.get_mut(&camera).unwrap().params.set("position", Value::Vec2([540.0, 0.0]));
    let on_screen = draw(&device, &queue, &kit, &scene, 0.1);
    set(&mut scene, "background_on_screen", Value::Bool(false));
    let in_world = draw(&device, &queue, &kit, &scene, 0.1);
    let (a, b) = (px(&on_screen, 135, 240), px(&in_world, 135, 240));
    // The camera looks 540 to the right, where the world's gradient has reached blue.
    assert!(a[0] > 20 && a[2] > 20 && a[0].abs_diff(a[2]) < 40, "{a:?}: half way");
    assert!(b[2] > b[0] + 60, "{b:?}: blue in the world");
}
