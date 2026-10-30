//! Picture tests for gradient paint inside shapes, on the software GPU.

use crate::{Frame, LookInstance, PaintGpu, Renderer, ShapeInstance, SoftVertex};
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

#[test]
fn a_soft_body_paints_stripes_across_its_box() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let row = frame.gradients.row(&red_blue_steps());
    let paint = PaintGpu { row: row as f32, pattern: 1.0, steps: 1.0, ..PaintGpu::default() };
    // A square 400 across each way, as two triangles, its corners at the box's corners.
    let corner = |x: f32, y: f32| SoftVertex {
        position: [x * 400.0, y * 400.0],
        inside: 1000.0,
        _pad: 0.0,
        // Green, so a body that ignores its paint fails.
        color: [0.0, 1.0, 0.0, 1.0],
        paint,
        local: [x, y],
    };
    frame.soft.extend_from_slice(&[
        corner(-1.0, -1.0),
        corner(1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, -1.0),
        corner(1.0, 1.0),
        corner(-1.0, 1.0),
    ]);
    assert_two_halves("paint-soft-stripes", &frame);
}

#[test]
fn a_glyph_paints_stripes_across_its_box() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    // Text is stuck to the screen, so it draws in the overlay, from the overlay's atlas.
    let o = frame.overlay_mut();
    let row = o.gradients.row(&red_blue_steps());
    let style = crate::text::TextStyle { color: [0.0, 1.0, 0.0, 1.0], ..crate::text::TextStyle::default() };
    let (face, align) = (crate::text::FontFace::Anton, crate::text::TextAlign::Center);
    crate::text::layout_text("H", face, 1200.0, glam::Vec2::ZERO, align, 0.0, usize::MAX, &style, &mut o.glyphs);
    assert_eq!(o.glyphs.len(), 1);
    let g = &mut o.glyphs[0];
    g.paint = PaintGpu { row: row as f32, pattern: 1.0, steps: 1.0, ..PaintGpu::default() };
    let rect = g.rect;
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, W, H);
    renderer.render(&device, &queue, &frame);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    crate::pictures::save("paint-glyph-stripes", &pixels, W, H);
    // The box's centre line on the output; the letter's ink either side of it, 4 pixels left out.
    let line = W as f32 / 2.0 + (rect[0] + rect[2]) * 0.25;
    let (mut left, mut right) = (0, 0);
    for y in 0..H {
        for x in 0..W {
            let i = (y * W + x) as usize * 4;
            let p = &pixels[i..i + 4];
            if p[0] as u32 + p[1] as u32 + p[2] as u32 <= 200 {
                continue;
            }
            let from_line = x as f32 + 0.5 - line;
            if from_line < -4.0 {
                assert!(p[0] > 150 && p[1] < 60 && p[2] < 60, "({x}, {y}) {p:?}: red left of the line");
                left += 1;
            } else if from_line > 4.0 {
                assert!(p[2] > 150 && p[0] < 60 && p[1] < 60, "({x}, {y}) {p:?}: blue right of the line");
                right += 1;
            }
        }
    }
    assert!(left > 1000 && right > 1000, "{left} {right}: the letter shows both sides");
}

/// A covering burst of confetti at the centre, green (so a shader that ignores its paint fails),
/// painted from `paint` the way `follow` says, drawn `at` seconds after it went off.
fn painted_burst(name: &str, frame: &mut Frame, paint: PaintGpu, follow: u32, at: f32) -> Option<Vec<u8>> {
    frame.time = at;
    frame.bursts.push(crate::Burst {
        origin: [0.0, 0.0],
        normal: [0.0, -1.0],
        time: 0.0,
        color: [0.0, 1.0, 0.0, 1.0],
        count: 300,
        speed: 500.0,
        spread: 360.0,
        size: 12.0,
        life: 1.0,
        gravity: 0.0,
        drag: 0.0,
        shape: 2,
        additive: false,
        seed: 11,
        paint,
        follow,
    });
    let (device, queue) = gpu()?;
    let mut renderer = Renderer::new(&device, W, H);
    renderer.render(&device, &queue, frame);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    crate::pictures::save(name, &pixels, W, H);
    Some(pixels)
}

/// The pixels a particle covers: the background is black.
fn lit(pixels: &[u8]) -> Vec<[u8; 3]> {
    pixels.chunks(4).map(|p| [p[0], p[1], p[2]]).filter(|p| p.iter().any(|&c| c > 40)).collect()
}

#[test]
fn particles_life_fades_through_colours() {
    for (at, red_first) in [(0.1, true), (0.9, false)] {
        let mut frame = Frame { bloom: 0.0, ..Frame::default() };
        let row = frame.gradients.row(&Gradient { transition: Transition::Smooth, ..red_blue_steps() });
        let paint = PaintGpu { row: row as f32, ..PaintGpu::default() };
        let name = format!("particles-life-{}", (at * 100.0) as u32);
        let Some(pixels) = painted_burst(&name, &mut frame, paint, 1, at) else { return };
        let lit = lit(&pixels);
        assert!(lit.len() > 50, "{name}: only {} pixels drawn", lit.len());
        for p in lit {
            assert!(p[1] < 40, "{name}: {p:?} is green, the burst's own colour");
            if red_first {
                assert!(p[0] > p[2], "{name}: {p:?} should be mostly red early in its life");
            } else {
                assert!(p[2] > p[0], "{name}: {p:?} should be mostly blue late in its life");
            }
        }
    }
}

#[test]
fn particles_order_takes_one_stop_each() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let row = frame.gradients.row(&red_blue_steps());
    let paint = PaintGpu { row: row as f32, steps: 1.0, scale: 2.0, ..PaintGpu::default() };
    let Some(pixels) = painted_burst("particles-order", &mut frame, paint, 2, 0.2) else { return };
    let lit = lit(&pixels);
    let red = lit.iter().filter(|p| p[0] > 40 && p[2] < 10).count();
    let blue = lit.iter().filter(|p| p[2] > 40 && p[0] < 10).count();
    assert!(red > 50 && blue > 50, "both stops show: {red} red, {blue} blue");
    // Overlapping particles may mix, but none is green or half way.
    assert!(lit.iter().all(|p| p[1] < 40), "no green");
}
