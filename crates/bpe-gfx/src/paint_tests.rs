//! Picture tests for gradient paint inside shapes, on the software GPU.

use crate::{Frame, LookInstance, PaintGpu, Renderer, ShapeInstance};
use bpe_core::{Color, GapBlend, Gradient, Stop};

/// A device on the first adapter, naming it so a run shows which GPU drew; `None` without one.
pub(crate) fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
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
    let steps = |at, color| Stop { blend: GapBlend::Steps, ..Stop::new(at, color) };
    Gradient {
        stops: vec![steps(0.0, Color::rgb(255, 0, 0)), steps(1.0, Color::rgb(0, 0, 255))],
        ..Gradient::default()
    }
}

/// A big green styled ball at the centre painting stripes from atlas row `row`, the middle of
/// the gradient on its centre line.
fn striped_ball(row: u32) -> LookInstance {
    LookInstance {
        center: [0.0, 0.0],
        radius: 400.0,
        seed: 0.0,
        // Green, so a ball that ignores its paint fails.
        color: [0.0, 1.0, 0.0, 1.0],
        outline_color: [0.0; 4],
        form: [0.0, 0.0, 4.0, 0.0],
        look: [0.0, 0.0, 1.0, 0.0],
        stretch: [1.0, 0.0, 1.0, 1.0],
        extent: [0.8, 1.0, 0.0, 0.0],
        paint: PaintGpu { row: row as f32, t0: 0.5, pattern: 5.0, scale: 1.0, ..PaintGpu::default() },
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
    let red_blue = Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
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
    let stripes = PaintGpu { row: row as f32, t0: 0.5, pattern: 5.0, ..PaintGpu::default() };
    // A disc 400 across each way, so as big as the striped ball.
    frame.shapes.push(ShapeInstance::disc(glam::Vec2::ZERO, 200.0, [0.0, 1.0, 0.0, 1.0], 0.0).painted(stripes));
    assert_two_halves("paint-shape-stripes", &frame);
}

#[test]
fn box_path_runs_clockwise_from_the_top_left() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let row = frame.gradients.row(&Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]));
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

/// `frame` drawn on the software GPU and saved as `name`; `None` without a GPU.
fn render(name: &str, frame: &Frame) -> Option<Vec<u8>> {
    let (device, queue) = gpu()?;
    let mut renderer = Renderer::new(&device, W, H);
    renderer.render(&device, &queue, frame);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    crate::pictures::save(name, &pixels, W, H);
    Some(pixels)
}

/// The red and the blue pixels along the horizontal and vertical lines through the middle of a
/// shape 200 pixels round on the output, 20 pixels in from its edge.
fn reds_and_blues(pixels: &[u8]) -> (usize, usize) {
    let (cx, cy) = (W as i32 / 2, H as i32 / 2);
    let (mut red, mut blue) = (0, 0);
    for d in -180..=180 {
        for (x, y) in [(cx + d, cy), (cx, cy + d)] {
            let i = (y as u32 * W + x as u32) as usize * 4;
            let p = &pixels[i..i + 3];
            if p[0] > 150 && p[1] < 60 && p[2] < 60 {
                red += 1;
            } else if p[2] > 150 && p[0] < 60 && p[1] < 60 {
                blue += 1;
            }
        }
    }
    (red, blue)
}

#[test]
fn inside_patterns_keep_repeating_whatever_the_start_size_and_scroll() {
    // With the paint's Clamp, a start of 1 used to clamp every pixel to the last colour, a size
    // of 3 squeezed the gradient into the middle and a scroll ran off the end after a while.
    // Stripes and rings, and a sweep as `paint_gpu` gives an old one: turned half way, started
    // half a sweep back and mirrored on Clamp.
    for pattern in [5.0, 6.0, 3.0] {
        for (t0, scale, scroll, time) in [(1.0, 3.0, 0.0, 0.0), (1.0, 3.0, 0.4, 10.0), (0.3, 2.0, 0.7, 10.0)] {
            let mut frame = Frame { bloom: 0.0, time, ..Frame::default() };
            let row = frame.gradients.row(&red_blue_steps());
            let mut ball = striped_ball(row);
            ball.paint = PaintGpu { row: row as f32, t0, pattern, scale, scroll, ..PaintGpu::default() };
            if pattern == 3.0 {
                ball.paint = PaintGpu { t0: t0 - 0.5 * scale, angle: 180.0, repeat: 2.0, ..ball.paint };
            }
            frame.looks.push(ball);
            let name = format!("paint-pattern-{pattern}-{t0}-{scale}-{scroll}");
            let Some(pixels) = render(&name, &frame) else { return };
            let (red, blue) = reds_and_blues(&pixels);
            assert!(red > 40 && blue > 40, "{name}: both colours show across the shape: {red} red, {blue} blue");
        }
    }
}

