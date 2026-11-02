//! Tests of the canvas thickness texture, lifts and the bleed pass on the graphics card.

use bpe_runtime::{Mark, MarkShape as S, MarkTarget, Sims};
use glam::Vec2;

use super::tests::*;
use super::*;
use crate::{Frame, headless_device};

/// Like `gpu()`, but a missing adapter fails the test when `BPE_REQUIRE_GPU` is set.
fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    match headless_device() {
        Ok(pair) => Some(pair),
        Err(e) => {
            assert!(std::env::var_os("BPE_REQUIRE_GPU").is_none(), "no GPU adapter: {e}");
            eprintln!("no GPU adapter available, skipping");
            None
        }
    }
}

/// The raw bytes of a [`SIDE`]-square R16Float texture.
fn read_body(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let row = SIDE * 2;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * SIDE) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    buffer.slice(..).get_mapped_range().unwrap().to_vec()
}

/// The thickness under canvas point `p`.
fn body_at(bytes: &[u8], p: Vec2) -> f32 {
    let (x, y) = ((p.x + SIDE as f32 / 2.0) as usize, (p.y + SIDE as f32 / 2.0) as usize);
    let i = (y * SIDE as usize + x) * 2;
    f16_to_f32(u16::from_le_bytes([bytes[i], bytes[i + 1]]))
}

/// What a canvas holds: paint, aux and body.
type Picture = (Vec<u8>, Vec<u8>, Vec<u8>);

fn read(device: &wgpu::Device, queue: &wgpu::Queue, flow: &FlowLayers) -> Picture {
    let t = flow.canvas_textures(MarkTarget::Canvas(CANVAS)).expect("the canvas has textures");
    (read_texture(device, queue, &t.paint), read_texture(device, queue, &t.aux), read_body(device, queue, &t.body))
}

fn flow_for(device: &wgpu::Device, queue: &wgpu::Queue, sims: &Sims, seconds: f64) -> (FlowLayers, Picture) {
    let (scene, resolved) = tiny_scene();
    let mut frame = Frame::default();
    frame.canvases.push(canvas_layer());
    let source = canvas_source(&scene, &resolved, sims);
    let mut flow = FlowLayers::new(device);
    flow.prepare(device, queue, &frame, &source);
    flow.step_to(device, queue, seconds, &source);
    let picture = read(device, queue, &flow);
    (flow, picture)
}

