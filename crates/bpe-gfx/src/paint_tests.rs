//! Picture tests for gradient paint inside shapes, on the software GPU.

use crate::{Frame, LookInstance, PaintGpu, Renderer, ShapeInstance};
use bpe_core::{Color, Gradient, Stop, Transition};

/// A device on the first adapter, naming it so a run shows which GPU drew; `None` without one.
fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    })) else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    println!("adapter: {}", adapter.get_info().name);
    pollster::block_on(
        adapter.request_device(&wgpu::DeviceDescriptor { label: Some("paint tests"), ..Default::default() }),
    )
    .ok()
}

fn red_blue_steps() -> Gradient {
    Gradient {
        stops: vec![Stop { at: 0.0, color: Color::rgb(255, 0, 0) }, Stop { at: 1.0, color: Color::rgb(0, 0, 255) }],
        transition: Transition::Steps,
    }
}

/// A big green styled ball at the centre painting stripes from atlas row `row`.
fn striped_ball(row: u32) -> LookInstance {
    LookInstance {
        center: [0.0, 0.0],
        radius: 400.0,
        seed: 0.0,
        // Green, so a ball that ignores its paint fails.
        color: [0.0, 1.0, 0.0, 1.0],
        color2: [0.0, 1.0, 0.0, 1.0],
        outline_color: [0.0; 4],
        form: [0.0, 0.0, 4.0, 0.0],
        look: [0.0, 0.0, 1.0, 0.0],
        stretch: [1.0, 0.0, 1.0, 1.0],
        extent: [0.8, 1.0, 0.0, 0.0],
        paint: PaintGpu { row: row as f32, pattern: 1.0, steps: 1.0, scale: 1.0, ..PaintGpu::default() },
    }
}

const W: u32 = 540;
const H: u32 = 960;

/// Draws `frame` and checks the ball is red left of the centre line and blue right of it,
/// 2 pixels either side of the line left out.
fn assert_two_halves(name: &str, frame: &Frame) {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, W, H);
    renderer.render(&device, &queue, frame);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    crate::pictures::save(name, &pixels, W, H);

    // The ball is 200 pixels across each way from the centre line at x = 270.
    let centre = W as f32 / 2.0;
    for y in [H / 2 - 100, H / 2, H / 2 + 100] {
        for x in 100..440u32 {
            let i = (y * W + x) as usize * 4;
            let p = &pixels[i..i + 4];
            let from_line = x as f32 + 0.5 - centre;
            if from_line < -2.0 {
                assert!(p[0] > 150 && p[1] < 60 && p[2] < 60, "({x}, {y}) {p:?}: red left of the line");
            } else if from_line > 2.0 {
                assert!(p[2] > 150 && p[0] < 60 && p[1] < 60, "({x}, {y}) {p:?}: blue right of the line");
            }
        }
    }
}

#[test]
fn pattern_stripes_paint_two_halves() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let row = frame.gradients.row(&red_blue_steps());
    frame.looks.push(striped_ball(row));
    assert_two_halves("paint-stripes", &frame);
}

#[test]
fn things_stuck_to_the_screen_paint_from_their_own_atlas() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    // The world's row 0 is all green, so reading the wrong atlas shows.
    frame.gradients.row(&Gradient::solid(Color::rgb(0, 255, 0)));
    let overlay = frame.overlay_mut();
    let row = overlay.gradients.row(&red_blue_steps());
    overlay.looks.push(striped_ball(row));
    assert_two_halves("paint-stripes-overlay", &frame);
}

#[test]
fn ring_path_gradient_goes_round() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let red_blue = Gradient { transition: Transition::Smooth, ..red_blue_steps() };
    let row = frame.gradients.row(&red_blue);
    let ring =
        bpe_runtime::Outline::Ring { center: glam::Vec2::ZERO, radius: 400.0, sides: 0, angle: 0.0, thickness: 60.0 };
    // Green, so a wall that ignores its paint fails.
    let paint = PaintGpu { row: row as f32, pattern: 4.0, ..PaintGpu::default() };
    frame.shapes.push(ShapeInstance::outline(&ring, paint, [0.0, 1.0, 0.0, 1.0], 0.0));
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, W, H);
    renderer.render(&device, &queue, &frame);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    crate::pictures::save("paint-ring-path", &pixels, W, H);
    let at = |x: u32, y: u32| {
        let i = (y * W + x) as usize * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    // The ring is 200 pixels round on the output. Row H / 2 sits half a pixel below the centre,
    // just clockwise of angle 0 on the right.
    let start = at(W / 2 + 200, H / 2);
    assert!(start[0] > 150 && start[1] < 60 && start[2] < 60, "{start:?}: red at angle 0");
    let quarter = at(W / 2, H / 2 + 200);
    assert!(quarter[0] > quarter[2] && quarter[2] > 40 && quarter[1] < 60, "{quarter:?}: mostly red at 90°");
    let half = at(W / 2 - 200, H / 2);
    assert!(half[0] > 40 && half[2] > 40 && half[1] < 60, "{half:?}: between red and blue at 180°");
    assert!(half[0].abs_diff(half[2]) < 40, "{half:?}: half way at 180°");
    let three_quarters = at(W / 2, H / 2 - 200);
    assert!(three_quarters[2] > three_quarters[0] && three_quarters[1] < 60, "{three_quarters:?}: mostly blue at 270°");
}

#[test]
fn an_inside_pattern_paints_across_a_shape() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let row = frame.gradients.row(&red_blue_steps());
    let stripes = PaintGpu { row: row as f32, pattern: 1.0, steps: 1.0, ..PaintGpu::default() };
    // A disc 400 across each way, so as big as the striped ball.
    frame.shapes.push(ShapeInstance::disc(glam::Vec2::ZERO, 200.0, [0.0, 1.0, 0.0, 1.0], 0.0).painted(stripes));
    assert_two_halves("paint-shape-stripes", &frame);
}

#[test]
fn box_path_runs_clockwise_from_the_top_left() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let row = frame.gradients.row(&Gradient { transition: Transition::Smooth, ..red_blue_steps() });
    let wall = bpe_runtime::Outline::Box {
        center: glam::Vec2::ZERO,
        half: glam::Vec2::new(400.0, 400.0),
        angle: 0.0,
        thickness: 40.0,
    };
    let paint = PaintGpu { row: row as f32, pattern: 4.0, ..PaintGpu::default() };
    frame.shapes.push(ShapeInstance::outline(&wall, paint, [0.0, 1.0, 0.0, 1.0], 0.0));
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, W, H);
    renderer.render(&device, &queue, &frame);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    crate::pictures::save("paint-box-path", &pixels, W, H);
    let at = |x: u32, y: u32| {
        let i = (y * W + x) as usize * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    // The box is 200 pixels each way from the centre on the output.
    let (cx, cy) = (W / 2, H / 2);
    let near_start = at(cx - 190, cy - 200);
    assert!(near_start[0] > 150 && near_start[1] < 60 && near_start[2] < 60, "{near_start:?}: red at the top left");
    let top = at(cx, cy - 200);
    let bottom = at(cx, cy + 200);
    assert!(top[0] > top[2] && top[1] < 60, "{top:?}: an eighth of the way round, mostly red");
    assert!(bottom[2] > bottom[0] && bottom[1] < 60, "{bottom:?}: five eighths of the way round, mostly blue");
}