#[test]
fn a_path_keeps_going_round_as_it_scrolls() {
    let mut frame = Frame { bloom: 0.0, time: 10.0, ..Frame::default() };
    let row = frame.gradients.row(&red_blue_steps());
    let ring =
        bpe_runtime::Outline::Ring { center: glam::Vec2::ZERO, radius: 400.0, sides: 0, angle: 0.0, thickness: 60.0 };
    let paint = PaintGpu { row: row as f32, pattern: 4.0, scroll: 0.37, ..PaintGpu::default() };
    frame.shapes.push(ShapeInstance::outline(&ring, paint, [0.0, 1.0, 0.0, 1.0], 0.0));
    let Some(pixels) = render("paint-path-scrolled", &frame) else { return };
    let (mut red, mut blue) = (0, 0);
    for i in 0..64 {
        let a = i as f32 / 64.0 * std::f32::consts::TAU;
        let (x, y) = ((W as f32 / 2.0 + 200.0 * a.cos()) as u32, (H as f32 / 2.0 + 200.0 * a.sin()) as u32);
        let k = (y * W + x) as usize * 4;
        let p = &pixels[k..k + 3];
        red += (p[0] > 150 && p[2] < 60) as u32;
        blue += (p[2] > 150 && p[0] < 60) as u32;
    }
    assert!(red > 10 && blue > 10, "both colours still go round the ring: {red} red, {blue} blue");
}

#[test]
fn turning_a_sweep_half_way_swaps_its_sides() {
    let mut sides = Vec::new();
    for angle in [90.0, 270.0] {
        let mut frame = Frame { bloom: 0.0, ..Frame::default() };
        let row = frame.gradients.row(&red_blue_steps());
        let mut ball = striped_ball(row);
        // What `paint_gpu` gives an old sweep at `angle` from 0.5: turned half way, started half a
        // sweep back and mirrored.
        ball.paint = PaintGpu {
            row: row as f32,
            t0: 0.0,
            pattern: 3.0,
            angle: angle + 180.0,
            repeat: 2.0,
            ..PaintGpu::default()
        };
        frame.looks.push(ball);
        let Some(pixels) = render(&format!("paint-sweep-{angle}"), &frame) else { return };
        let at = |x: u32| {
            let i = ((H / 2) * W + x) as usize * 4;
            [pixels[i], pixels[i + 1], pixels[i + 2]]
        };
        sides.push((at(W / 2 - 120), at(W / 2 + 120)));
    }
    let red = |p: [u8; 3]| p[0] > 150 && p[1] < 60 && p[2] < 60;
    let blue = |p: [u8; 3]| p[2] > 150 && p[0] < 60 && p[1] < 60;
    let ((l0, r0), (l1, r1)) = (sides[0], sides[1]);
    assert!(blue(l0) && red(r0), "at 90°: blue left {l0:?}, red right {r0:?}");
    assert!(red(l1) && blue(r1), "at 270°: red left {l1:?}, blue right {r1:?}");
}