fn run(device: &wgpu::Device, queue: &wgpu::Queue, marking: fn(u64) -> Vec<Mark>, seconds: f64) -> Picture {
    let mut sims = Sims::default();
    sims.add_thing(Marking(marking));
    flow_for(device, queue, &sims, seconds).1
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

#[test]
fn body_adds_ink() {
    let Some((device, queue)) = gpu() else { return };
    let (_, _, body) = run(
        &device,
        &queue,
        |step| {
            if step != 1 {
                return Vec::new();
            }
            vec![
                mark(1, S::Stroke, v(-100.0, -100.0), v(-50.0, -100.0), 10.0),
                mark(1, S::Stroke, v(0.0, 0.0), v(100.0, 0.0), 10.0),
                mark(1, S::Stroke, v(50.0, -50.0), v(50.0, 50.0), 10.0),
                mark(1, S::Splat, v(-100.0, 100.0), v(-100.0, 100.0), 30.0),
            ]
        },
        0.1,
    );
    let near = |got: f32, want: f32| (got - want).abs() < 0.01;
    assert!(near(body_at(&body, v(-75.0, -100.0)), 96.0 / 255.0), "{}", body_at(&body, v(-75.0, -100.0)));
    assert!(near(body_at(&body, v(50.0, 0.0)), 192.0 / 255.0), "{}", body_at(&body, v(50.0, 0.0)));
    assert!(near(body_at(&body, v(-100.0, 100.0)), 200.0 / 255.0), "{}", body_at(&body, v(-100.0, 100.0)));
    assert_eq!(body_at(&body, v(-150.0, 0.0)), 0.0);
}

#[test]
fn drip_join_has_no_bead() {
    let Some((device, queue)) = gpu() else { return };
    let (_, _, body) = run(
        &device,
        &queue,
        |step| {
            if step != 1 {
                return Vec::new();
            }
            let piece = |a: f32, b: f32, joined: bool| {
                let mut m = mark(1, S::Drip, v(0.0, a), v(0.0, b), 6.0);
                if joined {
                    m.variant = 8;
                    m.angle = std::f32::consts::FRAC_PI_2;
                }
                m
            };
            vec![piece(-60.0, -20.0, false), piece(-20.0, 20.0, true), piece(20.0, 60.0, true)]
        },
        0.1,
    );
    for y in -50..50 {
        let thick = body_at(&body, v(0.0, y as f32));
        assert!((thick - 40.0 / 255.0).abs() < 0.01, "y {y}: {thick}");
    }
}

#[test]
fn lift_thins_paint_and_body() {
    let Some((device, queue)) = gpu() else { return };
    fn splat(step: u64) -> Vec<Mark> {
        (step == 1).then(|| mark(1, S::Splat, v(0.0, 0.0), v(0.0, 0.0), 30.0)).into_iter().collect()
    }
    fn lifted(step: u64) -> Vec<Mark> {
        let mut out = splat(step);
        if step == 3 {
            let mut m = mark(3, S::Lift, v(0.0, 0.0), v(0.0, 0.0), 20.0);
            m.radius_to = 0.5;
            out.push(m);
        }
        out
    }
    let (paint, aux, body) = run(&device, &queue, lifted, 0.1);
    let (paint0, aux0, body0) = run(&device, &queue, splat, 0.1);
    let near = |got: f32, want: f32| (got - want).abs() < 0.02;
    assert!(near(texel(&paint, v(0.0, 0.0))[3], 0.5), "{:?}", texel(&paint, v(0.0, 0.0)));
    assert!(near(texel(&paint, v(0.0, 0.0))[0], 0.5));
    assert!(near(body_at(&body, v(0.0, 0.0)), 0.5 * 200.0 / 255.0), "{}", body_at(&body, v(0.0, 0.0)));
    assert_eq!(texel(&aux, v(0.0, 0.0)), texel(&aux0, v(0.0, 0.0)), "the time stays");
    assert!(texel(&aux, v(0.0, 0.0))[0] > 0.0);
    // Outside the lift nothing changes.
    for p in [v(25.0, 0.0), v(0.0, 24.0), v(-22.0, 22.0)] {
        assert_eq!(texel(&paint, p), texel(&paint0, p), "{p}");
        assert_eq!(body_at(&body, p), body_at(&body0, p), "{p}");
    }
}

#[test]
fn drip_and_picture_boxes_cover_their_shapes() {
    let Some((device, queue)) = gpu() else { return };
    let (paint, _, _) = run(
        &device,
        &queue,
        |step| (step == 1).then(|| mark(1, S::Drip, v(-50.0, 0.0), v(50.0, 0.0), 6.0)).into_iter().collect(),
        0.1,
    );
    assert!(texel(&paint, v(48.0, 0.0))[3] > 0.9, "the far end of a long drip is painted");
    assert!(texel(&paint, v(0.0, 0.0))[3] > 0.9);
    assert_eq!(texel(&paint, v(0.0, 12.0))[3], 0.0);
}

/// A red stroke ending at x = 0 and a blue one starting there, both wet, and a Bleed over them
/// that dries after `dry` seconds.
fn wet_pair(bleed: Option<f32>) -> fn(u64) -> Vec<Mark> {
    match bleed {
        None => |step| strokes(step, None),
        Some(d) if d > 1.0 => |step| strokes(step, Some(5.0)),
        Some(_) => |step| strokes(step, Some(0.001)),
    }
}

fn strokes(step: u64, dry: Option<f32>) -> Vec<Mark> {
    match step {
        1 => {
            let mut red = mark(1, S::Stroke, v(-60.0, 0.0), v(-10.0, 0.0), 10.0);
            red.variant = 1;
            let mut blue = mark(1, S::Stroke, v(60.0, 0.0), v(10.0, 0.0), 10.0);
            blue.variant = 1;
            blue.colour = [0.0, 0.0, 1.0, 1.0];
            blue.colour_to = blue.colour;
            vec![red, blue]
        }
        8 => dry
            .map(|dry| {
                let mut b = mark(8, S::Bleed, v(-60.0, -60.0), v(60.0, 60.0), dry);
                b.radius_to = 1.0;
                b.colour = [0.0; 4];
                b.colour_to = [0.0; 4];
                vec![b]
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

#[test]
fn bleed_blends_wet_texels_only() {
    let Some((device, queue)) = gpu() else { return };
    let (before, _, _) = run(&device, &queue, wet_pair(None), 0.2);
    let (wet, _, _) = run(&device, &queue, wet_pair(Some(5.0)), 0.2);
    let (dry, _, _) = run(&device, &queue, wet_pair(Some(0.001)), 0.2);
    let edge = v(-1.0, 0.0);
    assert_eq!(texel(&before, edge)[2], 0.0, "no blue before");
    let blue = texel(&wet, edge)[2];
    assert!(blue > 0.03 && blue < 0.2, "blue seeped over the edge: {blue}");
    assert!(texel(&wet, v(1.0, 0.0))[0] > 0.03, "and red the other way");
    assert_eq!(texel(&wet, v(-40.0, 0.0)), texel(&before, v(-40.0, 0.0)), "far from the edge nothing mixes");
    assert!(dry == before, "dry paint does not bleed");
}

#[test]
fn bleed_grows_paint_into_empty_texels() {
    let Some((device, queue)) = gpu() else { return };
    let (paint, _, body) = run(
        &device,
        &queue,
        |step| match step {
            1 => (0..3).map(|_| mark(1, S::Stroke, v(-40.0, 0.0), v(40.0, 0.0), 10.0)).collect(),
            8 => {
                let mut b = mark(8, S::Bleed, v(-80.0, -60.0), v(80.0, 60.0), 5.0);
                b.radius_to = 1.0;
                vec![b]
            }
            _ => Vec::new(),
        },
        0.2,
    );
    assert!(texel(&paint, v(0.0, 10.0))[3] > 0.0, "{:?}", texel(&paint, v(0.0, 10.0)));
    assert!(body_at(&body, v(0.0, 10.0)) > 0.0);
    assert_eq!(texel(&paint, v(0.0, 22.0))[3], 0.0);
    assert_eq!(texel(&paint, v(0.0, -22.0))[3], 0.0);
}

fn busy(step: u64) -> Vec<Mark> {
    let mut out = Vec::new();
    if step.is_multiple_of(10) {
        let at = v(((step * 37) % 300) as f32 - 150.0, ((step * 53) % 300) as f32 - 150.0);
        let mut m = mark(step, S::Splat, at, at, 25.0);
        m.variant = (step % 7) as u8;
        m.colour = [(step % 3) as f32 / 2.0, (step % 5) as f32 / 4.0, 0.5, 1.0];
        out.push(m);
    }
    if step.is_multiple_of(8) {
        let mut b = mark(step, S::Bleed, v(-192.0, -192.0), v(192.0, 192.0), 2.0);
        b.radius_to = 0.5;
        out.push(b);
    }
    out
}

#[test]
fn seeking_back_with_bleed_restores_bit_for_bit() {
    let Some((device, queue)) = gpu() else { return };
    let (scene, resolved) = tiny_scene();
    let mut sims = Sims::default();
    sims.add_thing(Marking(busy));
    let mut frame = Frame::default();
    frame.canvases.push(canvas_layer());
    let source = canvas_source(&scene, &resolved, &sims);
    let mut flow = FlowLayers::new(&device);
    flow.prepare(&device, &queue, &frame, &source);
    flow.step_to(&device, &queue, 5.0, &source);
    let first = read(&device, &queue, &flow);
    flow.step_to(&device, &queue, 1.0, &source);
    flow.step_to(&device, &queue, 5.0, &source);
    let again = read(&device, &queue, &flow);
    assert!(first == again, "seeking back and forth gives the same canvas");
    let (_, straight) = flow_for(&device, &queue, &sims, 5.0);
    assert!(first == straight, "and the same as running straight");
    let nonzero = |b: &[u8]| b.iter().any(|x| *x != 0);
    assert!(nonzero(&first.0) && nonzero(&first.2));
}

#[test]
fn checkpoints_hold_three_textures() {
    let Some((device, queue)) = gpu() else { return };
    let mut sims = Sims::default();
    sims.add_thing(Marking(busy));
    let (flow, _) = flow_for(&device, &queue, &sims, 3.0);
    let driver = flow.driver.as_ref().unwrap();
    let snapshot = driver.checkpoints.last().unwrap().layers.values().next().unwrap();
    assert_eq!(snapshot.bytes(), (SIDE * SIDE * 18) as u64);
    let (_, _, bytes) = flow.checkpoint_stats();
    assert!(bytes >= (SIDE * SIDE * 18) as u64);
}

#[test]
fn canvas_shaders_validate() {
    for (name, wgsl) in [
        ("stamp", include_str!("../shaders/canvas_stamp.wgsl")),
        ("bleed", include_str!("../shaders/canvas_bleed.wgsl")),
        ("composite", include_str!("../shaders/canvas.wgsl")),
    ] {
        let module =
            naga::front::wgsl::parse_str(wgsl).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(wgsl)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }
}

/// Leaves `marks(step)` on the canvas like `Marking`, and names `pictures` in its world.
struct WithPictures(&'static [&'static str], fn(u64) -> Vec<Mark>);
impl bpe_runtime::ThingSim for WithPictures {
    fn kind(&self) -> &'static str {
        "balls"
    }
    fn build(&self, _: &bpe_core::Thing, _: &bpe_core::ResolvedThing, world: &mut bpe_runtime::World) {
        world.pictures = self.0.iter().map(|p| p.to_string()).collect();
    }
    fn after_step(&self, _: bpe_core::ThingId, world: &mut bpe_runtime::World, log: &mut bpe_runtime::Log) {
        for m in (self.1)(world.step) {
            log.push_mark(m);
        }
    }
}

/// A fresh folder holding `name` as a 64 by 64 PNG, left half `left` and right half `right`.
fn halves_folder(name: &str, left: [u8; 4], right: [u8; 4]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bpe-pic-gpu-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let image = image::RgbaImage::from_fn(64, 64, |x, _| image::Rgba(if x < 32 { left } else { right }));
    image.save(dir.join("halves.png")).unwrap();
    dir
}

/// Runs a canvas whose world names `pictures`, reading them from `folder`.
fn run_with_pictures(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pictures: &'static [&'static str],
    folder: Option<std::path::PathBuf>,
    marking: fn(u64) -> Vec<Mark>,
) -> (FlowLayers, Picture) {
    let (scene, resolved) = tiny_scene();
    let mut sims = Sims::default();
    sims.add_thing(WithPictures(pictures, marking));
    let mut frame = Frame::default();
    frame.canvases.push(canvas_layer());
    let mut source = canvas_source(&scene, &resolved, &sims);
    source.picture_folder = folder;
    let mut flow = FlowLayers::new(device);
    flow.prepare(device, queue, &frame, &source);
    flow.step_to(device, queue, 0.1, &source);
    let picture = read(device, queue, &flow);
    (flow, picture)
}

fn picture_mark(step: u64, angle: f32, variant: u8, colour: [f32; 4]) -> Vec<Mark> {
    if step != 1 {
        return Vec::new();
    }
    let mut m = mark(1, S::Picture, v(0.0, 0.0), v(0.0, 0.0), 60.0);
    m.angle = angle;
    m.variant = variant;
    m.colour = colour;
    m.colour_to = colour;
    vec![m]
}

#[test]
fn picture_stamps_its_halves() {
    let Some((device, queue)) = gpu() else { return };
    let dir = halves_folder("halves", [255, 0, 0, 255], [0, 0, 255, 255]);
    let (_, (paint, ..)) =
        run_with_pictures(&device, &queue, &["halves.png"], Some(dir.clone()), |s| picture_mark(s, 0.0, 0, [1.0; 4]));
    let (l, r) = (texel(&paint, v(-30.0, 0.0)), texel(&paint, v(30.0, 0.0)));
    assert!(l[0] > 0.8 && l[2] < 0.2, "{l:?}");
    assert!(r[2] > 0.8 && r[0] < 0.2, "{r:?}");
    let (_, (turned, ..)) = run_with_pictures(&device, &queue, &["halves.png"], Some(dir), |s| {
        picture_mark(s, std::f32::consts::PI, 0, [1.0; 4])
    });
    let (l, r) = (texel(&turned, v(-30.0, 0.0)), texel(&turned, v(30.0, 0.0)));
    assert!(l[2] > 0.8 && l[0] < 0.2, "turned: {l:?}");
    assert!(r[0] > 0.8 && r[2] < 0.2, "turned: {r:?}");
    // Outside the square nothing is painted.
    assert_eq!(texel(&paint, v(70.0, 0.0))[3], 0.0);
}

#[test]
fn tinted_picture_keeps_light_and_dark() {
    let Some((device, queue)) = gpu() else { return };
    let dir = halves_folder("tinted", [255, 255, 255, 255], [0, 0, 0, 255]);
    let (_, (paint, ..)) = run_with_pictures(&device, &queue, &["halves.png"], Some(dir), |s| {
        picture_mark(s, 0.0, 32, [0.0, 1.0, 0.0, 1.0])
    });
    let (l, r) = (texel(&paint, v(-30.0, 0.0)), texel(&paint, v(30.0, 0.0)));
    assert!(l[1] > 0.9, "{l:?}");
    assert!(r[1] < 0.1 && r[3] > 0.95, "{r:?}");
}

fn is_checker(c: [f32; 4]) -> bool {
    let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.02);
    near([c[0], c[1], c[2]], [1.0, 0.0782, 0.6867]) || near([c[0], c[1], c[2]], [0.0144; 3])
}

#[test]
fn slot_past_the_atlas_draws_the_checker() {
    let Some((device, queue)) = gpu() else { return };
    let dir = halves_folder("slot", [255, 0, 0, 255], [0, 0, 255, 255]);
    let (_, (paint, ..)) =
        run_with_pictures(&device, &queue, &["halves.png"], Some(dir), |s| picture_mark(s, 0.0, 5, [1.0; 4]));
    let c = texel(&paint, v(10.0, 10.0));
    assert!(is_checker(c), "{c:?}");
}

#[test]
fn missing_file_draws_the_checker_on_the_gpu() {
    let Some((device, queue)) = gpu() else { return };
    let dir = std::env::temp_dir().join(format!("bpe-pic-gpu-missing-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (flow, (paint, ..)) =
        run_with_pictures(&device, &queue, &["gone.png"], Some(dir), |s| picture_mark(s, 0.0, 0, [1.0; 4]));
    let c = texel(&paint, v(10.0, 10.0));
    assert!(is_checker(c), "{c:?}");
    assert_eq!(flow.missing_pictures(), ["gone.png"]);
}

// ---- The paint styles, seen through the renderer ----

use crate::{MaskShape, PaintLook, PaintStyle, Renderer};

/// Leaves `marks(step)` on the canvas, or on its skin.
struct Marker(fn(u64) -> Vec<Mark>, bool);
impl bpe_runtime::ThingSim for Marker {
    fn kind(&self) -> &'static str {
        "balls"
    }
    fn build(&self, _: &bpe_core::Thing, _: &bpe_core::ResolvedThing, _: &mut bpe_runtime::World) {}
    fn after_step(&self, _: bpe_core::ThingId, world: &mut bpe_runtime::World, log: &mut bpe_runtime::Log) {
        for mut m in (self.0)(world.step) {
            if self.1 {
                m.target = MarkTarget::Skin(CANVAS);
            }
            log.push_mark(m);
        }
    }
}

/// The canvas drawn in `look` at scene time `time`, as 540 by 960 RGBA bytes: two world pixels
/// to an output pixel, the middle in the middle. With a mask it is a skin.
fn composite(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    look: PaintLook,
    marks: fn(u64) -> Vec<Mark>,
    time: f64,
    mask: Option<Vec<MaskShape>>,
) -> Vec<u8> {
    let (scene, resolved) = tiny_scene();
    let mut sims = Sims::default();
    sims.add_thing(Marker(marks, mask.is_some()));
    let mut layer = canvas_layer();
    layer.look = look;
    layer.time = time as f32;
    if let Some(mask) = mask {
        layer.key = MarkTarget::Skin(CANVAS);
        layer.place = crate::CanvasPlace::OnThing;
        layer.mask = mask;
    }
    let frame = Frame { bloom: 0.0, time: time as f32, canvases: vec![layer], ..Default::default() };
    let source = canvas_source(&scene, &resolved, &sims);
    let mut renderer = Renderer::new(device, 540, 960);
    assert!(renderer.render_with_flow(device, queue, &frame, &source).caught_up);
    renderer.read_output(device, queue).unwrap()
}

/// The output pixel at world point (x, y).
fn px(out: &[u8], x: f32, y: f32) -> [u8; 3] {
    let (ox, oy) = ((x / 2.0 + 270.0).floor() as usize, (y / 2.0 + 480.0).floor() as usize);
    let i = (oy * 540 + ox) * 4;
    [out[i], out[i + 1], out[i + 2]]
}

fn sum(p: [u8; 3]) -> i32 {
    p.iter().map(|c| *c as i32).sum()
}

fn look(style: PaintStyle) -> PaintLook {
    PaintLook { style, dry_time: 1.0, light_angle: 315.0, gloss: 1.0, grain: 0.0, ..PaintLook::default() }
}

/// The brightest pixel (by r + g + b) in the world square `centre` plus or minus `half`.
fn brightest(out: &[u8], centre: Vec2, half: f32) -> i32 {
    let mut best = 0;
    let mut y = -half;
    while y <= half {
        let mut x = -half;
        while x <= half {
            best = best.max(sum(px(out, centre.x + x, centre.y + y)));
            x += 2.0;
        }
        y += 2.0;
    }
    best
}

fn one_splat(step: u64) -> Vec<Mark> {
    (step == 1).then(|| mark(1, S::Splat, v(0.0, 0.0), v(0.0, 0.0), 60.0)).into_iter().collect()
}

fn two_splats(step: u64) -> Vec<Mark> {
    if step != 1 {
        return Vec::new();
    }
    vec![mark(1, S::Splat, v(-20.0, 0.0), v(-20.0, 0.0), 60.0), mark(1, S::Splat, v(20.0, 0.0), v(20.0, 0.0), 60.0)]
}

fn disc(step: u64) -> Vec<Mark> {
    (step == 1).then(|| mark(1, S::Stroke, v(0.0, 0.0), v(0.0, 0.0), 60.0)).into_iter().collect()
}

fn bar(step: u64) -> Vec<Mark> {
    (step == 1).then(|| mark(1, S::Stroke, v(-80.0, 0.0), v(80.0, 0.0), 30.0)).into_iter().collect()
}

fn thin_line(step: u64) -> Vec<Mark> {
    (step == 1)
        .then(|| {
            let mut m = mark(1, S::Stroke, v(-80.0, 0.0), v(80.0, 0.0), 4.0);
            m.colour = [0.1, 0.02, 0.02, 1.0];
            m.colour_to = m.colour;
            m
        })
        .into_iter()
        .collect()
}

fn dim_line(step: u64) -> Vec<Mark> {
    let mut marks = thin_line(step);
    for m in &mut marks {
        m.colour = [0.01, 0.002, 0.002, 1.0];
        m.colour_to = m.colour;
    }
    marks
}

#[test]
fn flat_is_the_same_wet_or_dry() {
    let Some((device, queue)) = gpu() else { return };
    let wet = composite(&device, &queue, look(PaintStyle::Flat), one_splat, 0.05, None);
    let dry = composite(&device, &queue, look(PaintStyle::Flat), one_splat, 3.0, None);
    assert!(wet == dry);
}

#[test]
fn wet_paint_highlight_fades_as_it_dries() {
    let Some((device, queue)) = gpu() else { return };
    let wet = composite(&device, &queue, look(PaintStyle::WetPaint), one_splat, 0.05, None);
    let dry = composite(&device, &queue, look(PaintStyle::WetPaint), one_splat, 3.0, None);
    let (a, b) = (brightest(&wet, Vec2::ZERO, 50.0), brightest(&dry, Vec2::ZERO, 50.0));
    assert!(a >= b + 40, "wet {a}, dry {b}");
}

#[test]
fn wet_paint_overlaps_shine_more() {
    let Some((device, queue)) = gpu() else { return };
    let one = composite(&device, &queue, look(PaintStyle::WetPaint), one_splat, 0.05, None);
    let two = composite(&device, &queue, look(PaintStyle::WetPaint), two_splats, 0.05, None);
    let (a, b) = (brightest(&one, Vec2::ZERO, 60.0), brightest(&two, Vec2::ZERO, 60.0));
    assert!(b >= a + 20, "one {a}, two {b}");
}

#[test]
fn light_angle_moves_the_highlight() {
    let Some((device, queue)) = gpu() else { return };
    let rim = |out: &[u8], sx: f32, sy: f32| {
        let mut total = 0;
        let mut count = 0;
        for i in 0..16 {
            for r in [57.0, 58.5, 60.0, 61.5] {
                let a = std::f32::consts::FRAC_PI_2 * (0.1 + 0.8 * i as f32 / 15.0);
                let p = v(sx * a.cos() * r, sy * a.sin() * r);
                total += sum(px(out, p.x, p.y));
                count += 1;
            }
        }
        total as f32 / count as f32
    };
    let mut l = look(PaintStyle::WetPaint);
    l.light_angle = 315.0;
    let from_top_left = composite(&device, &queue, l, disc, 0.05, None);
    assert!(rim(&from_top_left, -1.0, -1.0) > rim(&from_top_left, 1.0, 1.0) + 10.0);
    l.light_angle = 135.0;
    let from_bottom_right = composite(&device, &queue, l, disc, 0.05, None);
    assert!(rim(&from_bottom_right, 1.0, 1.0) > rim(&from_bottom_right, -1.0, -1.0) + 10.0);
}

#[test]
fn watercolour_footprint_shrinks_as_it_dries() {
    let Some((device, queue)) = gpu() else { return };
    let none = composite(&device, &queue, look(PaintStyle::Watercolour), |_| Vec::new(), 0.05, None);
    let count =
        |out: &[u8]| out.chunks(4).zip(none.chunks(4)).filter(|(a, b)| (0..3).any(|c| a[c].abs_diff(b[c]) > 8)).count();
    let wet = count(&composite(&device, &queue, look(PaintStyle::Watercolour), bar, 0.05, None));
    let dry = count(&composite(&device, &queue, look(PaintStyle::Watercolour), bar, 3.0, None));
    assert!(wet > dry, "wet {wet}, dry {dry}");
}

#[test]
fn watercolour_edges_darken() {
    let Some((device, queue)) = gpu() else { return };
    let mut l = look(PaintStyle::Watercolour);
    l.edge_dark = 1.0;
    let dark = composite(&device, &queue, l, bar, 3.0, None);
    l.edge_dark = 0.0;
    let plain = composite(&device, &queue, l, bar, 3.0, None);
    let (a, b) = (sum(px(&dark, 0.0, 28.0)), sum(px(&plain, 0.0, 28.0)));
    assert!(a + 10 <= b, "dark {a}, plain {b}");
}

#[test]
fn neon_glow_goes_as_it_dries() {
    let Some((device, queue)) = gpu() else { return };
    let mut l = look(PaintStyle::NeonInk);
    l.glow_size = 18.0;
    let background = composite(&device, &queue, l, |_| Vec::new(), 0.05, None);
    let wet = composite(&device, &queue, l, thin_line, 0.05, None);
    let dry = composite(&device, &queue, l, thin_line, 3.0, None);
    let dim_neon = composite(&device, &queue, l, dim_line, 3.0, None);
    let dim_flat = composite(&device, &queue, look(PaintStyle::Flat), dim_line, 3.0, None);
    let off = |out: &[u8]| sum(px(out, 0.0, 8.0)) - sum(px(&background, 0.0, 8.0));
    assert!(off(&wet) >= 15, "wet glow {}", off(&wet));
    assert!(off(&dry).abs() <= 2, "dry glow {}", off(&dry));
    // A dim line, so the tone curve has not squashed the extra light yet. The picture is
    // encoded for the screen, so 1.5 times the light is about 1.2 times the code value.
    let bg = sum(px(&background, 0.0, 0.0));
    let (line, plain) = (sum(px(&dim_neon, 0.0, 0.0)) - bg, sum(px(&dim_flat, 0.0, 0.0)) - bg);
    assert!(line as f32 >= 1.15 * plain as f32, "line {line}, flat {plain}");
}

#[test]
fn neon_glow_shows_past_a_skin_line_but_not_past_its_size() {
    let Some((device, queue)) = gpu() else { return };
    let mut l = look(PaintStyle::NeonInk);
    l.glow_size = 18.0;
    let capsule = MaskShape { kind: MaskShape::CAPSULE, a: [-100.0, 0.0, 100.0, 0.0], b: [4.0, 0.0, 0.0, 0.0] };
    let background = composite(&device, &queue, l, |_| Vec::new(), 0.05, Some(vec![capsule]));
    let wet = composite(&device, &queue, l, thin_line, 0.05, Some(vec![capsule]));
    let off = |y: f32| sum(px(&wet, 0.0, y)) - sum(px(&background, 0.0, y));
    assert!(off(8.0) >= 10, "near {}", off(8.0));
    assert!(off(30.0).abs() <= 2, "far {}", off(30.0));
}
