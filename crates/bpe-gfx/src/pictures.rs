//! Picture tests for the stage 5 passes: styled balls, trails and particles, on the software GPU.

use crate::{Frame, LookInstance, PaintGpu, Renderer, headless_device};
use glam::Vec2;

/// Renders `frame` at `w` by `h`, or `None` without a GPU.
pub(crate) fn render_sized(frame: &Frame, w: u32, h: u32) -> Option<Vec<u8>> {
    let Ok((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available, skipping");
        return None;
    };
    let mut renderer = Renderer::new(&device, w, h);
    renderer.render(&device, &queue, frame);
    Some(renderer.read_output(&device, &queue).unwrap())
}

/// The pixel at a world position of a 1080 by 1920 frame drawn at `w` by `h`.
pub(crate) fn pixel_sized(pixels: &[u8], w: u32, h: u32, at: Vec2) -> [u8; 4] {
    let x = (((at.x / 1080.0 + 0.5) * w as f32) as usize).min(w as usize - 1);
    let y = (((at.y / 1920.0 + 0.5) * h as f32) as usize).min(h as usize - 1);
    let i = (y * w as usize + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// Saves a picture under target/pictures when BPE_SAVE_RENDERS is set.
pub(crate) fn save(name: &str, pixels: &[u8], w: u32, h: u32) {
    if std::env::var_os("BPE_SAVE_RENDERS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pictures");
    std::fs::create_dir_all(&dir).unwrap();
    // A plain PPM, so the tests need no image crate.
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in pixels.chunks(4) {
        out.extend_from_slice(&p[..3]);
    }
    std::fs::write(dir.join(format!("{name}.ppm")), out).unwrap();
}

const PINK: [f32; 4] = [1.0, 0.1046, 0.3813, 1.0];
const BLUE: [f32; 4] = [0.01, 0.025, 1.0, 1.0];
const CYAN: [f32; 4] = [0.01, 0.6, 1.0, 1.0];

fn look(center: Vec2, radius: f32) -> LookInstance {
    LookInstance {
        center: center.into(),
        radius,
        seed: 0.3,
        color: PINK,
        color2: BLUE,
        outline_color: CYAN,
        form: [0.0, 0.0, 4.0, 0.15],
        look: [0.0, 0.0, 1.0, 0.0],
        stretch: [1.0, 0.0, 1.0, 1.0],
        extent: [0.8, 3.0, 0.0, 0.0],
        paint: PaintGpu::default(),
    }
}

#[test]
fn every_shape_and_fill_draws_a_styled_ball() {
    const W: u32 = 540;
    const H: u32 = 960;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let at = |i: usize| Vec2::new(-405.0 + (i % 4) as f32 * 270.0, -800.0 + (i / 4) as f32 * 230.0);
    for shape in 0..8 {
        let mut l = look(at(shape), 80.0);
        l.form[0] = shape as f32;
        l.look[0] = 4.0;
        frame.looks.push(l);
    }
    for fill in 0..12 {
        let mut l = look(at(8 + fill), 80.0);
        l.form[1] = fill as f32;
        l.look[1] = 0.3;
        frame.looks.push(l);
    }
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("ball-styles", &pixels, W, H);
    for (i, l) in frame.looks.iter().enumerate() {
        let p = pixel_sized(&pixels, W, H, Vec2::from(l.center) + Vec2::new(0.0, 30.0));
        if i == 7 {
            assert!(p[0] < 60, "the donut should have a hole: {p:?}");
            continue;
        }
        assert!(p[0] > 120 || p[2] > 120, "ball {i} (shape {}, fill {}) is missing: {p:?}", l.form[0], l.form[1]);
    }
    let corner = Vec2::new(58.0, 58.0);
    let square = pixel_sized(&pixels, W, H, at(1) + corner);
    let circle = pixel_sized(&pixels, W, H, at(0) + corner);
    assert!(square[0] > 120 || square[2] > 120, "the square has corners: {square:?}");
    assert!(circle[0] < 60 && circle[2] < 60, "the circle has none: {circle:?}");
    // The outline shows just inside the edge, in cyan.
    let edge = pixel_sized(&pixels, W, H, at(0) + Vec2::new(0.0, 77.0));
    assert!(edge[2] > 150 && edge[1] > 100 && edge[0] < 100, "{edge:?}");
}

#[test]
fn a_stretched_ball_reaches_further_along_its_motion() {
    const W: u32 = 270;
    const H: u32 = 480;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let mut l = look(Vec2::ZERO, 80.0);
    l.stretch = [1.0, 0.0, 1.6, 0.8];
    frame.looks.push(l);
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    let along = pixel_sized(&pixels, W, H, Vec2::new(112.0, 0.0));
    let across = pixel_sized(&pixels, W, H, Vec2::new(0.0, 112.0));
    assert!(along[0] > 120, "{along:?}");
    assert!(across[0] < 40, "{across:?}");
    // An additive styled ball adds light and covers nothing: the output alpha stays opaque
    // and the colour shows.
    frame.looks[0].color = [1.0, 0.1, 0.4, -1.0];
    let pixels = render_sized(&frame, W, H).unwrap();
    assert!(pixel_sized(&pixels, W, H, Vec2::ZERO)[0] > 120);
}

#[test]
fn a_streak_fades_to_its_tail_and_sits_behind_the_balls() {
    use crate::{BallInstance, push_streak};
    const W: u32 = 270;
    const H: u32 = 480;
    let mut frame = Frame { bloom: 0.0, ..Frame::default() };
    let points: Vec<Vec2> = (0..=10).map(|i| Vec2::new(0.0, -40.0 * i as f32)).collect();
    push_streak(&mut frame.trails, &points, 24.0, 1.0, [1.0, 0.0, 0.0, -1.0], 4.0);
    frame.balls.push(BallInstance {
        center: [0.0, 0.0],
        radius: 60.0,
        glow: 0.0,
        color: [0.0, 0.0, 1.0, 1.0],
        code: [0.0; 4],
        paint: crate::PaintGpu::default(),
    });
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("streak", &pixels, W, H);
    let head = pixel_sized(&pixels, W, H, Vec2::new(0.0, -80.0));
    let tail = pixel_sized(&pixels, W, H, Vec2::new(0.0, -390.0));
    let beside = pixel_sized(&pixels, W, H, Vec2::new(60.0, -200.0));
    let ball = pixel_sized(&pixels, W, H, Vec2::ZERO);
    assert!(head[0] > 120, "the streak is red near its head: {head:?}");
    assert!(tail[0] < head[0] / 3, "and fades towards its tail: {tail:?} vs {head:?}");
    assert!(beside[0] < 20, "nothing beside it: {beside:?}");
    // Drawn four times smaller than the video, the edge still softens over an output pixel
    // instead of being cut off by the quad: just outside the half width there is still light.
    // A quarter of the way down the streak is 18 px wide (tapering from 24).
    let edge = pixel_sized(&pixels, W, H, Vec2::new(19.5, -100.0));
    let outside = pixel_sized(&pixels, W, H, Vec2::new(24.0, -100.0));
    assert!(edge[0] > 30 && edge[0] < 200, "a soft edge: {edge:?}");
    assert!(outside[0] < 20, "dark past the margin: {outside:?}");
    assert!(ball[2] > 100 && ball[0] < 80, "the ball covers the streak: {ball:?}");
    // A covering streak hides the background instead of adding to it.
    let mut covering = Frame { bloom: 0.0, ..Frame::default() };
    covering.background.row = covering.gradients.row(&bpe_core::Gradient::solid(bpe_core::Color::WHITE));
    push_streak(&mut covering.trails, &points, 24.0, 0.0, [0.0, 0.0, 0.0, 1.0], 1.0);
    let pixels = render_sized(&covering, W, H).unwrap();
    let head = pixel_sized(&pixels, W, H, Vec2::new(0.0, -20.0));
    assert!(head[0] < 120, "a black covering streak darkens a white background: {head:?}");
}

#[test]
fn a_burst_of_sparks_flies_the_way_its_normal_points_and_dies_after_its_life() {
    use crate::Burst;
    const W: u32 = 270;
    const H: u32 = 480;
    let mut frame = Frame { bloom: 0.0, time: 0.3, ..Frame::default() };
    frame.bursts.push(Burst {
        origin: [0.0, 0.0],
        normal: [0.0, -1.0],
        time: 0.0,
        color: [1.0, 0.2, 0.2, 1.0],
        count: 300,
        speed: 400.0,
        spread: 40.0,
        size: 6.0,
        life: 1.0,
        gravity: 0.0,
        drag: 0.0,
        shape: 0,
        additive: true,
        seed: 3,
        paint: crate::PaintGpu::default(),
        follow: 0,
        amount: 1.0,
    });
    let Some(pixels) = render_sized(&frame, W, H) else { return };
    save("sparks", &pixels, W, H);
    let red_in = |pixels: &[u8], x0: f32, x1: f32, y0: f32, y1: f32| {
        let mut sum = 0u32;
        let mut y = y0;
        while y < y1 {
            let mut x = x0;
            while x < x1 {
                sum += pixel_sized(pixels, W, H, Vec2::new(x, y))[0] as u32;
                x += 4.0;
            }
            y += 4.0;
        }
        sum
    };
    let above = red_in(&pixels, -150.0, 150.0, -300.0, -20.0);
    let below = red_in(&pixels, -150.0, 150.0, 20.0, 300.0);
    let aside = red_in(&pixels, 300.0, 500.0, -300.0, 300.0);
    assert!(above > 2000, "sparks fly up: {above}");
    assert!(below < above / 20, "and not down: {below} vs {above}");
    assert!(aside < 50, "nothing far off to the side: {aside}");
    // Rendered after the particles' life, nothing is left; a covering burst draws too.
    frame.time = 1.5;
    let pixels = render_sized(&frame, W, H).unwrap();
    assert!(red_in(&pixels, -150.0, 150.0, -300.0, 300.0) < 50);
    frame.time = 0.3;
    frame.bursts[0].additive = false;
    frame.bursts[0].shape = 2;
    let pixels = render_sized(&frame, W, H).unwrap();
    assert!(red_in(&pixels, -150.0, 150.0, -300.0, -20.0) > 1000);
}