#[test]
fn inside_pattern_varies_per_ball() {
    // Three striped balls in a row, each turned by its own hue: red's left half turns
    // yellow-green at a quarter turn, purple at minus a quarter, and stays red a hair back.
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let row = frame.gradients.row(&red_blue_steps());
    let turns = [0.25, -0.25, -1e-5 / 360.0];
    for (k, turn) in turns.into_iter().enumerate() {
        let mut ball = striped_ball(row);
        ball.center = [(k as f32 - 1.0) * 340.0, 0.0];
        ball.radius = 160.0;
        ball.paint.vary = [turn, 0.0, 1.0, 0.0];
        frame.looks.push(ball);
    }
    let Some(pixels) = render("paint-varied-balls", &frame) else { return };
    // Each ball's red half, 40 pixels left of its centre.
    let at = |k: usize| {
        let x = (W as f32 / 2.0 + (k as f32 - 1.0) * 170.0 - 40.0) as u32;
        let i = ((H / 2) * W + x) as usize * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let (a, b, c) = (at(0), at(1), at(2));
    assert!(a[1] > 200 && (120..235).contains(&a[0]) && a[2] < 40, "a quarter turn from red: {a:?}");
    assert!(b[2] > 200 && (120..235).contains(&b[0]) && b[1] < 40, "minus a quarter: {b:?}");
    assert!(c[0] > 200 && c[1] < 40 && c[2] < 40, "a hair back from red is red: {c:?}");

    // No variation: every ball paints the pattern's own red.
    for ball in &mut frame.looks {
        ball.paint.vary = crate::NO_VARY;
    }
    let Some(plain) = render("paint-unvaried-balls", &frame) else { return };
    let at = |k: usize| {
        let x = (W as f32 / 2.0 + (k as f32 - 1.0) * 170.0 - 40.0) as u32;
        let i = ((H / 2) * W + x) as usize * 4;
        [plain[i], plain[i + 1], plain[i + 2]]
    };
    assert!((0..3).all(|k| at(k)[0] > 200 && at(k)[1] < 40), "all red: {:?}", [at(0), at(1), at(2)]);
}

/// A big styled circle at the centre, `color` its own colour, painting `paint` inside.
fn look_ball(color: [f32; 4], paint: PaintGpu) -> LookInstance {
    LookInstance { color, paint, ..striped_ball(0) }
}

/// The linear-light scene colour an output byte shows: sRGB decoded, and the post pass's
/// highlight shoulder above 0.8 undone.
fn linear(b: u8) -> f32 {
    let c = b as f32 / 255.0;
    let x = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    if x < 0.8 { x } else { 0.8 - 0.2 * (1.0 - ((x - 0.8) / 0.2).min(0.999)).ln() }
}

/// The linear-light colour at output pixel (`x`, `y`).
fn linear_at(pixels: &[u8], x: u32, y: u32) -> [f32; 3] {
    let i = (y * W + x) as usize * 4;
    [linear(pixels[i]), linear(pixels[i + 1]), linear(pixels[i + 2])]
}

fn near(a: [f32; 3], b: [f32; 3], tolerance: f32) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() <= tolerance)
}

/// The ball's top and bottom, 190 pixels above and below its centre (it is 200 pixels round).
fn top_and_bottom(pixels: &[u8]) -> ([f32; 3], [f32; 3]) {
    (linear_at(pixels, W / 2, H / 2 - 190), linear_at(pixels, W / 2, H / 2 + 190))
}

#[test]
fn inside_linear_shade_runs_from_base_to_darker_top_to_bottom() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let shade = PaintGpu { row: -2.0, t0: -1.0, pattern: 1.0, angle: 90.0, ..PaintGpu::default() };
    frame.looks.push(look_ball([1.0, 0.0, 0.0, 1.0], shade));
    let Some(pixels) = render("inside-linear-shade", &frame) else { return };
    let (top, bottom) = top_and_bottom(&pixels);
    assert!(near(top, [1.0, 0.0, 0.0], 0.05), "red at the top: {top:?}");
    assert!(near(bottom, [0.0, 0.0, 0.0], 0.05), "black at the bottom: {bottom:?}");
}

#[test]
fn inside_linear_own_gradient_uses_its_stops() {
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let row = frame.gradients.row(&Gradient::even(&[Color::rgb(0, 0, 255), Color::rgb(0, 255, 0)]));
    let own = PaintGpu { row: row as f32, pattern: 1.0, angle: 90.0, ..PaintGpu::default() };
    // Red, so a ball that paints its own colour fails.
    frame.looks.push(look_ball([1.0, 0.0, 0.0, 1.0], own));
    let Some(pixels) = render("inside-linear-own", &frame) else { return };
    let (top, bottom) = top_and_bottom(&pixels);
    assert!(near(top, [0.0, 0.0, 1.0], 0.05), "blue at the top: {top:?}");
    assert!(near(bottom, [0.0, 1.0, 0.0], 0.05), "green at the bottom: {bottom:?}");
}

