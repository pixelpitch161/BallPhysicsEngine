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
