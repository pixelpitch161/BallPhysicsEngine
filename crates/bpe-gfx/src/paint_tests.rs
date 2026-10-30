//! Picture tests for gradient paint inside shapes, on the software GPU.

use crate::{Frame, LookInstance, PaintGpu, Renderer};
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