#[test]
fn inside_linear_with_one_colour_is_flat() {
    let base = [0.8, 0.3, 0.1];
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let row = frame.gradients.row(&Gradient::solid(Color::rgb(40, 200, 120)));
    let own = PaintGpu { row: row as f32, pattern: 1.0, angle: 90.0, ..PaintGpu::default() };
    let shade = PaintGpu { row: -2.0, t0: 0.0, pattern: 1.0, angle: 90.0, ..PaintGpu::default() };
    for (name, paint) in [("inside-flat-own", own), ("inside-flat-shade", shade)] {
        frame.looks = vec![look_ball([base[0], base[1], base[2], 1.0], paint)];
        let Some(pixels) = render(name, &frame) else { return };
        let middle = linear_at(&pixels, W / 2, H / 2);
        assert!(middle.iter().all(|c| c.is_finite()), "{name}: {middle:?}");
        if paint.row < -1.5 {
            assert!(near(middle, base, 0.02), "{name}: no shade keeps the ball's colour: {middle:?}");
        }
        for y in (H / 2 - 180..H / 2 + 180).step_by(10) {
            for x in (W / 2 - 180..W / 2 + 180).step_by(10) {
                let (dx, dy) = (x as f32 - W as f32 / 2.0, y as f32 - H as f32 / 2.0);
                if dx.hypot(dy) > 180.0 {
                    continue;
                }
                let p = linear_at(&pixels, x, y);
                assert!(near(p, middle, 0.02), "{name}: ({x}, {y}) {p:?} vs the middle {middle:?}");
            }
        }
    }
}

#[test]
fn inside_masks_do_not_wrap_whatever_the_repeat() {
    let red = [1.0, 0.0, 0.0, 1.0];
    let row_of =
        |frame: &mut Frame| frame.gradients.row(&Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]));
    for (name, shade) in [("own", false), ("shade", true)] {
        let mut drawn = Vec::new();
        for repeat in [0.0, 1.0] {
            let mut frame = Frame { bloom: 0.0, ..Frame::default() };
            let row = row_of(&mut frame) as f32;
            let (row, t0) = if shade { (-2.0, -1.0) } else { (row, 0.0) };
            let dots = PaintGpu { row, t0, pattern: 7.0, scale: 2.0, repeat, ..PaintGpu::default() };
            frame.looks.push(look_ball(red, dots));
            let Some(pixels) = render(&format!("inside-dots-{name}-repeat-{repeat}"), &frame) else { return };
            // The ball's centre is a dot's centre: the gradient's last colour, or the full shade.
            let centre = linear_at(&pixels, W / 2, H / 2);
            let want = if shade { [0.0, 0.0, 0.0] } else { [0.0, 0.0, 1.0] };
            assert!(near(centre, want, 0.05), "{name}, repeat {repeat}: the dot's centre is {centre:?}");
            drawn.push(pixels);
        }
        assert!(drawn[0] == drawn[1], "{name}: Repeat draws the dots as Clamp does");
    }
}

/// How much of the second colour the old Styled look's Dots fill showed at `p` (unit radius,
/// y down), copied from `pattern(p, 6, size)` in styled.wgsl before the Inside change, with the
/// softened edge left out: 1 inside a dot, 0 outside, and how far the point is from the edge in
/// cell units.
fn old_fill_dots(p: [f32; 2], size: f32) -> (f32, f32) {
    let n = size * 0.5;
    let cell = |x: f32| {
        let v = x * n + 0.5;
        v - v.floor() - 0.5
    };
    let d = cell(p[0]).hypot(cell(p[1])) - 0.28;
    (if d < 0.0 { 1.0 } else { 0.0 }, d.abs())
}

#[test]
fn inside_dots_mask_matches_the_old_fill_dots() {
    // The old Styled look drew Dots at its Pattern size (4 by default) as the mix of its two
    // colours. Inside Dots at Size 2 (the shader doubles it for the masks) must put the same
    // dots in the same places, the gradient's first colour between them and its last inside.
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let row = frame.gradients.row(&Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]));
    let dots = PaintGpu { row: row as f32, pattern: 7.0, scale: 2.0, ..PaintGpu::default() };
    frame.looks.push(look_ball([0.0, 1.0, 0.0, 1.0], dots));
    let Some(pixels) = render("inside-dots-as-old-fill-dots", &frame) else { return };
    let (mut inside, mut outside) = (0, 0);
    // The ball is 200 pixels round at the centre of the output, y down.
    for y in (H / 2 - 190..H / 2 + 190).step_by(2) {
        for x in (W / 2 - 190..W / 2 + 190).step_by(2) {
            let p = [(x as f32 + 0.5 - W as f32 / 2.0) / 200.0, (y as f32 + 0.5 - H as f32 / 2.0) / 200.0];
            // Inside the ball, and over 3 pixels (0.03 cell units at size 4) from a dot's edge.
            let (dot, from_edge) = old_fill_dots(p, 4.0);
            if p[0].hypot(p[1]) > 0.95 || from_edge < 0.03 {
                continue;
            }
            let i = (y * W + x) as usize * 4;
            let c = &pixels[i..i + 3];
            if dot > 0.5 {
                assert!(c[2] > 150 && c[0] < 60 && c[1] < 60, "({x}, {y}) {c:?}: inside an old dot is blue");
                inside += 1;
            } else {
                assert!(c[0] > 150 && c[1] < 60 && c[2] < 60, "({x}, {y}) {c:?}: between old dots is red");
                outside += 1;
            }
        }
    }
    assert!(inside > 2000 && outside > 2000, "{inside} {outside}: both dots and the gaps were checked");
}

/// A frame whose background is laid out as Ripples (spacing 100 px, from `list`) from the given
/// balls, red to blue on Clamp (so it runs back and forth).
fn rippled(list: f32, balls: &[([f32; 2], f32)]) -> Frame {
    let mut frame = Frame { bloom: 0.0, ripples: true, ..Frame::default() };
    let row = frame.gradients.row(&Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]));
    frame.background = crate::BackgroundPaint {
        row,
        place: [90.0, 1000.0, 0.0, 0.0],
        mode: 3.0,
        pattern: bpe_core::GradientForm::Ripples.code(),
        form: [100.0, 0.0, list, 0.0],
        ..crate::BackgroundPaint::default()
    };
    for &(c, r) in balls {
        frame.push_ripple_ball(c, r);
    }
    frame
}

#[test]
fn ripples_run_out_from_a_ball_and_only_frames_with_ripples_build_their_field() {
    let Some((device, queue)) = gpu() else { return };
    let mut renderer = Renderer::new(&device, W, H);
    // One ball of radius 50 at the middle: rings 100 px apart from its edge, back and forth.
    let frame = rippled(0.0, &[([0.0, 0.0], 50.0)]);
    renderer.render(&device, &queue, &frame);
    assert!(renderer.ripples_built());
    let pixels = renderer.read_output(&device, &queue).unwrap();
    crate::pictures::save("ripples", &pixels, W, H);
    let at = |x: f32, y: f32| crate::pictures::pixel_sized(&pixels, W, H, glam::Vec2::new(x, y));
    let red = |p: [u8; 4]| p[0] > 150 && p[2] < 80;
    let blue = |p: [u8; 4]| p[2] > 150 && p[0] < 80;
    assert!(red(at(0.0, 0.0)), "inside the ball: the start {:?}", at(0.0, 0.0));
    assert!(blue(at(0.0, 150.0)), "one spacing out: the end {:?}", at(0.0, 150.0));
    assert!(red(at(-250.0, 0.0)), "two spacings out: back at the start {:?}", at(-250.0, 0.0));
    let half = at(200.0, 0.0);
    assert!(half[0] > 60 && half[2] > 60, "one and a half spacings out: the middle {half:?}");
    // Two balls: each pixel measures from the nearer.
    let two = rippled(0.0, &[([0.0, -400.0], 20.0), ([0.0, 400.0], 20.0)]);
    renderer.render(&device, &queue, &two);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    let at = |x: f32, y: f32| crate::pictures::pixel_sized(&pixels, W, H, glam::Vec2::new(x, y));
    assert!(blue(at(0.0, -280.0)) && blue(at(0.0, 280.0)), "{:?} {:?}", at(0.0, -280.0), at(0.0, 280.0));
    // Only shapes asked for, and none drawn: nothing to measure from, the start everywhere.
    let shapes = rippled(2.0, &[([0.0, 0.0], 50.0)]);
    renderer.render(&device, &queue, &shapes);
    let pixels = renderer.read_output(&device, &queue).unwrap();
    let at = |x: f32, y: f32| crate::pictures::pixel_sized(&pixels, W, H, glam::Vec2::new(x, y));
    assert!(red(at(0.0, 150.0)), "{:?}", at(0.0, 150.0));
    // A frame without Ripples builds nothing.
    renderer.render(&device, &queue, &Frame { bloom: 0.0, ..Frame::default() });
    assert!(!renderer.ripples_built());
}
